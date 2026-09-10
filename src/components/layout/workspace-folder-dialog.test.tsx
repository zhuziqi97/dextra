import { useState } from "react"
import { act, fireEvent, render, screen, within } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import enMessages from "@/i18n/messages/en.json"
import type {
  DirectoryEntry,
  FolderDetail,
  FolderLinkDetail,
  FolderLinkPlan,
} from "@/lib/types"
import { WorkspaceFolderDialog } from "./workspace-folder-dialog"

const api = vi.hoisted(() => ({
  getHomeDirectory: vi.fn(),
  listDirectoryEntries: vi.fn(),
  listFolderLinks: vi.fn(),
  previewFolderLinks: vi.fn(),
  createFolderLinks: vi.fn(),
  renameFolderLink: vi.fn(),
  repairFolderLink: vi.fn(),
  removeFolderLink: vi.fn(),
  queryCerebroFolderConfiguration: vi.fn(),
  saveCerebroFolderConfiguration: vi.fn(),
  listCerebroConfigurationProjects: vi.fn(),
  listCerebroConfigurationModules: vi.fn(),
}))
vi.mock("@/lib/api", () => api)

// Default to the in-app browser path: the native picker is a shortcut, not a
// separate flow, and jsdom has no Tauri. Individual tests opt into desktop.
const platform = vi.hoisted(() => ({
  desktop: false,
  openFileDialog: vi.fn(),
}))
vi.mock("@/lib/platform", () => ({
  isDesktop: () => platform.desktop,
  openFileDialog: platform.openFileDialog,
}))
vi.mock("@/lib/transport", () => ({
  getActiveRemoteConnectionId: () => null,
  getTransport: () => ({ subscribe: async () => () => {} }),
}))

const openFolder = vi.hoisted(() => vi.fn())
vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: unknown) => unknown) =>
    selector({ openFolder }),
}))

const toast = vi.hoisted(() => ({
  error: vi.fn(),
  warning: vi.fn(),
  success: vi.fn(),
}))
vi.mock("sonner", () => ({ toast }))

const dir = (
  name: string,
  path: string,
  hasChildren = false
): DirectoryEntry => ({ name, path, hasChildren })

const folder = (overrides: Partial<FolderDetail> = {}): FolderDetail => ({
  id: 7,
  name: "root",
  path: "/home/me/root",
  git_branch: null,
  default_agent_type: null,
  last_opened_at: "2026-08-03T00:00:00Z",
  sort_order: 1,
  color: "inherit",
  parent_id: null,
  kind: "regular",
  alias: null,
  ...overrides,
})

const link = (overrides: Partial<FolderLinkDetail> = {}): FolderLinkDetail => ({
  id: 1,
  folderId: 7,
  name: "api",
  targetPath: "/home/me/work/api",
  status: "ok",
  ...overrides,
})

const plan = (overrides: Partial<FolderLinkPlan> = {}): FolderLinkPlan => ({
  targetPath: "/home/me/work/api",
  baseName: "api",
  name: "api",
  renamed: false,
  collidesWithExistingEntry: false,
  rejection: null,
  existingLinkName: null,
  ...overrides,
})

function Harness({ manage }: { manage?: FolderDetail }) {
  const [open, setOpen] = useState(true)
  return (
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <WorkspaceFolderDialog
        open={open}
        onOpenChange={setOpen}
        folder={manage ?? null}
      />
    </NextIntlClientProvider>
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  api.queryCerebroFolderConfiguration.mockResolvedValue({
    configuration: null,
    error: null,
  })
  api.listCerebroConfigurationProjects.mockResolvedValue({
    items: [],
    total: 0,
  })
  api.listCerebroConfigurationModules.mockResolvedValue({
    items: [],
    truncated: false,
  })
  platform.desktop = false
  api.getHomeDirectory.mockResolvedValue("/home/me")
  api.listDirectoryEntries.mockResolvedValue([])
  api.listFolderLinks.mockResolvedValue([])
  api.previewFolderLinks.mockResolvedValue([])
  api.createFolderLinks.mockResolvedValue([])
  openFolder.mockResolvedValue(folder())
})

