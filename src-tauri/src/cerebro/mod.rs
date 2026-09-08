//! Dextra 与 Cerebro 的生产 adapter。
//!
//! 这里投影 Codeg 已有事实，不拥有 Folder、WorkTask 或 ACP 生命周期。

pub mod connection;
pub mod credential_storage;
pub mod identity;
pub mod mcp;
pub mod mcp_bridge;
pub mod remote;
pub mod session_binding;
pub mod target_projection;
pub mod task_link;
pub mod task_protocol;

pub use connection::run_runner_connection_supervisor;
pub use identity::{
    cancel_pairing, forget_runner_credential, get_auth_state, poll_pairing, refresh_access_token,
    start_pairing, CerebroAuthState, CerebroMcpPrincipal, CerebroPairingPoll, CerebroPairingStart,
    CerebroRunnerAccess, PairingPollStatus,
};
pub use remote::CerebroRemoteRuntime;

pub use target_projection::{
    project_folder_targets, FolderTargetProjection, TargetAvailability, TargetProjectionReason,
    TargetProjectionReasonCode,
};
pub use task_link::{
    cancel_linked_work_task, create_linked_work_task, reconcile_all_linked_work_tasks,
    reconcile_linked_work_task, LinkedWorkTaskCancelOutcome, LinkedWorkTaskCancelReceipt,
    LinkedWorkTaskCreateOutcome, LinkedWorkTaskSnapshot, LinkedWorkTaskState,
};
