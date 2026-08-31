import { describe, expect, it, vi } from "vitest"

import type { AcpEvent, EventEnvelope, LiveSessionSnapshot } from "@/lib/types"
import {
  CerebroRemoteTransport,
  CerebroTransportError,
  type CerebroPlatformPeer,
  type CerebroRunnerHello,
  type CerebroStreamAttachRequest,
} from "./cerebro-remote-transport"
import type { CerebroPlatformOperation } from "./cerebro-command-registry"
import type { AttachHandlers, CallOptions, UnsubscribeFn } from "./types"

interface RecordedCall {
  operation?: CerebroPlatformOperation
  command: string
  args: Record<string, unknown>
  options?: CallOptions
}

class FakeCerebroPlatform implements CerebroPlatformPeer {
  readonly operations: RecordedCall[] = []
  readonly relays: RecordedCall[] = []
  readonly attaches: CerebroStreamAttachRequest[] = []

  private readonly events: EventEnvelope[] = []
  private readonly streamHandlers = new Map<string, AttachHandlers>()
  private readonly reconnectHandlers = new Set<() => void>()
  private connectionId = "codeg-connection-1"

  constructor(
    private readonly helloOverride: Partial<CerebroRunnerHello> = {}
  ) {}

  async runnerHello(): Promise<CerebroRunnerHello> {
    return {
      PROTOCOL_VERSION: 1,
      TYPE: "HELLO",
      MESSAGE_ID: "hello-1",
      OCCURRED_AT: "2026-08-31T00:00:00Z",
      PAYLOAD: {
        RUNNER_BUILD_ID: "dextra-p0-c",
        CODEG_API_REVISION: 1,
        CODEG_UPSTREAM_VERSION: "v0.29.0",
        CODEG_UPSTREAM_COMMIT: "769610c626f1fc4b18c11d3e289326acf097b99f",
      },
      ...this.helloOverride,
    }
  }

  async invokeOperation<T>(
    operation: CerebroPlatformOperation,
    codegCommand: string,
    args: Record<string, unknown>,
    options?: CallOptions
  ): Promise<T> {
    this.operations.push({ operation, command: codegCommand, args, options })
    if (operation === "SESSION_CREATE") {
      return { connectionId: this.connectionId } as T
    }
    if (operation === "TASK_START") {
      this.emit({
        type: "permission_request",
        request_id: "permission-1",
        tool_call: { title: "写入文件" },
        options: [
          { option_id: "allow-once", name: "允许一次", kind: "allow_once" },
        ],
      })
      return { accepted: true } as T
    }
    if (operation === "APPROVAL_DECIDE") {
      this.emit({ type: "permission_resolved", request_id: "permission-1" })
      return undefined as T
    }
    if (operation === "TASK_CANCEL") {
      this.emit({ type: "status_changed", status: "connected" })
      return undefined as T
    }
    return undefined as T
  }

  async invokeRelay<T>(
    codegCommand: string,
    args: Record<string, unknown>,
    options?: CallOptions
  ): Promise<T> {
    this.relays.push({ command: codegCommand, args, options })
    return this.snapshot() as T
  }

  async subscribeChannel(): Promise<UnsubscribeFn> {
    return () => undefined
  }

  async attachStream(
    request: CerebroStreamAttachRequest,
    handlers: AttachHandlers
  ): Promise<UnsubscribeFn> {
    this.attaches.push({ ...request })
    this.streamHandlers.set(request.subscriptionId, handlers)
    if (request.sinceSeq === undefined) {
      handlers.onSnapshot(this.snapshot(), this.events.length)
    } else {
      handlers.onReplay(
        this.events.filter((event) => event.seq > request.sinceSeq!),
        this.events.length
      )
    }
    return () => this.streamHandlers.delete(request.subscriptionId)
  }

  onReconnect(callback: () => void): UnsubscribeFn {
    this.reconnectHandlers.add(callback)
    return () => this.reconnectHandlers.delete(callback)
  }

  // 断线窗口只停止实时投递，历史事件仍由 Runner 保留给重连 replay。
  disconnect(): void {
    this.streamHandlers.clear()
  }

  reconnect(): void {
    for (const callback of this.reconnectHandlers) callback()
  }

  emitMissedStatus(): void {
    this.emit({ type: "status_changed", status: "prompting" })
  }

