import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, expect, it, vi } from "vitest"
import { useClientConfigurationDraft } from "./use-client-configuration-draft"
const fixture = vi.hoisted(() => ({
  query: vi.fn(),
  listener: undefined as undefined | ((value: unknown) => void),
}))
vi.mock("@/lib/api", () => ({ queryCerebroFolderConfiguration: fixture.query }))
vi.mock("@/lib/transport", () => ({
  getTransport: () => ({
    subscribe: async (_: string, listener: typeof fixture.listener) => {
      fixture.listener = listener
      return () => {}
    },
  }),
}))
const original = {
  runner_id: "client",
  target_id: "target",
  execution_module_id: null,
  mcp_enabled: false,
  mcp_scope_modules: [],
  mcp_capabilities: { gitnexus_enabled: false },
  execution_project: null,
  binding_id: null,
  grant_id: null,
  credential_expires_at: null,
  mcp_scope_details: [],
  module_labels: {},
}
beforeEach(() => {
  fixture.listener = undefined
  fixture.query.mockReset()
  fixture.query.mockResolvedValue({ configuration: original, error: null })
})

it("freezes the loaded baseline while edited and becomes clean again after a full revert", async () => {
  const { result } = renderHook(() => useClientConfigurationDraft(1, true))
  await waitFor(() => expect(result.current.baseline).not.toBeNull())
  act(() => result.current.edit({ mcp_enabled: true }))
  expect(result.current.dirty).toBe(true)
  act(() =>
    fixture.listener?.({ ...original, execution_module_id: "another-module" })
  )
  expect(result.current.baseline?.execution_module_id).toBeNull()
  expect(result.current.input.execution_module_id).toBeNull()
  act(() => result.current.edit({ mcp_enabled: false }))
  expect(result.current.dirty).toBe(false)
  act(() =>
    fixture.listener?.({ ...original, execution_module_id: "another-module" })
  )
  expect(result.current.input.execution_module_id).toBe("another-module")
  expect(result.current.dirty).toBe(false)
})

it("keeps cached configuration on a failed read and discards back to the same baseline", async () => {
  fixture.query.mockResolvedValue({
    configuration: { ...original, execution_module_id: "saved-module" },
    error: "Server unavailable",
  })
  const { result } = renderHook(() => useClientConfigurationDraft(1, true))
  await waitFor(() => expect(result.current.error).toBe("Server unavailable"))
  act(() => result.current.edit({ execution_module_id: null }))
  expect(result.current.dirty).toBe(true)
  act(() => result.current.discard())
  expect(result.current.input.execution_module_id).toBe("saved-module")
  expect(result.current.dirty).toBe(false)
})

it("starts a fresh baseline when switching folders instead of carrying the old draft", async () => {
  let loaded: (value: unknown) => void = () => {}
  fixture.query
    .mockResolvedValueOnce({ configuration: original, error: null })
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          loaded = resolve
        })
    )
  const { result, rerender } = renderHook(
    ({ id }) => useClientConfigurationDraft(id, true),
    { initialProps: { id: 1 } }
  )
  await waitFor(() => expect(result.current.baseline).not.toBeNull())
  act(() => result.current.edit({ mcp_enabled: true }))
  rerender({ id: 2 })
  expect(result.current.baseline).toBeNull()
  expect(result.current.input.mcp_enabled).toBe(false)
  await act(async () => {
    loaded({
      configuration: {
        ...original,
        target_id: "second",
        execution_module_id: "module-b",
      },
      error: null,
    })
  })
  expect(result.current.input.execution_module_id).toBe("module-b")
  expect(result.current.dirty).toBe(false)
})
