use serde::{Deserialize, Serialize};

use crate::models::message::{ContentBlock, MessageTurn, TurnRole};

/// 发送本轮 prompt 前读取的持久化边界；不以 prompt 文本或时间推断轮次。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionStart {
    pub run_seq: i32,
    pub conversation_id: i32,
    pub external_session_id: String,
    pub after_turn_id: Option<String>,
}

impl ExecutionStart {
    /// 从明确边界之后提取主 assistant 正文；边界丢失必须保留诊断。
    pub fn partial_from_turns(&self, turns: &[MessageTurn]) -> Result<Option<String>, String> {
        let start = match self.after_turn_id.as_deref() {
            None => 0,
            Some(id) => turns
                .iter()
                .position(|turn| turn.id == id)
                .map(|index| index + 1)
                .ok_or_else(|| format!("执行起点消息 {id} 不在持久化 transcript 中"))?,
        };
        let text = turns[start..]
            .iter()
            .filter(|turn| matches!(turn.role, TurnRole::Assistant))
            .flat_map(|turn| &turn.blocks)
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        Ok((!text.trim().is_empty()).then_some(text))
    }
}

/// 使用现有 provider parser，恢复读取不能走 UI 的空历史降级。
pub fn read_recovery_transcript(
    agent_type: crate::models::AgentType,
    session_id: &str,
) -> Result<crate::models::ConversationDetail, crate::parsers::ParseError> {
    use crate::models::AgentType;
    use crate::parsers::AgentParser;
    match agent_type {
        AgentType::Codex => {
            crate::parsers::codex::CodexParser::new().get_conversation_for_recovery(session_id)
        }
        AgentType::ClaudeCode => {
            crate::parsers::claude::ClaudeParser::new().get_conversation_for_recovery(session_id)
        }
        custom @ AgentType::Custom(_) => crate::parsers::acp_native::AcpNativeParser::new(custom)
            .get_conversation_for_recovery(session_id),
        AgentType::Cline => crate::parsers::cline::ClineParser::new().get_conversation(session_id),
        AgentType::OpenCode => {
            crate::parsers::opencode::OpenCodeParser::new().get_conversation(session_id)
        }
        AgentType::Hermes => {
            crate::parsers::hermes::HermesParser::new().get_conversation_for_recovery(session_id)
        }
        AgentType::Pi => {
            crate::parsers::pi::PiParser::new().get_conversation_for_recovery(session_id)
        }
        AgentType::DeepSeek => crate::parsers::deepseek::DeepSeekParser::new()
            .get_conversation_for_recovery(session_id),
        AgentType::KimiCode => crate::parsers::kimi_code::KimiCodeParser::new()
            .get_conversation_for_recovery(session_id),
        AgentType::CodeBuddy => crate::parsers::codebuddy::CodeBuddyParser::new()
            .get_conversation_for_recovery(session_id),
        AgentType::Grok => {
            crate::parsers::grok::GrokParser::new().get_conversation_for_recovery(session_id)
        }
        AgentType::Cursor => {
            crate::parsers::cursor::CursorParser::new().get_conversation_for_recovery(session_id)
        }
        AgentType::Gemini => {
            crate::parsers::gemini::GeminiParser::new().get_conversation_for_recovery(session_id)
        }
        AgentType::Qoder => {
            crate::parsers::qoder::QoderParser::new().get_conversation_for_recovery(session_id)
        }
        AgentType::OpenClaw => crate::parsers::openclaw::OpenClawParser::new()
            .get_conversation_for_recovery(session_id),
        AgentType::Antigravity => crate::parsers::antigravity::AntigravityParser::new()
            .get_conversation_for_recovery(session_id),
    }
}