describe("WorkspaceFolderDialog — creation flow", () => {
  it("opens the picked folder and advances to the linking step", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("root", "/home/me/root"),
      dir("other", "/home/me/other"),
    ])
    render(<Harness />)
    await screen.findByDisplayValue("/home/me")

    fireEvent.click(screen.getByRole("button", { name: "root" }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Next" }))
    })

    expect(openFolder).toHaveBeenCalledWith("/home/me/root")
    // Step 2 is up: the root is echoed back and links can be added.
    expect(await screen.findByText("/home/me/root")).toBeInTheDocument()
    expect(
      screen.getByRole("button", { name: /Add folders/ })
    ).toBeInTheDocument()
  })

  it("stays on step 1 and reports when the folder cannot be opened", async () => {
    api.listDirectoryEntries.mockResolvedValue([dir("root", "/home/me/root")])
    openFolder.mockRejectedValue(new Error("nope"))
    render(<Harness />)
    await screen.findByDisplayValue("/home/me")

    fireEvent.click(screen.getByRole("button", { name: "root" }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Next" }))
    })

    expect(toast.error).toHaveBeenCalled()
    expect(screen.getByRole("button", { name: "Next" })).toBeInTheDocument()
  })
})

describe("WorkspaceFolderDialog — manage mode", () => {
  it("starts on the links list and skips root selection", async () => {
    api.listFolderLinks.mockResolvedValue([link()])
    render(<Harness manage={folder()} />)

    expect(await screen.findByText("api")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Next" })).toBeNull()
    // The root cannot be re-picked while managing an existing folder.
    expect(screen.queryByRole("button", { name: "Change" })).toBeNull()
  })

  it("shows the empty state when nothing is linked", async () => {
    render(<Harness manage={folder()} />)
    expect(
      await screen.findByText("No linked folders yet.")
    ).toBeInTheDocument()
  })

  it("surfaces a broken link with a repair action", async () => {
    api.listFolderLinks.mockResolvedValue([link({ status: "missing" })])
    render(<Harness manage={folder()} />)

    expect(
      await screen.findByText("The link is gone from this folder")
    ).toBeInTheDocument()
    const repair = screen.getByRole("button", { name: "Recreate link" })
    api.repairFolderLink.mockResolvedValue(link())
    await act(async () => {
      fireEvent.click(repair)
    })
    expect(api.repairFolderLink).not.toHaveBeenCalled()
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save" }))
    })
    expect(api.repairFolderLink).toHaveBeenCalledWith(1)
  })

  it("removes a link on demand", async () => {
    api.listFolderLinks.mockResolvedValue([link()])
    api.removeFolderLink.mockResolvedValue(undefined)
    render(<Harness manage={folder()} />)
    await screen.findByText("api")

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Remove link" }))
    })
    expect(api.removeFolderLink).not.toHaveBeenCalled()
    expect(screen.getByText("Pending removal")).toBeVisible()
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save" }))
    })
    expect(api.removeFolderLink).toHaveBeenCalledWith(1, true)
  })

  it("blocks a rename that duplicates another link's name", async () => {
    api.listFolderLinks.mockResolvedValue([
      link(),
      link({ id: 2, name: "web", targetPath: "/home/me/work/web" }),
    ])
    render(<Harness manage={folder()} />)
    await screen.findByText("api")

    fireEvent.click(screen.getAllByRole("button", { name: "Rename" })[0])
    const input = screen.getByDisplayValue("api")
    // Case-insensitive: `WEB` and `web` are the same entry on macOS/Windows.
    fireEvent.change(input, { target: { value: "WEB" } })

    expect(screen.getByText("This name is already used")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Apply name" })).toBeDisabled()
    expect(api.renameFolderLink).not.toHaveBeenCalled()
  })

  it("commits a valid rename", async () => {
    api.listFolderLinks.mockResolvedValue([link()])
    api.renameFolderLink.mockResolvedValue(link({ name: "backend" }))
    render(<Harness manage={folder()} />)
    await screen.findByText("api")

    fireEvent.click(screen.getByRole("button", { name: "Rename" }))
    fireEvent.change(screen.getByDisplayValue("api"), {
      target: { value: "backend" },
    })
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Apply name" }))
    })
    expect(api.renameFolderLink).not.toHaveBeenCalled()
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save" }))
    })
    expect(api.renameFolderLink).toHaveBeenCalledWith(1, "backend")
  })
})

