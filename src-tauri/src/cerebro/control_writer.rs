//! Bounded writer for control messages only.
use futures_util::{Sink, SinkExt};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
type Item = (Value, oneshot::Sender<Result<(), String>>);
#[derive(Clone)]
pub struct Outbound {
    control: mpsc::Sender<Item>,
}
pub struct Writer {
    control: mpsc::Receiver<Item>,
}
impl Outbound {
    pub fn new() -> (Self, Writer) {
        let (control, control_rx) = mpsc::channel(128);
        (
            Self { control },
            Writer {
                control: control_rx,
            },
        )
    }
    pub async fn send(&self, value: Value) -> Result<(), String> {
        let (done, result) = oneshot::channel();
        self.control
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
            let item = self.control.recv().await;
            let Some((value, done)) = item else {
                return Ok(());
            };
            if done.is_closed() {
                continue;
            }
            let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
            let result = match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                sink.send(Message::Text(text.into())),
            )
            .await
            {
                Ok(result) => result.map_err(|e| e.to_string()),
                Err(_) => Err("Runner 控制消息写入超时".to_string()),
            };
            let failed = result.clone();
            let _ = done.send(result);
            failed?;
        }
    }
}
