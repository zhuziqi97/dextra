import type { EventEnvelope, LiveSessionSnapshot } from "@/lib/types"
import { randomUUID } from "@/lib/utils"
import {
  CODEG_API_REVISION,
  CODEG_UPSTREAM_COMMIT,
  CODEG_UPSTREAM_VERSION,
  CEREBRO_PROTOCOL_VERSION,
  getCerebroChannelPolicy,
  getCerebroCommandPolicy,
  type CerebroPlatformOperation,
} from "./cerebro-command-registry"
import type {
  AttachDetachReason,
  AttachHandlers,
  AttachOptions,
  CallOptions,
  EventStream,
  EventStreamSubscription,
  Transport,
  UnsubscribeFn,
} from "./types"

export interface CerebroRunnerHello {
  PROTOCOL_VERSION: number
  TYPE: "HELLO"
  MESSAGE_ID: string
  OCCURRED_AT: string
  PAYLOAD: {
    RUNNER_BUILD_ID: string
    CODEG_API_REVISION: number
    CODEG_UPSTREAM_VERSION: string
    CODEG_UPSTREAM_COMMIT: string
  }
}

export interface CerebroStreamAttachRequest {
  subscriptionId: string
  connectionId: string
  sinceSeq?: number
}

/**
 * 平台 shell 提供的窄接口。实现可以使用 HTTP/WS、postMessage 或测试内存
 * peer，但不能让 Codeg 子应用取得 Runner URL、本地 bearer 或设备凭据。
 */
export interface CerebroPlatformPeer {
  runnerHello(): Promise<CerebroRunnerHello>
  invokeOperation<T>(
    operation: CerebroPlatformOperation,
    codegCommand: string,
    args: Record<string, unknown>,
    options?: CallOptions
  ): Promise<T>
  invokeRelay<T>(
    codegCommand: string,
    args: Record<string, unknown>,
    options?: CallOptions
  ): Promise<T>
  subscribeChannel<T>(
    channel: string,
    handler: (payload: T) => void
  ): Promise<UnsubscribeFn>
  attachStream(
    request: CerebroStreamAttachRequest,
    handlers: AttachHandlers
  ): Promise<UnsubscribeFn>
  waitForReady?(): Promise<void>
  onReconnect?(callback: () => void): UnsubscribeFn
  destroy?(): void
}

export class CerebroTransportError extends Error {
  constructor(
    readonly code: string,
    message: string
  ) {
    super(message)
    this.name = "CerebroTransportError"
  }
}

interface ActiveStream {
  subscriptionId: string
  connectionId: string
  lastAppliedSeq?: number
  handlers: AttachHandlers
  generation: number
  active: boolean
  unsubscribe?: UnsubscribeFn
}

class CerebroEventStream implements EventStream {
  private readonly active = new Map<string, ActiveStream>()

  constructor(
    private readonly peer: CerebroPlatformPeer,
    private readonly ensureReady: () => Promise<void>
  ) {}

  attach(
    connectionId: string,
    options: AttachOptions,
    handlers: AttachHandlers
  ): EventStreamSubscription {
    const subscriptionId = randomUUID()
    const stream: ActiveStream = {
      subscriptionId,
      connectionId,
      lastAppliedSeq: options.sinceSeq,
      handlers,
      generation: 0,
      active: true,
    }
    this.active.set(subscriptionId, stream)
    void this.start(stream)

    return {
      subscriptionId,
      detach: () => this.detach(subscriptionId),
    }
  }

  async reattachAll(): Promise<void> {
    await Promise.all(
      [...this.active.values()]
        .filter((stream) => stream.active)
        .map((stream) => this.start(stream))
    )
  }

  destroy(): void {
    for (const subscriptionId of [...this.active.keys()]) {
      this.detach(subscriptionId)
    }
  }

  private detach(subscriptionId: string): void {
    const stream = this.active.get(subscriptionId)
    if (!stream || !stream.active) return
    stream.active = false
    stream.generation += 1
    this.active.delete(subscriptionId)
    stream.unsubscribe?.()
    stream.unsubscribe = undefined
  }

  private async start(stream: ActiveStream): Promise<void> {
    stream.generation += 1
    const generation = stream.generation
    stream.unsubscribe?.()
    stream.unsubscribe = undefined

    await this.ensureReady()
    if (!stream.active || generation !== stream.generation) return

    const unsubscribe = await this.peer.attachStream(
      {
        subscriptionId: stream.subscriptionId,
        connectionId: stream.connectionId,
        sinceSeq: stream.lastAppliedSeq,
      },
      {
        onSnapshot: (snapshot: LiveSessionSnapshot, eventSeq: number) => {
          if (!stream.active || generation !== stream.generation) return
          stream.lastAppliedSeq = eventSeq
          stream.handlers.onSnapshot(snapshot, eventSeq)
        },
        onReplay: (events: EventEnvelope[], highWaterSeq: number) => {
          if (!stream.active || generation !== stream.generation) return
          stream.lastAppliedSeq = highWaterSeq
          stream.handlers.onReplay(events, highWaterSeq)
        },
        onEvent: (envelope: EventEnvelope) => {
          if (!stream.active || generation !== stream.generation) return
          if (
            stream.lastAppliedSeq !== undefined &&
            envelope.seq <= stream.lastAppliedSeq
          ) {
            return
          }
          stream.lastAppliedSeq = envelope.seq
          stream.handlers.onEvent(envelope)
        },
        onDetached: (reason: AttachDetachReason) => {
          if (!stream.active || generation !== stream.generation) return
          stream.handlers.onDetached(reason)
        },
      }
    )

    if (!stream.active || generation !== stream.generation) {
      unsubscribe()
      return
    }
    stream.unsubscribe = unsubscribe
  }
}

