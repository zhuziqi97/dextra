//! Dextra 与 Cerebro 的生产 adapter。
//!
//! 这里投影 Codeg 已有事实，不拥有 Folder、WorkTask 或 ACP 生命周期。

pub mod connection;
pub mod identity;
pub mod target_projection;
pub mod task_link;

pub use connection::run_runner_connection_supervisor;
pub use identity::{
    cancel_pairing, forget_runner_credential, get_auth_state, poll_pairing, refresh_access_token,
    start_pairing, CerebroAuthState, CerebroPairingPoll, CerebroPairingStart, CerebroRunnerAccess,
    PairingPollStatus,
};

pub use target_projection::{
    project_folder_targets, FolderTargetProjection, TargetAvailability, TargetProjectionReason,
    TargetProjectionReasonCode,
};
pub use task_link::{
    cancel_linked_work_task, create_linked_work_task, reconcile_linked_work_task,
    LinkedWorkTaskCancelOutcome, LinkedWorkTaskCancelReceipt, LinkedWorkTaskCreateOutcome,
    LinkedWorkTaskSnapshot, LinkedWorkTaskState,
};
