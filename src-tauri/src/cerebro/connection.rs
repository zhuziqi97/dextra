//! Dextra 主动维护的 Cerebro Runner 生产 WebSocket。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex, Notify};
use serde::Serialize;
use tokio_tungstenite::tungstenite::Message;

use super::control_writer::Outbound;
use super::identity;
use super::protocol::{runner_heartbeat, runner_hello, runner_targets_report};
use super::runtime::CerebroRuntime;

const RUNNER_WEBSOCKET_PATH: &str = "api/v1/execution-runners/ws";
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const TARGET_REPORT_INTERVAL: Duration = Duration::from_secs(30);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

static SUPERVISOR_STARTED: AtomicBool = AtomicBool::new(false);
static CONNECTION_STATUS: LazyLock<RwLock<ConnectionStatus>> = LazyLock::new(|| RwLock::new(ConnectionStatus::default()));
static RETRY_CONNECTION: Notify = Notify::const_new();

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub status: &'static str,
    pub message: Option<String>,
    pub server_version: Option<String>,
    pub client_protocols: Vec<String>,
    pub server_protocols: Vec<String>,
    pub help_url: Option<String>,
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        Self { status: "OFFLINE", message: None, server_version: None,
            client_protocols: vec![super::protocol::integration_protocol_header()],
            server_protocols: Vec::new(), help_url: None }
    }
}

pub fn status() -> ConnectionStatus {
    CONNECTION_STATUS.read().expect("connection status lock poisoned").clone()
}

fn set_status(value: ConnectionStatus) {
    *CONNECTION_STATUS.write().expect("connection status lock poisoned") = value;
}

pub fn retry() {
    set_status(ConnectionStatus { status: "OFFLINE", ..Default::default() });
    RETRY_CONNECTION.notify_one();
}

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

pub(super) fn data_endpoint(base_url: &str) -> Result<reqwest::Url, String> {
    let mut url = websocket_endpoint(base_url)?;
    let path = url
        .path()
        .strip_suffix("/ws")
        .ok_or("Runner endpoint invalid")?
        .to_owned()
        + "/data";
    url.set_path(&path);
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

fn validate_server_message(
    value: &Value,
    expected_runner_id: &str,
    expected_connection_id: Option<&str>,
) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let message_type = value.get("TYPE").and_then(Value::as_str);
    if !matches!(
        message_type,
        Some("HELLO_ACK" | "HELLO_REJECT" | "HEARTBEAT_ACK" | "CONFIGURATION_CHANGED" | "TARGETS_REPORT_ACK" | "OPEN_DATA_CHANNEL" | "CANCEL_DATA_CHANNEL")
    ) {
        return Err("Cerebro returned an unsupported Runner message".to_string());
    }
    if matches!(message_type, Some("OPEN_DATA_CHANNEL" | "CANCEL_DATA_CHANNEL")) {
        if expected_connection_id.is_none()
            || value.get("CONNECTION_ID").and_then(Value::as_str) != expected_connection_id
            || value.get("REQUEST_ID").and_then(Value::as_str).filter(|id| !id.is_empty()).is_none()
        {
            return Err("Cerebro returned a data channel for another connection".to_string());
        }
        return Ok(());
    }
    if value.get("PROTOCOL_VERSION").and_then(Value::as_u64) != Some(1) {
        return Err("Cerebro returned an unsupported Runner protocol version".to_string());
    }
    if value.get("RUNNER_ID").and_then(Value::as_str) != Some(expected_runner_id) {
        return Err("Cerebro returned a mismatched Runner identity".to_string());
    }
    Ok(())
}

fn validate_selected_protocol(payload: &Value) -> Result<(), String> {
    if payload["HEARTBEAT_INTERVAL_SECONDS"].as_u64().filter(|value| *value > 0).is_none()
        || payload["SERVER_VERSION"].as_str().filter(|value| !value.is_empty()).is_none() {
        return Err("HELLO_ACK 缺少服务端版本或心跳间隔".into());
    }
    let selected = &payload["SELECTED_PROTOCOL"];
    let major = selected["MAJOR"].as_u64().ok_or("Selected protocol missing major")?;
    let minor = selected["MINOR"].as_u64().ok_or("Selected protocol missing minor")?;
    let offered = major == super::protocol::INTEGRATION_MAJOR
        && minor == super::protocol::INTEGRATION_MINOR;
    let supported = payload["SERVER_PROTOCOLS"].as_array()
        .ok_or("Server protocol list missing")?
        .iter().any(|entry| entry["MAJOR"].as_u64() == Some(major)
            && entry["MAX_MINOR"].as_u64().is_some_and(|maximum| maximum >= minor));
    if offered && supported { Ok(()) }
    else { Err("Cerebro selected an unsupported Dextra protocol".into()) }
}