describe("WorkspaceFolderDialog — adding link targets", () => {
  async function goToAddView() {
    render(<Harness manage={folder()} />)
    await screen.findByText("No linked folders yet.")
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Add folders/ }))
    })
  }

  it("previews the selection and queues it with the resolved name", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
    ])
    api.previewFolderLinks.mockResolvedValue([plan()])
    await goToAddView()

    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    expect(api.previewFolderLinks).toHaveBeenCalledWith(7, [
      "/home/me/work/api",
    ])
    expect(screen.getByDisplayValue("api")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
  })

  it("explains an auto-rename caused by an existing entry", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
    ])
    api.previewFolderLinks.mockResolvedValue([
      plan({ name: "api-2", renamed: true, collidesWithExistingEntry: true }),
    ])
    await goToAddView()

    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    expect(screen.getByDisplayValue("api-2")).toBeInTheDocument()
    expect(
      screen.getByText("Renamed — api already exists in this folder")
    ).toBeInTheDocument()
  })

  it("lists a rejected pick instead of silently dropping it", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("inner", "/home/me/root/inner"),
    ])
    api.previewFolderLinks.mockResolvedValue([
      plan({ targetPath: "/home/me/root/inner", rejection: "inside_root" }),
    ])
    await goToAddView()

    fireEvent.click(await screen.findByRole("button", { name: /inner/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    expect(screen.getByText("Already inside the workspace")).toBeInTheDocument()
    // Nothing queued, so there is no create button.
    expect(screen.queryByRole("button", { name: /^Link \d/ })).toBeNull()
  })

  it("names the existing link when the target is already linked", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
    ])
    api.previewFolderLinks.mockResolvedValue([
      plan({ rejection: "already_linked", existingLinkName: "backend" }),
    ])
    await goToAddView()

    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    expect(screen.getByText("Already linked as backend")).toBeInTheDocument()
  })

  it("blocks creation while a queued name is invalid", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
    ])
    api.previewFolderLinks.mockResolvedValue([plan()])
    await goToAddView()

    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    fireEvent.change(screen.getByDisplayValue("api"), {
      target: { value: "a/b" },
    })
    expect(
      screen.getByText('Cannot contain / \\ : * ? " < > |')
    ).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
  })

  it("creates the queued links with the edited names", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
    ])
    api.previewFolderLinks.mockResolvedValue([plan()])
    api.createFolderLinks.mockResolvedValue([link()])
    await goToAddView()

    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })
    fireEvent.change(screen.getByDisplayValue("api"), {
      target: { value: "backend" },
    })
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save" }))
    })

    expect(api.createFolderLinks).toHaveBeenCalledWith(
      7,
      [{ path: "/home/me/work/api", name: "backend" }],
      true
    )
  })

  it("keeps unfinished additions after partial failure and never recreates the successful link", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
      dir("web", "/home/me/work/web"),
    ])
    api.previewFolderLinks
      .mockResolvedValueOnce([
        plan(),
        plan({ targetPath: "/home/me/work/web", baseName: "web", name: "web" }),
      ])
      .mockResolvedValue([
        plan({ targetPath: "/home/me/work/web", baseName: "web", name: "web" }),
      ])
    api.createFolderLinks
      .mockResolvedValueOnce([link()])
      .mockRejectedValueOnce(new Error("Disk write failed"))
      .mockResolvedValueOnce([
        link({ id: 2, name: "web", targetPath: "/home/me/work/web" }),
      ])
    await goToAddView()
    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    fireEvent.click(screen.getByRole("button", { name: /web/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add 2" }))
    })
    api.listFolderLinks.mockResolvedValue([link()])
    expect(api.createFolderLinks).not.toHaveBeenCalled()
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save" }))
    })
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Disk write failed"
    )
    expect(screen.getByDisplayValue("web")).toBeVisible()
    expect(screen.queryByDisplayValue("api")).not.toBeInTheDocument()
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Retry" }))
    })
    expect(
      api.createFolderLinks.mock.calls.filter(
        ([, items]) => items[0].path === "/home/me/work/api"
      )
    ).toHaveLength(1)
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

  it("keeps earlier picks when a second batch is added", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
      dir("web", "/home/me/work/web"),
    ])
    api.previewFolderLinks.mockResolvedValueOnce([plan()])
    await goToAddView()
    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    api.previewFolderLinks.mockResolvedValueOnce([
      plan({ targetPath: "/home/me/work/web", baseName: "web", name: "web" }),
    ])
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Add folders/ }))
    })
    fireEvent.click(await screen.findByRole("button", { name: /web/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    expect(screen.getByDisplayValue("api")).toBeInTheDocument()
    expect(screen.getByDisplayValue("web")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument()
  })

  it("flags two queued picks that resolve to the same name", async () => {
    // The backend disambiguates within a batch, but the user can still edit one
    // into a collision before pressing create.
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
      dir("web", "/home/me/work/web"),
    ])
    api.previewFolderLinks.mockResolvedValue([
      plan(),
      plan({ targetPath: "/home/me/work/web", baseName: "web", name: "web" }),
    ])
    await goToAddView()
    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    fireEvent.click(screen.getByRole("button", { name: /web/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add 2" }))
    })

    fireEvent.change(screen.getByDisplayValue("web"), {
      target: { value: "api" },
    })
    expect(screen.getByText("This name is already used")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
  })

  it("discards the queue without creating anything", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
    ])
    api.previewFolderLinks.mockResolvedValue([plan()])
    await goToAddView()
    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    expect(api.createFolderLinks).not.toHaveBeenCalled()
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

  it("returns to the list without adding anything on Back", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
    ])
    await goToAddView()
    fireEvent.click(await screen.findByRole("button", { name: /api/ }))

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Back" }))
    })
    expect(api.previewFolderLinks).not.toHaveBeenCalled()
    expect(screen.getByText("No linked folders yet.")).toBeInTheDocument()
  })

  it("respects the git-exclude opt-out", async () => {
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
    ])
    api.previewFolderLinks.mockResolvedValue([plan()])
    api.createFolderLinks.mockResolvedValue([link()])
    await goToAddView()
    fireEvent.click(await screen.findByRole("button", { name: /api/ }))
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    const toggle = screen.getByRole("checkbox")
    fireEvent.click(toggle)
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save" }))
    })

    expect(api.createFolderLinks).toHaveBeenCalledWith(
      7,
      expect.anything(),
      false
    )
  })
})

