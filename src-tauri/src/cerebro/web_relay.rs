//! One outbound data socket per browser request; no application flow-control protocol.
use crate::{app_state::AppState, web::shutdown::ShutdownSignal};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, OnceCell};
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

const IO_TIMEOUT: Duration = Duration::from_secs(180);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const CHUNK_BYTES: usize = 48 * 1024;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Scoped to one Runner connection: reuse its short-lived token for page assets.
/// Refreshing per asset would repeat password verification and a database write.
pub struct DataAccess {
    access: super::identity::CerebroRunnerAccess,
    refresh_at: tokio::time::Instant,
}
impl DataAccess {
    pub fn new(access: super::identity::CerebroRunnerAccess) -> Self {
        let refresh_at =
            tokio::time::Instant::now() + Duration::from_secs(access.expires_in.saturating_sub(10));
        Self { access, refresh_at }
    }
    async fn token(&mut self) -> Result<String, String> {
        if tokio::time::Instant::now() >= self.refresh_at {
            let access = super::identity::refresh_access_token()
                .await
                .map_err(|e| e.to_string())?;
            if access.runner_id != self.access.runner_id
                || access.cerebro_base_url != self.access.cerebro_base_url
            {
                return Err("客户端配对身份已改变".into());
            }
            *self = Self::new(access);
        }
        Ok(self.access.access_token.clone())
    }
}

struct WebPeer {
    url: String,
    token: String,
    client: reqwest::Client,
    server: JoinHandle<()>,
    shutdown: Arc<ShutdownSignal>,
}
impl Drop for WebPeer {
    fn drop(&mut self) {
        self.shutdown.trigger();
        self.server.abort();
    }
}

#[derive(Clone)]
pub struct ClientWebRelay {
    state: Arc<AppState>,
    static_dir: PathBuf,
    peer: Arc<OnceCell<Arc<WebPeer>>>,
    streams: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}
