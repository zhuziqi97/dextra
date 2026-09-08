//! 桌面与独立服务端共用同一 AppState 和完整 Web router。

use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub struct CerebroRuntime {
    pub(super) state: Arc<crate::app_state::AppState>,
    pub(super) web: Arc<super::web_relay::ClientWebRelay>,
}

impl CerebroRuntime {
    pub fn new(state: Arc<crate::app_state::AppState>, static_dir: PathBuf) -> Self {
        Self { web: Arc::new(super::web_relay::ClientWebRelay::new(state.clone(), static_dir)), state }
    }
}
