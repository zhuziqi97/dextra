import { useState } from "react"
import { fireEvent, render, screen, within } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { expect, it, vi } from "vitest"
import messages from "@/i18n/messages/en.json"
import { ClientMCPScope } from "./client-mcp-scope"
import type { MCPScopeItem } from "@/lib/generated/cerebro/MCPScopeItem"
const details: [] = []
vi.mock("@/lib/api", () => ({
  listCerebroConfigurationProjects: async (_: number, search?: string) => ({
    items: [
      { id: "first", name: "First", path: "owner/first" },
      { id: "second", name: "Second", path: "owner/second" },
    ].filter((item) => !search || item.name.includes(search)),
    total_pages: 1,
    total: search ? 1 : 2,
  }),
  listCerebroConfigurationModules: async (project: string) => ({
    items: [
      {
        id: project,
        name: `${project} module`,
        path: `owner/${project}/module`,
        can_write: project === "second",
      },
    ],
    truncated: false,
  }),
}))
function Harness() {
  const [selected, setSelected] = useState<MCPScopeItem[]>([])
  return (
    <NextIntlClientProvider locale="en" messages={messages}>
      <ClientMCPScope
        selected={selected}
        details={details}
        disabled={false}
        onChange={setSelected}
      />
      <output data-testid="saved">{JSON.stringify(selected)}</output>
    </NextIntlClientProvider>
  )
}
it("stages cross-project scope, restricts read-only modules and cancels only nested edits", async () => {
  render(<Harness />)
  fireEvent.click(
    screen.getByRole("button", { name: "Configure access scope" })
  )
  const dialog = screen.getByRole("dialog")
  fireEvent.click(
    await within(dialog).findByRole("button", { name: "First（owner/first）" })
  )
  fireEvent.click(
    await within(dialog).findByRole("checkbox", {
      name: "first module（owner/first/module）",
    })
  )
  fireEvent.keyDown(
    within(dialog).getAllByRole("combobox", { name: /first module/ })[0],
    { key: "ArrowDown" }
  )
  expect(
    screen.queryByRole("option", { name: "Read / write" })
  ).not.toBeInTheDocument()
  fireEvent.click(screen.getByRole("option", { name: "Read only" }))
  fireEvent.change(
    within(dialog).getByRole("textbox", { name: "Search projects" }),
    { target: { value: "Second" } }
  )
  fireEvent.click(
    await within(dialog).findByRole("button", {
      name: "Second（owner/second）",
    })
  )
  fireEvent.click(
    await within(dialog).findByRole("checkbox", {
      name: "second module（owner/second/module）",
    })
  )
  fireEvent.keyDown(
    within(dialog).getAllByRole("combobox", { name: /second module/ })[0],
    { key: "ArrowDown" }
  )
  fireEvent.click(await screen.findByRole("option", { name: "Read / write" }))
  expect(screen.getByTestId("saved")).toHaveTextContent("[]")
  fireEvent.click(within(dialog).getByRole("button", { name: "Confirm" }))
  expect(JSON.parse(screen.getByTestId("saved").textContent!)).toEqual([
    { module_id: "first", permission: "READ" },
    { module_id: "second", permission: "WRITE" },
  ])
  fireEvent.click(
    screen.getByRole("button", { name: "Configure access scope" })
  )
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: /Remove module first/,
    })
  )
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" })
  )
  expect(JSON.parse(screen.getByTestId("saved").textContent!)).toHaveLength(2)
})
