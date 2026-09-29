import { act, renderHook } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { AgentInstallEvent } from "@/lib/types"
import { useAgentInstallStream } from "./use-agent-install-stream"

const platform = vi.hoisted(() => ({
  subscribe: vi.fn(),
  onTransportDisconnect: vi.fn(),
}))

vi.mock("@/lib/platform", () => platform)

let onInstall: (event: AgentInstallEvent) => void
let onDisconnect: () => void

beforeEach(() => {
  platform.subscribe.mockReset()
  platform.onTransportDisconnect.mockReset()
  platform.subscribe.mockImplementation(
    async (_channel: string, handler: typeof onInstall) => {
      onInstall = handler
      return vi.fn()
    }
  )
  platform.onTransportDisconnect.mockImplementation((handler: () => void) => {
    onDisconnect = handler
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

  it("reports a disconnected event stream after a task starts", async () => {
    const { result, unmount } = renderHook(() => useAgentInstallStream())
    await act(async () => result.current.start("install-3"))
    act(() => onInstall({ task_id: "install-3", kind: "started", payload: "" }))
    const terminal = result.current.waitForTerminal("install-3")
    act(() => onDisconnect())
    await expect(terminal).resolves.toEqual({
      kind: "disconnected",
      payload: "",
    })
    expect(result.current.status).toBe("disconnected")
    unmount()
  })

  it("records a disconnect before the caller waits for a terminal result", async () => {
    const { result, unmount } = renderHook(() => useAgentInstallStream())
    await act(async () => result.current.start("install-other"))
    act(() =>
      onInstall({ task_id: "install-other", kind: "started", payload: "" })
    )
    act(() => onDisconnect())
    expect(result.current.status).toBe("disconnected")
    expect(result.current.wasDisconnected("install-other")).toBe(true)
    let terminal!: Promise<unknown>
    act(() => {
      terminal = result.current.waitForTerminal("install-other")
    })
    await expect(terminal).resolves.toEqual({
      kind: "disconnected",
      payload: "",
    })
    expect(result.current.status).toBe("disconnected")
    unmount()
  })

  it("reports a disconnect even when no started event was received", async () => {
    const { result, unmount } = renderHook(() => useAgentInstallStream())
    await act(async () => result.current.start("install-no-start"))
    act(() => onDisconnect())
    expect(result.current.wasStarted("install-no-start")).toBe(false)
    expect(result.current.wasDisconnected("install-no-start")).toBe(true)
    expect(result.current.status).toBe("disconnected")
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
