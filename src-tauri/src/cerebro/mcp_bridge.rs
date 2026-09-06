//! 透明 stdio / Streamable HTTP MCP 转发；不拥有业务 handler 或目录。

use super::CerebroMcpPrincipal;
use crate::acp::delegation::transport::{client_credentials_round_trip, client_watch_token};
use async_trait::async_trait;
use futures::{
    future::{BoxFuture, Shared},
    FutureExt, StreamExt,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, Mutex, RwLock},
    time::Instant,
};

#[async_trait]
pub trait PrincipalSource: Send + Sync {
    async fn issue(&self) -> Result<CerebroMcpPrincipal, String>;
}

struct IpcSource {
    socket_path: String,
    token: String,
}

#[async_trait]
impl PrincipalSource for IpcSource {
    async fn issue(&self) -> Result<CerebroMcpPrincipal, String> {
        let response = client_credentials_round_trip(&self.socket_path, &self.token)
            .await
            .map_err(|e| e.to_string())?
            .outcome;
        if response["success"] != true {
            return Err(response["error"].to_string());
        }
        serde_json::from_value(response["data"].clone()).map_err(|e| e.to_string())
    }
}

struct CachedPrincipal {
    value: CerebroMcpPrincipal,
    deadline: Instant,
}

type PrincipalRefresh = Shared<BoxFuture<'static, Result<CerebroMcpPrincipal, String>>>;

pub struct Bridge {
    client: reqwest::Client,
    source: Arc<dyn PrincipalSource>,
    cached: Arc<Mutex<Option<CachedPrincipal>>>,
    refreshing: Mutex<Option<PrincipalRefresh>>,
    session_id: RwLock<Option<String>>,
    protocol_version: RwLock<Option<String>>,
}

impl Bridge {
    pub fn new(source: Arc<dyn PrincipalSource>) -> Self {
        Self {
            client: reqwest::Client::new(),
            source,
            cached: Arc::new(Mutex::new(None)),
            refreshing: Mutex::new(None),
            session_id: RwLock::new(None),
            protocol_version: RwLock::new(None),
        }
    }

    async fn principal(&self) -> Result<CerebroMcpPrincipal, String> {
        // 同一批调用共享领取结果（包括错误），不串行化业务请求。
        let mut refreshing = self.refreshing.lock().await;
        if let Some(pending) = refreshing.as_ref() {
            let pending = pending.clone();
            drop(refreshing);
            return self.finish_refresh(pending).await;
        }
        let cached = self.cached.lock().await;
        if let Some(current) = cached.as_ref() {
            if current.deadline.saturating_duration_since(Instant::now()) >= Duration::from_secs(30)
            {
                return Ok(current.value.clone());
            }
        }
        drop(cached);
        let source = self.source.clone();
        let cache = self.cached.clone();
        let pending = async move {
            let started = Instant::now();
            let value = source.issue().await?;
            *cache.lock().await = Some(CachedPrincipal {
                deadline: started + Duration::from_secs(value.expires_in),
                value: value.clone(),
            });
            Ok(value)
        }
        .boxed()
        .shared();
        *refreshing = Some(pending.clone());
        drop(refreshing);
        self.finish_refresh(pending).await
    }

    async fn finish_refresh(
        &self,
        pending: PrincipalRefresh,
    ) -> Result<CerebroMcpPrincipal, String> {
        let result = pending.clone().await;
        let mut refreshing = self.refreshing.lock().await;
        // 迟到的等待者不能清除下一批已经开始的刷新。
        if refreshing
            .as_ref()
            .is_some_and(|current| current.ptr_eq(&pending))
        {
            refreshing.take();
        }
        result
    }

    async fn request(&self, method: reqwest::Method) -> Result<reqwest::RequestBuilder, String> {
        let principal = self.principal().await?;
        let mut request = self
            .client
            .request(method, &principal.mcp_url)
            .bearer_auth(&principal.access_token)
            .header("Accept", "application/json, text/event-stream");
        if let Some(session) = self.session_id.read().await.as_ref() {
            request = request.header("Mcp-Session-Id", session);
        }
        if let Some(version) = self.protocol_version.read().await.as_ref() {
            request = request.header("MCP-Protocol-Version", version);
        }
        Ok(request)
    }

