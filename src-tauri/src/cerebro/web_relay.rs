//! 将服务端网页流转发给完整 Dextra Web router，不维护业务命令白名单。

use super::web_flow::{Flow, Outbound, CHUNK_BYTES, MAX_WS_MESSAGE};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::{mpsc, Mutex, OnceCell};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use crate::app_state::AppState;
use crate::web::shutdown::ShutdownSignal;

#[derive(Deserialize)]
struct Frame {
    #[serde(rename = "TYPE")]
    kind: String,
    #[serde(rename = "STREAM_ID")]
    stream_id: String,
    #[serde(rename = "PAYLOAD")]
    payload: Value,
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
enum StreamInput {
    Http(mpsc::Sender<Vec<u8>>),
    WebSocket(mpsc::Sender<Value>),
}

struct ActiveStream {
    input: Option<StreamInput>,
    task: JoinHandle<()>,
    flow: Arc<Flow>,
}

pub struct ClientWebRelay {
    state: Arc<AppState>,
    static_dir: PathBuf,
    peer: OnceCell<Arc<WebPeer>>,
    streams: Arc<Mutex<HashMap<String, ActiveStream>>>,
}

impl ClientWebRelay {
    pub fn new(state: Arc<AppState>, static_dir: PathBuf) -> Self {
        Self {
            state,
            static_dir,
            peer: OnceCell::new(),
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
        for (_, stream) in self.streams.lock().await.drain() {
            stream.flow.close();
            stream.task.abort();
        }
    }

    pub async fn handle(&self, value: Value, outbound: Outbound) -> bool {
        if !value["TYPE"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("WEB_"))
        {
            return false;
        }
        let stream_id = value["STREAM_ID"].as_str().unwrap_or_default().to_string();
        let result = match serde_json::from_value::<Frame>(value) {
            Ok(frame) => self.dispatch(frame, outbound.clone()).await,
            Err(error) => Err(error.to_string()),
        };
        if let Err(message) = result {
            if let Some(stream) = self.streams.lock().await.remove(&stream_id) {
                stream.flow.close();
                stream.task.abort();
            }
            let _ = send(
                &outbound,
                &stream_id,
                "WEB_ERROR",
                json!({"MESSAGE": message}),
            )
            .await;
        }
        true
    }

    async fn dispatch(&self, frame: Frame, outbound: Outbound) -> Result<(), String> {
        match frame.kind.as_str() {
            "WEB_WINDOW_UPDATE" => {
                if let Some(stream) = self.streams.lock().await.get(&frame.stream_id) {
                    stream
                        .flow
                        .add(frame.payload["CREDIT"].as_u64().ok_or("缺少额度")? as usize)?;
                }
            }
            "WEB_HTTP_OPEN" => {
                let peer = self.peer().await?;
                let method = frame.payload["METHOD"]
                    .as_str()
                    .ok_or("HTTP 请求缺少方法")?
                    .parse::<reqwest::Method>()
                    .map_err(|error| error.to_string())?;
                let path = frame.payload["PATH"].as_str().ok_or("HTTP 请求缺少路径")?;
                let headers: Vec<(String, String)> =
                    serde_json::from_value(frame.payload["HEADERS"].clone())
                        .map_err(|error| error.to_string())?;
                let flow = Flow::new();
                let body_flow = flow.clone();
                let body_outbound = outbound.clone();
                let body_id = frame.stream_id.clone();
                let (body_tx, body_rx) = mpsc::channel::<Vec<u8>>(16);
                let body = futures_util::stream::unfold(
                    (body_rx, false, body_flow, body_outbound, body_id),
                    |(mut receiver, pending, flow, outbound, id)| async move {
                        if pending {
                            flow.consumed();
                            if send(&outbound, &id, "WEB_WINDOW_UPDATE", json!({"CREDIT": 1}))
                                .await
                                .is_err()
                            {
                                return None;
                            }
                        }
                        receiver.recv().await.map(|bytes| {
                            (
                                Ok::<_, std::io::Error>(bytes),
                                (receiver, true, flow, outbound, id),
                            )
                        })
                    },
                );
                let mut request = peer.client.request(method, format!("{}{path}", peer.url));
                for (name, value) in headers {
                    request = request.header(name, value);
                }
                request = request
                    .bearer_auth(&peer.token)
                    .header("Accept-Encoding", "identity")
                    .body(reqwest::Body::wrap_stream(body));
                let streams = self.streams.clone();
                let stream_id = frame.stream_id.clone();
                let mut guard = self.streams.lock().await;
                if guard.len() >= 64 {
                    return Err("客户端网页同时请求数量超过 64".into());
                }
                let task_flow = flow.clone();
                let task = tokio::spawn(async move {
                    let result = async {
                        let response = request.send().await.map_err(|error| error.to_string())?;
                        let status = response.status().as_u16();
                        let headers: Vec<(String, String)> = response
                            .headers()
                            .iter()
                            .filter_map(|(name, value)| {
                                value
                                    .to_str()
                                    .ok()
                                    .map(|value| (name.to_string(), value.to_string()))
                            })
                            .collect();
                        send(
                            &outbound,
                            &stream_id,
                            "WEB_HTTP_RESPONSE",
                            json!({"STATUS": status, "HEADERS": headers}),
                        )
                        .await?;
                        let mut body = response.bytes_stream();
                        loop {
                            task_flow.take().await?;
                            let Some(chunk) = body.next().await else {
                                break;
                            };
                            let chunk = chunk.map_err(|error| error.to_string())?;
                            if chunk.is_empty() {
                                task_flow.add(1)?;
                                continue;
                            }
                            for (index, bytes) in chunk.chunks(CHUNK_BYTES).enumerate() {
                                if index > 0 {
                                    task_flow.take().await?;
                                }
                                send(
                                    &outbound,
                                    &stream_id,
                                    "WEB_HTTP_DATA",
                                    json!({"DATA": STANDARD.encode(bytes)}),
                                )
                                .await?;
                            }
                        }
                        send(&outbound, &stream_id, "WEB_HTTP_DONE", json!({})).await
                    }
                    .await;
                    if let Err(message) = result {
                        let _ = send(
                            &outbound,
                            &stream_id,
                            "WEB_ERROR",
                            json!({"MESSAGE": message}),
                        )
                        .await;
                    }
                    streams.lock().await.remove(&stream_id);
                });
                guard.insert(
                    frame.stream_id,
                    ActiveStream {
                        input: Some(StreamInput::Http(body_tx)),
                        task,
                        flow,
                    },
                );
            }
            "WEB_HTTP_BODY" | "WEB_WS_DATA" => {
                let data = STANDARD
                    .decode(frame.payload["DATA"].as_str().ok_or("网页内容帧缺少字节")?)
                    .map_err(|error| error.to_string())?;
                let guard = self.streams.lock().await;
                if let Some(stream) = guard.get(&frame.stream_id) {
                    stream.flow.receive(data.len())?;
                    match &stream.input {
                        Some(StreamInput::Http(sender)) => {
                            // A native handler may finish without consuming the body.
                            if let Err(mpsc::error::TrySendError::Full(_)) = sender.try_send(data) {
                                return Err("网页上传队列已满".into());
                            }
                        }
                        Some(StreamInput::WebSocket(sender)) => {
                            sender
                                .try_send(frame.payload)
                                .map_err(|_| "客户端 WebSocket 已关闭或接收额度无效".to_string())?;
                        }
                        None => {}
                    }
                }
            }
            "WEB_HTTP_END" => {
                if let Some(stream) = self.streams.lock().await.get_mut(&frame.stream_id) {
                    stream.input.take();
                }
            }
            "WEB_WS_OPEN" => {
                let peer = self.peer().await?;
                let path = frame.payload["PATH"].as_str().ok_or("WebSocket 缺少路径")?;
                let url = format!("{}{path}", peer.url.replacen("http://", "ws://", 1));
                let mut request = url
                    .into_client_request()
                    .map_err(|error| error.to_string())?;
                request.headers_mut().insert("Authorization", format!("Bearer {}", peer.token).parse().map_err(|error: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| error.to_string())?);
                if let Some(protocol) = frame.payload["PROTOCOL"].as_str() {
                    request.headers_mut().insert("Sec-WebSocket-Protocol", protocol.parse().map_err(|error: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| error.to_string())?);
                }
                let flow = Flow::new();
                let task_flow = flow.clone();
                let (input_tx, mut input_rx) = mpsc::channel::<Value>(16);
                let streams = self.streams.clone();
                let stream_id = frame.stream_id.clone();
                let mut guard = self.streams.lock().await;
                if guard.len() >= 64 {
                    return Err("客户端网页同时请求数量超过 64".into());
                }
                let task = tokio::spawn(async move {
                    let result = async {
                        let (socket, _) = tokio_tungstenite::connect_async(request).await.map_err(|error| error.to_string())?;
                        let (mut sink, mut source) = socket.split();
                        send(&outbound, &stream_id, "WEB_WS_ACCEPT", json!({})).await?;
                        // Separate socket directions: a slow receiver must not block uploads.
                        let output_flow = task_flow.clone();
                        let output_id = stream_id.clone();
                        let output_sender = outbound.clone();
                        let output = async move {
                            loop {
                                output_flow.take().await?;
                                let Some(incoming) = source.next().await else { break; };
                                match incoming.map_err(|e| e.to_string())? {
                                    Message::Text(text) => send_message(&output_sender, &output_id, &output_flow, text.as_bytes(), false).await?,
                                    Message::Binary(bytes) => send_message(&output_sender, &output_id, &output_flow, &bytes, true).await?,
                                    Message::Close(frame) => {
                                        send(&output_sender, &output_id, "WEB_WS_CLOSE", json!({"CODE": frame.as_ref().map(|f| u16::from(f.code)).unwrap_or(1000), "REASON": frame.map(|f| f.reason.to_string()).unwrap_or_default()})).await?;
                                        return Ok::<(), String>(());
                                    }
                                    _ => { output_flow.add(1)?; }
                                }
                            }
                            send(&output_sender, &output_id, "WEB_WS_CLOSE", json!({"CODE": 1000, "REASON": ""})).await
                        };
                        let input = async {
                            let mut message = Vec::new();
                            let mut binary = None;
                            while let Some(payload) = input_rx.recv().await {
                                let kind = payload["BINARY"].as_bool().ok_or("缺少 WebSocket 消息类型")?;
                                if binary.is_some_and(|old| old != kind) { return Err("WebSocket 分块类型不一致".into()); }
                                binary = Some(kind);
                                message.extend(STANDARD.decode(payload["DATA"].as_str().ok_or("缺少数据")?).map_err(|e| e.to_string())?);
                                if message.len() > MAX_WS_MESSAGE { return Err("WebSocket 消息超过大小限制".into()); }
                                if payload["END"].as_bool().ok_or("缺少 WebSocket 消息结束标记")? {
                                    let bytes = std::mem::take(&mut message);
                                    let value = if kind { Message::Binary(bytes.into()) } else { Message::Text(String::from_utf8(bytes).map_err(|e| e.to_string())?.into()) };
                                    sink.send(value).await.map_err(|e| e.to_string())?;
                                    binary = None;
                                }
                                task_flow.consumed();
                                send(&outbound, &stream_id, "WEB_WINDOW_UPDATE", json!({"CREDIT": 1})).await?;
                            }
                            Ok::<(), String>(())
                        };
                        tokio::pin!(output, input);
                        tokio::select! { result = &mut output => result, result = &mut input => result }
                    }.await;
                    task_flow.close();
                    if let Err(message) = result {
                        let _ = send(
                            &outbound,
                            &stream_id,
                            "WEB_ERROR",
                            json!({"MESSAGE": message}),
                        )
                        .await;
                    }
                    streams.lock().await.remove(&stream_id);
                });
                guard.insert(
                    frame.stream_id,
                    ActiveStream {
                        input: Some(StreamInput::WebSocket(input_tx)),
                        task,
                        flow,
                    },
                );
            }
            "WEB_CANCEL" => {
                if let Some(stream) = self.streams.lock().await.remove(&frame.stream_id) {
                    stream.flow.close();
                    stream.task.abort();
                }
            }
            _ => return Err(format!("不支持的网页传输消息: {}", frame.kind)),
        }
        Ok(())
    }
}

async fn send_message(
    outbound: &Outbound,
    id: &str,
    flow: &Flow,
    bytes: &[u8],
    binary: bool,
) -> Result<(), String> {
    // The first credit was acquired before reading the original socket message.
    for offset in (0..bytes.len().max(1)).step_by(CHUNK_BYTES) {
        if offset > 0 {
            flow.take().await?;
        }
        let end = (offset + CHUNK_BYTES).min(bytes.len());
        send(outbound, id, "WEB_WS_DATA", json!({"DATA": STANDARD.encode(&bytes[offset..end]), "BINARY": binary, "END": end == bytes.len()})).await?;
    }
    Ok(())
}

async fn send(
    outbound: &Outbound,
    stream_id: &str,
    kind: &str,
    payload: Value,
) -> Result<(), String> {
    outbound
        .send(json!({"TYPE": kind, "STREAM_ID": stream_id, "PAYLOAD": payload}))
        .await
}
