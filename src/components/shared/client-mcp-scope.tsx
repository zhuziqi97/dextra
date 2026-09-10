"use client"

import { useEffect, useState } from "react"
import {
  Search,
  Folder,
  Layers3,
  Check,
  X,
  ShieldCheck,
  ChevronLeft,
  ChevronRight,
} from "lucide-react"
import styles from "./client-mcp-scope.module.css"
import { useTranslations } from "next-intl"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogDescription,
} from "@/components/ui/dialog"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  listCerebroConfigurationModules,
  listCerebroConfigurationProjects,
} from "@/lib/api"
import { configurationErrorMessage } from "@/hooks/use-client-configuration-draft"
import type { MCPScopeItem } from "@/lib/generated/cerebro/MCPScopeItem"
import type { MCPScopeDetail } from "@/lib/generated/cerebro/MCPScopeDetail"
import type { ProjectOption } from "@/lib/generated/cerebro/ProjectOption"
import type { ModuleOption } from "@/lib/generated/cerebro/ModuleOption"

export function ClientMCPScope({
  selected,
  details: savedDetails,
  disabled,
  onChange,
}: {
  selected: MCPScopeItem[]
  details: MCPScopeDetail[]
  disabled: boolean
  onChange: (items: MCPScopeItem[]) => void
}) {
  const t = useTranslations("CerebroFolder")
  const [draft, setDraft] = useState<MCPScopeItem[] | null>(null)
  const [details, setDetails] = useState<MCPScopeDetail[]>(savedDetails)
  const [projects, setProjects] = useState<ProjectOption[]>([])
  const [project, setProject] = useState<ProjectOption | null>(null)
  const [modules, setModules] = useState<ModuleOption[]>([])
  const [search, setSearch] = useState("")
  const [page, setPage] = useState(1)
  const [hasMore, setHasMore] = useState(false)
  const [loading, setLoading] = useState(false)
  const [modulesLoading, setModulesLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [retry, setRetry] = useState(0)
  const open = draft !== null
  const [detailSource, setDetailSource] = useState(savedDetails)
  if (detailSource !== savedDetails) {
    setDetailSource(savedDetails)
    setDetails(savedDetails)
  }
  function beginLoading() {
    setLoading(true)
    setError(null)
  }
  useEffect(() => {
    if (!open) return
    let active = true
    listCerebroConfigurationProjects(page, search || undefined)
      .then((result) => {
        if (!active) return
        setProjects(result.items)
        setProject((current) => current ?? result.items[0] ?? null)
        setHasMore(page < result.total_pages)
      })
      .catch((cause) => {
        if (active) setError(configurationErrorMessage(cause))
      })
      .finally(() => {
        if (active) setLoading(false)
      })
    return () => {
      active = false
    }
  }, [open, search, page, retry])
  useEffect(() => {
    if (!open || !project) return
    let active = true
    listCerebroConfigurationModules(project.id)
      .then((result) => {
        if (!active) return
        setModules(result.items)
        setDetails((current) => [
          ...new Map(
            [
              ...current,
              ...result.items.map((module) => ({
                module_id: module.id,
                module_name: `${module.display_name || module.name}（${module.path}）`,
                project_id: project.id,
                project_name: `${project.display_name || project.name}（${project.path}）`,
                can_write: module.can_write,
              })),
            ].map((item) => [item.module_id, item])
          ).values(),
        ])
        if (result.truncated) setError(t("tooManyModules"))
      })
      .catch((cause) => {
        if (active) setError(configurationErrorMessage(cause))
      })
      .finally(() => {
        if (active) setModulesLoading(false)
      })
    return () => {
      active = false
    }
  }, [open, project, retry, t])

  function permissionControl(
    item: MCPScopeItem,
    items: MCPScopeItem[],
    change: (items: MCPScopeItem[]) => void
  ) {
    const detail = details.find((value) => value.module_id === item.module_id)
    return (
      <Select
        value={item.permission}
        disabled={disabled}
        onValueChange={(permission) =>
          change(
            items.map((value) =>
              value.module_id === item.module_id
                ? { ...value, permission: permission as "READ" | "WRITE" }
                : value
            )
          )
        }
      >
        <SelectTrigger
          aria-label={`${detail?.module_name ?? item.module_id} ${t("permission")}`}
          className={styles.permissionSelect}
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="READ">{t("read")}</SelectItem>
          {(detail?.can_write || item.permission === "WRITE") && (
            <SelectItem value="WRITE" disabled={!detail?.can_write}>
              {t("write")}
            </SelectItem>
          )}
        </SelectContent>
      </Select>
    )
  }

  function summary(
    items: MCPScopeItem[],
    change: (items: MCPScopeItem[]) => void
  ) {
    const groups = [
      ...new Set(
        items.map(
          (item) =>
            details.find((detail) => detail.module_id === item.module_id)
              ?.project_id ?? ""
        )
      ),
    ]
    return (
      <div className={styles.summary}>
        {!items.length && (
          <div className={styles.emptySelection}>
            <Layers3 size={16} />
            <span>{t("emptyScope")}</span>
          </div>
        )}
        {groups.map((id) => (
          <div key={id} className={styles.selectedGroup}>
            <div className={styles.groupTitle}>
              <Folder size={13} />
              <span>
                {details.find((detail) => detail.project_id === id)
                  ?.project_name ?? t("mcpModules")}
              </span>
            </div>
            <div className={styles.selectedGrid}>
              {items
                .filter(
                  (item) =>
                    (details.find(
                      (detail) => detail.module_id === item.module_id
                    )?.project_id ?? "") === id
                )
                .map((item) => {
                  const detail = details.find(
                    (value) => value.module_id === item.module_id
                  )
                  return (
                    <div key={item.module_id} className={styles.selectedItem}>
                      <Check size={14} className={styles.selectedCheck} />
                      <span
                        className={styles.selectedName}
                        title={detail?.module_name ?? item.module_id}
                      >
                        {detail?.module_name ?? item.module_id}
                      </span>
                      {permissionControl(item, items, change)}
                      <button
                        type="button"
                        className={styles.remove}
                        disabled={disabled}
                        aria-label={`${t("removeModule")} ${detail?.module_name ?? item.module_id}`}
                        onClick={() =>
                          change(
                            items.filter(
                              (value) => value.module_id !== item.module_id
                            )
                          )
                        }
                      >
                        <X size={14} />
                      </button>
                    </div>
                  )
                })}
            </div>
          </div>
        ))}
      </div>
    )
  }

  const draftCount = draft?.length ?? 0
  return (
    <div className="space-y-3">
      <Button
        size="sm"
        variant="outline"
        disabled={disabled}
        onClick={() => {
          beginLoading()
          setModulesLoading(true)
          setModules([])
          setDraft(selected.map((item) => ({ ...item })))
        }}
      >
        <Layers3 size={14} />
        {t("configureScope")}
      </Button>
      {summary(selected, onChange)}
      <Dialog
        open={open}
        onOpenChange={(value) => {
          if (!value) setDraft(null)
        }}
      >
        <DialogContent className={styles.dialog}>
          <DialogHeader className={styles.header}>
            <span className={styles.headerIcon}>
              <ShieldCheck size={21} />
            </span>
            <div>
              <DialogTitle className={styles.title}>
                {t("configureScope")}
              </DialogTitle>
              <DialogDescription className={styles.subtitle}>
                {t("scopeDescription")}
              </DialogDescription>
            </div>
          </DialogHeader>
          {error && (
            <div role="alert" className={styles.error}>
              {error}
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  beginLoading()
                  setModulesLoading(true)
                  setRetry((value) => value + 1)
                }}
              >
                {t("retry")}
              </Button>
            </div>
          )}
          <div className={styles.browser}>
            <aside className={styles.projects}>
              <div className={styles.sectionLabel}>{t("project")}</div>
              <div className={styles.search}>
                <Search size={15} />
                <Input
                  aria-label={t("searchProjects")}
                  placeholder={t("searchProjects")}
                  value={search}
                  onChange={(event) => {
                    beginLoading()
                    setSearch(event.target.value)
                    setPage(1)
                  }}
                />
              </div>
              <div className={styles.projectList}>
                {loading && <p className={styles.loading}>{t("loading")}</p>}
                {!loading && !projects.length && (
                  <p className={styles.loading}>{t("noProjects")}</p>
                )}
                {projects.map((item) => {
                  const count =
                    draft?.filter(
                      (scope) =>
                        details.find(
                          (detail) => detail.module_id === scope.module_id
                        )?.project_id === item.id
                    ).length ?? 0
                  return (
                    <button
                      type="button"
                      key={item.id}
                      aria-label={`${item.display_name || item.name}（${item.path}）`}
                      aria-pressed={project?.id === item.id}
                      className={styles.project}
                      onClick={() => {
                        if (project?.id === item.id) return
                        setError(null)
                        setModules([])
                        setModulesLoading(true)
                        setProject(item)
                      }}
                    >
                      <Folder size={17} />
                      <span className={styles.projectCopy}>
                        <strong>{item.display_name || item.name}</strong>
                        <span title={item.path}>{item.path}</span>
                      </span>
                      {count > 0 && (
                        <span className={styles.count}>{count}</span>
                      )}
                    </button>
                  )
                })}
              </div>
              {(page > 1 || hasMore) && (
                <div className={styles.pagination}>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t("previous")}
                    disabled={loading || page === 1}
                    onClick={() => {
                      beginLoading()
                      setPage((value) => value - 1)
                    }}
                  >
                    <ChevronLeft size={16} />
                  </Button>
                  <span>{page}</span>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t("next")}
                    disabled={loading || !hasMore}
                    onClick={() => {
                      beginLoading()
                      setPage((value) => value + 1)
                    }}
                  >
                    <ChevronRight size={16} />
                  </Button>
                </div>
              )}
            </aside>
            <section className={styles.modules}>
              <div className={styles.moduleHeader}>
                <div>
                  <div className={styles.sectionLabel}>
                    {t("availableModules")}
                  </div>
                  <h3>
                    {project?.display_name ||
                      project?.name ||
                      t("selectProject")}
                  </h3>
                </div>
                <span className={styles.readHint}>{t("defaultRead")}</span>
              </div>
              <div className={styles.moduleList}>
                {modulesLoading && project && (
                  <p className={styles.loading}>{t("loading")}</p>
                )}
                {!modulesLoading && project && !modules.length && (
                  <div className={styles.moduleEmpty}>
                    <Layers3 size={28} />
                    <p>{t("noModules")}</p>
                  </div>
                )}
                {!project && !loading && (
                  <div className={styles.moduleEmpty}>
                    <Folder size={28} />
                    <p>{t("selectProject")}</p>
                  </div>
                )}
                {modules.map((module) => {
                  const selectedItem = draft?.find(
                    (item) => item.module_id === module.id
                  )
                  return (
                    <div
                      key={module.id}
                      className={styles.module}
                      data-selected={Boolean(selectedItem)}
                    >
                      <label className={styles.moduleChoice}>
                        <Checkbox
                          aria-label={`${module.display_name || module.name}（${module.path}）`}
                          checked={Boolean(selectedItem)}
                          onCheckedChange={(checked) =>
                            setDraft((current) =>
                              checked
                                ? [
                                    ...(current ?? []),
                                    {
                                      module_id: module.id,
                                      permission: "READ",
                                    },
                                  ]
                                : (current ?? []).filter(
                                    (item) => item.module_id !== module.id
                                  )
                            )
                          }
                        />
                        <span className={styles.moduleIcon}>
                          <Layers3 size={19} />
                        </span>
                        <span className={styles.moduleCopy}>
                          <strong>{module.display_name || module.name}</strong>
                          <span title={module.path}>{module.path}</span>
                        </span>
                      </label>
                      {selectedItem && draft ? (
                        <div className={styles.modulePermission}>
                          {permissionControl(selectedItem, draft, setDraft)}
                        </div>
                      ) : (
                        <span className={styles.unselected}>
                          {t("notSelected")}
                        </span>
                      )}
                    </div>
                  )
                })}
              </div>
            </section>
          </div>
          <section className={styles.selection}>
            <div className={styles.selectionHeader}>
              <span>{t("selectedScope")}</span>
              <span className={styles.count}>{draftCount}</span>
            </div>
            <div className={styles.selectionScroll}>
              {draft && summary(draft, setDraft)}
            </div>
          </section>
          <DialogFooter className={styles.footer}>
            <span className={styles.footerHint}>
              {t("scopeCount", { count: draftCount })}
            </span>
            <Button variant="outline" onClick={() => setDraft(null)}>
              {t("cancel")}
            </Button>
            <Button
              onClick={() => {
                if (draft) onChange(draft)
                setDraft(null)
              }}
            >
              {t("confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
