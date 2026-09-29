import { act, renderHook } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { AgentInstallEvent } from "@/lib/types"
import { useAgentInstallStream } from "./use-agent-install-stream"

const platform = vi.hoisted(() => ({
  subscribe: vi.fn(),
  onTransportReconnect: vi.fn(),
}))

vi.mock("@/lib/platform", () => platform)

let onInstall: (event: AgentInstallEvent) => void
let onReconnect: () => void

beforeEach(() => {
  platform.subscribe.mockReset()
  platform.onTransportReconnect.mockReset()
  platform.subscribe.mockImplementation(
    async (_channel: string, handler: typeof onInstall) => {
      onInstall = handler
      return vi.fn()
    }
  )
  platform.onTransportReconnect.mockImplementation((handler: () => void) => {
    onReconnect = handler
    return vi.fn()
  })
})

afterEach(() => {
  vi.clearAllMocks()
})

describe("useAgentInstallStream", () => {
  it("waits for the npm task's completed event after a started event", async () => {
    const { result, unmount } = renderHook(() => useAgentInstallStream())
    await act(async () => result.current.start("install-1"))
    act(() => onInstall({ task_id: "install-1", kind: "started", payload: "" }))
    expect(result.current.wasStarted("install-1")).toBe(true)

    let settled = false
    const terminal = result.current
      .waitForTerminal("install-1")
      .then((value) => {
        settled = true
        return value
      })
    await Promise.resolve()
    expect(settled).toBe(false)
    act(() =>
      onInstall({
        task_id: "install-1",
        kind: "completed",
        payload: "Codex v1.13.1 installed successfully",
      })
    )
    await expect(terminal).resolves.toMatchObject({ kind: "completed" })
    expect(result.current.status).toBe("success")
    unmount()
  })

  it("uses the task's failed event as the installation failure", async () => {
    const { result, unmount } = renderHook(() => useAgentInstallStream())
    await act(async () => result.current.start("install-2"))
    act(() => onInstall({ task_id: "install-2", kind: "started", payload: "" }))
    const terminal = result.current.waitForTerminal("install-2")
    act(() =>
      onInstall({
        task_id: "install-2",
        kind: "failed",
        payload: "npm exited with code 1",
      })
    )
    await expect(terminal).resolves.toEqual({
      kind: "failed",
      payload: "npm exited with code 1",
    })
    expect(result.current.status).toBe("failed")
    unmount()
  })

  it("reports an unknown result if the event stream reconnects", async () => {
    const { result, unmount } = renderHook(() => useAgentInstallStream())
    await act(async () => result.current.start("install-3"))
    act(() => onInstall({ task_id: "install-3", kind: "started", payload: "" }))
    const terminal = result.current.waitForTerminal("install-3")
    act(() => onReconnect())
    await expect(terminal).resolves.toEqual({ kind: "unknown", payload: "" })
    expect(result.current.status).toBe("unknown")
    unmount()
  })

  it("releases a pending waiter when settings closes", async () => {
    const { result, unmount } = renderHook(() => useAgentInstallStream())
    await act(async () => result.current.start("install-4"))
    const terminal = result.current.waitForTerminal("install-4")
    act(() => result.current.reset())
    await expect(terminal).resolves.toEqual({ kind: "unknown", payload: "" })
    unmount()
  })
})
