import rawRegistry from "./cerebro-command-registry.json"

/** 浏览器与 Runner 共用的命令分类，新增命令必须先明确归属而不是默认透传。 */
export type CerebroCommandGroup =
  | "SESSION_READ"
  | "RUNTIME_READ"
  | "TASK_CONTROL"
  | "APPROVAL_RESPONSE"
  | "WORKSPACE_READ"
  | "WORKSPACE_WRITE"
  | "TERMINAL"
  | "GIT_MUTATE"
  | "FORGE_MUTATE"
  | "LOCAL_OS_INTEGRATION"
  | "SETTINGS"
  | "CREDENTIALS"
  | "INSTALL"

export type CerebroCommandRoute = "OPERATION" | "RELAY" | "DENY"

export type CerebroPlatformOperation =
  | "SESSION_CREATE"
  | "TASK_START"
  | "TASK_CANCEL"
  | "SESSION_CLOSE"
  | "APPROVAL_DECIDE"

export interface CerebroCommandPolicy {
  command: string
  group: CerebroCommandGroup
  route: CerebroCommandRoute
  operation?: CerebroPlatformOperation
  requiredScope: string
  effects: string[]
  sharedUser: boolean
  idempotency: "COMMAND_ID" | "READ_ONLY" | "NONE"
  timeoutMs: number
  audit: "DOMAIN_OPERATION" | "READ_METADATA" | "DENIED"
}

export interface CerebroChannelPolicy {
  channel: string
  group: "SESSION_READ" | "RUNTIME_READ"
  route: "RELAY"
  requiredScope: string
}

interface CerebroRegistry {
  protocolVersion: number
  codegApiRevision: number
  codegUpstreamVersion: string
  codegUpstreamCommit: string
  commands: CerebroCommandPolicy[]
  channels: CerebroChannelPolicy[]
}

const COMMAND_ROUTES = new Set<CerebroCommandRoute>([
  "OPERATION",
  "RELAY",
  "DENY",
])

function parseRegistry(value: unknown): CerebroRegistry {
  if (!value || typeof value !== "object") {
    throw new Error("CEREBRO_COMMAND_REGISTRY_INVALID: root must be an object")
  }
  const registry = value as Partial<CerebroRegistry>
  if (
    registry.protocolVersion !== 1 ||
    registry.codegApiRevision !== 1 ||
    registry.codegUpstreamVersion !== "v0.29.0" ||
    registry.codegUpstreamCommit !==
      "769610c626f1fc4b18c11d3e289326acf097b99f" ||
    !Array.isArray(registry.commands) ||
    !Array.isArray(registry.channels)
  ) {
    throw new Error("CEREBRO_COMMAND_REGISTRY_INVALID: baseline mismatch")
  }

  const commandNames = new Set<string>()
  for (const policy of registry.commands) {
    if (
      !policy ||
      typeof policy.command !== "string" ||
      policy.command.length === 0 ||
      !COMMAND_ROUTES.has(policy.route) ||
      typeof policy.requiredScope !== "string" ||
      !Array.isArray(policy.effects) ||
      typeof policy.sharedUser !== "boolean" ||
      typeof policy.timeoutMs !== "number"
    ) {
      throw new Error("CEREBRO_COMMAND_REGISTRY_INVALID: malformed command")
    }
    if (commandNames.has(policy.command)) {
      throw new Error(
        `CEREBRO_COMMAND_REGISTRY_INVALID: duplicate command ${policy.command}`
      )
    }
    commandNames.add(policy.command)
    if (policy.route === "OPERATION" && !policy.operation) {
      throw new Error(
        `CEREBRO_COMMAND_REGISTRY_INVALID: ${policy.command} lacks operation`
      )
    }
    if (policy.route !== "OPERATION" && policy.operation) {
      throw new Error(
        `CEREBRO_COMMAND_REGISTRY_INVALID: ${policy.command} has stray operation`
      )
    }
  }

  const channelNames = new Set<string>()
  for (const policy of registry.channels) {
    if (
      !policy ||
      typeof policy.channel !== "string" ||
      policy.channel.length === 0 ||
      policy.route !== "RELAY"
    ) {
      throw new Error("CEREBRO_COMMAND_REGISTRY_INVALID: malformed channel")
    }
    if (channelNames.has(policy.channel)) {
      throw new Error(
        `CEREBRO_COMMAND_REGISTRY_INVALID: duplicate channel ${policy.channel}`
      )
    }
    channelNames.add(policy.channel)
  }
  return registry as CerebroRegistry
}

// 模块加载时只校验一次生产 registry；调用路径只做确定性的 Map 查询。
export const CEREBRO_COMMAND_REGISTRY = parseRegistry(rawRegistry)
export const CEREBRO_PROTOCOL_VERSION = CEREBRO_COMMAND_REGISTRY.protocolVersion
export const CODEG_API_REVISION = CEREBRO_COMMAND_REGISTRY.codegApiRevision
export const CODEG_UPSTREAM_VERSION =
  CEREBRO_COMMAND_REGISTRY.codegUpstreamVersion
export const CODEG_UPSTREAM_COMMIT =
  CEREBRO_COMMAND_REGISTRY.codegUpstreamCommit

const commandPolicies = new Map(
  CEREBRO_COMMAND_REGISTRY.commands.map((policy) => [policy.command, policy])
)
const channelPolicies = new Map(
  CEREBRO_COMMAND_REGISTRY.channels.map((policy) => [policy.channel, policy])
)

export function getCerebroCommandPolicy(
  command: string
): CerebroCommandPolicy | undefined {
  return commandPolicies.get(command)
}

export function getCerebroChannelPolicy(
  channel: string
): CerebroChannelPolicy | undefined {
  return channelPolicies.get(channel)
}