describe("WorkspaceFolderDialog — native picker", () => {
  it("only fills the path box — opening still waits for the confirm", async () => {
    platform.desktop = true
    platform.openFileDialog.mockResolvedValue("/home/me/picked")
    render(<Harness />)
    await screen.findByDisplayValue("/home/me")

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "System picker" }))
    })

    expect(platform.openFileDialog).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
    })
    // The box moved, but nothing has landed in the sidebar yet.
    expect(screen.getByDisplayValue("/home/me/picked")).toBeInTheDocument()
    expect(openFolder).not.toHaveBeenCalled()

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Next" }))
    })
    expect(openFolder).toHaveBeenCalledWith("/home/me/picked")
  })

  it("stages a multi-select pick instead of linking it outright", async () => {
    platform.desktop = true
    platform.openFileDialog.mockResolvedValue([
      "/home/me/work/api",
      "/home/me/work/web",
    ])
    api.previewFolderLinks.mockResolvedValue([plan()])
    render(<Harness manage={folder()} />)
    await screen.findByText("No linked folders yet.")
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Add folders/ }))
    })

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "System picker" }))
    })

    expect(platform.openFileDialog).toHaveBeenCalledWith({
      directory: true,
      multiple: true,
    })
    expect(api.previewFolderLinks).not.toHaveBeenCalled()

    // Both picks are queued as ticks, so the confirm reads them, not the box.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add 2" }))
    })
    expect(api.previewFolderLinks).toHaveBeenCalledWith(7, [
      "/home/me/work/api",
      "/home/me/work/web",
    ])
  })

  it("stays out of the way when there is no native picker", async () => {
    render(<Harness />)
    await screen.findByDisplayValue("/home/me")
    expect(screen.queryByRole("button", { name: "System picker" })).toBeNull()
  })
})

describe("WorkspaceFolderDialog — link rows", () => {
  it("shows each link's real target path", async () => {
    api.listFolderLinks.mockResolvedValue([link()])
    render(<Harness manage={folder()} />)
    const row = (await screen.findByText("api")).closest("div")!
    expect(
      within(row.parentElement!).getByText("/home/me/work/api")
    ).toBeInTheDocument()
  })
})

