//! Cerebro Runner Task 命令与状态回传的生产 DTO/adapter。

use chrono::{DateTime, Utc};
use sea_orm::{DatabaseConnection, EntityTrait};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::target_projection::resolve_background_task_folder;
use super::{
    create_linked_work_task, reconcile_all_linked_work_tasks, LinkedWorkTaskCreateOutcome,
    LinkedWorkTaskSnapshot,
};
use crate::models::WorkTaskDraft;
use crate::web::event_bridge::EventEmitter;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub struct TaskCommandEnvelope {
    pub protocol_version: u32,
    #[serde(rename = "TYPE")]
    pub message_type: TaskCommandType,
    pub message_id: String,
    pub occurred_at: DateTime<Utc>,
    pub runner_id: String,
    pub correlation_id: String,
    pub command_id: String,
    pub sequence: u64,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskCommandType {
    TaskStart,
    TaskCancel,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
struct TaskStartPayload {
    task_id: String,
    target_id: String,
    binding_id: String,
    module_id: String,
    owner_user_id: String,
    prompt: String,
    mcp_scope: TaskMcpScope,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
struct TaskMcpScope {
    authorized_module_ids: Vec<String>,
    allowed_capability_ids: Vec<String>,
    claim_endpoint: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
struct TaskCancelPayload {
    task_id: String,
    reason: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum AckStatus {
    Accepted,
    Duplicate,
    Rejected,
}

fn envelope(runner_id: &str, message_type: &str, sequence: u64, payload: Value) -> Value {
    json!({
        "PROTOCOL_VERSION": 1,
        "TYPE": message_type,
        "MESSAGE_ID": uuid::Uuid::new_v4().to_string(),
        "OCCURRED_AT": Utc::now(),
        "RUNNER_ID": runner_id,
        "CORRELATION_ID": null,
        "COMMAND_ID": null,
        "SEQUENCE": sequence,
        "PAYLOAD": payload,
    })
}

fn ack(
    command: &TaskCommandEnvelope,
    status: AckStatus,
    local_ref: Option<String>,
    error: Option<Value>,
) -> Value {
    let mut value = envelope(
        &command.runner_id,
        "COMMAND_ACK",
        command.sequence,
        json!({
            "TASK_ID": command.payload["TASK_ID"],
            "ACK_STATUS": status,
            "LOCAL_WORK_TASK_REF": local_ref,
            "ERROR": error,
        }),
    );
    value["CORRELATION_ID"] = json!(command.message_id);
    value["COMMAND_ID"] = json!(command.command_id);
    value
}

fn rejection(command: &TaskCommandEnvelope, code: &str, message: String) -> Value {
    ack(
        command,
        AckStatus::Rejected,
        None,
        Some(json!({
            "CODE": code,
            "MESSAGE": message,
            "RETRYABLE": false,
            "DETAILS": {},
        })),
    )
}

/// 解析并提交一个可靠命令，只有本地事实提交后才返回 ACK。
pub async fn handle_task_command(
    emitter: &EventEmitter,
    conn: &DatabaseConnection,
    runner_id: &str,
    value: Value,
) -> Result<Vec<Value>, String> {
    let command: TaskCommandEnvelope = serde_json::from_value(value)
        .map_err(|error| format!("Invalid Cerebro Task command: {error}"))?;
    if command.protocol_version != 1 || command.runner_id != runner_id {
        return Err("Cerebro Task command identity or version mismatch".into());
    }
    if command.message_id.trim().is_empty()
        || command.correlation_id.trim().is_empty()
        || command.command_id.trim().is_empty()
        || command.sequence == 0
    {
        return Err("Cerebro Task command is missing reliable identity".into());
    }

    match command.message_type {
        TaskCommandType::TaskStart => {
            let payload: TaskStartPayload = serde_json::from_value(command.payload.clone())
                .map_err(|error| format!("Invalid TASK_START payload: {error}"))?;
            // 已提交关联的补发只恢复事实，不重新判断首次接收期限和当前 Folder 可用性。
            if crate::db::entities::cerebro_task_link::Entity::find_by_id(&payload.task_id)
                .one(conn)
                .await
                .map_err(|error| error.to_string())?
                .is_some()
            {
                let snapshot = super::reconcile_linked_work_task(conn, &payload.task_id)
                    .await
                    .map_err(|error| error.to_string())?;
                let mut responses = vec![ack(
                    &command,
                    AckStatus::Duplicate,
                    Some(snapshot.local_work_task_id.to_string()),
                    None,
                )];
                responses.extend(task_projection(runner_id, command.sequence, snapshot));
                return Ok(responses);
            }
            if payload.expires_at <= Utc::now() {
                return Ok(vec![rejection(
                    &command,
                    "TASK_EXPIRED",
                    "Task has expired".into(),
                )]);
            }
            if payload.prompt.trim().is_empty()
                || payload.mcp_scope.authorized_module_ids != vec![payload.module_id.clone()]
                || payload.mcp_scope.allowed_capability_ids.is_empty()
                || payload.mcp_scope.claim_endpoint.trim().is_empty()
                || payload.binding_id.trim().is_empty()
                || payload.owner_user_id.trim().is_empty()
            {
                return Ok(vec![rejection(
                    &command,
                    "INVALID_ENVELOPE",
                    "TASK_START scope is invalid".into(),
                )]);
            }
            let folder_id =
                match resolve_background_task_folder(conn, runner_id, &payload.target_id).await {
                    Ok(folder_id) => folder_id,
                    Err(error) => {
                        return Ok(vec![rejection(
                            &command,
                            "TARGET_UNAVAILABLE",
                            error.to_string(),
                        )])
                    }
                };
            let draft = WorkTaskDraft {
                folder_id,
                title: payload.prompt.chars().take(120).collect(),
                config: json!({
                    "display_text": payload.prompt,
                    "prompt_blocks": [{"type": "text", "text": payload.prompt}],
                    "cerebro_task": {
                        "task_id": payload.task_id,
                        "binding_id": payload.binding_id,
                        "module_id": payload.module_id,
                        "claim_endpoint": payload.mcp_scope.claim_endpoint,
                    }
                }),
            };
            match create_linked_work_task(emitter, conn, &payload.task_id, draft).await {
                Ok(LinkedWorkTaskCreateOutcome::Created(task)) => Ok(vec![
                    ack(
                        &command,
                        AckStatus::Accepted,
                        Some(task.id.to_string()),
                        None,
                    ),
                    task_state(
                        runner_id,
                        command.sequence,
                        &payload.task_id,
                        "QUEUED",
                        None,
                    ),
                ]),
                Ok(LinkedWorkTaskCreateOutcome::Duplicate(task)) => {
                    let snapshot = super::reconcile_linked_work_task(conn, &payload.task_id)
                        .await
                        .map_err(|error| error.to_string())?;
                    let mut responses = vec![ack(
                        &command,
                        AckStatus::Duplicate,
                        Some(task.id.to_string()),
                        None,
                    )];
                    responses.extend(task_projection(runner_id, command.sequence, snapshot));
                    Ok(responses)
                }
                Err(error) => Ok(vec![rejection(
                    &command,
                    "WORK_TASK_CREATE_FAILED",
                    error.to_string(),
                )]),
            }
        }
        TaskCommandType::TaskCancel => {
            let payload: TaskCancelPayload = serde_json::from_value(command.payload.clone())
                .map_err(|error| format!("Invalid TASK_CANCEL payload: {error}"))?;
            let Some(engine) = crate::work_task::engine() else {
                return Ok(vec![rejection(
                    &command,
                    "CORE_OPERATION_FAILED",
                    "Task engine is not running".into(),
                )]);
            };
            match super::cancel_linked_work_task(
                &engine,
                emitter,
                conn,
                &payload.task_id,
                &command.command_id,
                Some(&payload.reason),
            )
            .await
            {
                Ok(super::LinkedWorkTaskCancelOutcome::Accepted(receipt)) => Ok(vec![
                    ack(
                        &command,
                        AckStatus::Accepted,
                        Some(receipt.local_work_task_id.to_string()),
                        None,
                    ),
                    task_state(
                        runner_id,
                        command.sequence,
                        &payload.task_id,
                        "CANCELLED",
                        None,
                    ),
                ]),
                Ok(super::LinkedWorkTaskCancelOutcome::Duplicate { local_work_task_id }) => {
                    Ok(vec![
                        ack(
                            &command,
                            AckStatus::Duplicate,
                            Some(local_work_task_id.to_string()),
                            None,
                        ),
                        task_state(
                            runner_id,
                            command.sequence,
                            &payload.task_id,
                            "CANCELLED",
                            None,
                        ),
                    ])
                }
                Err(error) => Ok(vec![rejection(
                    &command,
                    "CORE_OPERATION_FAILED",
                    error.to_string(),
                )]),
            }
        }
    }
}

fn task_state(
    runner_id: &str,
    sequence: u64,
    task_id: &str,
    state: &str,
    state_reason: Option<Value>,
) -> Value {
    envelope(
        runner_id,
        "TASK_STATE",
        sequence,
        json!({
            "TASK_ID": task_id,
            "STATE": state,
            "STATE_AT": Utc::now(),
            "STATE_REASON": state_reason,
        }),
    )
}

fn projected_state_reason(task: &LinkedWorkTaskSnapshot) -> Option<Value> {
    let code = task
        .failure_reason
        .as_deref()
        .unwrap_or("WORK_TASK_FAILED")
        .to_ascii_uppercase();
    let message = task
        .last_error
        .as_deref()
        .or(task.failure_reason.as_deref())?
        .to_string();
    Some(json!({"code": code, "message": message}))
}

fn task_projection(runner_id: &str, sequence: u64, task: LinkedWorkTaskSnapshot) -> Vec<Value> {
    let state =
        serde_json::to_value(task.state).expect("LinkedWorkTaskState serialization cannot fail");
    let state = state.as_str().expect("state serializes as string");
    let state_reason = projected_state_reason(&task);
    let mut frames = vec![task_state(
        runner_id,
        sequence,
        &task.platform_task_id,
        state,
        state_reason,
    )];
    if let Some(summary) = task.result_summary {
        frames.push(envelope(
            runner_id,
            "TASK_RESULT",
            sequence,
            json!({
                "TASK_ID": task.platform_task_id,
                "STATE": state,
                "IS_PARTIAL": !matches!(state, "RESULT_READY" | "COMPLETED"),
                "SUMMARY": summary,
                "CHANGESET": null,
                "TESTS": [],
            }),
        ));
    }
    frames
}

/// 周期投影所有 linked WorkTask，使丢失的状态/结果帧最终收敛。
pub async fn linked_task_frames(
    conn: &DatabaseConnection,
    runner_id: &str,
    sequence: u64,
) -> Result<Vec<Value>, String> {
    let tasks = reconcile_all_linked_work_tasks(conn)
        .await
        .map_err(|error| error.to_string())?;
    Ok(tasks
        .into_iter()
        .flat_map(|task| task_projection(runner_id, sequence, task))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use sea_orm::{ActiveModelTrait, EntityTrait, PaginatorTrait, Set};
    use tempfile::TempDir;

    use super::*;
    use crate::db::entities::{cerebro_task_link, work_task};
    use crate::db::service::folder_service;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_folder};
    use crate::models::agent::AgentType;

    fn contract_example(message_type: &str) -> Value {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/contracts/cerebro-runner-stream.schema.json"
        ))
        .unwrap();
        fixture["examples"][message_type].clone()
    }

    fn normalize_generated_fields(actual: &mut Value, expected: &Value) {
        actual["MESSAGE_ID"] = expected["MESSAGE_ID"].clone();
        actual["OCCURRED_AT"] = expected["OCCURRED_AT"].clone();
        if expected["PAYLOAD"].get("STATE_AT").is_some() {
            actual["PAYLOAD"]["STATE_AT"] = expected["PAYLOAD"]["STATE_AT"].clone();
        }
    }

    fn init_repo(path: &std::path::Path) {
        std::fs::create_dir_all(path).unwrap();
        let output = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    fn start_command(runner_id: &str, target_id: &str) -> Value {
        json!({
            "PROTOCOL_VERSION": 1,
            "TYPE": "TASK_START",
            "MESSAGE_ID": "00000000-0000-4000-8000-000000000001",
            "OCCURRED_AT": Utc::now(),
            "RUNNER_ID": runner_id,
            "CORRELATION_ID": "00000000-0000-4000-8000-000000000001",
            "COMMAND_ID": "00000000-0000-4000-8000-000000000002",
            "SEQUENCE": 1,
            "PAYLOAD": {
                "TASK_ID": "00000000-0000-4000-8000-000000000003",
                "TARGET_ID": target_id,
                "BINDING_ID": "00000000-0000-4000-8000-000000000004",
                "MODULE_ID": "00000000-0000-4000-8000-000000000005",
                "OWNER_USER_ID": "00000000-0000-4000-8000-000000000006",
                "PROMPT": "完成真实纵向切片",
                "MCP_SCOPE": {
                    "AUTHORIZED_MODULE_IDS": ["00000000-0000-4000-8000-000000000005"],
                    "ALLOWED_CAPABILITY_IDS": ["MODULE_BOUND_MCP"],
                    "CLAIM_ENDPOINT": "/api/v1/execution-task-principals/create"
                },
                "EXPIRES_AT": Utc::now() + chrono::Duration::hours(1)
            }
        })
    }

    #[test]
    fn task_protocol_parsers_and_serializers_match_cerebro_contract() {
        let start_value = contract_example("TASK_START");
        let command: TaskCommandEnvelope = serde_json::from_value(start_value).unwrap();
        let start: TaskStartPayload = serde_json::from_value(command.payload.clone()).unwrap();
        assert_eq!(command.message_type, TaskCommandType::TaskStart);
        assert_eq!(
            start.mcp_scope.authorized_module_ids,
            vec![start.module_id.clone()]
        );
        assert_eq!(
            start.mcp_scope.claim_endpoint,
            "/api/v1/execution-task-principals/create"
        );

        let expected_ack = contract_example("COMMAND_ACK");
        let mut actual_ack = ack(
            &command,
            AckStatus::Accepted,
            Some("opaque-work-task-ref".into()),
            None,
        );
        assert_eq!(actual_ack["CORRELATION_ID"], command.message_id);
        assert_eq!(actual_ack["COMMAND_ID"], command.command_id);
        assert_eq!(actual_ack["SEQUENCE"], command.sequence);
        normalize_generated_fields(&mut actual_ack, &expected_ack);
        actual_ack["CORRELATION_ID"] = expected_ack["CORRELATION_ID"].clone();
        actual_ack["COMMAND_ID"] = expected_ack["COMMAND_ID"].clone();
        actual_ack["SEQUENCE"] = expected_ack["SEQUENCE"].clone();
        actual_ack["PAYLOAD"]["TASK_ID"] = expected_ack["PAYLOAD"]["TASK_ID"].clone();
        assert_eq!(actual_ack, expected_ack);

        let expected_state = contract_example("TASK_STATE");
        let mut actual_state = task_state(
            expected_state["RUNNER_ID"].as_str().unwrap(),
            expected_state["SEQUENCE"].as_u64().unwrap(),
            expected_state["PAYLOAD"]["TASK_ID"].as_str().unwrap(),
            expected_state["PAYLOAD"]["STATE"].as_str().unwrap(),
            None,
        );
        normalize_generated_fields(&mut actual_state, &expected_state);
        assert_eq!(actual_state, expected_state);

        let expected_result = contract_example("TASK_RESULT");
        let projection = task_projection(
            expected_result["RUNNER_ID"].as_str().unwrap(),
            expected_result["SEQUENCE"].as_u64().unwrap(),
            LinkedWorkTaskSnapshot {
                platform_task_id: expected_result["PAYLOAD"]["TASK_ID"]
                    .as_str()
                    .unwrap()
                    .into(),
                local_work_task_id: 1,
                state: crate::cerebro::LinkedWorkTaskState::ResultReady,
                failure_reason: None,
                last_error: None,
                result_summary: Some("Task completed".into()),
                remote_workbench_available: true,
                deleted: false,
            },
        );
        let mut actual_result = projection[1].clone();
        normalize_generated_fields(&mut actual_result, &expected_result);
        assert_eq!(actual_result, expected_result);

        let interrupted = task_projection(
            expected_result["RUNNER_ID"].as_str().unwrap(),
            2,
            LinkedWorkTaskSnapshot {
                platform_task_id: "interrupted-task".into(),
                local_work_task_id: 2,
                state: crate::cerebro::LinkedWorkTaskState::Interrupted,
                failure_reason: Some("interrupted".into()),
                last_error: Some("interrupted by restart".into()),
                result_summary: Some("partial response".into()),
                remote_workbench_available: false,
                deleted: false,
            },
        );
        assert_eq!(interrupted[0]["PAYLOAD"]["STATE"], "INTERRUPTED");
        assert_eq!(
            interrupted[0]["PAYLOAD"]["STATE_REASON"],
            json!({"code": "INTERRUPTED", "message": "interrupted by restart"})
        );
        assert_eq!(interrupted[1]["PAYLOAD"]["IS_PARTIAL"], true);
        assert_eq!(interrupted[1]["PAYLOAD"]["SUMMARY"], "partial response");
    }

    #[tokio::test]
    async fn expired_first_start_does_not_create_a_work_task() {
        let db = fresh_in_memory_db().await;
        let runner_id = "00000000-0000-4000-8000-000000000000";
        let mut command = start_command(runner_id, "unavailable-target");
        command["PAYLOAD"]["EXPIRES_AT"] = json!(Utc::now() - chrono::Duration::days(1));
        let frames = handle_task_command(&EventEmitter::Noop, &db.conn, runner_id, command)
            .await
            .unwrap();
        assert_eq!(frames[0]["PAYLOAD"]["ACK_STATUS"], "REJECTED");
        assert_eq!(frames[0]["PAYLOAD"]["ERROR"]["CODE"], "TASK_EXPIRED");
        assert_eq!(work_task::Entity::find().count(&db.conn).await.unwrap(), 0);
        assert_eq!(
            cerebro_task_link::Entity::find()
                .count(&db.conn)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn disconnect_before_local_dispatch_reconnect_creates_one_work_task() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, temp.path().to_str().unwrap()).await;
        folder_service::update_folder_default_agent(&db.conn, folder_id, Some(AgentType::Codex))
            .await
            .unwrap();
        let runner_id = "00000000-0000-4000-8000-000000000000";
        let command = start_command(
            runner_id,
            &super::super::target_projection::target_id(runner_id, folder_id),
        );

        // 首次连接在本地 dispatch 前断开，没有任何本地事实可提交。
        assert_eq!(work_task::Entity::find().count(&db.conn).await.unwrap(), 0);
        assert_eq!(
            cerebro_task_link::Entity::find()
                .count(&db.conn)
                .await
                .unwrap(),
            0
        );

        let reconnected = handle_task_command(&EventEmitter::Noop, &db.conn, runner_id, command)
            .await
            .unwrap();
        assert_eq!(reconnected[0]["PAYLOAD"]["ACK_STATUS"], "ACCEPTED");
        assert_eq!(work_task::Entity::find().count(&db.conn).await.unwrap(), 1);
        assert_eq!(
            cerebro_task_link::Entity::find()
                .count(&db.conn)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn disconnect_after_local_commit_before_ack_reconnects_to_same_work_task() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, temp.path().to_str().unwrap()).await;
        folder_service::update_folder_default_agent(&db.conn, folder_id, Some(AgentType::Codex))
            .await
            .unwrap();
        let runner_id = "00000000-0000-4000-8000-000000000000";
        let command = start_command(
            runner_id,
            &super::super::target_projection::target_id(runner_id, folder_id),
        );

        let first = handle_task_command(&EventEmitter::Noop, &db.conn, runner_id, command.clone())
            .await
            .unwrap();
        let local_task_id = first[0]["PAYLOAD"]["LOCAL_WORK_TASK_REF"]
            .as_str()
            .unwrap()
            .parse::<i32>()
            .unwrap();
        work_task::ActiveModel {
            id: Set(local_task_id),
            status: Set(crate::db::entities::work_task::WorkTaskStatus::Review),
            result_summary: Set(Some("ready after lost ACK".into())),
            ..Default::default()
        }
        .update(&db.conn)
        .await
        .unwrap();
        // 首次返回帧在网络断线中丢失；重连补发原命令。
        let mut command = command;
        command["PAYLOAD"]["EXPIRES_AT"] = json!(Utc::now() - chrono::Duration::days(1));
        // 已有任务不再依赖可用 Folder；同时覆盖重新解析 Target 会失败的场景。
        folder_service::update_folder_default_agent(&db.conn, folder_id, None)
            .await
            .unwrap();
        let repeated = handle_task_command(&EventEmitter::Noop, &db.conn, runner_id, command)
            .await
            .unwrap();

        assert_eq!(first[0]["PAYLOAD"]["ACK_STATUS"], "ACCEPTED");
        assert_eq!(repeated[0]["PAYLOAD"]["ACK_STATUS"], "DUPLICATE");
        assert_eq!(repeated[1]["PAYLOAD"]["STATE"], "RESULT_READY");
        assert_eq!(repeated[2]["PAYLOAD"]["IS_PARTIAL"], false);
        assert_eq!(repeated[2]["PAYLOAD"]["SUMMARY"], "ready after lost ACK");
        assert_eq!(
            first[0]["PAYLOAD"]["LOCAL_WORK_TASK_REF"],
            repeated[0]["PAYLOAD"]["LOCAL_WORK_TASK_REF"]
        );
        assert_eq!(work_task::Entity::find().count(&db.conn).await.unwrap(), 1);
        assert_eq!(
            cerebro_task_link::Entity::find()
                .count(&db.conn)
                .await
                .unwrap(),
            1
        );
    }
}