    pub async fn forward(
        &self,
        message: &Value,
        output: &mpsc::Sender<Value>,
    ) -> Result<(), String> {
        // send 之后不再重试，包括认证失败；下一次独立调用重新领凭据。
        let response = self
            .request(reqwest::Method::POST)
            .await?
            .json(message)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let expected_id = message
            .get("id")
            .filter(|_| message.get("method").is_some());
        self.consume(response, output, expected_id).await
    }

    async fn listen(&self, output: &mpsc::Sender<Value>) -> Result<(), String> {
        let response = self
            .request(reqwest::Method::GET)
            .await?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        // MCP 允许不提供独立服务端事件流。
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Ok(());
        }
        self.consume(response, output, None).await
    }

    async fn publish(&self, message: Value, output: &mpsc::Sender<Value>) -> Result<(), String> {
        if let Some(version) = message
            .pointer("/result/protocolVersion")
            .and_then(Value::as_str)
        {
            *self.protocol_version.write().await = Some(version.to_string());
        }
        output.send(message).await.map_err(|e| e.to_string())
    }

    async fn consume(
        &self,
        response: reqwest::Response,
        output: &mpsc::Sender<Value>,
        expected_id: Option<&Value>,
    ) -> Result<(), String> {
        let status = response.status();
        if matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            self.cached.lock().await.take();
        }
        if !status.is_success() {
            let body = response.text().await.map_err(|e| e.to_string())?;
            return Err(format!("MCP HTTP {status}: {body}"));
        }
        if let Some(session) = response.headers().get("Mcp-Session-Id") {
            *self.session_id.write().await =
                Some(session.to_str().map_err(|e| e.to_string())?.to_string());
        }
        if status == reqwest::StatusCode::ACCEPTED || status == reqwest::StatusCode::NO_CONTENT {
            return Ok(());
        }
        let is_sse = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.starts_with("text/event-stream"));
        if !is_sse {
            let message = response.json::<Value>().await.map_err(|e| e.to_string())?;
            return self.publish(message, output).await;
        }
        let mut decoder = SseDecoder::default();
        let mut stream = response.bytes_stream();
        let mut responded = false;
        let result = async {
            while let Some(chunk) = stream.next().await {
                for message in decoder.push(&chunk.map_err(|e| e.to_string())?)? {
                    let terminal = expected_id.is_some_and(|id| message.get("id") == Some(id))
                        && (message.get("result").is_some() || message.get("error").is_some());
                    self.publish(message, output).await?;
                    responded |= terminal;
                }
            }
            if expected_id.is_some() && !responded {
                return Err(
                    "MCP SSE stream closed before the request received a response".to_string(),
                );
            }
            Ok(())
        }
        .await;
        if responded {
            // 已成功交付的响应不因后续事件流断开而被第二个错误响应覆盖。
            if let Err(error) = result {
                eprintln!("{error}");
            }
            Ok(())
        } else {
            result
        }
    }
}

#[derive(Default)]
struct SseDecoder {
    pending: Vec<u8>,
    data: Vec<String>,
}

impl SseDecoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Value>, String> {
        self.pending.extend_from_slice(bytes);
        let mut messages = Vec::new();
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line = self.pending.drain(..=end).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8(line).map_err(|e| e.to_string())?;
            if line.is_empty() && !self.data.is_empty() {
                messages
                    .push(serde_json::from_str(&self.data.join("\n")).map_err(|e| e.to_string())?);
                self.data.clear();
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data
                    .push(data.strip_prefix(' ').unwrap_or(data).to_string());
            }
        }
        Ok(messages)
    }
}

