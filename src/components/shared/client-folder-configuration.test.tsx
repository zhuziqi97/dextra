import { act, fireEvent, render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { expect, it, vi } from "vitest"
import messages from "@/i18n/messages/en.json"
import { clientProjectFixture } from "@/hooks/client-project.test-fixture"
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
    sameConfiguration(original, {
      ...original,
      mcp_capabilities: { ...original.mcp_capabilities, issue_enabled: true },
    })
  ).toBe(false)
  expect(sameConfiguration(original, { ...original })).toBe(true)
})

it("shows unbound without selecting the first project or exposing credentials", async () => {
  const input = {
    mcp_enabled: false,
    mcp_scope_modules: [],
    mcp_capabilities: {
      code_graph_enabled: false,
      forge_enabled: false,
      issue_enabled: false,
    },
  }
  const change = vi.fn()
  await act(async () => {
    render(
      <NextIntlClientProvider locale="en" messages={messages}>
        <ClientFolderConfiguration
          configuration={{ ...clientProjectFixture(), ...input }}
          input={input}
          loading={false}
          busy={false}
          error={null}
          onChange={change}
          onRetry={vi.fn()}
        />
      </NextIntlClientProvider>
    )
  })

  expect(
    screen.queryByRole("combobox", { name: "Execution module" })
  ).toBeNull()
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
        configuration={{ ...clientProjectFixture(), ...input }}
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
    screen.queryByRole("combobox", { name: "Execution module" })
  ).toBeNull()
  expect(change).not.toHaveBeenCalled()
})

it("tracks per-module permissions and the graph switch independently of order", () => {
  const original = {
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

it("preserves browsing on unrelated refreshes and clears it when changing client projects", async () => {
  const input = {
    mcp_enabled: false,
    mcp_scope_modules: [],
    mcp_capabilities: {
      code_graph_enabled: false,
      forge_enabled: false,
      issue_enabled: false,
    },
  }
  const configuration = { ...clientProjectFixture(), ...input }
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
  expect(filter).toHaveTextContent("No project selected")
  fireEvent.keyDown(filter, { key: "ArrowDown" })
  fireEvent.click(
    await screen.findByRole("option", { name: "Project（owner/project）" })
  )
  expect(
    screen.queryByRole("combobox", { name: "Execution module" })
  ).toBeNull()
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
    rerender(view({ ...configuration, client_project_id: "another-project" }))
  })
  expect(filter).toHaveTextContent("No project selected")
})
