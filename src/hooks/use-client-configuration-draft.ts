import { useCallback, useEffect, useState } from "react"
import { queryCerebroFolderConfiguration } from "@/lib/api"
import { extractAppCommandError, toErrorMessage } from "@/lib/app-error"
import { getTransport } from "@/lib/transport"
import type { ClientConfiguration } from "@/lib/generated/cerebro/ClientConfiguration"
import type { ConfigurationInput } from "@/lib/generated/cerebro/ConfigurationInput"

export function configurationInput(
  value?: ConfigurationInput | null
): ConfigurationInput {
  return {
    execution_module_id: value?.execution_module_id ?? null,
    mcp_enabled: value?.mcp_enabled ?? false,
    mcp_scope_modules: (value?.mcp_scope_modules ?? []).map(
      ({ module_id, permission }) => ({ module_id, permission })
    ),
    mcp_capabilities: {
      code_graph_enabled: value?.mcp_capabilities.code_graph_enabled ?? false,
      forge_enabled: value?.mcp_capabilities.forge_enabled ?? false,
      issue_enabled: value?.mcp_capabilities.issue_enabled ?? false,
    },
  }
}

/** 只比较可编辑业务值，模块顺序和展示字段不代表修改。 */
export function sameConfiguration(
  left: ConfigurationInput,
  right: ConfigurationInput
): boolean {
  const modules = new Map(
    left.mcp_scope_modules.map((item) => [item.module_id, item.permission])
  )
  return (
    (left.execution_module_id ?? null) ===
      (right.execution_module_id ?? null) &&
    left.mcp_enabled === right.mcp_enabled &&
    left.mcp_capabilities.forge_enabled === right.mcp_capabilities.forge_enabled &&
    left.mcp_capabilities.issue_enabled === right.mcp_capabilities.issue_enabled &&
    left.mcp_capabilities.code_graph_enabled ===
      right.mcp_capabilities.code_graph_enabled &&
    modules.size === right.mcp_scope_modules.length &&
    right.mcp_scope_modules.every(
      (item) => modules.get(item.module_id) === item.permission
    )
  )
}

export function configurationErrorMessage(error: unknown): string {
  return extractAppCommandError(error)?.message ?? toErrorMessage(error)
}

interface DraftState {
  configuration: ClientConfiguration | null
  baseline: ConfigurationInput | null
  input: ConfigurationInput
  loading: boolean
  error: string | null
}
function emptyState(): DraftState {
  return {
    configuration: null,
    baseline: null,
    input: configurationInput(),
    loading: false,
    error: null,
  }
}
function dirty(state: DraftState): boolean {
  return (
    state.baseline !== null && !sameConfiguration(state.baseline, state.input)
  )
}

/** 状态属于整个文件夹窗口；页签只是同一草稿的不同视图。 */
export function useClientConfigurationDraft(
  folderId: number | null,
  open: boolean
) {
  const [state, setState] = useState<DraftState>(() => ({
    ...emptyState(),
    loading: open && folderId !== null,
  }))
  const [scope, setScope] = useState({ folderId, open })
  const [retry, setRetry] = useState(0)
  // 换目录或重新打开时立即换草稿，避免旧目录状态进入新一轮渲染。
  if (scope.folderId !== folderId || scope.open !== open) {
    setScope({ folderId, open })
    setState({ ...emptyState(), loading: open && folderId !== null })
  }

  useEffect(() => {
    if (!open || folderId === null) return
    let active = true
    queryCerebroFolderConfiguration(folderId)
      .then((result) => {
        if (!active) return
        setState((current) => {
          if (dirty(current))
            return { ...current, loading: false, error: result.error }
          const configuration = result.configuration
          return {
            configuration,
            baseline: configuration ? configurationInput(configuration) : null,
            input: configurationInput(configuration),
            loading: false,
            error: result.error,
          }
        })
      })
      .catch((error) => {
        if (active)
          setState((current) => ({
            ...current,
            loading: false,
            error: configurationErrorMessage(error),
          }))
      })
    return () => {
      active = false
    }
  }, [folderId, open, retry])

  const targetId = state.configuration?.target_id
  useEffect(() => {
    if (!open || !targetId) return
    let active = true
    let stop: (() => void) | undefined
    getTransport()
      .subscribe<ClientConfiguration>(
        "cerebro://configuration-changed",
        (value) => {
          if (!active || value.target_id !== targetId) return
          setState((current) =>
            dirty(current)
              ? current
              : {
                  configuration: value,
                  baseline: configurationInput(value),
                  input: configurationInput(value),
                  loading: false,
                  error: null,
                }
          )
        }
      )
      .then((unsubscribe) => {
        if (active) stop = unsubscribe
        else unsubscribe()
      })
      .catch((error) => {
        if (active)
          setState((current) => ({
            ...current,
            error: configurationErrorMessage(error),
          }))
      })
    return () => {
      active = false
      stop?.()
    }
  }, [open, targetId])

  const edit = useCallback((change: Partial<ConfigurationInput>) => {
    setState((current) => ({
      ...current,
      input: configurationInput({ ...current.input, ...change }),
    }))
  }, [])
  const accept = useCallback((value: ClientConfiguration) => {
    setState({
      configuration: value,
      baseline: configurationInput(value),
      input: configurationInput(value),
      loading: false,
      error: null,
    })
  }, [])
  const discard = useCallback(() => {
    setState((current) => ({
      ...current,
      input: configurationInput(current.baseline),
    }))
  }, [])
  return {
    ...state,
    dirty: dirty(state),
    edit,
    accept,
    discard,
    retry: () => {
      setState((current) => ({ ...current, loading: true }))
      setRetry((value) => value + 1)
    },
  }
}
