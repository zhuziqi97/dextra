//! Dextra 主动维护的 Cerebro Runner 生产 WebSocket。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;

use super::identity;
use super::protocol::{runner_heartbeat, runner_hello, runner_targets_report};
use super::runtime::CerebroRuntime;
use super::web_flow::Outbound;

const RUNNER_WEBSOCKET_PATH: &str = "api/v1/execution-runners/ws";
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const TARGET_REPORT_INTERVAL: Duration = Duration::from_secs(30);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

static SUPERVISOR_STARTED: AtomicBool = AtomicBool::new(false);

type ReportWaiter = oneshot::Sender<()>;
static REPORT_REQUESTS: LazyLock<(
    mpsc::Sender<ReportWaiter>,
    Mutex<mpsc::Receiver<ReportWaiter>>,
)> = LazyLock::new(|| {
    let (sender, receiver) = mpsc::channel(16);
    (sender, Mutex::new(receiver))
});

/// 新目录配置等待本次目录上报确认，不依赖周期上报或固定延时。
pub async fn synchronize_targets() -> Result<(), String> {
    if !SUPERVISOR_STARTED.load(Ordering::Acquire) {
        return Ok(());
    }
    let (sender, receiver) = oneshot::channel();
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        REPORT_REQUESTS
            .0
            .send(sender)
            .await
            .map_err(|_| "客户端连接已关闭".to_string())?;
        receiver.await.map_err(|_| "客户端连接已断开".to_string())
    })
    .await
    .map_err(|_| "服务端尚未确认目录，请恢复连接后重试".to_string())?
}

fn websocket_endpoint(base_url: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|error| format!("Invalid stored Cerebro URL: {error}"))?;
    let websocket_scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        scheme => return Err(format!("Unsupported Cerebro URL scheme: {scheme}")),
    };
    url.set_scheme(websocket_scheme)
        .map_err(|_| "Failed to convert Cerebro URL to WebSocket".to_string())?;
    let base_path = url.path().trim_end_matches('/');
    url.set_path(&format!("{base_path}/{RUNNER_WEBSOCKET_PATH}"));
    url.set_fragment(None);
    Ok(url)
}

async fn send_json<S>(sink: &mut S, value: &impl serde::Serialize) -> Result<(), String>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let serialized = serde_json::to_string(value)
        .map_err(|error| format!("Failed to encode Runner message: {error}"))?;
    sink.send(Message::Text(serialized.into()))
        .await
        .map_err(|error| format!("Failed to send Runner message: {error}"))
}

fn parse_server_message(message: Message) -> Result<Value, String> {
    match message {
        Message::Text(text) => serde_json::from_str(text.as_ref())
            .map_err(|error| format!("Cerebro returned an invalid Runner message: {error}")),
        Message::Close(frame) => Err(frame.map_or_else(
            || "Cerebro closed the Runner connection".to_string(),
            |frame| format!("Cerebro closed the Runner connection: {}", frame.reason),
        )),
        Message::Ping(_) | Message::Pong(_) => Ok(Value::Null),
        Message::Binary(_) | Message::Frame(_) => {
            Err("Cerebro returned an unsupported Runner frame".to_string())
        }
    }
}

fn validate_server_message(value: &Value, expected_runner_id: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let message_type = value.get("TYPE").and_then(Value::as_str);
    if !matches!(
        message_type,
        Some("HELLO_ACK" | "HEARTBEAT_ACK" | "CONFIGURATION_CHANGED" | "TARGETS_REPORT_ACK")
    ) {
        return Err("Cerebro returned an unsupported Runner message".to_string());
    }
    if value.get("PROTOCOL_VERSION").and_then(Value::as_u64) != Some(1) {
        return Err("Cerebro returned an unsupported Runner protocol version".to_string());
    }
    if value.get("RUNNER_ID").and_then(Value::as_str) != Some(expected_runner_id) {
        return Err("Cerebro returned a mismatched Runner identity".to_string());
    }
    Ok(())
}

async fn send_target_report<S>(
    conn: &sea_orm::DatabaseConnection,
    sink: &mut S,
    runner_id: &str,
) -> Result<(), String>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let targets = super::project_folder_targets(conn, runner_id)
        .await
        .map_err(|error| format!("Failed to project Dextra Folder Targets: {error}"))?;
    send_json(sink, &runner_targets_report(runner_id, targets)).await
}

