use serde::{Deserialize, Serialize};

/// 跨平台桥接层对外稳定的错误码；消息可补充事实，但调用方只依赖错误码分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BridgeErrorCode {
    ProtocolVersionUnsupported,
    CodegApiRevisionUnsupported,
    InvalidEnvelope,
    CodegCommandNotRemote,
    CodegCommandRemoteDenied,
    CodegCommandRequiresOperation,
    CodegChannelNotRemote,
    CoreOperationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
/// 协议边界错误，不把底层 ACP 或网络错误改写成成功结果。
pub struct BridgeError {
    pub code: BridgeErrorCode,
    pub message: String,
}

impl BridgeError {
    pub fn new(code: BridgeErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(BridgeErrorCode::InvalidEnvelope, message)
    }

    pub fn core(message: impl Into<String>) -> Self {
        Self::new(BridgeErrorCode::CoreOperationFailed, message)
    }
}
