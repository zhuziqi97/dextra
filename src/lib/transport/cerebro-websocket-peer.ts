import { randomUUID } from "@/lib/utils"
import type {
  CerebroPlatformPeer,
  CerebroRunnerHello,
  CerebroStreamAttachRequest,
} from "./cerebro-remote-transport"
import { CerebroTransportError } from "./cerebro-remote-transport"
import type { CerebroPlatformOperation } from "./cerebro-command-registry"
import type { AttachHandlers, CallOptions, UnsubscribeFn } from "./types"

interface PendingRequest {
  resolve: (value: unknown) => void
  reject: (error: Error) => void
  timeout: ReturnType<typeof setTimeout>
}

interface RemoteError {
  CODE?: string
  MESSAGE?: string
}

interface RemoteMessage {
  TYPE?: string
  CORRELATION_ID?: string
  PAYLOAD?: {
    SNAPSHOT?: RemoteWorkspaceSnapshot
    RESULT?: unknown
    ERROR?: RemoteError
    FRAME?: Record<string, unknown>
  }
}

export interface RemoteWorkspaceSnapshot {
  type: "workspace"
  rootPath: "/workspace"
  workspace: string
  repository: string | null
  branch: string | null
  workTaskId: string | null
  runnerHello: CerebroRunnerHello
  status: "READY"
}

function websocketUrl(platformOrigin: string): string {
  const url = new URL("/api/v1/remote-workbench-sessions/ws", platformOrigin)
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:"
  return url.toString()
}

/**
 * 平台托管的 Codeg 子应用只拿一次性 ticket，并通过当前平台 origin 建立
 * owner relay。它不知道 Dextra 地址、本地 bearer 或设备凭据。
 */
export class CerebroWebSocketPeer implements CerebroPlatformPeer {
  private readonly socket: WebSocket
  private readonly pending = new Map<string, PendingRequest>()
  private readonly channelHandlers = new Map<
    string,
    (payload: unknown) => void
  >()
  private readonly streamHandlers = new Map<string, AttachHandlers>()
  private snapshot: RemoteWorkspaceSnapshot | null = null
  private readyResolve!: () => void
  private readyReject!: (error: Error) => void
  private readonly ready: Promise<void>
  private destroyed = false

  constructor(platformOrigin: string, ticket: string) {
    this.ready = new Promise<void>((resolve, reject) => {
      this.readyResolve = resolve
      this.readyReject = reject
    })
    this.socket = new WebSocket(websocketUrl(platformOrigin))
    this.socket.addEventListener("open", () => {
      this.socket.send(JSON.stringify({ TYPE: "AUTHENTICATE", TICKET: ticket }))
    })
    this.socket.addEventListener("message", (event) =>
      this.handleMessage(event.data)
    )
    this.socket.addEventListener("error", () => {
      this.failAll(
        new CerebroTransportError(
          "REMOTE_CONNECTION_FAILED",
          "远程工作台连接失败"
        )
      )
    })
    this.socket.addEventListener("close", () => {
      this.destroyed = true
      this.failAll(
        new CerebroTransportError(
          "REMOTE_CONNECTION_CLOSED",
          "远程工作台连接已关闭"
        )
      )
    })
  }

  async runnerHello(): Promise<CerebroRunnerHello> {
    await this.ready
    if (!this.snapshot) {
      throw new CerebroTransportError(
        "REMOTE_ATTACH_FAILED",
        "Dextra 未返回工作台快照"
      )
    }
    return this.snapshot.runnerHello
  }

  async invokeOperation<T>(
    operation: CerebroPlatformOperation,
    codegCommand: string
  ): Promise<T> {
    throw new CerebroTransportError(
      "CODEG_COMMAND_REQUIRES_OPERATION",
      `${codegCommand} 需要平台 operation ${operation}`
    )
  }

  async invokeRelay<T>(
    codegCommand: string,
    args: Record<string, unknown>,
    options?: CallOptions
  ): Promise<T> {
    return this.request<T>(
      { KIND: "CALL", NAME: codegCommand },
      args,
      options?.timeoutMs
    )
  }

  async subscribeChannel<T>(
    channel: string,
    handler: (payload: T) => void
  ): Promise<UnsubscribeFn> {
    const subscriptionId = randomUUID()
    this.channelHandlers.set(
      subscriptionId,
      handler as (payload: unknown) => void
    )
    try {
      await this.request(
        { KIND: "CHANNEL_SUBSCRIBE", NAME: channel },
        { subscriptionId }
      )
    } catch (error) {
      this.channelHandlers.delete(subscriptionId)
      throw error
    }
    return () => {
      this.channelHandlers.delete(subscriptionId)
      void this.request(
        { KIND: "CHANNEL_UNSUBSCRIBE", NAME: channel },
        { subscriptionId }
      ).catch(() => undefined)
    }
  }

  async attachStream(
    request: CerebroStreamAttachRequest,
    handlers: AttachHandlers
  ): Promise<UnsubscribeFn> {
    this.streamHandlers.set(request.subscriptionId, handlers)
    try {
      await this.request(
        { KIND: "STREAM_ATTACH", NAME: "acp_session" },
        {
          subscriptionId: request.subscriptionId,
          connectionId: request.connectionId,
          sinceSeq: request.sinceSeq,
        }
      )
    } catch (error) {
      this.streamHandlers.delete(request.subscriptionId)
      throw error
    }
    return () => {
      this.streamHandlers.delete(request.subscriptionId)
      void this.request(
        { KIND: "STREAM_DETACH", NAME: "acp_session" },
        { subscriptionId: request.subscriptionId }
      ).catch(() => undefined)
    }
  }