async fn connect_once(runtime: &CerebroRuntime) -> Result<(), String> {
    let access = identity::refresh_access_token()
        .await
        .map_err(|error| error.to_string())?;
    let endpoint = websocket_endpoint(&access.cerebro_base_url)?;
    let (stream, _) = tokio_tungstenite::connect_async(endpoint.as_str())
        .await
        .map_err(|error| format!("Failed to connect to Cerebro Runner WebSocket: {error}"))?;
    let (mut sink, mut source) = stream.split();

    send_json(
        &mut sink,
        &json!({
            "TYPE": "AUTHENTICATE",
            "TOKEN": access.access_token,
        }),
    )
    .await?;
    send_json(
        &mut sink,
        &runner_hello(&access.runner_id, env!("CARGO_PKG_VERSION")),
    )
    .await?;

    let hello_ack = tokio::time::timeout(HANDSHAKE_TIMEOUT, source.next())
        .await
        .map_err(|_| "Cerebro Runner HELLO timed out".to_string())?
        .ok_or_else(|| "Cerebro closed before acknowledging Runner HELLO".to_string())?
        .map_err(|error| format!("Failed to receive Runner HELLO response: {error}"))?;
    let hello_ack = parse_server_message(hello_ack)?;
    validate_server_message(&hello_ack, &access.runner_id)?;
    if hello_ack.get("TYPE").and_then(Value::as_str) != Some("HELLO_ACK") {
        return Err("Cerebro did not acknowledge Runner HELLO".to_string());
    }
    tracing::info!(
        "[cerebro] Runner {} connected to {}",
        access.runner_id,
        access.cerebro_base_url
    );

    send_target_report(&runtime.state.db.conn, &mut sink, &access.runner_id).await?;

    let (remote_outbound, writer) = Outbound::new();
    let mut writer_task = tokio::spawn(writer.run(sink));
    // Dropping this guard also aborts the writer when the connection future is cancelled.
    struct WriterGuard(tokio::task::AbortHandle);
    impl Drop for WriterGuard {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _writer_guard = WriterGuard(writer_task.abort_handle());

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.tick().await;
    let mut target_report = tokio::time::interval(TARGET_REPORT_INTERVAL);
    target_report.tick().await;
    let mut report_requests = REPORT_REQUESTS.1.lock().await;
    let mut report_waiters = HashMap::<String, ReportWaiter>::new();
    loop {
        tokio::select! {
            Some(waiter) = report_requests.recv() => {
                if waiter.is_closed() { continue; }
                let targets = super::project_folder_targets(&runtime.state.db.conn, &access.runner_id).await.map_err(|error| error.to_string())?;
                let report = runner_targets_report(&access.runner_id, targets);
                let report_id = report.message_id.clone();
                report_waiters.retain(|_, pending| !pending.is_closed());
                report_waiters.insert(report_id, waiter);
                remote_outbound.send(serde_json::to_value(report).map_err(|e| e.to_string())?).await?;
            }
            _ = identity::wait_for_runner_identity_change() => {
                return Ok(());
            }
            _ = heartbeat.tick() => {
                remote_outbound.send(serde_json::to_value(runner_heartbeat(&access.runner_id)).map_err(|e| e.to_string())?).await?;
            }
            _ = target_report.tick() => {
                let targets = super::project_folder_targets(&runtime.state.db.conn, &access.runner_id).await.map_err(|e| e.to_string())?;
                remote_outbound.send(serde_json::to_value(runner_targets_report(&access.runner_id, targets)).map_err(|e| e.to_string())?).await?;
            }
            incoming = source.next() => {
                let message = incoming
                    .ok_or_else(|| "Cerebro closed the Runner connection".to_string())?
                    .map_err(|error| format!("Runner WebSocket receive failed: {error}"))?;
                let value = parse_server_message(message)?;
                let message_type = value.get("TYPE").and_then(Value::as_str);
                if runtime.web.handle(value.clone(), remote_outbound.clone()).await {
                    continue;
                } else if message_type == Some("CONFIGURATION_CHANGED") {
                    validate_server_message(&value, &access.runner_id)?;
                    let target_id = value["PAYLOAD"]["TARGET_ID"].as_str().ok_or("配置刷新缺少目录")?;
                    super::configuration::refresh_and_emit(&runtime.state.db.conn, &runtime.state.emitter, target_id).await;
                } else if message_type == Some("TARGETS_REPORT_ACK") {
                    validate_server_message(&value, &access.runner_id)?;
                    let configurations: Vec<super::configuration::ClientConfiguration> = serde_json::from_value(value["PAYLOAD"]["CONFIGURATIONS"].clone()).map_err(|error| error.to_string())?;
                    if let Some(report_id) = value["PAYLOAD"]["REPORT_ID"].as_str() {
                        if let Some(waiter) = report_waiters.remove(report_id) { let _ = waiter.send(()); }
                    }
                    for configuration in configurations {
                        if let Err(error) = super::configuration::cache(&runtime.state.db.conn, &configuration).await {
                            tracing::warn!("[cerebro] 刷新目录缓存失败: {error}");
                        }
                        crate::web::event_bridge::emit_event(&runtime.state.emitter, super::configuration::CONFIGURATION_EVENT, configuration);
                    }
                } else {
                    validate_server_message(&value, &access.runner_id)?;
                }
            }
            result = &mut writer_task => {
                return result.map_err(|e| e.to_string())?;
            }
        }
    }
}

async fn run_supervisor(runtime: CerebroRuntime) {
    loop {
        let state = identity::get_auth_state().await;
        match state {
            Ok(state) if state.paired => {
                if let Err(error) = connect_once(&runtime).await {
                    tracing::warn!("[cerebro] Runner connection ended: {error}");
                }
                runtime.web.close_all().await;
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!("[cerebro] Failed to read Runner identity: {error}");
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(RECONNECT_DELAY) => {}
            _ = identity::wait_for_runner_identity_change() => {}
        }
    }
}

/// 运行当前 Dextra 进程唯一的 Runner 连接监督任务。
pub async fn run_runner_connection_supervisor(runtime: CerebroRuntime) {
    if SUPERVISOR_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    run_supervisor(runtime).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_url_preserves_deployment_path_and_query() {
        let url =
            websocket_endpoint("http://cerebro.internal:9999/nested/?tenant=dev#ignored").unwrap();
        assert_eq!(
            url.as_str(),
            "ws://cerebro.internal:9999/nested/api/v1/execution-runners/ws?tenant=dev"
        );
    }

    #[test]
    fn server_ack_requires_the_current_runner_identity() {
        let ack = json!({
            "PROTOCOL_VERSION": 1,
            "TYPE": "HELLO_ACK",
            "RUNNER_ID": "runner-1",
        });
        validate_server_message(&ack, "runner-1").unwrap();
        assert!(validate_server_message(&ack, "runner-2").is_err());
    }
}
