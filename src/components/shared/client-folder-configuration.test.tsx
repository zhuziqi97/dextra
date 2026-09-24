import { act, fireEvent, render, screen } from "@testing-library/react"
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
    mcp_scope_modules: [
      { module_id: "a", permission: "READ" as const },
      { module_id: "b", permission: "WRITE" as const },
    ],
    mcp_capabilities: {
      code_graph_enabled: false,
      forge_enabled: false,
      issue_enabled: false,
    },
  }
  expect(
    sameConfiguration(original, {
      ...original,
      mcp_scope_modules: [...original.mcp_scope_modules].reverse(),
    })
  ).toBe(true)
  expect(
    sameConfiguration(original, {
      ...original,
      mcp_scope_modules: original.mcp_scope_modules.slice(0, 1),
    })
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
    mcp_scope_modules: [],
    mcp_capabilities: {
      code_graph_enabled: false,
      forge_enabled: false,
      issue_enabled: false,
    },
  }
  const change = vi.fn()
  render(
    <NextIntlClientProvider locale="en" messages={messages}>
      <ClientFolderConfiguration
        configuration={{
          ...input,
          runner_id: "client",
          target_id: "folder",
          execution_project: null,
          binding_id: null,
          mcp_scope_details: [],
          module_labels: {},
          grant_id: null,
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
  expect(
    await screen.findByRole("combobox", { name: "Execution module" })
  ).toHaveTextContent("Not bound")
  expect(
    screen.getByRole("combobox", { name: "Project filter" })
  ).toHaveTextContent("No project selected")
  expect(screen.queryByText("MCP credential")).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument()
  fireEvent.click(screen.getByRole("checkbox", { name: "Issue" }))
  expect(change).toHaveBeenCalledWith({
    mcp_capabilities: {
      code_graph_enabled: false,
      forge_enabled: false,
      issue_enabled: true,
    },
  })
})

it("lets the user clear the project filter without changing saved module access", async () => {
  const input = {
    execution_module_id: "module",
    mcp_enabled: true,
    mcp_scope_modules: [{ module_id: "module", permission: "READ" as const }],
    mcp_capabilities: {
      code_graph_enabled: false,
      forge_enabled: false,
      issue_enabled: false,
    },
  }
  const change = vi.fn()
  render(
    <NextIntlClientProvider locale="en" messages={messages}>
      <ClientFolderConfiguration
        configuration={{
          ...input,
          runner_id: "client",
          target_id: "folder",
          execution_project: null,
          binding_id: "binding",
          mcp_scope_details: [],
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
  const project = screen.getByRole("combobox", { name: "Project filter" })
  fireEvent.keyDown(project, { key: "ArrowDown" })
  fireEvent.click(
    await screen.findByRole("option", { name: "Project（owner/project）" })
  )
  expect(project).toHaveTextContent("Project（owner/project）")
  fireEvent.keyDown(project, { key: "ArrowDown" })
  fireEvent.click(
    await screen.findByRole("option", { name: "No project selected" })
  )
  expect(project).toHaveTextContent("No project selected")
  expect(
    screen.getByRole("combobox", { name: "Execution module" })
  ).toHaveTextContent("Saved module")
  expect(change).not.toHaveBeenCalled()
})

it("tracks per-module permissions and the graph switch independently of order", () => {
  const original = {
    execution_module_id: null,
    mcp_enabled: true,
    mcp_scope_modules: [
      { module_id: "a", permission: "READ" as const },
      { module_id: "b", permission: "WRITE" as const },
    ],
    mcp_capabilities: {
      code_graph_enabled: false,
      forge_enabled: false,
      issue_enabled: false,
    },
  }
  expect(
    sameConfiguration(original, {
      ...original,
      mcp_scope_modules: [...original.mcp_scope_modules].reverse(),
    })
  ).toBe(true)
  expect(
    sameConfiguration(original, {
      ...original,
      mcp_scope_modules: original.mcp_scope_modules.map((item) => ({
        ...item,
        permission: "READ",
      })),
    })
  ).toBe(false)
  expect(
    sameConfiguration(original, {
      ...original,
      mcp_capabilities: {
        code_graph_enabled: true,
        forge_enabled: false,
        issue_enabled: false,
      },
    })
  ).toBe(false)
  expect(
    sameConfiguration(original, {
      ...original,
      mcp_capabilities: { ...original.mcp_capabilities, issue_enabled: true },
    })
  ).toBe(false)
})

it("restores the bound project and preserves browsing on unrelated refreshes", async () => {
  const input = {
    execution_module_id: "module",
    mcp_enabled: false,
    mcp_scope_modules: [],
    mcp_capabilities: {
      code_graph_enabled: false,
      forge_enabled: false,
      issue_enabled: false,
    },
  }
  const configuration = {
    ...input,
    runner_id: "runner",
    target_id: "folder",
    binding_id: "binding",
    execution_project: {
      id: "project",
      name: "Project",
      display_name: null,
      path: "owner/project",
    },
    module_labels: { module: "Saved module" },
    mcp_scope_details: [],
    grant_id: null,
    credential_expires_at: null,
  }
  const change = vi.fn()
  const view = (value: typeof configuration) => (
    <NextIntlClientProvider locale="en" messages={messages}>
      <ClientFolderConfiguration
        configuration={value}
        input={input}
        loading={false}
        busy={false}
        error={null}
        onChange={change}
        onRetry={vi.fn()}
      />
    </NextIntlClientProvider>
  )
  const { rerender } = render(view(configuration))
  const filter = screen.getByRole("combobox", { name: "Project filter" })
  expect(filter).toHaveTextContent("Project（owner/project）")
  expect(
    screen.getByRole("combobox", { name: "Execution module" })
  ).toHaveTextContent("Saved module")
  fireEvent.keyDown(filter, { key: "ArrowDown" })
  fireEvent.click(
    await screen.findByRole("option", { name: "No project selected" })
  )
  expect(change).not.toHaveBeenCalled()
  await act(async () => {
    rerender(view({ ...configuration, mcp_enabled: true }))
  })
  expect(filter).toHaveTextContent("No project selected")
  await act(async () => {
    rerender(view({ ...configuration, target_id: "another-folder" }))
  })
  expect(filter).toHaveTextContent("Project（owner/project）")
})