  async waitForReady(): Promise<void> {
    await this.ready
  }

  onReconnect(): UnsubscribeFn {
    return () => undefined
  }

  workspaceSnapshot(): RemoteWorkspaceSnapshot | null {
    return this.snapshot
  }

  destroy(): void {
    if (this.destroyed) return
    this.destroyed = true
    if (this.socket.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify({ TYPE: "REMOTE_DETACH" }))
      this.socket.close(1000, "workbench closed")
    } else {
      this.socket.close()
    }
    this.failAll(
      new CerebroTransportError(
        "CEREBRO_TRANSPORT_DESTROYED",
        "远程工作台已销毁"
      )
    )
  }

  private request<T>(
    commandIdentity: { KIND: string; NAME: string },
    argumentsValue: Record<string, unknown>,
    timeoutMs = 30_000
  ): Promise<T> {
    return this.ready.then(
      () =>
        new Promise<T>((resolve, reject) => {
          if (this.destroyed || this.socket.readyState !== WebSocket.OPEN) {
            reject(
              new CerebroTransportError(
                "REMOTE_CONNECTION_CLOSED",
                "远程工作台连接已关闭"
              )
            )
            return
          }
          const requestId = randomUUID()
          const timeout = setTimeout(() => {
            this.pending.delete(requestId)
            reject(
              new CerebroTransportError(
                "REMOTE_REQUEST_TIMEOUT",
                "远程调用超时"
              )
            )
          }, timeoutMs)
          this.pending.set(requestId, {
            resolve: resolve as (value: unknown) => void,
            reject,
            timeout,
          })
          this.socket.send(
            JSON.stringify({
              TYPE: "REMOTE_REQUEST",
              REQUEST_ID: requestId,
              COMMAND_IDENTITY: commandIdentity,
              ARGUMENTS: argumentsValue,
            })
          )
        })
    )
  }

  private handleMessage(raw: unknown): void {
    if (typeof raw !== "string") return
    let message: RemoteMessage
    try {
      message = JSON.parse(raw) as RemoteMessage
    } catch {
      return
    }
    if (message.TYPE === "REMOTE_ATTACHED" && message.PAYLOAD?.SNAPSHOT) {
      this.snapshot = message.PAYLOAD.SNAPSHOT
      this.readyResolve()
      return
    }
    if (message.TYPE === "REMOTE_ATTACH_FAILED") {
      const error = this.toError(message.PAYLOAD?.ERROR)
      this.readyReject(error)
      this.failAll(error)
      return
    }
    if (message.TYPE === "REMOTE_RESPONSE" && message.CORRELATION_ID) {
      const pending = this.pending.get(message.CORRELATION_ID)
      if (!pending) return
      this.pending.delete(message.CORRELATION_ID)
      clearTimeout(pending.timeout)
      if (message.PAYLOAD?.ERROR)
        pending.reject(this.toError(message.PAYLOAD.ERROR))
      else pending.resolve(message.PAYLOAD?.RESULT)
      return
    }
    if (message.TYPE === "REMOTE_EVENT" && message.PAYLOAD?.FRAME) {
      this.dispatchEvent(message.PAYLOAD.FRAME)
    }
  }

  private dispatchEvent(frame: Record<string, unknown>): void {
    const type = frame.type
    const subscriptionId =
      typeof frame.subscriptionId === "string"
        ? frame.subscriptionId
        : typeof frame.subscription_id === "string"
          ? frame.subscription_id
          : null
    if (!subscriptionId) return
    if (type === "channel_event") {
      this.channelHandlers.get(subscriptionId)?.(frame.payload)
      return
    }
    const handlers = this.streamHandlers.get(subscriptionId)
    if (!handlers) return
    if (type === "snapshot" && typeof frame.event_seq === "number") {
      handlers.onSnapshot(
        frame.snapshot as Parameters<AttachHandlers["onSnapshot"]>[0],
        frame.event_seq
      )
    } else if (type === "replay" && typeof frame.high_water_seq === "number") {
      handlers.onReplay(
        frame.events as Parameters<AttachHandlers["onReplay"]>[0],
        frame.high_water_seq
      )
    } else if (type === "event") {
      handlers.onEvent(
        frame.envelope as Parameters<AttachHandlers["onEvent"]>[0]
      )
    } else if (type === "detached" && typeof frame.reason === "string") {
      handlers.onDetached(
        frame.reason as Parameters<AttachHandlers["onDetached"]>[0]
      )
    }
  }

  private toError(error?: RemoteError): CerebroTransportError {
    return new CerebroTransportError(
      error?.CODE || "REMOTE_OPERATION_FAILED",
      error?.MESSAGE || "远程操作失败"
    )
  }

  private failAll(error: Error): void {
    this.readyReject(error)
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timeout)
      pending.reject(error)
    }
    this.pending.clear()
  }
}
