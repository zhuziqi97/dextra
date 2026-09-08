import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import CerebroWorkbenchPage from "./page"

vi.mock("@/components/diff/unified-diff-preview", () => ({
  UnifiedDiffPreview: ({ diffText }: { diffText: string }) => (
    <pre data-testid="diff-preview">{diffText}</pre>
  ),
}))

vi.mock("@/components/ui/scroll-area", () => ({
  ScrollArea: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}))

type Listener = (event: { data?: unknown }) => void

class MockWebSocket {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 3
  static instances: MockWebSocket[] = []

  readonly sent: Record<string, unknown>[] = []
  readonly listeners = new Map<string, Listener[]>()
  readyState = MockWebSocket.CONNECTING

  constructor(readonly url: string) {
    MockWebSocket.instances.push(this)
  }

  addEventListener(type: string, listener: Listener): void {
    const listeners = this.listeners.get(type) ?? []
    listeners.push(listener)
    this.listeners.set(type, listeners)
  }

  send(raw: string): void {
    const frame = JSON.parse(raw) as Record<string, unknown>
    this.sent.push(frame)
    if (frame.TYPE !== "REMOTE_REQUEST") return
    const identity = frame.COMMAND_IDENTITY as { NAME: string }
    queueMicrotask(() => {
      this.receive({
        TYPE: "REMOTE_RESPONSE",
        CORRELATION_ID: frame.REQUEST_ID,
        PAYLOAD: { RESULT: resultFor(identity.NAME) },
      })
    })
  }

  close(): void {
    this.readyState = MockWebSocket.CLOSED
  }

  open(): void {
    this.readyState = MockWebSocket.OPEN
    this.emit("open")
  }

  receive(message: Record<string, unknown>): void {
    this.emit("message", { data: JSON.stringify(message) })
  }

  private emit(type: string, event: { data?: unknown } = {}): void {
    for (const listener of this.listeners.get(type) ?? []) listener(event)
  }
}

function resultFor(command: string): unknown {
  if (command === "work_task_get") {
    return {
      id: 42,
      folder_id: 7,
      title: "实现结果投影",
      status: "review",
      result_summary: "完成远程工作台接线",
      files_changed: 1,
      additions: 12,
      deletions: 2,
    }
  }
  if (command === "work_task_changed_files") {
    return [{ file: "src/main.rs", additions: 12, deletions: 2 }]
  }
  if (command === "work_task_diff") {
    return "diff --git a/src/main.rs b/src/main.rs\n+ready"
  }
  if (command === "read_file_for_edit") {
    return {
      path: "src/main.rs",
      content: "fn main() {}\n",
      etag: "before",
      mtime_ms: 1,
      readonly: false,
      line_ending: "lf",
    }
  }
  if (command === "save_file_content") {
    return {
      path: "src/main.rs",
      etag: "after",
      mtime_ms: 2,
      readonly: false,
      line_ending: "lf",
    }
  }
  if (command === "work_task_merge") return false
  return null
}

function attach(ws: MockWebSocket): void {
  ws.receive({
    TYPE: "REMOTE_ATTACHED",
    CORRELATION_ID: "attach-1",
    PAYLOAD: {
      SNAPSHOT: {
        type: "workspace",
        rootPath: "/workspace",
        workspace: "Runner workspace",
        repository: "owner/repo",
        branch: "codex/task",
        workTaskId: "42",
        status: "READY",
        runnerHello: {
          PROTOCOL_VERSION: 1,
          TYPE: "HELLO",
          MESSAGE_ID: "hello-1",
          OCCURRED_AT: "2026-09-04T00:00:00Z",
          PAYLOAD: {
            RUNNER_BUILD_ID: "dextra-p0-c",
            CODEG_API_REVISION: 1,
            CODEG_UPSTREAM_VERSION: "v0.29.0",
            CODEG_UPSTREAM_COMMIT: "769610c626f1fc4b18c11d3e289326acf097b99f",
          },
        },
      },
    },
  })
}

beforeEach(() => {
  MockWebSocket.instances = []
  localStorage.clear()
  vi.stubGlobal("WebSocket", MockWebSocket)
  vi.spyOn(window.parent, "postMessage").mockImplementation(() => undefined)
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  localStorage.clear()
})

describe("CerebroWorkbenchPage", () => {
  it("从父页面内存消息连接绑定任务，并通过 relay 修改文件和合并", async () => {
    const { unmount } = render(<CerebroWorkbenchPage />)
    expect(window.parent.postMessage).toHaveBeenCalledWith(
      { TYPE: "CEREBRO_REMOTE_WORKBENCH_READY" },
      "*"
    )

    await act(async () => {
      window.dispatchEvent(
        new MessageEvent("message", {
          source: window,
          origin: "https://platform.example",
          data: {
            TYPE: "CEREBRO_REMOTE_WORKBENCH_INIT",
            TICKET: "short-lived-ticket",
          },
        })
      )
    })
    await waitFor(() => expect(MockWebSocket.instances).toHaveLength(1))
    const ws = MockWebSocket.instances[0]
    await act(async () => {
      ws.open()
      attach(ws)
    })

    expect(await screen.findByText("实现结果投影")).toBeInTheDocument()
    expect(screen.getByText("Runner workspace")).toBeInTheDocument()
    expect(screen.getByText("owner/repo")).toBeInTheDocument()
    expect(screen.getByTestId("diff-preview")).toHaveTextContent("src/main.rs")
    expect(document.body).not.toHaveTextContent("/workspace")
    expect(localStorage.getItem("short-lived-ticket")).toBeNull()
    expect(JSON.stringify(ws.sent.slice(1))).not.toContain("short-lived-ticket")

    fireEvent.click(screen.getByRole("button", { name: "编辑 src/main.rs" }))
    const editor = await screen.findByRole("textbox", { name: "文件内容" })
    fireEvent.change(editor, { target: { value: "fn main() { ready(); }\n" } })
    fireEvent.click(screen.getByRole("button", { name: "保存文件" }))
    await waitFor(() => {
      expect(
        ws.sent.some(
          (frame) =>
            (frame.COMMAND_IDENTITY as { NAME?: string } | undefined)?.NAME ===
              "save_file_content" &&
            (frame.ARGUMENTS as { path?: string; content?: string } | undefined)
              ?.path === "src/main.rs" &&
            (frame.ARGUMENTS as { content?: string } | undefined)?.content ===
              "fn main() { ready(); }\n"
        )
      ).toBe(true)
    })
    expect(await screen.findByText("已保存 src/main.rs")).toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: "合并变更" }))
    await waitFor(() => {
      expect(
        ws.sent.some(
          (frame) =>
            (frame.COMMAND_IDENTITY as { NAME?: string } | undefined)?.NAME ===
              "work_task_merge" &&
            (frame.ARGUMENTS as { id?: number } | undefined)?.id === 42
        )
      ).toBe(true)
    })
    expect(await screen.findByText("已开始合并")).toBeInTheDocument()

    unmount()
    expect(ws.sent[ws.sent.length - 1]).toEqual({ TYPE: "REMOTE_DETACH" })
  })
})
