//! Per-stream credit; one pending frame per producer keeps the writer bounded and fair.
use futures_util::{Sink, SinkExt};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio_tungstenite::tungstenite::Message;

pub const WINDOW: usize = 16;
pub const CHUNK_BYTES: usize = 48 * 1024;
pub const MAX_WS_MESSAGE: usize = 64 * 1024 * 1024;

pub struct Flow {
    pub credit: Semaphore,
    receiving: AtomicUsize,
}
impl Flow {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            credit: Semaphore::new(WINDOW),
            receiving: AtomicUsize::new(WINDOW),
        })
    }
    pub async fn take(&self) -> Result<(), String> {
        self.credit
            .acquire()
            .await
            .map_err(|_| "网页流已关闭".to_string())?
            .forget();
        Ok(())
    }
    pub fn add(&self, n: usize) -> Result<(), String> {
        if n == 0 || n > WINDOW || self.credit.available_permits() + n > WINDOW {
            return Err("无效网页发送额度".into());
        }
        self.credit.add_permits(n);
        Ok(())
    }
    pub fn receive(&self, bytes: usize) -> Result<(), String> {
        if bytes > CHUNK_BYTES {
            return Err("网页分块超过大小限制".into());
        }
        self.receiving
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .map_err(|_| "网页流超出接收额度".to_string())?;
        Ok(())
    }
    pub fn consumed(&self) {
        self.receiving.fetch_add(1, Ordering::SeqCst);
    }
    pub fn close(&self) {
        self.credit.close();
    }
}

type Item = (Value, oneshot::Sender<Result<(), String>>);
#[derive(Clone)]
pub struct Outbound {
    control: mpsc::Sender<Item>,
    data: mpsc::Sender<Item>,
}
pub struct Writer {
    control: mpsc::Receiver<Item>,
    data: mpsc::Receiver<Item>,
}
impl Outbound {
    pub fn new() -> (Self, Writer) {
        let (control, control_rx) = mpsc::channel(128);
        let (data, data_rx) = mpsc::channel(128);
        (
            Self { control, data },
            Writer {
                control: control_rx,
                data: data_rx,
            },
        )
    }
    pub async fn send(&self, value: Value) -> Result<(), String> {
        let queue = if matches!(
            value["TYPE"].as_str(),
            Some("WEB_HTTP_BODY" | "WEB_HTTP_DATA" | "WEB_WS_DATA")
        ) {
            &self.data
        } else {
            &self.control
        };
        let (done, result) = oneshot::channel();
        queue
            .send((value, done))
            .await
            .map_err(|_| "客户端连接已断开".to_string())?;
        result.await.map_err(|_| "客户端连接已断开".to_string())?
    }
}
impl Writer {
    pub async fn run<S>(mut self, mut sink: S) -> Result<(), String>
    where
        S: Sink<Message> + Unpin,
        S::Error: std::fmt::Display,
    {
        loop {
            let item = tokio::select! {
                biased;
                item = self.control.recv() => item,
                item = self.data.recv() => item,
            };
            let Some((value, done)) = item else {
                return Ok(());
            };
            if done.is_closed() {
                continue;
            }
            let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
            let result = sink
                .send(Message::Text(text.into()))
                .await
                .map_err(|e| e.to_string());
            let failed = result.clone();
            let _ = done.send(result);
            failed?;
        }
    }
}
