import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { EventEnvelope, LiveSessionSnapshot } from "@/lib/types"
import { CerebroWebSocketPeer } from "./cerebro-websocket-peer"

type Listener = (event: { data?: unknown }) => void

class MockWebSocket {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 3
  static instances: MockWebSocket[] = []

  readonly sent: string[] = []
  readonly listeners = new Map<string, Listener[]>()
  readyState = MockWebSocket.CONNECTING
  closeCode: number | undefined

  constructor(readonly url: string) {
    MockWebSocket.instances.push(this)
  }

  addEventListener(type: string, listener: Listener): void {
    const listeners = this.listeners.get(type) ?? []
    listeners.push(listener)
    this.listeners.set(type, listeners)
  }

  send(data: string): void {
    this.sent.push(data)
  }

  close(code?: number): void {
    this.closeCode = code
    this.readyState = MockWebSocket.CLOSED
  }

  open(): void {
    this.readyState = MockWebSocket.OPEN
    this.emit("open")
  }

  receive(message: Record<string, unknown>): void {
    this.emit("message", { data: JSON.stringify(message) })
  }

  private emit(type: string, event: { data?: unknown } = {}): void {
    for (const listener of this.listeners.get(type) ?? []) listener(event)
  }
}

function socket(): MockWebSocket {
  const instance = MockWebSocket.instances[MockWebSocket.instances.length - 1]
  if (!instance) throw new Error("WebSocket was not created")
  return instance
}

function sentFrame(ws: MockWebSocket, index: number): Record<string, unknown> {
  return JSON.parse(ws.sent[index]) as Record<string, unknown>
}

function attach(ws: MockWebSocket): void {
  ws.receive({
    TYPE: "REMOTE_ATTACHED",
    CORRELATION_ID: "attach-1",
    PAYLOAD: {
      SNAPSHOT: {
        type: "workspace",
        rootPath: "/workspace",
        workspace: "Demo",
        repository: "owner/repo",
        branch: "codex/task",
        workTaskId: "42",
        status: "READY",
        runnerHello: {
          PROTOCOL_VERSION: 1,
          TYPE: "HELLO",
          MESSAGE_ID: "hello-1",
          OCCURRED_AT: "2026-09-04T00:00:00Z",
          PAYLOAD: {
            RUNNER_BUILD_ID: "dextra-p0-c",
            CODEG_API_REVISION: 1,
            CODEG_UPSTREAM_VERSION: "v0.29.0",
            CODEG_UPSTREAM_COMMIT: "769610c626f1fc4b18c11d3e289326acf097b99f",
          },
        },
      },
    },
  })
}

beforeEach(() => {
  MockWebSocket.instances = []
  vi.stubGlobal("WebSocket", MockWebSocket)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe("CerebroWebSocketPeer", () => {
  it("只在首帧发送 ticket，并让单次调用错误不影响后续调用", async () => {
    const peer = new CerebroWebSocketPeer(
      "https://platform.example/base",
      "ticket-one"
    )
    const ws = socket()

    expect(ws.url).toBe(
      "wss://platform.example/api/v1/remote-workbench-sessions/ws"
    )
    ws.open()
    expect(sentFrame(ws, 0)).toEqual({
      TYPE: "AUTHENTICATE",
      TICKET: "ticket-one",
    })
    attach(ws)
    expect((await peer.runnerHello()).TYPE).toBe("HELLO")
    expect(peer.workspaceSnapshot()?.workTaskId).toBe("42")

    const failed = peer.invokeRelay("read_file_preview", { path: "README.md" })
    await vi.waitFor(() => expect(ws.sent).toHaveLength(2))
    const failedRequest = sentFrame(ws, 1)
    expect(JSON.stringify(failedRequest)).not.toContain("ticket-one")
    ws.receive({
      TYPE: "REMOTE_RESPONSE",
      CORRELATION_ID: failedRequest.REQUEST_ID,
      PAYLOAD: { ERROR: { CODE: "FILE_NOT_FOUND", MESSAGE: "文件不存在" } },
    })
    await expect(failed).rejects.toMatchObject({ code: "FILE_NOT_FOUND" })

    const succeeded = peer.invokeRelay<{ content: string }>(
      "read_file_preview",
      {
        path: "README.md",
      }
    )
    await vi.waitFor(() => expect(ws.sent).toHaveLength(3))
    const successfulRequest = sentFrame(ws, 2)
    ws.receive({
      TYPE: "REMOTE_RESPONSE",
      CORRELATION_ID: successfulRequest.REQUEST_ID,
      PAYLOAD: { RESULT: { content: "ok" } },
    })
    await expect(succeeded).resolves.toEqual({ content: "ok" })
  })

  it("按订阅路由 channel 与 stream 事件，并在销毁时 detach", async () => {
    const peer = new CerebroWebSocketPeer(
      "http://platform.example",
      "ticket-two"
    )
    const ws = socket()
    ws.open()
    attach(ws)

    const channelHandler = vi.fn()
    const channelSubscription = peer.subscribeChannel(
      "terminal://output/terminal-1",
      channelHandler
    )
    await vi.waitFor(() => expect(ws.sent).toHaveLength(2))
    const channelRequest = sentFrame(ws, 1)
    const channelArgs = channelRequest.ARGUMENTS as { subscriptionId: string }
    ws.receive({
      TYPE: "REMOTE_RESPONSE",
      CORRELATION_ID: channelRequest.REQUEST_ID,
      PAYLOAD: { RESULT: null },
    })
    const unsubscribeChannel = await channelSubscription
    ws.receive({
      TYPE: "REMOTE_EVENT",
      PAYLOAD: {
        FRAME: {
          type: "channel_event",
          subscriptionId: channelArgs.subscriptionId,
          payload: { chunk: "hello" },
        },
      },
    })
    expect(channelHandler).toHaveBeenCalledWith({ chunk: "hello" })

    const handlers = {
      onSnapshot: vi.fn(),
      onReplay: vi.fn(),
      onEvent: vi.fn(),
      onDetached: vi.fn(),
    }
    const streamSubscription = peer.attachStream(
      { subscriptionId: "stream-1", connectionId: "connection-1" },
      handlers
    )
    await vi.waitFor(() => expect(ws.sent).toHaveLength(3))
    const streamRequest = sentFrame(ws, 2)
    ws.receive({
      TYPE: "REMOTE_RESPONSE",
      CORRELATION_ID: streamRequest.REQUEST_ID,
      PAYLOAD: { RESULT: null },
    })
    const unsubscribeStream = await streamSubscription

    const snapshot = { connection_id: "connection-1" } as LiveSessionSnapshot
    const envelope = { seq: 8, type: "status_changed" } as EventEnvelope
    ws.receive({
      TYPE: "REMOTE_EVENT",
      PAYLOAD: {
        FRAME: {
          type: "snapshot",
          subscriptionId: "stream-1",
          snapshot,
          event_seq: 7,
        },
      },
    })
    ws.receive({
      TYPE: "REMOTE_EVENT",
      PAYLOAD: {
        FRAME: {
          type: "event",
          subscriptionId: "stream-1",
          envelope,
        },
      },
    })
    expect(handlers.onSnapshot).toHaveBeenCalledWith(snapshot, 7)
    expect(handlers.onEvent).toHaveBeenCalledWith(envelope)

    unsubscribeChannel()
    unsubscribeStream()
    await vi.waitFor(() => expect(ws.sent).toHaveLength(5))
    peer.destroy()
    expect(sentFrame(ws, 5)).toEqual({ TYPE: "REMOTE_DETACH" })
    expect(ws.closeCode).toBe(1000)
  })
})
