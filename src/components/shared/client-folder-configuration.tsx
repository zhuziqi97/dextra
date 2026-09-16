"use client"

import { useEffect, useState } from "react"
import { useTranslations } from "next-intl"
import { Loader2 } from "lucide-react"
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

import { ClientMCPScope } from "./client-mcp-scope"

const EMPTY_SCOPE_DETAILS: import("@/lib/generated/cerebro/MCPScopeDetail").MCPScopeDetail[] =
  []

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
  const [projectId, setProjectId] = useState(
    configuration?.execution_project?.id ?? ""
  )
  const [modules, setModules] = useState<ModuleOption[]>([])
  const [optionLabels, setLabels] = useState<
    Record<string, string | undefined>
  >({})
  const [projectsError, setProjectsError] = useState<string | null>(null)
  const [modulesError, setModulesError] = useState<string | null>(null)
  const error = projectsError ?? modulesError
  const labels = { ...configuration?.module_labels, ...optionLabels }
  const targetId = configuration?.target_id
  const [bindingSource, setBindingSource] = useState({
    targetId,
    moduleId: configuration?.execution_module_id,
  })
  // 只在目录或已保存的执行绑定变化时恢复项目；普通刷新不打断当前筛选。
  if (
    bindingSource.targetId !== targetId ||
    bindingSource.moduleId !== configuration?.execution_module_id
  ) {
    setBindingSource({ targetId, moduleId: configuration?.execution_module_id })
    setProjectId(configuration?.execution_project?.id ?? "")
    setModules([])
    setModulesError(null)
  }
  const executionProject = configuration?.execution_project
  const projectOptions =
    executionProject &&
    !projects.some((project) => project.id === executionProject.id)
      ? [executionProject, ...projects]
      : projects
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
        <SettingRow
          title={t("projectFilter")}
          description={t("projectFilterHint")}
        >
          <Select
            value={projectId || "NONE"}
            onValueChange={(value) => {
              setModules([])
              setModulesError(null)
              setProjectId(value === "NONE" ? "" : value)
            }}
            disabled={busy || !configuration}
          >
            <SelectTrigger className="w-full" aria-label={t("projectFilter")}>
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
              <SelectItem value="NONE">{t("noProjectFilter")}</SelectItem>
              {projectOptions.map((project) => (
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
          <ClientMCPScope
            selected={input.mcp_scope_modules}
            details={configuration?.mcp_scope_details ?? EMPTY_SCOPE_DETAILS}
            disabled={busy || !configuration}
            onChange={(items) => edit({ mcp_scope_modules: items })}
          />
        </SettingRow>
        <SettingRow
          title={t("codeGraph")}
          control={
            <Checkbox
              aria-label={t("codeGraph")}
              checked={input.mcp_capabilities.code_graph_enabled}
              disabled={busy || !configuration}
              onCheckedChange={(checked) =>
                edit({
                  mcp_capabilities: { ...input.mcp_capabilities, code_graph_enabled: checked === true },
                })
              }
            />
          }
        />
        <SettingRow
          title="Forge"
          control={
            <Checkbox
              aria-label="Forge"
              checked={input.mcp_capabilities.forge_enabled}
              disabled={busy || !configuration}
              onCheckedChange={(checked) => edit({
                mcp_capabilities: { ...input.mcp_capabilities, forge_enabled: checked === true },
              })}
            />
          }
        />
      </SettingCard>
    </div>
  )
}