pub async fn run(socket_path: String, token: String) -> Result<(), String> {
    let bridge = Arc::new(Bridge::new(Arc::new(IpcSource {
        socket_path: socket_path.clone(),
        token: token.clone(),
    })));
    let (output, mut messages) = mpsc::channel::<Value>(64);
    let mut writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(message) = messages.recv().await {
            let mut line = serde_json::to_vec(&message).map_err(|e| e.to_string())?;
            line.push(b'\n');
            stdout.write_all(&line).await.map_err(|e| e.to_string())?;
            stdout.flush().await.map_err(|e| e.to_string())?;
        }
        Ok::<(), String>(())
    });
    let watch = client_watch_token(&socket_path, &token);
    tokio::pin!(watch);
    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    let mut pending = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut watch => break,
            result = &mut writer => {
                pending.abort_all();
                return result.map_err(|e| e.to_string())?;
            },
            Some(result) = pending.join_next(), if !pending.is_empty() => {
                if let Err(error) = result { eprintln!("{error}"); }
            },
            line = stdin.next_line() => {
                let Some(line) = line.map_err(|e| e.to_string())? else { break; };
                let message: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                let bridge = bridge.clone();
                let output = output.clone();
                if message["method"] == "notifications/initialized" {
                    // 初始化通知先到达上游，再接收后续客户端请求。
                    tokio::select! {
                        result = bridge.forward(&message, &output) => result?,
                        _ = &mut watch => break,
                    }
                    pending.spawn(async move { if let Err(error) = bridge.listen(&output).await { eprintln!("{error}"); } });
                } else {
                    pending.spawn(async move {
                        if let Err(error) = bridge.forward(&message, &output).await {
                            if let Some(id) = message.get("id").filter(|_| message.get("method").is_some()) {
                                let _ = output.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": error}})).await;
                            } else { eprintln!("{error}"); }
                        }
                    });
                }
            },
        }
    }
    pending.abort_all();
    writer.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct Source {
        url: String,
        calls: AtomicUsize,
        fail: AtomicBool,
    }
    #[async_trait]
    impl PrincipalSource for Source {
        async fn issue(&self) -> Result<CerebroMcpPrincipal, String> {
            let count = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            tokio::task::yield_now().await;
            if self.fail.load(Ordering::SeqCst) {
                return Err("MODULE_WRITE_REQUIRED: 权限已撤销".into());
            }
            Ok(CerebroMcpPrincipal {
                mcp_url: self.url.clone(),
                access_token: format!("token-{count}"),
                token_type: "bearer".into(),
                expires_in: 600,
            })
        }
    }

    fn source(url: String) -> Arc<Source> {
        Arc::new(Source {
            url,
            calls: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
        })
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_boundary_and_concurrent_requests_share_one_principal() {
        let source = source("http://unused/mcp/stream".into());
        let bridge = Bridge::new(source.clone());
        assert_eq!(bridge.principal().await.unwrap().access_token, "token-1");
        tokio::time::advance(Duration::from_secs(570)).await;
        assert_eq!(bridge.principal().await.unwrap().access_token, "token-1");
        tokio::time::advance(Duration::from_secs(1)).await;
        let refreshed = futures::future::join_all((0..8).map(|_| bridge.principal())).await;
        assert!(refreshed
            .into_iter()
            .all(|value| value.unwrap().access_token == "token-2"));
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn sse_preserves_multiline_json_and_chunked_unicode() {
        let mut decoder = SseDecoder::default();
        let wire = "event: message\r\ndata: {\r\ndata: \"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"本轮输出\"}\r\n\r\n";
        let mut values = Vec::new();
        for chunk in wire.as_bytes().chunks(3) {
            values.extend(decoder.push(chunk).unwrap());
        }
        assert_eq!(
            values,
            vec![json!({"jsonrpc":"2.0", "id":1, "result":"本轮输出"})]
        );
    }

    #[tokio::test]
    async fn concurrent_refresh_failure_is_shared_and_next_call_retries() {
        struct BlockedSource {
            calls: AtomicUsize,
            release: tokio::sync::Semaphore,
        }
        #[async_trait]
        impl PrincipalSource for BlockedSource {
            async fn issue(&self) -> Result<CerebroMcpPrincipal, String> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.release.acquire().await.unwrap().forget();
                Err("凭据服务不可用".into())
            }
        }
        let source = Arc::new(BlockedSource {
            calls: AtomicUsize::new(0),
            release: tokio::sync::Semaphore::new(0),
        });
        let bridge = Bridge::new(source.clone());
        let (output, _) = mpsc::channel(8);
        let call = json!({"jsonrpc":"2.0", "id":1, "method":"tools/call"});
        let batch = futures::future::join_all((0..8).map(|_| bridge.forward(&call, &output)));
        tokio::pin!(batch);
        assert!(futures::poll!(&mut batch).is_pending());
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        source.release.add_permits(1);
        assert!(batch
            .await
            .into_iter()
            .all(|result| result.unwrap_err() == "凭据服务不可用"));
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        source.release.add_permits(1);
        assert_eq!(bridge.principal().await.unwrap_err(), "凭据服务不可用");
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn refresh_keeps_session_and_never_replays_sent_write() {
        use axum::{
            extract::State,
            http::{HeaderMap, StatusCode},
            response::IntoResponse,
            routing::post,
            Json, Router,
        };
        #[derive(Clone, Default)]
        struct Server {
            writes: Arc<AtomicUsize>,
            requests: Arc<AtomicUsize>,
            headers: Arc<Mutex<Vec<(String, String)>>>,
        }
        async fn handle(
            State(state): State<Server>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> axum::response::Response {
            state.requests.fetch_add(1, Ordering::SeqCst);
            if body["method"] == "initialize" {
                return ([("Mcp-Session-Id", "stable-session")], Json(json!({"jsonrpc":"2.0", "id":body["id"], "result":{"protocolVersion":"2025-03-26"}}))).into_response();
            }
            state.headers.lock().await.push((
                headers["authorization"].to_str().unwrap().to_string(),
                headers["mcp-session-id"].to_str().unwrap().to_string(),
            ));
            if body["params"]["name"] == "write" {
                state.writes.fetch_add(1, Ordering::SeqCst);
                return (StatusCode::UNAUTHORIZED, "token expired after dispatch").into_response();
            }
            Json(json!({"jsonrpc":"2.0", "id":body["id"], "result":{"content":[]}})).into_response()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source = source(format!(
            "http://{}/mcp/stream",
            listener.local_addr().unwrap()
        ));
        let server = Server::default();
        let app = Router::new()
            .route("/mcp/stream", post(handle))
            .with_state(server.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let bridge = Bridge::new(source.clone());
        let (output, mut received) = mpsc::channel(8);
        bridge
            .forward(
                &json!({"jsonrpc":"2.0", "id":1, "method":"initialize"}),
                &output,
            )
            .await
            .unwrap();
        assert_eq!(received.recv().await.unwrap()["id"], 1);
        let write =
            json!({"jsonrpc":"2.0", "id":2, "method":"tools/call", "params":{"name":"write"}});
        assert!(bridge
            .forward(&write, &output)
            .await
            .unwrap_err()
            .contains("token expired after dispatch"));
        assert_eq!(server.writes.load(Ordering::SeqCst), 1);
        let read =
            json!({"jsonrpc":"2.0", "id":3, "method":"tools/call", "params":{"name":"read"}});
        bridge.forward(&read, &output).await.unwrap();
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            *server.headers.lock().await,
            vec![
                ("Bearer token-1".into(), "stable-session".into()),
                ("Bearer token-2".into(), "stable-session".into())
            ]
        );
        bridge.cached.lock().await.as_mut().unwrap().deadline = Instant::now();
        source.fail.store(true, Ordering::SeqCst);
        assert!(bridge
            .forward(&write, &output)
            .await
            .unwrap_err()
            .contains("权限已撤销"));
        assert_eq!(server.requests.load(Ordering::SeqCst), 3);
        assert_eq!(server.writes.load(Ordering::SeqCst), 1);
        task.abort();
    }
}
