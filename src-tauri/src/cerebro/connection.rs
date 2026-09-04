//! Dextra 主动维护的 Cerebro Runner 生产 WebSocket。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::identity;
use super::remote::{CerebroRemoteRuntime, RemoteRelay};
use crate::cerebro_bridge::{runner_heartbeat, runner_hello, runner_targets_report};

const RUNNER_WEBSOCKET_PATH: &str = "api/v1/execution-runners/ws";
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const TARGET_REPORT_INTERVAL: Duration = Duration::from_secs(30);
const TASK_REPORT_INTERVAL: Duration = Duration::from_secs(2);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

static SUPERVISOR_STARTED: AtomicBool = AtomicBool::new(false);

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
    if !matches!(message_type, Some("HELLO_ACK" | "HEARTBEAT_ACK")) {
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

async fn connect_once(runtime: &CerebroRemoteRuntime, relay: &RemoteRelay) -> Result<(), String> {
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

    send_target_report(&runtime.db.conn, &mut sink, &access.runner_id).await?;

    let (remote_outbound, mut remote_source) = mpsc::channel::<Value>(128);

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.tick().await;
    let mut target_report = tokio::time::interval(TARGET_REPORT_INTERVAL);
    target_report.tick().await;
    let mut task_report = tokio::time::interval(TASK_REPORT_INTERVAL);
    task_report.tick().await;
    let mut task_sequence = 0_u64;
    loop {
        tokio::select! {
            _ = identity::wait_for_runner_identity_change() => {
                return Ok(());
            }
            _ = heartbeat.tick() => {
                send_json(&mut sink, &runner_heartbeat(&access.runner_id)).await?;
            }
            _ = target_report.tick() => {
                send_target_report(&runtime.db.conn, &mut sink, &access.runner_id).await?;
            }
            _ = task_report.tick() => {
                task_sequence += 1;
                for frame in super::task_protocol::linked_task_frames(
                    &runtime.db.conn,
                    &access.runner_id,
                    task_sequence,
                ).await? {
                    send_json(&mut sink, &frame).await?;
                }
            }
            incoming = source.next() => {
                let message = incoming
                    .ok_or_else(|| "Cerebro closed the Runner connection".to_string())?
                    .map_err(|error| format!("Runner WebSocket receive failed: {error}"))?;
                let value = parse_server_message(message)?;
                let message_type = value.get("TYPE").and_then(Value::as_str);
                if matches!(message_type, Some("TASK_START" | "TASK_CANCEL")) {
                    for response in super::task_protocol::handle_task_command(
                        &runtime.emitter,
                        &runtime.db.conn,
                        &access.runner_id,
                        value,
                    ).await? {
                        send_json(&mut sink, &response).await?;
                    }
                } else if relay
                    .handle_server_message(
                        &access.runner_id,
                        value.clone(),
                        remote_outbound.clone(),
                    )
                    .await
                {
                    continue;
                } else {
                    validate_server_message(&value, &access.runner_id)?;
                }
            }
            Some(outgoing) = remote_source.recv() => {
                send_json(&mut sink, &outgoing).await?;
            }
        }
    }
}

async fn run_supervisor(runtime: CerebroRemoteRuntime) {
    let relay = RemoteRelay::new(runtime.clone());
    loop {
        let state = identity::get_auth_state().await;
        match state {
            Ok(state) if state.paired => {
                if let Err(error) = connect_once(&runtime, &relay).await {
                    tracing::warn!("[cerebro] Runner connection ended: {error}");
                }
                relay.close_all().await;
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
pub async fn run_runner_connection_supervisor(runtime: CerebroRemoteRuntime) {
    if SUPERVISOR_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    run_supervisor(runtime).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract_example(message_type: &str) -> serde_json::Value {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/contracts/cerebro-runner-stream.schema.json"
        ))
        .unwrap();
        fixture["examples"][message_type].clone()
    }

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

    #[test]
    fn production_runner_serializers_match_cerebro_stream_contract() {
        let mut hello = runner_hello("00000000-0000-4000-8000-000000000000", "string");
        hello.message_id = "00000000-0000-4000-8000-000000000001".into();
        hello.occurred_at = "2026-01-01T00:00:00Z".parse().unwrap();
        assert_eq!(
            serde_json::to_value(hello).unwrap(),
            contract_example("HELLO")
        );

        let mut heartbeat = runner_heartbeat("00000000-0000-4000-8000-000000000000");
        heartbeat.message_id = "00000000-0000-4000-8000-000000000002".into();
        heartbeat.occurred_at = "2026-01-01T00:00:00Z".parse().unwrap();
        assert_eq!(
            serde_json::to_value(heartbeat).unwrap(),
            contract_example("HEARTBEAT")
        );

        let expected_report = contract_example("TARGETS_REPORT");
        let targets: Vec<crate::cerebro::FolderTargetProjection> =
            serde_json::from_value(expected_report["PAYLOAD"]["TARGETS"].clone()).unwrap();
        let mut report = runner_targets_report("00000000-0000-4000-8000-000000000000", targets);
        report.message_id = "00000000-0000-4000-8000-000000000004".into();
        report.occurred_at = "2026-01-01T00:00:00Z".parse().unwrap();
        assert_eq!(serde_json::to_value(report).unwrap(), expected_report);
    }
}
