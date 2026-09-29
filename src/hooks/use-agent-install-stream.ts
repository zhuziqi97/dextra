import { useCallback, useRef, useState } from "react"
import { onTransportDisconnect, subscribe } from "@/lib/platform"
import type { AgentInstallEvent, AgentInstallEventKind } from "@/lib/types"

const AGENT_INSTALL_EVENT = "app://agent-install"

export type AgentInstallStatus =
  | "idle"
  | "running"
  | "success"
  | "failed"
  | "disconnected"
  | "unknown"

export type AgentInstallTerminal =
  | { kind: "completed"; payload: string }
  | { kind: "failed"; payload: string }
  | { kind: "disconnected"; payload: "" }
  | { kind: "unknown"; payload: "" }

interface PendingInstall {
  taskId: string
  started: boolean
  disconnected: boolean
  waitingForTerminal: boolean
  finished: Promise<AgentInstallTerminal>
  resolve: (result: AgentInstallTerminal) => void
  result: AgentInstallTerminal | null
}

interface AgentInstallStreamState {
  status: AgentInstallStatus
  logs: string[]
  error: string | null
}

export function useAgentInstallStream() {
  const [state, setState] = useState<AgentInstallStreamState>({
    status: "idle",
    logs: [],
    error: null,
  })
  const unsubRef = useRef<(() => void) | null>(null)
  const disconnectUnsubRef = useRef<(() => void) | null>(null)
  const pendingRef = useRef<PendingInstall | null>(null)

  const settle = useCallback((result: AgentInstallTerminal) => {
    const pending = pendingRef.current
    if (!pending || pending.result) return
    pending.result = result
    pending.resolve(result)
    unsubRef.current?.()
    unsubRef.current = null
    disconnectUnsubRef.current?.()
    disconnectUnsubRef.current = null
  }, [])

  const start = useCallback(
    async (taskId: string) => {
      settle({ kind: "unknown", payload: "" })
      let resolve!: (result: AgentInstallTerminal) => void
      const finished = new Promise<AgentInstallTerminal>((next) => {
        resolve = next
      })
      pendingRef.current = {
        taskId,
        started: false,
        disconnected: false,
        waitingForTerminal: false,
        finished,
        resolve,
        result: null,
      }
      setState({ status: "running", logs: [], error: null })

      unsubRef.current?.()
      const unsub = await subscribe<AgentInstallEvent>(
        AGENT_INSTALL_EVENT,
        (event) => {
          if (event.task_id !== taskId) return
          const pending = pendingRef.current
          if (!pending || pending.taskId !== taskId) return
          pending.started = true

          switch (event.kind as AgentInstallEventKind) {
            case "started":
              setState((prev) => ({ ...prev, status: "running" }))
              break
            case "log":
              setState((prev) => ({
                ...prev,
                logs: [...prev.logs, event.payload],
              }))
              break
            case "completed":
              setState((prev) => ({
                ...prev,
                status: "success",
                logs: [...prev.logs, event.payload],
              }))
              settle({ kind: "completed", payload: event.payload })
              break
            case "failed":
              setState((prev) => ({
                ...prev,
                status: "failed",
                error: event.payload,
                logs: [...prev.logs, `ERROR: ${event.payload}`],
              }))
              settle({ kind: "failed", payload: event.payload })
              break
          }
        }
      ).catch((error) => {
        setState((prev) => ({ ...prev, status: "unknown" }))
        settle({ kind: "unknown", payload: "" })
        throw error
      })

      if (pendingRef.current?.taskId !== taskId || pendingRef.current.result) {
        unsub()
        return
      }
      unsubRef.current = unsub
      disconnectUnsubRef.current = onTransportDisconnect(() => {
        const pending = pendingRef.current
        if (pending?.taskId !== taskId || pending.result) return
        pending.disconnected = true
        setState((prev) => ({ ...prev, status: "disconnected" }))
        if (pending.waitingForTerminal) {
          settle({ kind: "disconnected", payload: "" })
        }
      })
    },
    [settle]
  )

  const wasStarted = useCallback(
    (taskId: string) =>
      pendingRef.current?.taskId === taskId && pendingRef.current.started,
    []
  )

  const wasDisconnected = useCallback(
    (taskId: string) =>
      pendingRef.current?.taskId === taskId && pendingRef.current.disconnected,
    []
  )

  const waitForTerminal = useCallback(
    (taskId: string) => {
      const pending = pendingRef.current
      if (pending?.taskId !== taskId) {
        return Promise.resolve<AgentInstallTerminal>({
          kind: "unknown",
          payload: "",
        })
      }
      pending.waitingForTerminal = true
      if (pending.disconnected && !pending.result) {
        setState((prev) => ({ ...prev, status: "disconnected" }))
        settle({ kind: "disconnected", payload: "" })
      }
      return pending.finished
    },
    [settle]
  )

  const confirmSuccess = useCallback(
    (taskId: string) => {
      if (pendingRef.current?.taskId !== taskId) return
      setState((prev) => ({ ...prev, status: "success" }))
      settle({ kind: "completed", payload: "" })
    },
    [settle]
  )

  const reset = useCallback(() => {
    settle({ kind: "unknown", payload: "" })
    pendingRef.current = null
    setState({ status: "idle", logs: [], error: null })
  }, [settle])

  return {
    ...state,
    start,
    reset,
    wasStarted,
    wasDisconnected,
    waitForTerminal,
    confirmSuccess,
  }
}
