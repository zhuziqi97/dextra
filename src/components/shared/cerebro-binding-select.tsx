"use client"

import { useEffect, useState } from "react"
import { queryCerebroLaunchBinding } from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import type { CerebroLaunchBinding } from "@/lib/generated/cerebro/CerebroLaunchBinding"
import type { CerebroSelection } from "@/lib/generated/cerebro/CerebroSelection"

export function CerebroBindingSelect({ folderId, conversationId, workTaskId, disabled, onChange }: {
  folderId: number | null
  conversationId?: number
  workTaskId?: number
  disabled?: boolean
  onChange: (selection: CerebroSelection | undefined, ready: boolean) => void
}) {
  const [context, setContext] = useState<CerebroLaunchBinding | null>(null)
  const [selection, setSelection] = useState<CerebroSelection>()
  const [error, setError] = useState<string | null>(null)
  const [retry, setRetry] = useState(0)
  useEffect(() => {
    let cancelled = false
    setContext(null)
    setSelection(undefined)
    setError(null)
    onChange(undefined, false)
    if (folderId == null) return
    queryCerebroLaunchBinding(folderId, conversationId, workTaskId).then((value) => {
      if (cancelled) return
      setContext(value)
      setError(value.binding_query_error)
      if (value.platform_task) {
        onChange(undefined, true)
        return
      }
      const binding = value.binding
      const chosen: CerebroSelection | undefined = value.selection ?? (
        binding == null ? { mode: "LOCAL" } : binding.unavailable_code == null
          ? { mode: "BINDING", binding_id: binding.binding_id } : undefined
      )
      const unavailable = chosen?.mode !== "LOCAL" && (
        binding?.unavailable_code != null ||
        (chosen?.mode === "BINDING" && chosen.binding_id !== binding?.binding_id)
      )
      setSelection(chosen)
      if (unavailable) setError(binding?.unavailable_message ?? "已保存的 Binding 不再可用，请明确选择后续操作")
      onChange(chosen, chosen != null && !unavailable)
    }).catch((cause) => {
      if (!cancelled) setError(toErrorMessage(cause))
    })
    return () => { cancelled = true }
  }, [folderId, conversationId, workTaskId, retry, onChange])

  if (context?.platform_task) return <p className="text-sm text-muted-foreground">Cerebro：使用平台 Task 固定模块</p>
  const binding = context?.binding
  const selectedValue = selection?.mode === "LOCAL" ? "LOCAL" : selection?.binding_id ?? ""
  return <div className="flex flex-col gap-2">
    <label className="text-sm">
      Cerebro 模块
      <select aria-label="Cerebro 模块" className="ml-2 rounded border bg-background p-1" value={selectedValue} disabled={disabled || folderId == null || (context == null && error == null)}
        onChange={(event) => {
          const chosen: CerebroSelection = event.target.value === "LOCAL"
            ? { mode: "LOCAL" } : { mode: "BINDING", binding_id: event.target.value }
          setSelection(chosen)
          onChange(chosen, true)
        }}>
        <option value="" disabled>{error ? "请选择后续操作" : "正在查询模块…"}</option>
        <option value="LOCAL">纯本地</option>
        {binding && <option value={binding.binding_id} disabled={binding.unavailable_code != null}>{binding.module_display_name}（{binding.module_path}）</option>}
        {selection?.mode === "BINDING" && selection.binding_id !== binding?.binding_id && <option value={selection.binding_id} disabled>已保存的 Binding（不可用）</option>}
      </select>
    </label>
    {error && <p role="alert" className="text-sm text-destructive">{error} <button type="button" onClick={() => setRetry((value) => value + 1)}>重新查询</button></p>}
  </div>
}