/// 恢复只读取本轮边界指定的会话，不启动 Agent 或重放请求。
pub async fn recover_partial(
    conn: &sea_orm::DatabaseConnection,
    task: &crate::db::entities::work_task::Model,
) -> Result<Option<String>, String> {
    let raw = task
        .execution_start
        .as_deref()
        .ok_or_else(|| "缺少可定位的执行起点，无法恢复本轮输出".to_string())?;
    let start: ExecutionStart =
        serde_json::from_str(raw).map_err(|error| format!("执行起点解析失败: {error}"))?;
    if start.run_seq != task.run_seq || Some(start.conversation_id) != task.conversation_id {
        return Err("执行起点不属于当前执行轮次，无法恢复本轮输出".into());
    }
    let conversation =
        crate::db::service::conversation_service::get_by_id(conn, start.conversation_id)
            .await
            .map_err(|error| error.to_string())?;
    tokio::task::spawn_blocking(move || {
        let detail = read_recovery_transcript(conversation.agent_type, &start.external_session_id)
            .map_err(|error| error.to_string())?;
        start.partial_from_turns(&detail.turns)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn persisted_current_run_survives_database_reopen_and_projects_partial() {
        // 环境路径只交给隔离子进程，不能改动同进程其它测试的会话目录。
        const CHILD: &str = "DEXTRA_PARTIAL_RECOVERY_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let root = tempfile::tempdir().unwrap();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "work_task::execution_output::tests::persisted_current_run_survives_database_reopen_and_projects_partial", "--nocapture"])
                .env(CHILD, "1")
                .env("CODEG_HOME", root.path())
                .output().unwrap();
            assert!(
                result.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        use crate::acp_transcript::{record_entry, record_header, EntryKind, TranscriptHeader};
        use crate::db::service::{conversation_service, work_task_service};
        use crate::models::{AgentType, WorkTaskDraft, WorkTaskStatus};
        use sea_orm::{ActiveModelTrait, Set};

        let root = tempfile::tempdir().unwrap();
        let db = crate::db::test_helpers::fresh_disk_db(root.path()).await;
        let folder = crate::db::test_helpers::seed_folder(&db, root.path().to_str().unwrap()).await;
        let agent = AgentType::custom("partial-recovery-test").unwrap();
        let conversation = conversation_service::create(&db.conn, folder, agent, None, None)
            .await
            .unwrap();
        let task = work_task_service::create(
            &db.conn,
            WorkTaskDraft {
                folder_id: folder,
                title: "恢复本轮输出".into(),
                config: serde_json::json!({
                    "prompt_blocks": [{"type":"text", "text":"继续"}]
                }),
            },
        )
        .await
        .unwrap();
        let seq = work_task_service::claim_for_run(&db.conn, task.id, WorkTaskStatus::Todo, "test")
            .await
            .unwrap()
            .unwrap();
        assert!(work_task_service::begin_setup(&db.conn, task.id, seq)
            .await
            .unwrap());
        assert!(work_task_service::mark_running(
            &db.conn,
            task.id,
            seq,
            conversation.id,
            "dead-worker"
        )
        .await
        .unwrap());

        let dir = "partial-recovery-test";
        let session = "persisted-session";
        record_header(
            dir,
            &TranscriptHeader::new(&agent.as_wire(), session, "/repo", 1),
        )
        .await
        .unwrap();
        let prompt = serde_json::json!([{"type":"text","text":"继续"}]);
        record_entry(dir, session, EntryKind::Prompt, prompt.clone())
            .await
            .unwrap();
        record_entry(
            dir,
            session,
            EntryKind::Update,
            serde_json::json!({
                "sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":"上一轮结果"}
            }),
        )
        .await
        .unwrap();
        record_entry(
            dir,
            session,
            EntryKind::TurnEnd,
            serde_json::json!({"stopReason":"end_turn"}),
        )
        .await
        .unwrap();
        // 模拟 session/load 失败后的真实续接：新 ID 仍包含旧轮次的持久化历史。
        let old_session = session;
        let session = "reminted-session";
        record_header(
            dir,
            &TranscriptHeader::new(&agent.as_wire(), session, "/repo", 2).continuing(old_session),
        )
        .await
        .unwrap();
        let previous = read_recovery_transcript(agent, session).unwrap();
        assert!(previous
            .turns
            .iter()
            .flat_map(|turn| &turn.blocks)
            .any(|block| matches!(block, ContentBlock::Text { text } if text == "上一轮结果")));
        let start = ExecutionStart {
            run_seq: seq,
            conversation_id: conversation.id,
            external_session_id: session.into(),
            after_turn_id: Some(previous.turns.last().unwrap().id.clone()),
        };
        assert!(
            work_task_service::save_execution_start(&db.conn, task.id, "dead-worker", &start)
                .await
                .unwrap()
        );
        record_entry(dir, session, EntryKind::Prompt, prompt)
            .await
            .unwrap();
        record_entry(dir, session, EntryKind::Update, serde_json::json!({
            "sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":"本轮尚未完成的输出"}
        })).await.unwrap();
        crate::db::entities::cerebro_task_link::ActiveModel {
            platform_task_id: Set("restart-partial".into()),
            local_work_task_id: Set(task.id),
            cancel_command_id: Set(None),
            created_at: Set(chrono::Utc::now()),
        }
        .insert(&db.conn)
        .await
        .unwrap();
        db.conn.close().await.unwrap();

        let conn = sea_orm::Database::connect(format!(
            "sqlite:{}?mode=rw",
            root.path().join("source.db").display()
        ))
        .await
        .unwrap();
        assert_eq!(
            work_task_service::boot_reconcile_interrupted(&conn)
                .await
                .unwrap(),
            1
        );
        let projection = crate::cerebro::reconcile_linked_work_task(&conn, "restart-partial")
            .await
            .unwrap();
        assert_eq!(
            projection.state,
            crate::cerebro::LinkedWorkTaskState::Interrupted
        );
        assert_eq!(
            projection.result_summary.as_deref(),
            Some("本轮尚未完成的输出")
        );
        let row = work_task_service::get_model(&conn, task.id).await.unwrap();
        assert_eq!(row.last_error.as_deref(), Some("interrupted by restart"));
        let recovered_conversation = conversation_service::get_by_id(&conn, conversation.id).await.unwrap();
        assert_eq!(recovered_conversation.status, "cancelled");
        assert_eq!(
            work_task_service::boot_reconcile_interrupted(&conn)
                .await
                .unwrap(),
            0
        );
        conn.close().await.unwrap();
    }

    fn turn(id: &str, role: TurnRole, text: &str) -> MessageTurn {
        MessageTurn {
            id: id.into(),
            role,
            blocks: vec![ContentBlock::Text { text: text.into() }],
            timestamp: chrono::Utc::now(),
            usage: None,
            duration_ms: None,
            model: None,
            completed_at: None,
        }
    }

    #[test]
    fn same_prompt_does_not_reuse_previous_output() {
        let mut turns = vec![
            turn("u1", TurnRole::User, "继续"),
            turn("a1", TurnRole::Assistant, "上一轮结果"),
        ];
        let start = ExecutionStart {
            run_seq: 2,
            conversation_id: 7,
            external_session_id: "session".into(),
            after_turn_id: Some("a1".into()),
        };
        assert_eq!(start.partial_from_turns(&turns).unwrap(), None);
        turns.push(turn("u2", TurnRole::User, "继续"));
        assert_eq!(start.partial_from_turns(&turns).unwrap(), None);
        turns.push(turn("a2", TurnRole::Assistant, "本轮输出"));
        assert_eq!(
            start.partial_from_turns(&turns).unwrap().as_deref(),
            Some("本轮输出")
        );
        assert!(start
            .partial_from_turns(&turns[2..])
            .unwrap_err()
            .contains("a1"));
    }
}
