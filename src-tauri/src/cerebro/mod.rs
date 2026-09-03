//! Dextra 与 Cerebro 的生产 adapter。
//!
//! 这里投影 Codeg 已有事实，不拥有 Folder、WorkTask 或 ACP 生命周期。

pub mod target_projection;

pub use target_projection::{
    project_folder_targets, FolderTargetProjection, TargetAvailability, TargetProjectionReason,
    TargetProjectionReasonCode,
};