describe("WorkspaceFolderDialog — selection bookkeeping", () => {
  it("does not re-add a folder the user unticked", async () => {
    // Clicking a row both ticks it and moves the path box onto it, so the box
    // still names the folder after it is unticked; only ticked rows count.
    api.listDirectoryEntries.mockResolvedValue([
      dir("api", "/home/me/work/api"),
      dir("web", "/home/me/work/web"),
    ])
    api.previewFolderLinks.mockResolvedValue([
      plan({ targetPath: "/home/me/work/web", baseName: "web", name: "web" }),
    ])
    render(<Harness manage={folder()} />)
    await screen.findByText("No linked folders yet.")
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Add folders/ }))
    })

    fireEvent.click(await screen.findByRole("button", { name: /web/ }))
    fireEvent.click(screen.getByRole("button", { name: /api/ }))
    fireEvent.click(screen.getByRole("button", { name: /api/ })) // untick
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    expect(api.previewFolderLinks).toHaveBeenCalledWith(7, [
      "/home/me/work/web",
    ])
  })

  it("uses the path box when nothing is ticked", async () => {
    api.listDirectoryEntries.mockResolvedValue([])
    api.previewFolderLinks.mockResolvedValue([])
    render(<Harness manage={folder()} />)
    await screen.findByText("No linked folders yet.")
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Add folders/ }))
    })

    const box = await screen.findByPlaceholderText("Enter directory path...")
    fireEvent.change(box, { target: { value: "/home/me/typed" } })
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }))
    })

    expect(api.previewFolderLinks).toHaveBeenCalledWith(7, ["/home/me/typed"])
  })
})

const configuration = {
  runner_id: "client",
  target_id: "target",
  execution_module_id: null,
  execution_project: null,
  binding_id: null,
  mcp_scope_modules: [],
  mcp_capabilities: { gitnexus_enabled: false },
  mcp_enabled: false,
  mcp_scope_details: [],
  module_labels: {},
  grant_id: null,
  credential_expires_at: null,
}
async function showConfiguration() {
  fireEvent.mouseDown(
    screen.getByRole("tab", { name: "Client configuration" }),
    { button: 0, ctrlKey: false }
  )
  return screen.findByRole("checkbox", { name: "Enabled" })
}

it("keeps both pages as drafts and commits local changes once when server save is retried", async () => {
  api.queryCerebroFolderConfiguration.mockResolvedValue({
    configuration,
    error: null,
  })
  api.listFolderLinks.mockResolvedValue([link()])
  api.removeFolderLink.mockResolvedValue(undefined)
  api.saveCerebroFolderConfiguration
    .mockRejectedValueOnce({
      code: "network_error",
      message: "Connection timed out",
    })
    .mockResolvedValueOnce({ ...configuration, mcp_enabled: true })
  render(<Harness manage={folder()} />)
  await screen.findByText("api")
  fireEvent.click(screen.getByRole("button", { name: "Remove link" }))
  fireEvent.click(await showConfiguration())
  fireEvent.mouseDown(screen.getByRole("tab", { name: "Folder links" }), {
    button: 0,
    ctrlKey: false,
  })
  expect(screen.getByText("Pending removal")).toBeVisible()
  fireEvent.click(await showConfiguration())
  fireEvent.click(screen.getByRole("checkbox", { name: "Enabled" }))
  expect(screen.getByRole("checkbox", { name: "Enabled" })).toBeChecked()
  expect(api.removeFolderLink).not.toHaveBeenCalled()
  expect(api.saveCerebroFolderConfiguration).not.toHaveBeenCalled()
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
  })
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Connection timed out"
  )
  expect(screen.getByRole("alert")).toHaveTextContent("not confirmed")
  expect(api.removeFolderLink).toHaveBeenCalledTimes(1)
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Retry" }))
  })
  expect(api.removeFolderLink).toHaveBeenCalledTimes(1)
  expect(api.saveCerebroFolderConfiguration).toHaveBeenCalledTimes(2)
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
})

it("can revert server edits after a communication failure and finish local changes offline", async () => {
  api.queryCerebroFolderConfiguration.mockResolvedValue({
    configuration,
    error: null,
  })
  api.listFolderLinks.mockResolvedValue([link()])
  api.removeFolderLink.mockResolvedValue(undefined)
  api.saveCerebroFolderConfiguration.mockRejectedValue({
    code: "network_error",
    message: "Offline",
  })
  render(<Harness manage={folder()} />)
  await screen.findByText("api")
  fireEvent.click(screen.getByRole("button", { name: "Remove link" }))
  fireEvent.click(await showConfiguration())
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
  })
  await screen.findByRole("alert")
  fireEvent.click(screen.getByRole("checkbox", { name: "Enabled" }))
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
  })
  expect(api.saveCerebroFolderConfiguration).toHaveBeenCalledTimes(1)
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
})

