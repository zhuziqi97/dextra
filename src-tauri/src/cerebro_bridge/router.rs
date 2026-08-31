use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use super::error::{BridgeError, BridgeErrorCode};
use super::protocol::{Envelope, MessageType};
use super::registry::{channel_policy, command_policy, CommandRoute, PlatformOperation};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct CommandAck {
    pub command_id: String,
    pub duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "TYPE", content = "PAYLOAD", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BridgeResponse {
    OperationResult {
        operation: PlatformOperation,
        result: Value,
    },
    CodegRpcResponse {
        command: String,
        result: Value,
    },
    CodegChannelSubscribed {
        channel: String,
        result: Value,
    },
    CodegChannelUnsubscribed {
        channel: String,
        result: Value,
    },
    CodegStreamFrame {
        result: Value,
    },
    CodegStreamDetached {
        result: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct DispatchResult {
    pub ack: CommandAck,
    pub response: BridgeResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodegRpcPayload {
    command: String,
    #[serde(default)]
    args: Value,
}

#[derive(Debug, Deserialize)]
struct ChannelPayload {
    channel: String,
}

#[async_trait]
/// 由真实 ACP manager 或测试 fake 实现的窄端口，桥接层不拥有 ACP 状态机。
pub trait CodegCorePort: Send + Sync {
    async fn execute_operation(
        &self,
        operation: PlatformOperation,
        envelope: &Envelope,
    ) -> Result<Value, BridgeError>;

    async fn relay_read(
        &self,
        command: &str,
        args: Value,
        envelope: &Envelope,
    ) -> Result<Value, BridgeError>;

    async fn subscribe_channel(
        &self,
        channel: &str,
        envelope: &Envelope,
    ) -> Result<Value, BridgeError>;

    async fn unsubscribe_channel(
        &self,
        channel: &str,
        envelope: &Envelope,
    ) -> Result<Value, BridgeError>;

    async fn attach_stream(&self, envelope: &Envelope) -> Result<Value, BridgeError>;

    async fn detach_stream(&self, envelope: &Envelope) -> Result<Value, BridgeError>;
}

/// P0-C 的内存 dispatcher。它未接入 daemon bootstrap，因此不会成为生产
/// 状态 owner；P0-B/P1 必须把相同 `COMMAND_ID` 语义接到 Runner 本地持久事实。
pub struct CerebroBridge {
    core: Arc<dyn CodegCorePort>,
    dispatch_gate: Mutex<()>,
    completed: Mutex<HashMap<String, DispatchResult>>,
}

impl CerebroBridge {
    pub fn new(core: Arc<dyn CodegCorePort>) -> Self {
        Self {
            core,
            dispatch_gate: Mutex::new(()),
            completed: Mutex::new(HashMap::new()),
        }
    }

    pub async fn dispatch(&self, envelope: Envelope) -> Result<DispatchResult, BridgeError> {
        envelope.validate()?;
        if envelope.message_type == MessageType::Hello {
            return Err(BridgeError::invalid(
                "HELLO is negotiated before command dispatch",
            ));
        }
        let command_id = envelope
            .command_id
            .clone()
            .expect("validated non-HELLO envelope has COMMAND_ID");

        // Spike 先串行化同一 Bridge 的命令，使 duplicate 检查与副作用之间没有
        // 竞态。生产实现会由持久 command inbox/unique key 接管，而不是扩大此锁。
        let _dispatch_guard = self.dispatch_gate.lock().await;
        if let Some(previous) = self.completed.lock().await.get(&command_id).cloned() {
            return Ok(DispatchResult {
                ack: CommandAck {
                    command_id,
                    duplicate: true,
                },
                response: previous.response,
            });
        }

        let response = self.dispatch_once(&envelope).await?;
        let result = DispatchResult {
            ack: CommandAck {
                command_id: command_id.clone(),
                duplicate: false,
            },
            response,
        };
        self.completed
            .lock()
            .await
            .insert(command_id, result.clone());
        Ok(result)
    }

    async fn dispatch_once(&self, envelope: &Envelope) -> Result<BridgeResponse, BridgeError> {
        let operation = match envelope.message_type {
            MessageType::SessionOpen => Some(PlatformOperation::SessionCreate),
            MessageType::TaskStart => Some(PlatformOperation::TaskStart),
            MessageType::TaskCancel => Some(PlatformOperation::TaskCancel),
            MessageType::SessionClose => Some(PlatformOperation::SessionClose),
            MessageType::ApprovalDecision => Some(PlatformOperation::ApprovalDecide),
            _ => None,
        };
        if let Some(operation) = operation {
            let result = self.core.execute_operation(operation, envelope).await?;
            return Ok(BridgeResponse::OperationResult { operation, result });
        }

        match envelope.message_type {
            MessageType::CodegRpcRequest => self.dispatch_rpc(envelope).await,
            MessageType::CodegChannelSubscribe => self.dispatch_channel(envelope, true).await,
            MessageType::CodegChannelUnsubscribe => self.dispatch_channel(envelope, false).await,
            MessageType::CodegStreamAttach => {
                let result = self.core.attach_stream(envelope).await?;
                Ok(BridgeResponse::CodegStreamFrame { result })
            }
            MessageType::CodegStreamDetach => {
                let result = self.core.detach_stream(envelope).await?;
                Ok(BridgeResponse::CodegStreamDetached { result })
            }
            MessageType::Hello
            | MessageType::SessionOpen
            | MessageType::TaskStart
            | MessageType::TaskCancel
            | MessageType::SessionClose
            | MessageType::ApprovalDecision => unreachable!("handled before match"),
        }
    }

    async fn dispatch_rpc(&self, envelope: &Envelope) -> Result<BridgeResponse, BridgeError> {
        let payload: CodegRpcPayload = serde_json::from_value(envelope.payload.clone())
            .map_err(|error| BridgeError::invalid(format!("invalid RPC payload: {error}")))?;
        let Some(policy) = command_policy(&payload.command) else {
            return Err(BridgeError::new(
                BridgeErrorCode::CodegCommandNotRemote,
                format!(
                    "command {} is not registered for remote use",
                    payload.command
                ),
            ));
        };
        match policy.route {
            CommandRoute::Deny => Err(BridgeError::new(
                BridgeErrorCode::CodegCommandRemoteDenied,
                format!("command group {} is not available remotely", policy.group),
            )),
            CommandRoute::Operation => Err(BridgeError::new(
                BridgeErrorCode::CodegCommandRequiresOperation,
                format!(
                    "command {} must use platform operation {}",
                    payload.command,
                    policy
                        .operation
                        .map(|operation| format!("{operation:?}"))
                        .unwrap_or_else(|| "UNKNOWN".into())
                ),
            )),
            CommandRoute::Relay => {
                let result = self
                    .core
                    .relay_read(&payload.command, payload.args, envelope)
                    .await?;
                Ok(BridgeResponse::CodegRpcResponse {
                    command: payload.command,
                    result,
                })
            }
        }
    }

    async fn dispatch_channel(
        &self,
        envelope: &Envelope,
        subscribe: bool,
    ) -> Result<BridgeResponse, BridgeError> {
        let payload: ChannelPayload = serde_json::from_value(envelope.payload.clone())
            .map_err(|error| BridgeError::invalid(format!("invalid channel payload: {error}")))?;
        if channel_policy(&payload.channel).is_none() {
            return Err(BridgeError::new(
                BridgeErrorCode::CodegChannelNotRemote,
                format!(
                    "channel {} is not registered for remote use",
                    payload.channel
                ),
            ));
        }
        if subscribe {
            let result = self
                .core
                .subscribe_channel(&payload.channel, envelope)
                .await?;
            Ok(BridgeResponse::CodegChannelSubscribed {
                channel: payload.channel,
                result,
            })
        } else {
            let result = self
                .core
                .unsubscribe_channel(&payload.channel, envelope)
                .await?;
            Ok(BridgeResponse::CodegChannelUnsubscribed {
                channel: payload.channel,
                result,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::cerebro_bridge::protocol::negotiate;

    #[derive(Default)]
    struct FakeAcpCore {
        state: Mutex<FakeState>,
    }

    #[derive(Default)]
    struct FakeState {
        connection_id: Option<String>,
        events: Vec<Value>,
        operations: Vec<PlatformOperation>,
        read_commands: Vec<String>,
    }

    #[async_trait]
    impl CodegCorePort for FakeAcpCore {
        async fn execute_operation(
            &self,
            operation: PlatformOperation,
            _envelope: &Envelope,
        ) -> Result<Value, BridgeError> {
            let mut state = self.state.lock().await;
            state.operations.push(operation);
            match operation {
                PlatformOperation::SessionCreate => {
                    state.connection_id = Some("codeg-connection-1".into());
                    Ok(json!({ "connectionId": "codeg-connection-1" }))
                }
                PlatformOperation::TaskStart => {
                    push_event(
                        &mut state,
                        json!({
                            "type": "permission_request",
                            "request_id": "permission-1",
                            "options": [{ "option_id": "allow-once" }]
                        }),
                    );
                    Ok(json!({ "accepted": true }))
                }
                PlatformOperation::ApprovalDecide => {
                    push_event(
                        &mut state,
                        json!({
                            "type": "permission_resolved",
                            "request_id": "permission-1"
                        }),
                    );
                    Ok(Value::Null)
                }
                PlatformOperation::TaskCancel => {
                    push_event(&mut state, json!({ "type": "cancelled" }));
                    Ok(Value::Null)
                }
                PlatformOperation::SessionClose => Ok(Value::Null),
            }
        }

        async fn relay_read(
            &self,
            command: &str,
            _args: Value,
            _envelope: &Envelope,
        ) -> Result<Value, BridgeError> {
            let mut state = self.state.lock().await;
            state.read_commands.push(command.into());
            Ok(json!({
                "connectionId": state.connection_id,
                "eventSeq": state.events.len(),
                "events": state.events
            }))
        }

        async fn subscribe_channel(
            &self,
            channel: &str,
            _envelope: &Envelope,
        ) -> Result<Value, BridgeError> {
            Ok(json!({ "channel": channel, "subscribed": true }))
        }

        async fn unsubscribe_channel(
            &self,
            channel: &str,
            _envelope: &Envelope,
        ) -> Result<Value, BridgeError> {
            Ok(json!({ "channel": channel, "subscribed": false }))
        }

        async fn attach_stream(&self, envelope: &Envelope) -> Result<Value, BridgeError> {
            let state = self.state.lock().await;
            let since_seq = envelope.payload.get("sinceSeq").and_then(Value::as_u64);
            match since_seq {
                None => Ok(json!({
                    "type": "snapshot",
                    "connection_id": state.connection_id,
                    "snapshot": { "pendingPermission": null },
                    "event_seq": state.events.len()
                })),
                Some(cursor) => Ok(json!({
                    "type": "replay",
                    "connection_id": state.connection_id,
                    "events": state.events.iter().skip(cursor as usize).collect::<Vec<_>>(),
                    "high_water_seq": state.events.len()
                })),
            }
        }

        async fn detach_stream(&self, _envelope: &Envelope) -> Result<Value, BridgeError> {
            Ok(json!({ "detached": true }))
        }
    }

    fn push_event(state: &mut FakeState, payload: Value) {
        let seq = state.events.len() + 1;
        state.events.push(json!({
            "seq": seq,
            "connection_id": "codeg-connection-1",
            "payload": payload
        }));
    }

    fn envelope(message_type: MessageType, command_id: &str, payload: Value) -> Envelope {
        Envelope {
            protocol_version: 1,
            message_type,
            message_id: format!("message-{command_id}"),
            occurred_at: Utc::now(),
            payload,
            runner_id: Some("runner-1".into()),
            correlation_id: Some(format!("correlation-{command_id}")),
            command_id: Some(command_id.into()),
            target_id: Some("target-1".into()),
            session_id: Some("session-1".into()),
            task_id: Some("task-1".into()),
        }
    }

    #[tokio::test]
    async fn vertical_flow_uses_operations_and_snapshot_replay() {
        negotiate(1, 1).unwrap();
        let core = Arc::new(FakeAcpCore::default());
        let bridge = CerebroBridge::new(core.clone());

        let opened = bridge
            .dispatch(envelope(MessageType::SessionOpen, "open-1", json!({})))
            .await
            .unwrap();
        assert!(matches!(
            opened.response,
            BridgeResponse::OperationResult {
                operation: PlatformOperation::SessionCreate,
                ..
            }
        ));

        let cold = bridge
            .dispatch(envelope(
                MessageType::CodegStreamAttach,
                "attach-1",
                json!({ "connectionId": "codeg-connection-1" }),
            ))
            .await
            .unwrap();
        let BridgeResponse::CodegStreamFrame { result } = cold.response else {
            panic!("cold attach must return stream frame");
        };
        assert_eq!(result["type"], "snapshot");
        assert_eq!(result["event_seq"], 0);

        bridge
            .dispatch(envelope(MessageType::TaskStart, "start-1", json!({})))
            .await
            .unwrap();
        let replay = bridge
            .dispatch(envelope(
                MessageType::CodegStreamAttach,
                "attach-2",
                json!({ "connectionId": "codeg-connection-1", "sinceSeq": 0 }),
            ))
            .await
            .unwrap();
        let BridgeResponse::CodegStreamFrame { result } = replay.response else {
            panic!("hot attach must return stream frame");
        };
        assert_eq!(result["type"], "replay");
        assert_eq!(result["events"][0]["payload"]["type"], "permission_request");

        bridge
            .dispatch(envelope(
                MessageType::ApprovalDecision,
                "approval-1",
                json!({ "requestId": "permission-1", "optionId": "allow-once" }),
            ))
            .await
            .unwrap();
        let cancel = envelope(MessageType::TaskCancel, "cancel-1", json!({}));
        let first_cancel = bridge.dispatch(cancel.clone()).await.unwrap();
        let duplicate_cancel = bridge.dispatch(cancel).await.unwrap();
        assert!(!first_cancel.ack.duplicate);
        assert!(duplicate_cancel.ack.duplicate);

        let read = bridge
            .dispatch(envelope(
                MessageType::CodegRpcRequest,
                "read-1",
                json!({ "command": "acp_get_session_snapshot", "args": {} }),
            ))
            .await
            .unwrap();
        let BridgeResponse::CodegRpcResponse { result, .. } = read.response else {
            panic!("read command must use RPC response");
        };
        assert_eq!(result["eventSeq"], 3);

        let state = core.state.lock().await;
        assert_eq!(
            state.operations,
            vec![
                PlatformOperation::SessionCreate,
                PlatformOperation::TaskStart,
                PlatformOperation::ApprovalDecide,
                PlatformOperation::TaskCancel,
            ]
        );
    }

    #[tokio::test]
    async fn rpc_cannot_bypass_operations_or_remote_denials() {
        let bridge = CerebroBridge::new(Arc::new(FakeAcpCore::default()));

        let operation_error = bridge
            .dispatch(envelope(
                MessageType::CodegRpcRequest,
                "rpc-write",
                json!({ "command": "acp_prompt", "args": {} }),
            ))
            .await
            .unwrap_err();
        assert_eq!(
            operation_error.code,
            BridgeErrorCode::CodegCommandRequiresOperation
        );

        let denied_error = bridge
            .dispatch(envelope(
                MessageType::CodegRpcRequest,
                "rpc-denied",
                json!({ "command": "forge_merge_change", "args": {} }),
            ))
            .await
            .unwrap_err();
        assert_eq!(denied_error.code, BridgeErrorCode::CodegCommandRemoteDenied);

        let unknown_error = bridge
            .dispatch(envelope(
                MessageType::CodegRpcRequest,
                "rpc-unknown",
                json!({ "command": "future_unclassified_command", "args": {} }),
            ))
            .await
            .unwrap_err();
        assert_eq!(unknown_error.code, BridgeErrorCode::CodegCommandNotRemote);
    }
}
