"use client"

import { useEffect, useState } from "react"
import { useTranslations } from "next-intl"
import { Loader2, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { SettingCard, SettingRow } from "@/components/shared/setting-card"
import {
  listCerebroConfigurationModules,
  listCerebroConfigurationProjects,
} from "@/lib/api"
import { configurationErrorMessage } from "@/hooks/use-client-configuration-draft"
import type { ClientConfiguration } from "@/lib/generated/cerebro/ClientConfiguration"
import type { ConfigurationInput } from "@/lib/generated/cerebro/ConfigurationInput"
import type { ModuleOption } from "@/lib/generated/cerebro/ModuleOption"
import type { ProjectOption } from "@/lib/generated/cerebro/ProjectOption"

interface Props {
  configuration: ClientConfiguration | null
  input: ConfigurationInput
  loading: boolean
  busy: boolean
  error: string | null
  onChange: (change: Partial<ConfigurationInput>) => void
  onRetry: () => void
}

export function ClientFolderConfiguration({
  configuration,
  input,
  loading,
  busy,
  error: configurationError,
  onChange: edit,
  onRetry,
}: Props) {
  const t = useTranslations("CerebroFolder")
  const [projects, setProjects] = useState<ProjectOption[]>([])
  const [projectId, setProjectId] = useState("")
  const [modules, setModules] = useState<ModuleOption[]>([])
  const [optionLabels, setLabels] = useState<
    Record<string, string | undefined>
  >({})
  const [projectsError, setProjectsError] = useState<string | null>(null)
  const [modulesError, setModulesError] = useState<string | null>(null)
  const error = projectsError ?? modulesError
  const labels = { ...configuration?.module_labels, ...optionLabels }
  const targetId = configuration?.target_id
  const [optionsRetry, setOptionsRetry] = useState(0)
  const retry = () => {
    setProjectsError(null)
    setModulesError(null)
    setOptionsRetry((value) => value + 1)
    onRetry()
  }
  useEffect(() => {
    if (!targetId) return
    let active = true
    void (async () => {
      const choices: ProjectOption[] = []
      for (let page = 1; active; page++) {
        const result = await listCerebroConfigurationProjects(page)
        if (!active) return
        choices.push(...result.items)
        if (choices.length >= result.total || result.items.length === 0) break
      }
      if (active) {
        setProjects(choices)
        setProjectsError(null)
      }
    })().catch((cause) => {
      if (active) setProjectsError(configurationErrorMessage(cause))
    })
    return () => {
      active = false
    }
  }, [targetId, optionsRetry])
  useEffect(() => {
    if (!projectId) return
    let active = true
    listCerebroConfigurationModules(projectId)
      .then((result) => {
        if (!active) return
        setModules(result.items)
        setModulesError(null)
        setLabels((previous) => ({
          ...previous,
          ...Object.fromEntries(
            result.items.map((module) => [
              module.id,
              `${module.display_name || module.name}（${module.path}）`,
            ])
          ),
        }))
        if (result.truncated) setModulesError(t("tooManyModules"))
      })
      .catch((cause) => {
        if (active) setModulesError(configurationErrorMessage(cause))
      })
    return () => {
      active = false
    }
  }, [projectId, t, optionsRetry])

  const selectedOutsideProject =
    input.execution_module_id &&
    !modules.some((module) => module.id === input.execution_module_id)
  if (loading && !configuration)
    return (
      <div className="flex items-center gap-2 py-4 text-sm">
        <Loader2 className="size-4 animate-spin" />
        {t("loading")}
      </div>
    )
  return (
    <div className="flex flex-col gap-3">
      {(configurationError || error) && (
        <div role="alert" className="text-sm text-destructive">
          {configurationError || error}
          <Button variant="ghost" size="sm" onClick={retry}>
            {t("retry")}
          </Button>
        </div>
      )}
      {!configuration && !configurationError && (
        <p className="text-sm text-muted-foreground">{t("localOnly")}</p>
      )}
      <SettingCard>
        <SettingRow title={t("project")}>
          <Select
            value={projectId || "NONE"}
            onValueChange={(value) => {
              setModules([])
              setModulesError(null)
              setProjectId(value === "NONE" ? "" : value)
            }}
            disabled={busy || !configuration}
          >
            <SelectTrigger className="w-full" aria-label={t("project")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent
              position="popper"
              side="bottom"
              align="start"
              sideOffset={4}
              avoidCollisions={false}
              className="max-h-[min(16rem,var(--radix-select-content-available-height))]"
            >
              <SelectItem value="NONE">{t("unbound")}</SelectItem>
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
            <SelectContent
              position="popper"
              side="bottom"
              align="start"
              sideOffset={4}
              avoidCollisions={false}
              className="max-h-[min(16rem,var(--radix-select-content-available-height))]"
            >
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
      </SettingCard>
    </div>
  )
}
