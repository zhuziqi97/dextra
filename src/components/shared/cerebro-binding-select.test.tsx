import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { CerebroBindingSelect } from "./cerebro-binding-select"
import { queryCerebroLaunchBinding } from "@/lib/api"

vi.mock("@/lib/api", () => ({ queryCerebroLaunchBinding: vi.fn() }))
afterEach(() => { cleanup(); vi.resetAllMocks() })
const binding = {
  binding_id: "chosen-binding", module_path: "owner/project/module", module_display_name: "模块一",
  status: "ACTIVE", unavailable_code: null, unavailable_message: null,
}

describe("Cerebro 模块选择", () => {
  it("已保存纯本地在查询失败时仍可重开，并展示真实错误", async () => {
    vi.mocked(queryCerebroLaunchBinding).mockResolvedValue({
      binding: null, selection: { mode: "LOCAL" }, platform_task: false,
      binding_query_error: "Cerebro 连接被拒绝",
    })
    const change = vi.fn()
    render(<CerebroBindingSelect folderId={1} conversationId={2} onChange={change} />)
    expect((await screen.findByRole("alert")).textContent).toContain("Cerebro 连接被拒绝")
    expect(change).toHaveBeenLastCalledWith({ mode: "LOCAL" }, true)
    expect((screen.getByRole("combobox") as HTMLSelectElement).value).toBe("LOCAL")
  })
  it("默认使用唯一模块，允许明确选择纯本地", async () => {
    vi.mocked(queryCerebroLaunchBinding).mockResolvedValue({ binding, selection: null, platform_task: false, binding_query_error: null })
    const change = vi.fn()
    render(<CerebroBindingSelect folderId={1} onChange={change} />)
    await waitFor(() => expect(change).toHaveBeenLastCalledWith({ mode: "BINDING", binding_id: binding.binding_id }, true))
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "LOCAL" } })
    expect(change).toHaveBeenLastCalledWith({ mode: "LOCAL" }, true)
  })

  it("禁用绑定不自动降级，但可以明确纯本地", async () => {
    vi.mocked(queryCerebroLaunchBinding).mockResolvedValue({
      binding: { ...binding, status: "DISABLED", unavailable_code: "BINDING_NOT_ACTIVE", unavailable_message: "绑定已停用" },
      selection: null, platform_task: false,
      binding_query_error: null,
    })
    const change = vi.fn()
    render(<CerebroBindingSelect folderId={1} onChange={change} />)
    await screen.findByRole("alert")
    expect(change).toHaveBeenLastCalledWith(undefined, false)
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "LOCAL" } })
    expect(change).toHaveBeenLastCalledWith({ mode: "LOCAL" }, true)
  })

  it("网络失败与无绑定严格区分", async () => {
    vi.mocked(queryCerebroLaunchBinding).mockRejectedValue(new Error("连接被拒绝"))
    const change = vi.fn()
    render(<CerebroBindingSelect folderId={1} onChange={change} />)
    expect((await screen.findByRole("alert")).textContent).toContain("连接被拒绝")
    expect(change).toHaveBeenLastCalledWith(undefined, false)
  })

  it("已保存引用被撤销后不自动使用替代绑定", async () => {
    const saved = { mode: "BINDING" as const, binding_id: "revoked" }
    vi.mocked(queryCerebroLaunchBinding).mockResolvedValue({ binding, selection: saved, platform_task: false, binding_query_error: null })
    const change = vi.fn()
    render(<CerebroBindingSelect folderId={1} conversationId={2} onChange={change} />)
    await screen.findByRole("alert")
    expect(change).toHaveBeenLastCalledWith(saved, false)
  })

  it("平台任务不提供覆盖入口", async () => {
    vi.mocked(queryCerebroLaunchBinding).mockResolvedValue({ binding: null, selection: null, platform_task: true, binding_query_error: null })
    const change = vi.fn()
    render(<CerebroBindingSelect folderId={1} workTaskId={2} onChange={change} />)
    await screen.findByText("Cerebro：使用平台 Task 固定模块")
    expect(screen.queryByRole("combobox")).toBeNull()
    expect(change).toHaveBeenLastCalledWith(undefined, true)
  })
})
