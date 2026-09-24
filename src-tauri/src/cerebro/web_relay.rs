//! 将服务端网页流转发给完整 Dextra Web router，不维护业务命令白名单。

use std::{collections::HashMap, path::PathBuf, sync::Arc};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex, OnceCell};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use crate::app_state::AppState;
use crate::web::shutdown::ShutdownSignal;

const CHUNK_BYTES: usize = 48 * 1024;

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
    WebSocket(mpsc::Sender<Message>),
}

struct ActiveStream {
    input: Option<StreamInput>,
    task: JoinHandle<()>,
}

pub struct ClientWebRelay {
    state: Arc<AppState>,
    static_dir: PathBuf,
    peer: OnceCell<Arc<WebPeer>>,
    streams: Arc<Mutex<HashMap<String, ActiveStream>>>,
}

impl ClientWebRelay {
    pub fn new(state: Arc<AppState>, static_dir: PathBuf) -> Self {
        Self { state, static_dir, peer: OnceCell::new(), streams: Arc::new(Mutex::new(HashMap::new())) }
    }

    async fn peer(&self) -> Result<Arc<WebPeer>, String> {
        self.peer.get_or_try_init(|| async {
            // 私有 loopback listener 复用完整 router，既不开放公网端口，也不改用户 Web 服务设置。
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|error| error.to_string())?;
            let address = listener.local_addr().map_err(|error| error.to_string())?;
            let token = crate::web::generate_random_token();
            let shutdown = Arc::new(ShutdownSignal::new());
            let router = crate::web::router::build_router(self.state.clone(), token.clone(), self.static_dir.clone(), shutdown.clone());
            let stopping = shutdown.clone();
            let server = tokio::spawn(async move {
                if let Err(error) = axum::serve(listener, router).with_graceful_shutdown(async move { stopping.wait().await }).await {
                    tracing::warn!("[cerebro] 客户端网页服务结束: {error}");
                }
            });
            let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|error| error.to_string())?;
            Ok(Arc::new(WebPeer { url: format!("http://{address}"), token, client, server, shutdown }))
        }).await.cloned()
    }

    pub async fn close_all(&self) {
        for (_, stream) in self.streams.lock().await.drain() { stream.task.abort(); }
    }

    pub async fn handle(&self, value: Value, outbound: mpsc::Sender<Value>) -> bool {
        if !value["TYPE"].as_str().is_some_and(|kind| kind.starts_with("WEB_")) { return false; }
        let stream_id = value["STREAM_ID"].as_str().unwrap_or_default().to_string();
        let result = match serde_json::from_value::<Frame>(value) {
            Ok(frame) => self.dispatch(frame, outbound.clone()).await,
            Err(error) => Err(error.to_string()),
        };
        if let Err(message) = result { let _ = send(&outbound, &stream_id, "WEB_ERROR", json!({"MESSAGE": message})).await; }
        true
    }

    async fn dispatch(&self, frame: Frame, outbound: mpsc::Sender<Value>) -> Result<(), String> {
        match frame.kind.as_str() {
            "WEB_HTTP_OPEN" => {
                let peer = self.peer().await?;
                let method = frame.payload["METHOD"].as_str().ok_or("HTTP 请求缺少方法")?.parse::<reqwest::Method>().map_err(|error| error.to_string())?;
                let path = frame.payload["PATH"].as_str().ok_or("HTTP 请求缺少路径")?;
                let headers: Vec<(String, String)> = serde_json::from_value(frame.payload["HEADERS"].clone()).map_err(|error| error.to_string())?;
                let (body_tx, body_rx) = mpsc::channel::<Vec<u8>>(16);
                let body = futures_util::stream::unfold(body_rx, |mut receiver| async move {
                    receiver.recv().await.map(|bytes| (Ok::<_, std::io::Error>(bytes), receiver))
                });
                let mut request = peer.client.request(method, format!("{}{path}", peer.url));
                for (name, value) in headers { request = request.header(name, value); }
                request = request.bearer_auth(&peer.token).header("Accept-Encoding", "identity").body(reqwest::Body::wrap_stream(body));
                let streams = self.streams.clone();
                let stream_id = frame.stream_id.clone();
                let mut guard = self.streams.lock().await;
                if guard.len() >= 64 { return Err("客户端网页同时请求数量超过 64".into()); }
                let task = tokio::spawn(async move {
                    let result = async {
                        let response = request.send().await.map_err(|error| error.to_string())?;
                        let status = response.status().as_u16();
                        let headers: Vec<(String, String)> = response.headers().iter().filter_map(|(name, value)| value.to_str().ok().map(|value| (name.to_string(), value.to_string()))).collect();
                        send(&outbound, &stream_id, "WEB_HTTP_RESPONSE", json!({"STATUS": status, "HEADERS": headers})).await?;
                        let mut body = response.bytes_stream();
                        while let Some(chunk) = body.next().await {
                            for bytes in chunk.map_err(|error| error.to_string())?.chunks(CHUNK_BYTES) {
                                send(&outbound, &stream_id, "WEB_HTTP_DATA", json!({"DATA": STANDARD.encode(bytes)})).await?;
                            }
                        }
                        send(&outbound, &stream_id, "WEB_HTTP_DONE", json!({})).await
                    }.await;
                    if let Err(message) = result { let _ = send(&outbound, &stream_id, "WEB_ERROR", json!({"MESSAGE": message})).await; }
                    streams.lock().await.remove(&stream_id);
                });
                guard.insert(frame.stream_id, ActiveStream { input: Some(StreamInput::Http(body_tx)), task });
            }
            "WEB_HTTP_BODY" | "WEB_WS_DATA" => {
                let data = STANDARD.decode(frame.payload["DATA"].as_str().ok_or("网页内容帧缺少字节")?).map_err(|error| error.to_string())?;
                let input = self.streams.lock().await.get(&frame.stream_id).and_then(|stream| stream.input.clone());
                match input {
                    Some(StreamInput::Http(sender)) => {
                        // 原生读取 handler 可以不消费请求体便返回；晚到的上传块不能否定已有响应。
                        // 连接失败由请求 task 返回原错误，此处不制造第二个错误。
                        let _ = sender.send(data).await;
                    }
                    Some(StreamInput::WebSocket(sender)) => {
                        let message = if frame.payload["BINARY"].as_bool().unwrap_or(false) { Message::Binary(data.into()) }
                            else { Message::Text(String::from_utf8(data).map_err(|error| error.to_string())?.into()) };
                        sender.send(message).await.map_err(|_| "客户端 WebSocket 已关闭".to_string())?;
                    }
                    None => {}
                }
            }
            "WEB_HTTP_END" => { if let Some(stream) = self.streams.lock().await.get_mut(&frame.stream_id) { stream.input.take(); } }
            "WEB_WS_OPEN" => {
                let peer = self.peer().await?;
                let path = frame.payload["PATH"].as_str().ok_or("WebSocket 缺少路径")?;
                let url = format!("{}{path}", peer.url.replacen("http://", "ws://", 1));
                let mut request = url.into_client_request().map_err(|error| error.to_string())?;
                request.headers_mut().insert("Authorization", format!("Bearer {}", peer.token).parse().map_err(|error: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| error.to_string())?);
                if let Some(protocol) = frame.payload["PROTOCOL"].as_str() { request.headers_mut().insert("Sec-WebSocket-Protocol", protocol.parse().map_err(|error: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| error.to_string())?); }
                let (input_tx, mut input_rx) = mpsc::channel::<Message>(16);
                let streams = self.streams.clone();
                let stream_id = frame.stream_id.clone();
                let mut guard = self.streams.lock().await;
                if guard.len() >= 64 { return Err("客户端网页同时请求数量超过 64".into()); }
                let task = tokio::spawn(async move {
                    let result = async {
                        let (socket, _) = tokio_tungstenite::connect_async(request).await.map_err(|error| error.to_string())?;
                        let (mut sink, mut source) = socket.split();
                        send(&outbound, &stream_id, "WEB_WS_ACCEPT", json!({})).await?;
                        loop {
                            tokio::select! {
                                incoming = source.next() => {
                                    let Some(message) = incoming else { break; };
                                    match message.map_err(|error| error.to_string())? {
                                        Message::Text(text) => send(&outbound, &stream_id, "WEB_WS_DATA", json!({"BINARY": false, "DATA": STANDARD.encode(text.as_bytes())})).await?,
                                        Message::Binary(bytes) => send(&outbound, &stream_id, "WEB_WS_DATA", json!({"BINARY": true, "DATA": STANDARD.encode(bytes)})).await?,
                                        Message::Close(frame) => { send(&outbound, &stream_id, "WEB_WS_CLOSE", json!({"CODE": frame.as_ref().map(|frame| u16::from(frame.code)).unwrap_or(1000), "REASON": frame.map(|frame| frame.reason.to_string()).unwrap_or_default()})).await?; return Ok(()); }
                                        Message::Ping(bytes) => sink.send(Message::Pong(bytes)).await.map_err(|error| error.to_string())?,
                                        Message::Pong(_) | Message::Frame(_) => {}
                                    }
                                }
                                incoming = input_rx.recv() => {
                                    let Some(message) = incoming else { break; };
                                    sink.send(message).await.map_err(|error| error.to_string())?;
                                }
                            }
                        }
                        send(&outbound, &stream_id, "WEB_WS_CLOSE", json!({"CODE": 1000, "REASON": ""})).await
                    }.await;
                    if let Err(message) = result { let _ = send(&outbound, &stream_id, "WEB_ERROR", json!({"MESSAGE": message})).await; }
                    streams.lock().await.remove(&stream_id);
                });
                guard.insert(frame.stream_id, ActiveStream { input: Some(StreamInput::WebSocket(input_tx)), task });
            }
            "WEB_CANCEL" => { if let Some(stream) = self.streams.lock().await.remove(&frame.stream_id) { stream.task.abort(); } }
            _ => return Err(format!("不支持的网页传输消息: {}", frame.kind)),
        }
        Ok(())
    }
}

async fn send(outbound: &mpsc::Sender<Value>, stream_id: &str, kind: &str, payload: Value) -> Result<(), String> {
    outbound.send(json!({"TYPE": kind, "STREAM_ID": stream_id, "PAYLOAD": payload})).await.map_err(|_| "客户端连接已断开".to_string())
}
