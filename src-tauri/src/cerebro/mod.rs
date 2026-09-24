//! Dextra 与 Cerebro 的生产 adapter。
//!
//! 这里投影 Dextra 已有事实，不拥有 Folder、WorkTask 或 ACP 生命周期。

pub mod connection;
pub mod configuration;
pub mod credential_storage;
pub mod identity;
pub mod mcp;
pub mod mcp_bridge;
pub mod runtime;
pub mod protocol;
pub mod web_relay;
pub mod target_projection;

pub use connection::run_runner_connection_supervisor;
pub use identity::{
    cancel_pairing, forget_runner_credential, get_auth_state, poll_pairing, refresh_access_token,
    start_pairing, CerebroAuthState, CerebroMcpPrincipal, CerebroPairingPoll, CerebroPairingStart,
    CerebroRunnerAccess, PairingPollStatus,
};
pub use runtime::CerebroRuntime;

pub use target_projection::{
    project_folder_targets, FolderTargetProjection, TargetAvailability, TargetProjectionReason,
    TargetProjectionReasonCode,
};