impl ClientWebRelay {
    pub fn new(state: Arc<AppState>, static_dir: PathBuf) -> Self {
        Self {
            state,
            static_dir,
            peer: Arc::new(OnceCell::new()),
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    async fn peer(&self) -> Result<Arc<WebPeer>, String> {
        self.peer
            .get_or_try_init(|| async {
                // 私有 loopback listener 复用完整 router，既不开放公网端口，也不改用户 Web 服务设置。
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .map_err(|error| error.to_string())?;
                let address = listener.local_addr().map_err(|error| error.to_string())?;
                let token = crate::web::generate_random_token();
                let shutdown = Arc::new(ShutdownSignal::new());
                let router = crate::web::router::build_router(
                    self.state.clone(),
                    token.clone(),
                    self.static_dir.clone(),
                    shutdown.clone(),
                );
                let stopping = shutdown.clone();
                let server = tokio::spawn(async move {
                    if let Err(error) = axum::serve(listener, router)
                        .with_graceful_shutdown(async move { stopping.wait().await })
                        .await
                    {
                        tracing::warn!("[cerebro] 客户端网页服务结束: {error}");
                    }
                });
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|error| error.to_string())?;
                Ok(Arc::new(WebPeer {
                    url: format!("http://{address}"),
                    token,
                    client,
                    server,
                    shutdown,
                }))
            })
            .await
            .cloned()
    }

    pub async fn close_all(&self) {
        let tasks: Vec<_> = self.streams.lock().await.drain().map(|(_, t)| t).collect();
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
    }

    pub async fn handle(
        &self,
        value: &Value,
        base_url: &str,
        connection_id: &str,
        access: Arc<Mutex<DataAccess>>,
    ) -> Result<bool, String> {
        let kind = value["TYPE"].as_str().unwrap_or_default();
        if !matches!(kind, "OPEN_DATA_CHANNEL" | "CANCEL_DATA_CHANNEL") {
            return Ok(false);
        }
        if value["CONNECTION_ID"].as_str() != Some(connection_id) {
            return Err("数据请求不属于当前控制连接".into());
        }
        let id = value["REQUEST_ID"]
            .as_str()
            .ok_or("缺少数据请求 ID")?
            .to_owned();
        if kind == "CANCEL_DATA_CHANNEL" {
            let task = self.streams.lock().await.remove(&id);
            if let Some(task) = task {
                task.abort();
                let _ = task.await;
            }
            return Ok(true);
        }
        let endpoint = super::connection::data_endpoint(base_url)?;
        let relay = self.clone();
        let task_id = id.clone();
        let connection = connection_id.to_owned();
        let mut streams = self.streams.lock().await;
        if streams.contains_key(&id) {
            return Err("数据请求 ID 重复".into());
        }
        let task = tokio::spawn(async move {
            let result = async {
                let token = access.lock().await.token().await?;
                let (mut socket, _) = tokio::time::timeout(HANDSHAKE_TIMEOUT, tokio_tungstenite::connect_async_with_config(endpoint.as_str(), Some(socket_config()), false)).await.map_err(|_| "数据连接建立超时".to_string())?.map_err(|e| e.to_string())?;
                send(&mut socket, Message::Text(json!({"TYPE":"AUTHENTICATE","TOKEN":token}).to_string().into())).await?;
                send(&mut socket, Message::Text(json!({"TYPE":"BIND_DATA_CHANNEL","REQUEST_ID":task_id,"CONNECTION_ID":connection}).to_string().into())).await?;
                relay.serve_channel(socket).await
            }.await;
            if let Err(error) = result {
                tracing::warn!("[cerebro] 数据请求 {task_id} 结束: {error}");
            }
            relay.streams.lock().await.remove(&task_id);
        });
        streams.insert(id, task);
        Ok(true)
    }

    /// Runs an authenticated/bound dedicated socket; also used by real loopback tests.
    pub async fn serve_channel(&self, mut socket: Socket) -> Result<(), String> {
        let open = tokio::time::timeout(HANDSHAKE_TIMEOUT, socket.next())
            .await
            .map_err(|_| "等待请求头超时".to_string())?
            .ok_or("数据连接已关闭")?
            .map_err(|e| e.to_string())?;
        let open: Value = serde_json::from_str(open.to_text().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let peer = self.peer().await?;
        match open["TYPE"].as_str() {
            Some("HTTP_OPEN") => self.http(socket, open, peer).await,
            Some("WS_OPEN") => self.websocket(socket, open, peer).await,
            _ => Err("数据连接请求类型无效".into()),
        }
    }

    async fn http(&self, socket: Socket, open: Value, peer: Arc<WebPeer>) -> Result<(), String> {
        let method = open["METHOD"]
            .as_str()
            .ok_or("缺少 HTTP 方法")?
            .parse::<reqwest::Method>()
            .map_err(|e| e.to_string())?;
        let path = open["PATH"].as_str().ok_or("缺少 HTTP 路径")?;
        let headers: Vec<(String, String)> =
            serde_json::from_value(open["HEADERS"].clone()).map_err(|e| e.to_string())?;
        let (mut sink, mut source) = socket.split();
        // One bounded chunk between socket reader and reqwest body. The reader
        // remains alive after HTTP_END so platform disconnect cancels the response.
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
        let input = async move {
            let mut tx = Some(tx);
            let mut ended = false;
            while let Some(message) = source.next().await {
                match message.map_err(|e| e.to_string())? {
                    Message::Binary(bytes) if !ended => {
                        if let Some(sender) = &tx {
                            if tokio::time::timeout(IO_TIMEOUT, sender.send(Ok(bytes.to_vec())))
                                .await
                                .map_err(|_| "本地 HTTP 上传持续阻塞".to_string())?
                                .is_err()
                            {
                                tx.take();
                            }
                        }
                    }
                    Message::Text(text) if !ended => {
                        let end: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                        if end["TYPE"] != "HTTP_END" {
                            return Err("HTTP 上传结束标记无效".to_string());
                        }
                        tx.take();
                        ended = true;
                    }
                    Message::Close(_) => return Err("平台关闭了数据请求".to_string()),
                    Message::Ping(_) | Message::Pong(_) => {}
                    _ => return Err("HTTP 数据消息无效".to_string()),
                }
            }
            Err::<(), String>("数据连接异常断开".into())
        };
        let body = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|bytes| (bytes, rx))
        });
        let mut request = peer.client.request(method, format!("{}{path}", peer.url));
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = async {
            let result = async {
                let response = request.bearer_auth(&peer.token).header("Accept-Encoding","identity").body(reqwest::Body::wrap_stream(body)).send().await.map_err(|e|e.to_string())?;
                let headers: Vec<_> = response.headers().iter().filter_map(|(k,v)|v.to_str().ok().map(|v|(k.to_string(),v.to_string()))).collect();
                send(&mut sink,Message::Text(json!({"TYPE":"HTTP_RESPONSE","STATUS":response.status().as_u16(),"HEADERS":headers}).to_string().into())).await?;
                let mut body = response.bytes_stream();
                while let Some(bytes) = body.next().await {
                    for chunk in bytes.map_err(|e|e.to_string())?.chunks(CHUNK_BYTES) { send(&mut sink,Message::Binary(chunk.to_vec().into())).await?; }
                }
                send(&mut sink,Message::Text(json!({"TYPE":"HTTP_DONE"}).to_string().into())).await?;
                send(&mut sink, Message::Close(None)).await
            }.await;
            if let Err(error) = &result {
                let _ = send(
                    &mut sink,
                    Message::Text(json!({"TYPE":"ERROR","MESSAGE":error}).to_string().into()),
                )
                .await;
            }
            result
        };
        tokio::pin!(input, response);
        tokio::select! {
            result = &mut response => {
                if result.is_ok() {
                    // Drain the close handshake before dropping TCP: an early
                    // native response can leave HTTP_END unread, causing RST
                    // and discarding the response tail on the other peer.
                    let _ = tokio::time::timeout(Duration::from_secs(5), &mut input).await;
                }
                result
            },
            result = &mut input => result
        }
    }

    async fn websocket(
        &self,
        mut socket: Socket,
        open: Value,
        peer: Arc<WebPeer>,
    ) -> Result<(), String> {
        let path = open["PATH"].as_str().ok_or("缺少 WebSocket 路径")?;
        let mut request = format!("{}{path}", peer.url.replacen("http://", "ws://", 1))
            .into_client_request()
            .map_err(|e| e.to_string())?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", peer.token).parse().map_err(
                |e: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| e.to_string(),
            )?,
        );
        if let Some(protocol) = open["PROTOCOL"].as_str() {
            request.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                protocol.parse().map_err(
                    |e: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| {
                        e.to_string()
                    },
                )?,
            );
        }
        let native = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            tokio_tungstenite::connect_async_with_config(request, Some(socket_config()), false),
        )
        .await
        .map_err(|_| "原生 WS 建连超时".to_string())?;
        let (native, _) = match native {
            Ok(value) => value,
            Err(error) => {
                send(
                    &mut socket,
                    Message::Text(
                        json!({"TYPE":"ERROR","MESSAGE":error.to_string()})
                            .to_string()
                            .into(),
                    ),
                )
                .await?;
                return Err(error.to_string());
            }
        };
        send(
            &mut socket,
            Message::Text(json!({"TYPE":"WS_ACCEPT"}).to_string().into()),
        )
        .await?;
        let (mut remote_sink, mut remote_source) = socket.split();
        let (mut local_sink, mut local_source) = native.split();
        let input = async {
            while let Some(message) = remote_source.next().await {
                let message = message.map_err(|e| e.to_string())?;
                let closed = message.is_close();
                if matches!(
                    message,
                    Message::Text(_) | Message::Binary(_) | Message::Close(_)
                ) {
                    send(&mut local_sink, message).await?;
                }
                if closed {
                    return Ok::<(), String>(());
                }
            }
            Err("平台数据连接异常断开".into())
        };
        let output = async {
            while let Some(message) = local_source.next().await {
                let message = message.map_err(|e| e.to_string())?;
                let closed = message.is_close();
                if matches!(
                    message,
                    Message::Text(_) | Message::Binary(_) | Message::Close(_)
                ) {
                    send(&mut remote_sink, message).await?;
                }
                if closed {
                    return Ok::<(), String>(());
                }
            }
            Err("原生 WebSocket 异常断开".into())
        };
        tokio::pin!(input, output);
        tokio::select! { result = &mut input => result, result = &mut output => result }
    }
}

async fn send<S>(sink: &mut S, message: Message) -> Result<(), String>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    tokio::time::timeout(IO_TIMEOUT, sink.send(message))
        .await
        .map_err(|_| "数据写入连续 180 秒未完成".to_string())?
        .map_err(|e| e.to_string())
}

fn socket_config() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(64 * 1024 * 1024))
        .max_frame_size(Some(64 * 1024 * 1024))
        .max_write_buffer_size(64 * 1024 * 1024 + 128 * 1024)
}