it("discards only server edits after a business error and does not undo the local save", async () => {
  api.queryCerebroFolderConfiguration.mockResolvedValue({
    configuration,
    error: null,
  })
  api.listFolderLinks.mockResolvedValue([link()])
  api.removeFolderLink.mockResolvedValue(undefined)
  api.saveCerebroFolderConfiguration.mockRejectedValue({
    code: "invalid_input",
    message: "Module is occupied",
    detail: "TARGET_OCCUPIED",
  })
  render(<Harness manage={folder()} />)
  await screen.findByText("api")
  fireEvent.click(screen.getByRole("button", { name: "Remove link" }))
  fireEvent.click(await showConfiguration())
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
  })
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Module is occupied"
  )
  expect(screen.getByRole("alert")).not.toHaveTextContent("not confirmed")
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Discard server changes and finish" })
    )
  })
  expect(api.removeFolderLink).toHaveBeenCalledTimes(1)
  expect(api.saveCerebroFolderConfiguration).toHaveBeenCalledTimes(1)
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
})

it("finishes local changes even when no server configuration has ever loaded", async () => {
  api.queryCerebroFolderConfiguration.mockResolvedValue({
    configuration: null,
    error: "Server unavailable",
  })
  api.listFolderLinks.mockResolvedValue([link()])
  api.removeFolderLink.mockResolvedValue(undefined)
  render(<Harness manage={folder()} />)
  await screen.findByText("api")
  fireEvent.click(screen.getByRole("button", { name: "Remove link" }))
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
  })
  expect(api.saveCerebroFolderConfiguration).not.toHaveBeenCalled()
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
})

it("undoes removals and repairs and drops unsaved edits on cancel", async () => {
  api.listFolderLinks.mockResolvedValue([link({ status: "missing" })])
  render(<Harness manage={folder()} />)
  await screen.findByText("api")
  fireEvent.click(screen.getByRole("button", { name: "Recreate link" }))
  fireEvent.click(screen.getByRole("button", { name: "Undo repair" }))
  fireEvent.click(screen.getByRole("button", { name: "Remove link" }))
  fireEvent.click(screen.getByRole("button", { name: "Undo removal" }))
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
  fireEvent.click(screen.getByRole("button", { name: "Remove link" }))
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
  expect(api.removeFolderLink).not.toHaveBeenCalled()
  expect(api.repairFolderLink).not.toHaveBeenCalled()
})

it("returns to a clean draft after renaming back to the loaded name", async () => {
  api.listFolderLinks.mockResolvedValue([link()])
  render(<Harness manage={folder()} />)
  await screen.findByText("api")
  fireEvent.click(screen.getByRole("button", { name: "Rename" }))
  fireEvent.change(screen.getByDisplayValue("api"), {
    target: { value: "backend" },
  })
  fireEvent.click(screen.getByRole("button", { name: "Apply name" }))
  fireEvent.click(screen.getByRole("button", { name: "Rename" }))
  fireEvent.change(screen.getByDisplayValue("backend"), {
    target: { value: "api" },
  })
  fireEvent.click(screen.getByRole("button", { name: "Apply name" }))
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
  expect(api.renameFolderLink).not.toHaveBeenCalled()
})

it("does not repeat a local write whose response was lost when rereading confirms the change", async () => {
  api.listFolderLinks
    .mockResolvedValueOnce([link()])
    .mockResolvedValue([link({ name: "backend" })])
  api.renameFolderLink.mockRejectedValue(new Error("Response lost"))
  render(<Harness manage={folder()} />)
  await screen.findByText("api")
  fireEvent.click(screen.getByRole("button", { name: "Rename" }))
  fireEvent.change(screen.getByDisplayValue("api"), {
    target: { value: "backend" },
  })
  fireEvent.click(screen.getByRole("button", { name: "Apply name" }))
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
  })
  expect(await screen.findByRole("alert")).toHaveTextContent("Response lost")
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
  })
  expect(api.renameFolderLink).toHaveBeenCalledTimes(1)
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
})
