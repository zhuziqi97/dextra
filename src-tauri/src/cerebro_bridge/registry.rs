use std::collections::HashSet;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// 命令穿过平台边界时唯一允许的三种路线。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommandRoute {
    Operation,
    Relay,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// 平台拥有的写操作；Codeg RPC 不得直接执行这些副作用。
pub enum PlatformOperation {
    SessionCreate,
    TaskStart,
    TaskCancel,
    SessionClose,
    ApprovalDecide,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 单个 Codeg command 的权限、效果、幂等与超时合同。
pub struct CommandPolicy {
    pub command: String,
    pub group: String,
    pub route: CommandRoute,
    #[serde(default)]
    pub operation: Option<PlatformOperation>,
    pub required_scope: String,
    pub effects: Vec<String>,
    pub shared_user: bool,
    pub idempotency: String,
    pub timeout_ms: u64,
    pub audit: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPolicy {
    pub channel: String,
    pub group: String,
    pub route: CommandRoute,
    pub required_scope: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRegistry {
    pub protocol_version: u32,
    pub codeg_api_revision: u32,
    pub codeg_upstream_version: String,
    pub codeg_upstream_commit: String,
    pub commands: Vec<CommandPolicy>,
    pub channels: Vec<ChannelPolicy>,
}

const REGISTRY_JSON: &str =
    include_str!("../../../src/lib/transport/cerebro-command-registry.json");

// Rust Runner 与 Web bundle 解析同一份 JSON，避免两侧维护平行命令表。
static REGISTRY: LazyLock<CommandRegistry> = LazyLock::new(|| {
    let registry: CommandRegistry =
        serde_json::from_str(REGISTRY_JSON).expect("Cerebro command registry must be valid JSON");
    validate_registry(&registry).expect("Cerebro command registry must be internally consistent");
    registry
});

pub fn registry() -> &'static CommandRegistry {
    &REGISTRY
}

pub fn command_policy(command: &str) -> Option<&'static CommandPolicy> {
    REGISTRY
        .commands
        .iter()
        .find(|policy| policy.command == command)
}

pub fn channel_policy(channel: &str) -> Option<&'static ChannelPolicy> {
    REGISTRY
        .channels
        .iter()
        .find(|policy| {
            policy.channel == channel
                || policy
                    .channel
                    .strip_suffix('*')
                    .is_some_and(|prefix| channel.starts_with(prefix))
        })
}

fn validate_registry(registry: &CommandRegistry) -> Result<(), String> {
    if registry.protocol_version != 1
        || registry.codeg_api_revision != 1
        || registry.codeg_upstream_version != "v0.29.0"
        || registry.codeg_upstream_commit != "769610c626f1fc4b18c11d3e289326acf097b99f"
    {
        return Err("fixed Codeg baseline does not match P0-A".into());
    }

    let mut commands = HashSet::new();
    for policy in &registry.commands {
        if !commands.insert(policy.command.as_str()) {
            return Err(format!("duplicate command {}", policy.command));
        }
        match (policy.route, policy.operation) {
            (CommandRoute::Operation, Some(_)) => {}
            (CommandRoute::Operation, None) => {
                return Err(format!(
                    "operation command {} lacks mapping",
                    policy.command
                ));
            }
            (_, Some(_)) => {
                return Err(format!(
                    "non-operation command {} has mapping",
                    policy.command
                ));
            }
            _ => {}
        }
    }

    let mut channels = HashSet::new();
    for policy in &registry.channels {
        if !channels.insert(policy.channel.as_str()) {
            return Err(format!("duplicate channel {}", policy.channel));
        }
        if policy.route != CommandRoute::Relay {
            return Err(format!("channel {} is not relay-only", policy.channel));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_freezes_the_audited_baseline() {
        let registry = registry();
        assert_eq!(registry.protocol_version, 1);
        assert_eq!(registry.codeg_api_revision, 1);
        assert_eq!(registry.codeg_upstream_version, "v0.29.0");
        assert_eq!(
            registry.codeg_upstream_commit,
            "769610c626f1fc4b18c11d3e289326acf097b99f"
        );
    }

    #[test]
    fn owner_workbench_operations_relay_and_local_admin_commands_are_denied() {
        for command in [
            "acp_connect",
            "acp_prompt",
            "acp_cancel",
            "acp_disconnect",
            "acp_respond_permission",
        ] {
            assert_eq!(
                command_policy(command).unwrap().route,
                CommandRoute::Relay
            );
        }
        assert!(channel_policy("terminal://output/terminal-1").is_some());
        assert!(channel_policy("credential://changed").is_none());
        for command in [
            "open_in_code",
            "acp_update_agent_env",
            "acp_download_agent_binary",
            "perform_app_update",
        ] {
            assert_eq!(command_policy(command).unwrap().route, CommandRoute::Deny);
        }
        for command in [
            "read_file_for_edit",
            "save_file_content",
            "terminal_spawn",
            "git_status",
            "git_commit",
            "work_task_diff",
            "work_task_merge",
            "forge_merge_change",
        ] {
            assert_eq!(command_policy(command).unwrap().route, CommandRoute::Relay);
        }
    }
}
