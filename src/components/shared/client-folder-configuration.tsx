"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { webPath } from "@/lib/web-mount"
import { useTranslations } from "next-intl"
import { Copy, Eye, Loader2, RefreshCw, X } from "lucide-react"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { SettingCard, SettingRow } from "@/components/shared/setting-card"
import {
  getCerebroFolderCredential,
  listCerebroConfigurationModules,
  listCerebroConfigurationProjects,
  queryCerebroFolderConfiguration,
  saveCerebroFolderConfiguration,
} from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import { getTransport } from "@/lib/transport"
import type { ClientConfiguration } from "@/lib/generated/cerebro/ClientConfiguration"
import type { ConfigurationInput } from "@/lib/generated/cerebro/ConfigurationInput"
import type { FolderCredential } from "@/lib/generated/cerebro/FolderCredential"
import type { ModuleOption } from "@/lib/generated/cerebro/ModuleOption"
import type { ProjectOption } from "@/lib/generated/cerebro/ProjectOption"

export function ClientFolderConfiguration({ folderId }: { folderId: number }) {
  const t = useTranslations("CerebroFolder")
  const [configuration, setConfiguration] =
    useState<ClientConfiguration | null>(null)
  const [input, setInput] = useState<ConfigurationInput>({
    execution_module_id: null,
    mcp_module_ids: [],
    mcp_enabled: false,
  })
  const [projects, setProjects] = useState<ProjectOption[]>([])
  const [projectId, setProjectId] = useState("")
  const [modules, setModules] = useState<ModuleOption[]>([])
  const [labels, setLabels] = useState<Record<string, string | undefined>>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [loading, setLoading] = useState(true)
  const [dirty, setDirty] = useState(false)
  const dirtyRef = useRef(false)
  const [credential, setCredential] = useState<FolderCredential | null>(null)
  const [showCredential, setShowCredential] = useState(false)
  const showCredentialRef = useRef(false)
  const [retry, setRetry] = useState(0)

  const apply = useCallback((value: ClientConfiguration) => {
    setConfiguration(value)
    setLabels((previous) => ({ ...previous, ...value.module_labels }))
    if (!dirtyRef.current)
      setInput({
        execution_module_id: value.execution_module_id,
        mcp_module_ids: value.mcp_module_ids,
        mcp_enabled: value.mcp_enabled,
      })
  }, [])

  useEffect(() => {
    let active = true
    setLoading(true)
    setError(null)
    queryCerebroFolderConfiguration(folderId)
      .then(async (state) => {
        if (!active) return
        if (state.configuration) apply(state.configuration)
        setError(state.error)
        if (!state.configuration || state.error) return
        let page = 1
        const choices: ProjectOption[] = []
        do {
          const result = await listCerebroConfigurationProjects(page++)
          if (!active) return
          choices.push(...result.items)
          if (choices.length >= result.total || result.items.length === 0) break
        } while (active)
        setProjects(choices)
        setProjectId((current) => current || choices[0]?.id || "")
      })
      .catch((cause) => {
        if (active) setError(toErrorMessage(cause))
      })
      .finally(() => {
        if (active) setLoading(false)
      })
    return () => {
      active = false
    }
  }, [folderId, retry, apply])

  useEffect(() => {
    let active = true
    let unsubscribe: (() => void) | undefined
    getTransport()
      .subscribe<ClientConfiguration>(
        "cerebro://configuration-changed",
        (value) => {
          if (value.target_id !== configuration?.target_id) return
          apply(value)
          if (!value.mcp_enabled || !value.mcp_module_ids.length)
            setCredential(null)
          else if (showCredentialRef.current) {
            getCerebroFolderCredential(folderId)
              .then(setCredential)
              .catch((cause) => setError(toErrorMessage(cause)))
          }
        }
      )
      .then((stop) => {
        if (active) unsubscribe = stop
        else stop()
      })
      .catch((cause) => {
        if (active) setError(toErrorMessage(cause))
      })
    return () => {
      active = false
      unsubscribe?.()
    }
  }, [configuration?.target_id, apply, folderId])

  useEffect(() => {
    if (!projectId) return
    let active = true
    setModules([])
    listCerebroConfigurationModules(projectId)
      .then((result) => {
        if (!active) return
        setModules(result.items)
        setLabels((previous) => ({
          ...previous,
          ...Object.fromEntries(
            result.items.map((module) => [
              module.id,
              `${module.display_name || module.name}（${module.path}）`,
            ])
          ),
        }))
        if (result.truncated) setError(t("tooManyModules"))
      })
      .catch((cause) => {
        if (active) setError(toErrorMessage(cause))
      })
    return () => {
      active = false
    }
  }, [projectId, t])

  function edit(change: Partial<ConfigurationInput>) {
    dirtyRef.current = true
    setDirty(true)
    setInput((current) => ({ ...current, ...change }))
  }

  async function save() {
    setBusy(true)
    setError(null)
    try {
      const value = await saveCerebroFolderConfiguration(folderId, input)
      dirtyRef.current = false
      setDirty(false)
      apply(value)
      setCredential(null)
      toast.success(t("saved"))
    } catch (cause) {
      setError(toErrorMessage(cause))
    } finally {
      setBusy(false)
    }
  }

  async function manageCredential(action: "view" | "copy" | "rotate") {
    setBusy(true)
    try {
      const value = await getCerebroFolderCredential(
        folderId,
        action === "rotate"
      )
      setCredential(value)
      if (action === "copy") {
        await navigator.clipboard.writeText(value.access_token)
        toast.success(t("copied"))
      } else {
        showCredentialRef.current = true
        setShowCredential(true)
      }
    } catch (cause) {
      setError(toErrorMessage(cause))
    } finally {
      setBusy(false)
    }
  }

  if (loading)
    return (
      <div className="flex items-center gap-2 py-4 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        {t("loading")}
      </div>
    )
  if (!configuration && !error)
    return (
      <Button asChild variant="outline">
        <a href={webPath("/settings/cerebro")}>{t("connect")}</a>
      </Button>
    )
  const selectedOutsideProject =
    input.execution_module_id &&
    !modules.some((module) => module.id === input.execution_module_id)
  return (
    <div className="flex flex-col gap-3">
      {error && (
        <div role="alert" className="text-sm text-destructive">
          {error}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setRetry((value) => value + 1)}
          >
            {t("retry")}
          </Button>
        </div>
      )}
      <SettingCard>
        <SettingRow title={t("project")}>
          <Select
            value={projectId}
            onValueChange={setProjectId}
            disabled={busy}
          >
            <SelectTrigger className="w-full" aria-label={t("project")}>
              <SelectValue placeholder={t("selectProject")} />
            </SelectTrigger>
            <SelectContent>
              {projects.map((project) => (
                <SelectItem key={project.id} value={project.id}>
                  {project.display_name || project.name}（{project.path}）
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingRow>
        <SettingRow title={t("executionModule")}>
          <Select
            value={input.execution_module_id ?? "NONE"}
            onValueChange={(value) =>
              edit({ execution_module_id: value === "NONE" ? null : value })
            }
            disabled={busy || !configuration}
          >
            <SelectTrigger className="w-full" aria-label={t("executionModule")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="NONE">{t("unbound")}</SelectItem>
              {selectedOutsideProject && (
                <SelectItem value={input.execution_module_id!}>
                  {labels[input.execution_module_id!] ??
                    input.execution_module_id}
                </SelectItem>
              )}
              {modules
                .filter(
                  (module) =>
                    module.can_write || module.id === input.execution_module_id
                )
                .map((module) => (
                  <SelectItem
                    key={module.id}
                    value={module.id}
                    disabled={!module.can_write}
                  >
                    {labels[module.id]}
                  </SelectItem>
                ))}
            </SelectContent>
          </Select>
        </SettingRow>
        <SettingRow
          title={t("mcpModules")}
          control={
            <label className="flex items-center gap-2 text-xs">
              <Checkbox
                checked={input.mcp_enabled}
                onCheckedChange={(value) =>
                  edit({ mcp_enabled: value === true })
                }
                disabled={busy || !configuration}
              />
              {t("enabled")}
            </label>
          }
        >
          <div className="max-h-36 space-y-2 overflow-y-auto">
            {modules.map((module) => (
              <label key={module.id} className="flex items-start gap-2 text-sm">
                <Checkbox
                  className="mt-0.5"
                  checked={input.mcp_module_ids.includes(module.id)}
                  disabled={busy || !configuration}
                  onCheckedChange={(checked) =>
                    edit({
                      mcp_module_ids: checked
                        ? [...input.mcp_module_ids, module.id]
                        : input.mcp_module_ids.filter((id) => id !== module.id),
                    })
                  }
                />
                <span className="break-all">{labels[module.id]}</span>
              </label>
            ))}
          </div>
          {input.mcp_module_ids
            .filter((id) => !modules.some((module) => module.id === id))
            .map((id) => (
              <div
                key={id}
                className="flex items-center justify-between gap-2 text-xs"
              >
                <span className="break-all">{labels[id] ?? id}</span>
                <Button
                  size="icon"
                  variant="ghost"
                  className="size-6 shrink-0"
                  aria-label={t("removeModule")}
                  onClick={() =>
                    edit({
                      mcp_module_ids: input.mcp_module_ids.filter(
                        (value) => value !== id
                      ),
                    })
                  }
                >
                  <X className="size-3" />
                </Button>
              </div>
            ))}
        </SettingRow>
        <SettingRow
          title={t("credential")}
          control={
            <div className="flex gap-1">
              {(
                [
                  { action: "view", Icon: Eye },
                  { action: "copy", Icon: Copy },
                  { action: "rotate", Icon: RefreshCw },
                ] as const
              ).map(({ action, Icon }) => (
                <Button
                  key={action}
                  size="icon"
                  variant="ghost"
                  className="size-7"
                  title={t(action)}
                  aria-label={t(action)}
                  disabled={
                    busy ||
                    dirty ||
                    !configuration?.mcp_enabled ||
                    !configuration.mcp_module_ids.length
                  }
                  onClick={() => manageCredential(action)}
                >
                  <Icon className="size-3.5" />
                </Button>
              ))}
            </div>
          }
        >
          {credential && showCredential && (
            <>
              <Input
                aria-label={t("credential")}
                readOnly
                value={credential.access_token}
                className="font-mono text-xs"
              />
              <span className="text-xs text-muted-foreground">
                {t("expiresAt", {
                  time: new Date(credential.expires_at).toLocaleString(),
                })}
              </span>
            </>
          )}
        </SettingRow>
      </SettingCard>
      <div className="flex justify-end">
        <Button onClick={save} disabled={busy || !dirty || !configuration}>
          {busy && <Loader2 className="size-4 animate-spin" />}
          {t("save")}
        </Button>
      </div>
    </div>
  )
}
