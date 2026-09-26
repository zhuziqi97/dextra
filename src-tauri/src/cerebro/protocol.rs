//! 客户端身份和目录事实的控制协议，业务请求复用完整 Web 服务。

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use super::FolderTargetProjection;

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Envelope {
    pub protocol_version: u32,
    #[serde(rename = "TYPE")]
    pub kind: &'static str,
    pub message_id: String,
    pub occurred_at: DateTime<Utc>,
    pub runner_id: String,
    pub payload: Value,
}

fn envelope(runner_id: impl Into<String>, kind: &'static str, payload: Value) -> Envelope {
    Envelope { protocol_version: 1, kind, message_id: uuid::Uuid::new_v4().to_string(), occurred_at: Utc::now(), runner_id: runner_id.into(), payload }
}

pub fn runner_hello(runner_id: impl Into<String>, build_id: impl Into<String>) -> Envelope {
    envelope(runner_id, "HELLO", json!({"RUNNER_BUILD_ID": build_id.into(), "DEXTRA_API_REVISION": 2, "DEXTRA_VERSION": concat!("v", env!("CARGO_PKG_VERSION"))}))
}

pub fn runner_heartbeat(runner_id: impl Into<String>) -> Envelope {
    envelope(runner_id, "HEARTBEAT", json!({}))
}

pub fn runner_targets_report(runner_id: impl Into<String>, targets: Vec<FolderTargetProjection>) -> Envelope {
    envelope(runner_id, "TARGETS_REPORT", json!({"TARGETS": targets}))
}
