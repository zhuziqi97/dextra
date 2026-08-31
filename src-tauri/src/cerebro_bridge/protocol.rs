use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{BridgeError, BridgeErrorCode};
use super::registry::registry;

/// P0-C 验证所需的最小消息集合；具体 Codeg DTO 仍封装在 PAYLOAD 内。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MessageType {
    Hello,
    // acp_connect 是创建执行会话的写操作，必须先经过平台 SESSION_CREATE。
    SessionOpen,
    TaskStart,
    TaskCancel,
    SessionClose,
    ApprovalDecision,
    CodegRpcRequest,
    CodegChannelSubscribe,
    CodegChannelUnsubscribe,
    CodegStreamAttach,
    CodegStreamDetach,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// Runner 协议的统一外层信封，作用域字段只在对应消息类型出现。
pub struct Envelope {
    pub protocol_version: u32,
    #[serde(rename = "TYPE")]
    pub message_type: MessageType,
    pub message_id: String,
    pub occurred_at: DateTime<Utc>,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

impl Envelope {
    /// 在进入命令路由前校验版本和当前消息类型真正需要的作用域字段。
    pub fn validate(&self) -> Result<(), BridgeError> {
        if self.protocol_version != registry().protocol_version {
            return Err(BridgeError::new(
                BridgeErrorCode::ProtocolVersionUnsupported,
                format!(
                    "expected protocol {}, received {}",
                    registry().protocol_version,
                    self.protocol_version
                ),
            ));
        }
        require(&self.message_id, "MESSAGE_ID")?;
        require_option(&self.runner_id, "RUNNER_ID")?;
        if self.message_type == MessageType::Hello {
            return Ok(());
        }
        require_option(&self.correlation_id, "CORRELATION_ID")?;
        require_option(&self.command_id, "COMMAND_ID")?;

        match self.message_type {
            MessageType::SessionOpen => {
                require_option(&self.target_id, "TARGET_ID")?;
                require_option(&self.session_id, "SESSION_ID")?;
            }
            MessageType::TaskStart => {
                require_option(&self.target_id, "TARGET_ID")?;
                require_option(&self.session_id, "SESSION_ID")?;
                require_option(&self.task_id, "TASK_ID")?;
            }
            MessageType::TaskCancel => {
                require_option(&self.task_id, "TASK_ID")?;
            }
            MessageType::SessionClose
            | MessageType::ApprovalDecision
            | MessageType::CodegStreamAttach
            | MessageType::CodegStreamDetach => {
                require_option(&self.session_id, "SESSION_ID")?;
            }
            MessageType::CodegRpcRequest
            | MessageType::CodegChannelSubscribe
            | MessageType::CodegChannelUnsubscribe => {
                require_option(&self.target_id, "TARGET_ID")?;
            }
            MessageType::Hello => unreachable!("handled above"),
        }
        Ok(())
    }
}

fn require(value: &str, field: &str) -> Result<(), BridgeError> {
    if value.trim().is_empty() {
        return Err(BridgeError::invalid(format!("{field} is required")));
    }
    Ok(())
}

fn require_option(value: &Option<String>, field: &str) -> Result<(), BridgeError> {
    match value {
        Some(value) => require(value, field),
        None => Err(BridgeError::invalid(format!("{field} is required"))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct RunnerHelloPayload {
    pub runner_build_id: String,
    pub codeg_api_revision: u32,
    pub codeg_upstream_version: String,
    pub codeg_upstream_commit: String,
}

pub fn runner_hello(runner_id: impl Into<String>, build_id: impl Into<String>) -> Envelope {
    let registry = registry();
    Envelope {
        protocol_version: registry.protocol_version,
        message_type: MessageType::Hello,
        message_id: uuid::Uuid::new_v4().to_string(),
        occurred_at: Utc::now(),
        payload: serde_json::to_value(RunnerHelloPayload {
            runner_build_id: build_id.into(),
            codeg_api_revision: registry.codeg_api_revision,
            codeg_upstream_version: registry.codeg_upstream_version.clone(),
            codeg_upstream_commit: registry.codeg_upstream_commit.clone(),
        })
        .expect("RunnerHelloPayload serialization cannot fail"),
        runner_id: Some(runner_id.into()),
        correlation_id: None,
        command_id: None,
        target_id: None,
        session_id: None,
        task_id: None,
    }
}

pub fn negotiate(protocol_version: u32, codeg_api_revision: u32) -> Result<(), BridgeError> {
    let registry = registry();
    if protocol_version != registry.protocol_version {
        return Err(BridgeError::new(
            BridgeErrorCode::ProtocolVersionUnsupported,
            format!(
                "expected protocol {}, received {protocol_version}",
                registry.protocol_version
            ),
        ));
    }
    if codeg_api_revision != registry.codeg_api_revision {
        return Err(BridgeError::new(
            BridgeErrorCode::CodegApiRevisionUnsupported,
            format!(
                "expected Codeg API revision {}, received {codeg_api_revision}",
                registry.codeg_api_revision
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_reports_the_fixed_vendor_boundary_without_null_scope_fields() {
        let hello = runner_hello("runner-1", "dextra-spike");
        hello.validate().unwrap();
        let value = serde_json::to_value(hello).unwrap();
        assert_eq!(value["TYPE"], "HELLO");
        assert_eq!(value["PAYLOAD"]["CODEG_API_REVISION"], 1);
        assert_eq!(value["PAYLOAD"]["CODEG_UPSTREAM_VERSION"], "v0.29.0");
        assert!(value.get("TASK_ID").is_none());
        assert!(value.get("COMMAND_ID").is_none());
    }

    #[test]
    fn incompatible_revision_fails_with_a_stable_error() {
        let error = negotiate(1, 2).unwrap_err();
        assert_eq!(error.code, BridgeErrorCode::CodegApiRevisionUnsupported);
    }
}
