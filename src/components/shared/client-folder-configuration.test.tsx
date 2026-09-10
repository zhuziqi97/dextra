import { fireEvent, render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { expect, it, vi } from "vitest"
import messages from "@/i18n/messages/en.json"
import { ClientFolderConfiguration } from "./client-folder-configuration"
import { sameConfiguration } from "@/hooks/use-client-configuration-draft"

vi.mock("@/lib/api", () => ({
  listCerebroConfigurationProjects: async () => ({
    items: [{ id: "project", name: "Project", path: "owner/project" }],
    total: 1,
  }),
  listCerebroConfigurationModules: async () => ({
    items: [],
    truncated: false,
  }),
}))

it("compares editable values as sets and restores clean state after reverting", () => {
  const original = {
    execution_module_id: null,
    mcp_enabled: false,
    mcp_module_ids: ["a", "b"],
  }
  expect(
    sameConfiguration(original, {
      ...original,
      mcp_module_ids: ["b", "a", "a"],
    })
  ).toBe(true)
  expect(
    sameConfiguration(original, { ...original, mcp_module_ids: ["a"] })
  ).toBe(false)
  expect(sameConfiguration(original, { ...original, mcp_enabled: true })).toBe(
    false
  )
  expect(
    sameConfiguration(original, { ...original, execution_module_id: "a" })
  ).toBe(false)
  expect(sameConfiguration(original, { ...original })).toBe(true)
})

it("shows unbound without selecting the first project or exposing credentials", async () => {
  const input = {
    execution_module_id: null,
    mcp_enabled: false,
    mcp_module_ids: [],
  }
  render(
    <NextIntlClientProvider locale="en" messages={messages}>
      <ClientFolderConfiguration
        configuration={{
          ...input,
          runner_id: "client",
          target_id: "folder",
          binding_id: null,
          module_labels: {},
          grant_id: null,
          credential_expires_at: null,
        }}
        input={input}
        loading={false}
        busy={false}
        error={null}
        onChange={vi.fn()}
        onRetry={vi.fn()}
      />
    </NextIntlClientProvider>
  )
  expect(
    await screen.findByRole("combobox", { name: "Execution module" })
  ).toHaveTextContent("Not bound")
  expect(screen.getByRole("combobox", { name: "Project" })).toHaveTextContent(
    "Not bound"
  )
  expect(screen.queryByText("MCP credential")).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument()
})

it("lets the user clear the project filter without changing saved module access", async () => {
  const input = {
    execution_module_id: "module",
    mcp_enabled: true,
    mcp_module_ids: ["module"],
  }
  const change = vi.fn()
  render(
    <NextIntlClientProvider locale="en" messages={messages}>
      <ClientFolderConfiguration
        configuration={{
          ...input,
          runner_id: "client",
          target_id: "folder",
          binding_id: "binding",
          module_labels: { module: "Saved module" },
          grant_id: "grant",
          credential_expires_at: null,
        }}
        input={input}
        loading={false}
        busy={false}
        error={null}
        onChange={change}
        onRetry={vi.fn()}
      />
    </NextIntlClientProvider>
  )
  const project = screen.getByRole("combobox", { name: "Project" })
  fireEvent.keyDown(project, { key: "ArrowDown" })
  fireEvent.click(
    await screen.findByRole("option", { name: "Project（owner/project）" })
  )
  expect(project).toHaveTextContent("Project（owner/project）")
  fireEvent.keyDown(project, { key: "ArrowDown" })
  fireEvent.click(await screen.findByRole("option", { name: "Not bound" }))
  expect(project).toHaveTextContent("Not bound")
  expect(
    screen.getByRole("combobox", { name: "Execution module" })
  ).toHaveTextContent("Saved module")
  expect(change).not.toHaveBeenCalled()
})