async fn connect_once(runtime: &CerebroRuntime) -> Result<(), String> {
    set_status(ConnectionStatus { status: "CONNECTING", ..Default::default() });
    let access = identity::refresh_access_token()
        .await
        .map_err(|error| error.to_string())?;
    let data_access = std::sync::Arc::new(Mutex::new(super::web_relay::DataAccess::new(
        access.clone(),
    )));
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
    validate_server_message(&hello_ack, &access.runner_id, None)?;
    if hello_ack["TYPE"] == "HELLO_REJECT" {
        let payload = &hello_ack["PAYLOAD"];
        if payload["CODE"] == "PROTOCOL_INCOMPATIBLE" {
            let message = payload["ADVICE"].as_str().filter(|value| !value.is_empty())
                .ok_or("HELLO_REJECT 缺少升级建议")?.to_owned();
            let server_version = payload["SERVER_VERSION"].as_str().filter(|value| !value.is_empty())
                .ok_or("HELLO_REJECT 缺少平台版本")?.to_owned();
            let help_url = payload["HELP_URL"].as_str().filter(|value| !value.is_empty())
                .ok_or("HELLO_REJECT 缺少帮助入口")?.to_owned();
            let client_protocols = payload["CLIENT_PROTOCOLS"].as_array()
                .ok_or("HELLO_REJECT 缺少客户端协议集合")?;
            if client_protocols.is_empty() || client_protocols.iter().any(|version|
                version["MAJOR"].as_u64().is_none() || version["MAX_MINOR"].as_u64().is_none()) {
                return Err("HELLO_REJECT 客户端协议集合无效".into());
            }
            let versions = payload["SERVER_PROTOCOLS"].as_array()
                .ok_or("HELLO_REJECT 缺少平台协议集合")?;
            if versions.is_empty() { return Err("HELLO_REJECT 平台协议集合为空".into()); }
            let server_protocols: Vec<String> = versions.iter()
                .map(|version| {
                    let major = version["MAJOR"].as_u64().ok_or("HELLO_REJECT 协议 major 无效")?;
                    let minor = version["MAX_MINOR"].as_u64().ok_or("HELLO_REJECT 协议 minor 无效")?;
                    Ok::<_, String>(format!("{major}.{minor}"))
                }).collect::<Result<_, _>>()?;
            set_status(ConnectionStatus {
                status: "INCOMPATIBLE", message: Some(message.clone()),
                server_version: Some(server_version),
                client_protocols: vec![super::protocol::integration_protocol_header()],
                server_protocols,
                help_url: Some(help_url),
            });
            return Err(message);
        }
        return Err("Cerebro rejected the Runner HELLO".into());
    }
    if hello_ack.get("TYPE").and_then(Value::as_str) != Some("HELLO_ACK") {
        return Err("Cerebro did not acknowledge Runner HELLO".to_string());
    }
    let connection_id = hello_ack["PAYLOAD"]["CONNECTION_ID"]
        .as_str()
        .ok_or("HELLO_ACK 缺少连接身份")?
        .to_owned();
    validate_selected_protocol(&hello_ack["PAYLOAD"])?;
    set_status(ConnectionStatus { status: "ONLINE", ..Default::default() });
    tracing::info!(
        "[cerebro] Runner {} connected to {}",
        access.runner_id,
        access.cerebro_base_url
    );

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

    // Folder projection may inspect many paths and Git repositories. Keep it
    // outside the control receive loop, with exactly one worker per connection.
    let (report_tx, mut report_rx) = mpsc::channel::<Option<ReportWaiter>>(16);
    let report_waiters = std::sync::Arc::new(Mutex::new(HashMap::<
        String,
        (ReportWaiter, Option<ReportWaiter>),
    >::new()));
    let reports = report_waiters.clone();
    let report_db = runtime.state.db.conn.clone();
    let report_runner = access.runner_id.clone();
    let report_outbound = remote_outbound.clone();
    let mut report_task = tokio::spawn(async move {
        while let Some(waiter) = report_rx.recv().await {
            let targets = super::project_folder_targets(&report_db, &report_runner)
                .await
                .map_err(|error| error.to_string())?;
            let report = runner_targets_report(&report_runner, targets);
            if waiter.as_ref().is_some_and(ReportWaiter::is_closed) {
                continue;
            }
            let (acknowledge, acknowledged) = oneshot::channel();
            reports
                .lock()
                .await
                .insert(report.message_id.clone(), (acknowledge, waiter));
            report_outbound
                .send(serde_json::to_value(report).map_err(|e| e.to_string())?)
                .await?;
            acknowledged
                .await
                .map_err(|_| "目录上报未获确认".to_string())?;
        }
        Ok::<(), String>(())
    });
    let _report_guard = WriterGuard(report_task.abort_handle());
    let _ = report_tx.try_send(None);

    // Latest configuration per folder wins; the worker owns SQLite writes and
    // UI events, so neither a slow cache nor network refresh blocks heartbeat.
    let pending = std::sync::Arc::new(Mutex::new(HashMap::<
        String,
        super::configuration::ClientConfiguration,
    >::new()));
    let notice = std::sync::Arc::new(Notify::new());
    let config_db = runtime.state.db.conn.clone();
    let config_emitter = runtime.state.emitter.clone();
    let config_pending = pending.clone();
    let config_notice = notice.clone();
    let mut config_task = tokio::spawn(async move {
        loop {
            let wake = config_notice.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();
            let batch = std::mem::take(&mut *config_pending.lock().await);
            if batch.is_empty() {
                wake.await;
                continue;
            }
            for (_, configuration) in batch {
                if let Err(error) = super::configuration::cache(&config_db, &configuration).await {
                    tracing::warn!("[cerebro] 刷新目录缓存失败: {error}");
                }
                crate::web::event_bridge::emit_event(
                    &config_emitter,
                    super::configuration::CONFIGURATION_EVENT,
                    configuration,
                );
            }
        }
    });
    let _config_guard = WriterGuard(config_task.abort_handle());

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.tick().await;
    let mut target_report = tokio::time::interval(TARGET_REPORT_INTERVAL);
    target_report.tick().await;
    let mut report_requests = REPORT_REQUESTS.1.lock().await;
    let mut writer_done = false;
    let mut report_done = false;
    let mut config_done = false;
    let result = async {
        loop {
            tokio::select! {
            Some(waiter) = report_requests.recv(), if report_tx.capacity() > 0 => {
                if waiter.is_closed() { continue; }
                report_tx.try_send(Some(waiter)).map_err(|_| "目录上报队列暂不可用".to_string())?;
            }
            _ = identity::wait_for_runner_identity_change() => {
                return Ok(());
            }
            _ = heartbeat.tick() => {
                remote_outbound.send(serde_json::to_value(runner_heartbeat(&access.runner_id)).map_err(|e| e.to_string())?).await?;
            }
            _ = target_report.tick() => {
                let _ = report_tx.try_send(None);
            }
            incoming = source.next() => {
                let message = incoming
                    .ok_or_else(|| "Cerebro closed the Runner connection".to_string())?
                    .map_err(|error| format!("Runner WebSocket receive failed: {error}"))?;
                let value = parse_server_message(message)?;
                validate_server_message(&value, &access.runner_id, Some(&connection_id))?;
                let message_type = value.get("TYPE").and_then(Value::as_str);
                if runtime.web.handle(&value, &access.cerebro_base_url, &connection_id, data_access.clone()).await? {
                    continue;
                } else if message_type == Some("CONFIGURATION_CHANGED") {
                    let configuration: super::configuration::ClientConfiguration = serde_json::from_value(value["PAYLOAD"]["CONFIGURATION"].clone()).map_err(|e| e.to_string())?;
                    if configuration.runner_id != access.runner_id { return Err("配置不属于当前客户端".into()); }
                    pending.lock().await.insert(configuration.target_id.clone(), configuration);
                    notice.notify_one();
                } else if message_type == Some("TARGETS_REPORT_ACK") {
                    let configurations: Vec<super::configuration::ClientConfiguration> = serde_json::from_value(value["PAYLOAD"]["CONFIGURATIONS"].clone()).map_err(|error| error.to_string())?;
                    let report_id = value["PAYLOAD"]["REPORT_ID"].as_str().ok_or("目录确认缺少报告 ID")?;
                    if let Some((acknowledge, waiter)) = report_waiters.lock().await.remove(report_id) {
                        let _ = acknowledge.send(());
                        if let Some(waiter) = waiter { let _ = waiter.send(()); }
                    }
                    for configuration in configurations {
                        if configuration.runner_id != access.runner_id { return Err("目录配置不属于当前客户端".into()); }
                        pending.lock().await.insert(configuration.target_id.clone(), configuration);
                    }
                    notice.notify_one();
                }
            }
            result = &mut writer_task => {
                writer_done = true;
                return result.map_err(|e| e.to_string())?;
            }
            result = &mut report_task => {
                report_done = true;
                return result.map_err(|e| e.to_string())?;
            }
            result = &mut config_task => {
                config_done = true;
                return Err(match result {
                    Ok(()) => "配置缓存任务已退出".to_string(),
                    Err(error) => format!("配置缓存任务已退出: {error}"),
                });
            }
            }
        }
    }.await;
    if !report_done {
        report_task.abort();
        let _ = report_task.await;
    }
    if !config_done {
        config_task.abort();
        let _ = config_task.await;
    }
    if !writer_done {
        writer_task.abort();
        let _ = writer_task.await;
    }
    result
}