/**
 * Codeg Web 子应用到 Cerebro command router 的唯一 Transport。
 *
 * 写操作进入平台领域 operation；只有 registry 明确登记的读取和事件流
 * 才能 Relay。未知、敏感或本机 OS 命令在离开浏览器前就失败。
 */
export class CerebroRemoteTransport implements Transport {
  private handshakePromise: Promise<void> | null = null
  private readonly reconnectCallbacks = new Set<() => void>()
  private readonly stream: CerebroEventStream
  private readonly unsubscribeReconnect?: UnsubscribeFn
  private destroyed = false

  constructor(private readonly peer: CerebroPlatformPeer) {
    this.stream = new CerebroEventStream(peer, () => this.ensureHandshake())
    this.unsubscribeReconnect = peer.onReconnect?.(() => {
      void this.handleReconnect()
    })
  }

  async call<T>(
    command: string,
    args: Record<string, unknown> = {},
    options?: CallOptions
  ): Promise<T> {
    await this.ensureHandshake()
    const policy = getCerebroCommandPolicy(command)
    if (!policy) {
      throw new CerebroTransportError(
        "CODEG_COMMAND_NOT_REMOTE",
        `Command ${command} is not registered for Cerebro remote use`
      )
    }
    if (policy.route === "DENY") {
      throw new CerebroTransportError(
        "CODEG_COMMAND_REMOTE_DENIED",
        `Command group ${policy.group} is not available remotely`
      )
    }

    const boundedOptions = {
      ...options,
      timeoutMs: Math.min(
        options?.timeoutMs ?? policy.timeoutMs,
        policy.timeoutMs
      ),
    }
    if (policy.route === "OPERATION") {
      return this.peer.invokeOperation<T>(
        policy.operation!,
        command,
        args,
        boundedOptions
      )
    }
    return this.peer.invokeRelay<T>(command, args, boundedOptions)
  }

  async subscribe<T>(
    event: string,
    handler: (payload: T) => void
  ): Promise<UnsubscribeFn> {
    await this.ensureHandshake()
    if (!getCerebroChannelPolicy(event)) {
      throw new CerebroTransportError(
        "CODEG_CHANNEL_NOT_REMOTE",
        `Channel ${event} is not registered for Cerebro remote use`
      )
    }
    return this.peer.subscribeChannel(event, handler)
  }

  isDesktop(): boolean {
    return false
  }

  onReconnect(callback: () => void): UnsubscribeFn {
    this.reconnectCallbacks.add(callback)
    return () => this.reconnectCallbacks.delete(callback)
  }

  async waitForReady(): Promise<void> {
    await this.peer.waitForReady?.()
    await this.ensureHandshake()
  }

  eventStream(): EventStream {
    return this.stream
  }

  destroy(): void {
    if (this.destroyed) return
    this.destroyed = true
    this.stream.destroy()
    this.unsubscribeReconnect?.()
    this.peer.destroy?.()
    this.reconnectCallbacks.clear()
  }

  private ensureHandshake(): Promise<void> {
    if (this.destroyed) {
      return Promise.reject(
        new CerebroTransportError(
          "CEREBRO_TRANSPORT_DESTROYED",
          "Cerebro transport has been destroyed"
        )
      )
    }
    if (!this.handshakePromise) {
      this.handshakePromise = this.peer.runnerHello().then((hello) => {
        if (hello.PROTOCOL_VERSION !== CEREBRO_PROTOCOL_VERSION) {
          throw new CerebroTransportError(
            "PROTOCOL_VERSION_UNSUPPORTED",
            `Expected protocol ${CEREBRO_PROTOCOL_VERSION}, received ${hello.PROTOCOL_VERSION}`
          )
        }
        if (hello.PAYLOAD.CODEG_API_REVISION !== CODEG_API_REVISION) {
          throw new CerebroTransportError(
            "CODEG_API_REVISION_UNSUPPORTED",
            `Expected Codeg API revision ${CODEG_API_REVISION}, received ${hello.PAYLOAD.CODEG_API_REVISION}`
          )
        }
        if (
          hello.PAYLOAD.CODEG_UPSTREAM_VERSION !== CODEG_UPSTREAM_VERSION ||
          hello.PAYLOAD.CODEG_UPSTREAM_COMMIT !== CODEG_UPSTREAM_COMMIT
        ) {
          throw new CerebroTransportError(
            "CODEG_UPSTREAM_MISMATCH",
            "Dextra Web bundle and Runner use different Codeg baselines"
          )
        }
      })
    }
    return this.handshakePromise
  }

  private async handleReconnect(): Promise<void> {
    if (this.destroyed) return
    this.handshakePromise = null
    try {
      await this.ensureHandshake()
      await this.stream.reattachAll()
      for (const callback of this.reconnectCallbacks) callback()
    } catch (error) {
      console.error("[CerebroRemoteTransport] reconnect failed:", error)
    }
  }
}
