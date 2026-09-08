import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, expect, it, vi } from "vitest"
import messages from "@/i18n/messages/en.json"
import { ClientFolderConfiguration } from "./client-folder-configuration"

const api = vi.hoisted(() => ({
  queryCerebroFolderConfiguration: vi.fn(),
  listCerebroConfigurationProjects: vi.fn(),
  listCerebroConfigurationModules: vi.fn(),
  saveCerebroFolderConfiguration: vi.fn(),
  getCerebroFolderCredential: vi.fn(),
}))
const events = vi.hoisted(() => ({
  listener: undefined as undefined | ((value: unknown) => void),
}))
vi.mock("@/lib/api", () => api)
vi.mock("@/lib/transport", () => ({
  getTransport: () => ({
    subscribe: async (_: string, listener: typeof events.listener) => {
      events.listener = listener
      return () => {}
    },
  }),
}))
vi.mock("sonner", () => ({ toast: { success: vi.fn() } }))
const initial = {
  runner_id: "client",
  target_id: "folder",
  execution_module_id: null,
  binding_id: null,
  mcp_module_ids: [],
  mcp_enabled: false,
  module_labels: {},
  grant_id: null,
  credential_expires_at: null,
}
beforeEach(() => {
  vi.clearAllMocks()
  events.listener = undefined
  api.queryCerebroFolderConfiguration.mockResolvedValue({
    configuration: initial,
    error: null,
  })
  api.listCerebroConfigurationProjects.mockResolvedValue({
    items: [],
    total: 0,
  })
})
function mount() {
  render(
    <NextIntlClientProvider locale="en" messages={messages}>
      <ClientFolderConfiguration folderId={1} />
    </NextIntlClientProvider>
  )
}

it("keeps edits when the server rejects save and only clears dirty state after its acknowledgement", async () => {
  api.saveCerebroFolderConfiguration
    .mockRejectedValueOnce(new Error("客户端离线"))
    .mockResolvedValueOnce({ ...initial, mcp_enabled: true })
  mount()
  const enabled = await screen.findByRole("checkbox")
  fireEvent.click(enabled)
  const save = screen.getByRole("button", { name: messages.CerebroFolder.save })
  fireEvent.click(save)
  expect(await screen.findByRole("alert")).toHaveTextContent("客户端离线")
  expect(enabled).toBeChecked()
  expect(save).toBeEnabled()
  fireEvent.click(save)
  await waitFor(() => expect(save).toBeDisabled())
  expect(api.saveCerebroFolderConfiguration).toHaveBeenLastCalledWith(1, {
    execution_module_id: null,
    mcp_module_ids: [],
    mcp_enabled: true,
  })
})

it("shows service updates while preserving a user's in-progress edit", async () => {
  mount()
  const enabled = await screen.findByRole("checkbox")
  await waitFor(() => expect(events.listener).toBeDefined())
  act(() => events.listener?.({ ...initial, mcp_enabled: true }))
  expect(enabled).toBeChecked()
  fireEvent.click(enabled)
  act(() => events.listener?.({ ...initial, mcp_enabled: true }))
  expect(enabled).not.toBeChecked()
  expect(
    screen.getByRole("button", { name: messages.CerebroFolder.save })
  ).toBeEnabled()
})