async fn run_supervisor(runtime: CerebroRuntime) {
    loop {
        let state = identity::get_auth_state().await;
        match state {
            Ok(state) if state.paired => {
                match connect_once(&runtime).await {
                    Ok(()) => set_status(ConnectionStatus::default()),
                    Err(error) => {
                        tracing::warn!("[cerebro] Runner connection ended: {error}");
                        if status().status != "INCOMPATIBLE" {
                            set_status(ConnectionStatus { status: "OFFLINE", message: Some(error), ..Default::default() });
                        }
                    }
                }
                runtime.web.close_all().await;
                if status().status == "INCOMPATIBLE" {
                    tokio::select! {
                        _ = RETRY_CONNECTION.notified() => {}
                        _ = identity::wait_for_runner_identity_change() => { retry(); }
                    }
                    continue;
                }
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
    fn data_url_uses_same_deployment_prefix_as_control() {
        assert_eq!(
            data_endpoint("https://cerebro.internal/nested/?tenant=dev#ignored")
                .unwrap()
                .as_str(),
            "wss://cerebro.internal/nested/api/v1/execution-runners/data?tenant=dev"
        );
    }

    #[test]
    fn server_ack_requires_the_current_runner_identity() {
        let ack = json!({
            "PROTOCOL_VERSION": 1,
            "TYPE": "HELLO_ACK",
            "RUNNER_ID": "runner-1",
        });
        validate_server_message(&ack, "runner-1", None).unwrap();
        assert!(validate_server_message(&ack, "runner-2", None).is_err());
        let open = json!({
            "TYPE": "OPEN_DATA_CHANNEL", "CONNECTION_ID": "connection-1",
            "REQUEST_ID": "request-1",
        });
        validate_server_message(&open, "runner-1", Some("connection-1")).unwrap();
        assert!(validate_server_message(&open, "runner-1", Some("connection-2")).is_err());
        assert!(validate_server_message(&open, "runner-1", None).is_err());
        let mut missing_request = open.clone();
        missing_request.as_object_mut().unwrap().remove("REQUEST_ID");
        assert!(validate_server_message(&missing_request, "runner-1", Some("connection-1")).is_err());
    }

    #[test]
    fn selected_protocol_must_be_in_both_supported_sets() {
        let valid = json!({"SELECTED_PROTOCOL": {"MAJOR": 5, "MINOR": 0},
            "HEARTBEAT_INTERVAL_SECONDS": 15, "SERVER_VERSION": "5.13.1",
            "SERVER_PROTOCOLS": [{"MAJOR": 5, "MAX_MINOR": 1}]});
        assert!(validate_selected_protocol(&valid).is_ok());
        let wrong_server = json!({"SELECTED_PROTOCOL": {"MAJOR": 5, "MINOR": 0},
            "HEARTBEAT_INTERVAL_SECONDS": 15, "SERVER_VERSION": "5.13.1",
            "SERVER_PROTOCOLS": [{"MAJOR": 6, "MAX_MINOR": 0}]});
        assert!(validate_selected_protocol(&wrong_server).is_err());
        let future_minor = json!({"SELECTED_PROTOCOL": {"MAJOR": 5, "MINOR": 1},
            "HEARTBEAT_INTERVAL_SECONDS": 15, "SERVER_VERSION": "5.13.1",
            "SERVER_PROTOCOLS": [{"MAJOR": 5, "MAX_MINOR": 1}]});
        assert!(validate_selected_protocol(&future_minor).is_err());
        let mut missing = valid;
        missing.as_object_mut().unwrap().remove("SERVER_VERSION");
        assert!(validate_selected_protocol(&missing).is_err());
    }
}