  private emit(event: AcpEvent): void {
    const envelope = {
      ...event,
      seq: this.events.length + 1,
      connection_id: this.connectionId,
    } as EventEnvelope
    this.events.push(envelope)
    for (const handlers of this.streamHandlers.values()) {
      handlers.onEvent(envelope)
    }
  }

  private snapshot(): LiveSessionSnapshot {
    return {
      connection_id: this.connectionId,
      status: "connected",
      event_seq: this.events.length,
      pending_permission: null,
    } as unknown as LiveSessionSnapshot
  }
}

describe("CerebroRemoteTransport", () => {
  it("完成 connect、prompt、snapshot、permission、cancel 与断线 replay", async () => {
    const peer = new FakeCerebroPlatform()
    const transport = new CerebroRemoteTransport(peer)

    const connected = await transport.call<{ connectionId: string }>(
      "acp_connect",
      {
        agent: "codex",
      }
    )
    expect(connected.connectionId).toBe("codeg-connection-1")

    const snapshots: number[] = []
    const events: EventEnvelope[] = []
    const replays: EventEnvelope[][] = []
    transport.eventStream().attach(
      connected.connectionId,
      {},
      {
        onSnapshot: (_snapshot, eventSeq) => snapshots.push(eventSeq),
        onReplay: (batch) => replays.push(batch),
        onEvent: (event) => events.push(event),
        onDetached: vi.fn(),
      }
    )
    await vi.waitFor(() => expect(snapshots).toEqual([0]))

    await transport.call("acp_prompt", {
      connectionId: connected.connectionId,
      prompt: "继续",
    })
    expect(events[events.length - 1]?.type).toBe("permission_request")
    await transport.call("acp_respond_permission", {
      connectionId: connected.connectionId,
      requestId: "permission-1",
      optionId: "allow-once",
    })
    expect(events[events.length - 1]?.type).toBe("permission_resolved")
    await transport.call("acp_cancel", { connectionId: connected.connectionId })
    expect(events[events.length - 1]).toMatchObject({
      type: "status_changed",
      status: "connected",
    })

    peer.disconnect()
    peer.emitMissedStatus()
    peer.reconnect()
    await vi.waitFor(() => expect(replays).toHaveLength(1))
    expect(replays[0]).toEqual([
      expect.objectContaining({
        seq: 4,
        type: "status_changed",
        status: "prompting",
      }),
    ])
    expect(peer.attaches[peer.attaches.length - 1]?.sinceSeq).toBe(3)

    expect(peer.operations.map((call) => call.operation)).toEqual([
      "SESSION_CREATE",
      "TASK_START",
      "APPROVAL_DECIDE",
      "TASK_CANCEL",
    ])
    expect(peer.operations[1].options?.timeoutMs).toBe(60_000)
  })

  it("只 Relay 登记读取，并在浏览器边界拒绝高风险、凭据与未知命令", async () => {
    const peer = new FakeCerebroPlatform()
    const transport = new CerebroRemoteTransport(peer)

    await transport.call("acp_get_session_snapshot", {
      connectionId: "codeg-connection-1",
    })
    expect(peer.relays.map((call) => call.command)).toEqual([
      "acp_get_session_snapshot",
    ])

    for (const command of [
      "forge_merge_change",
      "open_in_code",
      "terminal_spawn",
      "acp_update_agent_env",
      "acp_download_agent_binary",
      "perform_app_update",
      "command_that_does_not_exist",
    ]) {
      await expect(
        transport.call(command, { secret: "must-not-cross" })
      ).rejects.toBeInstanceOf(CerebroTransportError)
    }
    expect(peer.operations).toEqual([])
    expect(peer.relays).toHaveLength(1)
  })

  it("以稳定错误拒绝不兼容的 Codeg API revision", async () => {
    const peer = new FakeCerebroPlatform({
      PAYLOAD: {
        RUNNER_BUILD_ID: "dextra-p0-c",
        CODEG_API_REVISION: 2,
        CODEG_UPSTREAM_VERSION: "v0.29.0",
        CODEG_UPSTREAM_COMMIT: "769610c626f1fc4b18c11d3e289326acf097b99f",
      },
    })
    const transport = new CerebroRemoteTransport(peer)

    await expect(transport.call("acp_list_connections")).rejects.toMatchObject({
      code: "CODEG_API_REVISION_UNSUPPORTED",
    })
  })
})
