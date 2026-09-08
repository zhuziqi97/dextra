import { describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock("@/lib/transport", () => ({
  getTransport: () => ({ call: mocks.call }),
  getShellTransport: () => ({ call: vi.fn() }),
  isDesktop: () => false,
  isRemoteDesktopMode: () => false,
  getActiveRemoteConnectionId: () => null,
  notifyRemoteDesktopUnauthorized: vi.fn(),
}))

import { acpConnect } from "./api"

describe("Cerebro launch error boundary", () => {
  it("preserves the structured denial as an Error for connection consumers", async () => {
    const denied = { code: "configuration_invalid", message: "Binding disabled", detail: "BINDING_NOT_ACTIVE" }
    mocks.call.mockRejectedValue(denied)
    await expect(acpConnect("codex", "/repo")).rejects.toMatchObject(denied)
    await expect(acpConnect("codex", "/repo")).rejects.toBeInstanceOf(Error)
  })

  it("preserves existing unstructured connection failures", async () => {
    const failure = new Error("provider process exited")
    mocks.call.mockRejectedValue(failure)
    await expect(acpConnect("codex", "/repo")).rejects.toBe(failure)
  })
})
