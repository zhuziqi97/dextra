//! 客户端身份和目录事实的控制协议，业务请求复用完整 Web 服务。

use super::FolderTargetProjection;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};

pub const INTEGRATION_MAJOR: u64 = 5;
pub const INTEGRATION_MINOR: u64 = 0;
pub fn integration_protocol_header() -> String {
    format!("{INTEGRATION_MAJOR}.{INTEGRATION_MINOR}")
}

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
    Envelope {
        protocol_version: 1,
        kind,
        message_id: uuid::Uuid::new_v4().to_string(),
        occurred_at: Utc::now(),
        runner_id: runner_id.into(),
        payload,
    }
}

pub fn runner_hello(runner_id: impl Into<String>, build_id: impl Into<String>) -> Envelope {
    envelope(
        runner_id,
        "HELLO",
        json!({"RUNNER_BUILD_ID": build_id.into(), "SUPPORTED_PROTOCOLS": [{"MAJOR": INTEGRATION_MAJOR, "MAX_MINOR": INTEGRATION_MINOR}], "DEXTRA_VERSION": concat!("v", env!("CARGO_PKG_VERSION"))}),
    )
}

pub fn runner_heartbeat(runner_id: impl Into<String>) -> Envelope {
    envelope(runner_id, "HEARTBEAT", json!({}))
}

pub fn runner_targets_report(
    runner_id: impl Into<String>,
    targets: Vec<FolderTargetProjection>,
) -> Envelope {
    envelope(runner_id, "TARGETS_REPORT", json!({"TARGETS": targets}))
}
