"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { useTranslations } from "next-intl"
import {
  AlertTriangle,
  ArrowLeft,
  Check,
  FolderOpen,
  Link2,
  Link2Off,
  Loader2,
  MonitorDot,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ClientFolderConfiguration } from "@/components/shared/client-folder-configuration"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { InputGroupButton } from "@/components/ui/input-group"
import { Checkbox } from "@/components/ui/checkbox"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { toast } from "sonner"
import { cn } from "@/lib/utils"
import {
  DirectoryBrowser,
  normalizeFsPath,
  type DirectoryBrowserHandle,
} from "@/components/shared/directory-browser"
import {
  createFolderLinks,
  listFolderLinks,
  previewFolderLinks,
  removeFolderLink,
  renameFolderLink,
  repairFolderLink,
  saveCerebroFolderConfiguration,
} from "@/lib/api"
import { useImeGuard } from "@/hooks/use-ime-guard"
import { isDesktop, openFileDialog } from "@/lib/platform"
import { parentFsPath } from "@/lib/path-utils"
import { getActiveRemoteConnectionId } from "@/lib/transport"
import { extractAppCommandError, toErrorMessage } from "@/lib/app-error"
import {
  useClientConfigurationDraft,
  configurationErrorMessage,
  sameConfiguration,
} from "@/hooks/use-client-configuration-draft"
import {
  basenameOf,
  linkNameKey,
  partitionPlans,
  validateLinkName,
  type LinkNameIssue,
} from "@/lib/folder-links"
import type {
  FolderDetail,
  FolderLinkDetail,
  FolderLinkPlan,
  FolderLinkStatus,
} from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"

type View = "pick-root" | "links" | "add-targets" | "client-config"

interface WorkspaceFolderDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /**
   * Manage mode: skip root selection and edit this folder's links directly.
   * When absent the dialog starts at directory selection and opens the folder
   * before moving on.
   */
  folder?: FolderDetail | null
  /** Called once the workspace folder has been opened (creation flow only). */
  onFolderOpened?: (folder: FolderDetail) => void
}

/**
 * The single entry point for adding a workspace folder.
 *
 * Step 1 picks the workspace root (the same in-app browser on desktop and web —
 * the native picker is offered as a shortcut, not as a separate code path), and
 * step 2 links any number of other directories in as subdirectories of that
 * root. Reopened from the folder menu, it starts at step 2 so links can be
 * renamed, repaired, or removed later.
 */
export function WorkspaceFolderDialog({
  open,
  onOpenChange,
  folder,
  onFolderOpened,
}: WorkspaceFolderDialogProps) {
  const t = useTranslations("Folder.workspaceDialog")
  const tConfiguration = useTranslations("CerebroFolder")
  const tBrowser = useTranslations("DirectoryBrowser")
  const openFolder = useAppWorkspaceStore((s) => s.openFolder)

  const manageMode = !!folder
  const [view, setView] = useState<View>(manageMode ? "links" : "pick-root")
  const [rootFolder, setRootFolder] = useState<FolderDetail | null>(
    folder ?? null
  )
  const [rootPath, setRootPath] = useState("")
  const [browserBusy, setBrowserBusy] = useState(false)
  const [openingRoot, setOpeningRoot] = useState(false)
  const rootBrowserRef = useRef<DirectoryBrowserHandle>(null)

  // Link targets being picked in the "add" view.
  const [targetPath, setTargetPath] = useState("")
  const [pickedTargets, setPickedTargets] = useState<string[]>([])
  const [pending, setPending] = useState<PendingLink[]>([])
  const [skipped, setSkipped] = useState<FolderLinkPlan[]>([])
  const [previewing, setPreviewing] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const submittingRef = useRef(false)
  const [submitError, setSubmitError] = useState<{
    kind: "links" | "server"
    message: string
    uncertain?: boolean
  } | null>(null)
  const [renames, setRenames] = useState<Record<number, string>>({})
  const [removals, setRemovals] = useState<Set<number>>(new Set())
  const [repairs, setRepairs] = useState<Set<number>>(new Set())
  const [reconcileNeeded, setReconcileNeeded] = useState(false)
  const rootFolderId = rootFolder?.id ?? null
  const configuration = useClientConfigurationDraft(rootFolderId, open)
  const close = (nextOpen: boolean) => {
    if (!submittingRef.current) onOpenChange(nextOpen)
  }

  const [links, setLinks] = useState<FolderLinkDetail[]>([])
  const [loadingLinks, setLoadingLinks] = useState(false)
  const [gitExclude, setGitExclude] = useState(true)
  const [renamingId, setRenamingId] = useState<number | null>(null)
  const [renameValue, setRenameValue] = useState("")

  const nativePickerAvailable =
    isDesktop() && getActiveRemoteConnectionId() === null

  const linkBrowseStart = useMemo(
    () =>
      rootFolder ? (parentFsPath(rootFolder.path) ?? undefined) : undefined,
    [rootFolder]
  )

  // Reset to a clean session on every real open, so a dialog reopened after a
  // cancel never shows the previous run's picks.
  useEffect(() => {
    if (!open) return
    setView(folder ? "links" : "pick-root")
    setRootFolder(folder ?? null)
    setRootPath(folder?.path ?? "")
    setTargetPath("")
    setPickedTargets([])
    setPending([])
    setSkipped([])
    setLinks([])
    setRenamingId(null)
    setOpeningRoot(false)
    setSubmitting(false)
    setRenames({})
    setRemovals(new Set())
    setRepairs(new Set())
    setSubmitError(null)
    setReconcileNeeded(false)
    setPreviewing(false)
  }, [open, folder])

  useEffect(() => {
    if (!open || rootFolderId === null) return
    let active = true
    setLoadingLinks(true)
    listFolderLinks(rootFolderId)
      .then((value) => {
        if (active) setLinks(value)
      })
      .catch((error) => {
        if (active)
          setSubmitError({ kind: "links", message: toErrorMessage(error) })
      })
      .finally(() => {
        if (active) setLoadingLinks(false)
      })
    return () => {
      active = false
    }
  }, [open, rootFolderId])

  // ── Step 1: workspace root ────────────────────────────────────────────────

  const commitRoot = useCallback(
    async (path: string) => {
      setOpeningRoot(true)
      try {
        const detail = await openFolder(path)
        setRootFolder(detail)
        if (detail.id !== rootFolder?.id) {
          setPending([])
          setSkipped([])
          setLinks([])
          setRenames({})
          setRemovals(new Set())
          setRepairs(new Set())
          setSubmitError(null)
          setReconcileNeeded(false)
          setRenamingId(null)
        }
        onFolderOpened?.(detail)
        setView("links")
      } catch (err) {
        toast.error(t("openFailed"), { description: toErrorMessage(err) })
      } finally {
        setOpeningRoot(false)
      }
    },
    [openFolder, onFolderOpened, rootFolder?.id, t]
  )

  const handleConfirmRoot = useCallback(async () => {
    if (openingRoot || browserBusy) return
    const selected = await rootBrowserRef.current?.confirm()
    if (selected) await commitRoot(selected)
  }, [openingRoot, browserBusy, commitRoot])

  // The picker fills the path box and stops there. It sits inside that box, so
  // it has to behave like typing into it — opening the folder outright would
  // skip the confirm step and land it in the sidebar before the user asked.
  const handleNativeRootPick = useCallback(async () => {
    const selected = await openFileDialog({ directory: true, multiple: false })
    if (!selected) return
    const path = Array.isArray(selected) ? selected[0] : selected
    if (path) setRootPath(path)
  }, [])

  // ── Step 2: link targets ──────────────────────────────────────────────────

  const toggleTarget = useCallback((path: string) => {
    const key = normalizeFsPath(path)
    setPickedTargets((prev) =>
      prev.some((p) => normalizeFsPath(p) === key)
        ? prev.filter((p) => normalizeFsPath(p) !== key)
        : [...prev, path]
    )
  }, [])

  const runPreview = useCallback(
    async (paths: string[]) => {
      if (!rootFolder || paths.length === 0) return
      setPreviewing(true)
      try {
        const plans = await previewFolderLinks(rootFolder.id, paths)
        const { usable, skipped: rejected } = partitionPlans(plans)
        // Merge rather than replace: a second "add" round keeps what the user
        // already queued (and edited) instead of dropping it.
        setPending((prev) => {
          const seen = new Set(prev.map((p) => normalizeFsPath(p.targetPath)))
          const additions = usable
            .filter((plan) => !seen.has(normalizeFsPath(plan.targetPath)))
            .map<PendingLink>((plan) => ({
              targetPath: plan.targetPath,
              name: plan.name,
              baseName: plan.baseName,
              renamed: plan.renamed,
              collidesWithExistingEntry: plan.collidesWithExistingEntry,
            }))
          return [...prev, ...additions]
        })
        setSkipped(rejected)
        setView("links")
      } catch (err) {
        toast.error(t("previewFailed"), { description: toErrorMessage(err) })
      } finally {
        setPreviewing(false)
      }
    },
    [rootFolder, t]
  )

  const handleAddPicked = useCallback(async () => {
    // With nothing ticked, fall back to whatever is in the path box — that's
    // the whole interaction for a typed path. Once anything IS ticked the box
    // only tracks the last row clicked, so honouring it too would re-add a
    // folder the user had just unticked.
    const typed = targetPath.trim()
    const all = pickedTargets.length > 0 ? pickedTargets : typed ? [typed] : []
    setPickedTargets([])
    await runPreview(all)
  }, [targetPath, pickedTargets, runPreview])

  // Same rule as the root picker: stage, don't commit. A multi-select lands in
  // the tick list (which is what "Add" reads) with the box tracking the last
  // one, exactly as if the rows had been clicked in the tree.
  const handleNativeTargetPick = useCallback(async () => {
    const selected = await openFileDialog({ directory: true, multiple: true })
    if (!selected) return
    const paths = (Array.isArray(selected) ? selected : [selected]).filter(
      Boolean
    )
    if (paths.length === 0) return
    setPickedTargets((prev) => {
      const seen = new Set(prev.map(normalizeFsPath))
      return [...prev, ...paths.filter((p) => !seen.has(normalizeFsPath(p)))]
    })
    setTargetPath(paths[paths.length - 1])
  }, [])

  // Names already spoken for, so an inline edit can be validated before saving.
  const takenNames = useMemo(
    () =>
      new Set(
        links
          .filter((link) => !removals.has(link.id))
          .map((link) => linkNameKey(renames[link.id] ?? link.name))
      ),
    [links, removals, renames]
  )

  const pendingIssues = useMemo(() => {
    const issues = new Map<string, LinkNameIssue>()
    const seen = new Set(takenNames)
    for (const item of pending) {
      const issue = validateLinkName(item.name, seen)
      if (issue) issues.set(item.targetPath, issue)
      else seen.add(linkNameKey(item.name))
    }
    return issues
  }, [pending, takenNames])

  const renameIssue = useMemo(() => {
    if (renamingId === null) return null
    const others = new Set([
      ...links
        .filter((link) => link.id !== renamingId && !removals.has(link.id))
        .map((link) => linkNameKey(renames[link.id] ?? link.name)),
      ...pending.map((item) => linkNameKey(item.name)),
    ])
    return validateLinkName(renameValue, others)
  }, [renamingId, renameValue, links, renames, removals, pending])

  const commitRename = () => {
    if (renamingId === null || renameIssue) return
    const original = links.find((link) => link.id === renamingId)
    setRenames((current) => {
      const next = { ...current }
      if (original?.name === renameValue.trim()) delete next[renamingId]
      else next[renamingId] = renameValue.trim()
      return next
    })
    setRenamingId(null)
  }
  const toggleRemoval = (id: number) =>
    setRemovals((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  const toggleRepair = (id: number) =>
    setRepairs((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  const localDirty =
    pending.length > 0 ||
    Object.keys(renames).length > 0 ||
    removals.size > 0 ||
    repairs.size > 0

  async function submit(discardServer = false) {
    if (
      !rootFolder ||
      submittingRef.current ||
      pendingIssues.size ||
      renamingId !== null
    )
      return
    submittingRef.current = true
    setSubmitting(true)
    setSubmitError(null)
    if (discardServer) configuration.discard()
    let currentLinks = [...links]
    const names = { ...renames }
    const removed = new Set(removals)
    const repaired = new Set(repairs)
    let additions = [...pending]
    const publish = () => {
      setLinks([...currentLinks])
      setRenames({ ...names })
      setRemovals(new Set(removed))
      setRepairs(new Set(repaired))
      setPending([...additions])
    }
    // 失败后按真实链接事实收敛草稿，不自动重放可能已成功的写操作。
    const reconcile = async () => {
      currentLinks = await listFolderLinks(rootFolder.id)
      for (const id of removed)
        if (!currentLinks.some((link) => link.id === id)) {
          removed.delete(id)
          delete names[id]
          repaired.delete(id)
        }
      for (const [id, name] of Object.entries(names))
        if (
          currentLinks.some(
            (link) => link.id === Number(id) && link.name === name
          )
        )
          delete names[Number(id)]
      for (const id of repaired)
        if (currentLinks.some((link) => link.id === id && link.status === "ok"))
          repaired.delete(id)
      if (additions.length) {
        const plans = await previewFolderLinks(
          rootFolder.id,
          additions.map((item) => item.targetPath)
        )
        const existing = new Set(
          plans
            .filter((plan) => plan.rejection === "already_linked")
            .map((plan) => normalizeFsPath(plan.targetPath))
        )
        additions = additions.filter(
          (item) => !existing.has(normalizeFsPath(item.targetPath))
        )
      }
      publish()
      setReconcileNeeded(false)
    }
    try {
      try {
        if (reconcileNeeded) await reconcile()
        for (const id of [...removed]) {
          await removeFolderLink(id, true)
          currentLinks = currentLinks.filter((link) => link.id !== id)
          removed.delete(id)
          delete names[id]
          repaired.delete(id)
          publish()
        }
        // 先处理目标名称已空出的改名；不引入临时名称或批量互换路径。
        while (Object.keys(names).length) {
          const next = Object.entries(names).find(
            ([id, name]) =>
              !currentLinks.some(
                (link) =>
                  link.id !== Number(id) &&
                  linkNameKey(link.name) === linkNameKey(name)
              )
          )
          if (!next) throw new Error(t("nameIssue.duplicate"))
          const [key, name] = next
          const id = Number(key)
          const changed = await renameFolderLink(id, name)
          currentLinks = currentLinks.map((link) =>
            link.id === id ? changed : link
          )
          delete names[id]
          publish()
        }
        for (const id of [...repaired]) {
          const changed = await repairFolderLink(id)
          currentLinks = currentLinks.map((link) =>
            link.id === id ? changed : link
          )
          repaired.delete(id)
          publish()
        }
        for (const item of [...additions]) {
          const created = await createFolderLinks(
            rootFolder.id,
            [{ path: item.targetPath, name: item.name }],
            gitExclude
          )
          if (!created.length)
            throw new Error(
              tConfiguration("linkNotCreated", { path: item.targetPath })
            )
          currentLinks.push(...created)
          additions = additions.filter(
            (candidate) => candidate.targetPath !== item.targetPath
          )
          publish()
        }
      } catch (error) {
        setReconcileNeeded(true)
        let message = toErrorMessage(error)
        try {
          await reconcile()
        } catch (readError) {
          message += `；${toErrorMessage(readError)}`
        }
        setSubmitError({ kind: "links", message })
        return
      }
      if (
        !discardServer &&
        configuration.baseline &&
        !sameConfiguration(configuration.baseline, configuration.input)
      ) {
        try {
          const saved = await saveCerebroFolderConfiguration(
            rootFolder.id,
            configuration.input
          )
          configuration.accept(saved)
        } catch (error) {
          const parsed = extractAppCommandError(error)
          setSubmitError({
            kind: "server",
            message: configurationErrorMessage(error),
            uncertain: !parsed || parsed.code === "network_error",
          })
          return
        }
      }
      onOpenChange(false)
    } finally {
      submittingRef.current = false
      setSubmitting(false)
    }
  }

  // ── Render ────────────────────────────────────────────────────────────────

  const title =
    view === "client-config"
      ? tConfiguration("title")
      : view === "add-targets"
        ? t("addTargetsTitle")
        : manageMode
          ? t("manageTitle")
          : t("title")

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {view !== "client-config" && (
            <DialogDescription>
              {view === "pick-root"
                ? t("pickRootDescription")
                : view === "add-targets"
                  ? t("addTargetsDescription")
                  : t("linksDescription")}
            </DialogDescription>
          )}
        </DialogHeader>

        {rootFolder && (view === "links" || view === "client-config") && (
          <Tabs
            value={view}
            onValueChange={(value) => {
              if (!submitting) setView(value as "links" | "client-config")
            }}
          >
            <TabsList className="w-full">
              <TabsTrigger
                disabled={submitting}
                value="links"
                className="flex-1"
              >
                {tConfiguration("folderLinks")}
              </TabsTrigger>
              <TabsTrigger
                disabled={submitting}
                value="client-config"
                className="flex-1"
              >
                {tConfiguration("title")}
              </TabsTrigger>
            </TabsList>
          </Tabs>
        )}
        {open && rootFolder && (
          <div
            hidden={view !== "client-config"}
            className="min-h-0 overflow-y-auto"
          >
            <ClientFolderConfiguration
              key={rootFolder.id}
              configuration={configuration.configuration}
              input={configuration.input}
              loading={configuration.loading}
              busy={submitting}
              error={configuration.error}
              onChange={configuration.edit}
              onRetry={configuration.retry}
            />
          </div>
        )}

        {view === "pick-root" ? (
          <>
            <DirectoryBrowser
              ref={rootBrowserRef}
              active={open && view === "pick-root"}
              value={rootPath}
              onValueChange={setRootPath}
              onQuickSelect={commitRoot}
              onBusyChange={setBrowserBusy}
              pathInputAction={
                nativePickerAvailable ? (
                  <NativePickerButton
                    onPick={handleNativeRootPick}
                    disabled={openingRoot}
                  />
                ) : null
              }
            />
            <DialogFooter>
              <Button
                variant="outline"
                type="button"
                onClick={() => close(false)}
              >
                {tBrowser("cancel")}
              </Button>
              <Button
                type="button"
                onClick={handleConfirmRoot}
                disabled={!rootPath.trim() || openingRoot || browserBusy}
              >
                {openingRoot ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : null}
                {t("next")}
              </Button>
            </DialogFooter>
          </>
        ) : null}

        {view === "add-targets" ? (
          <>
            <DirectoryBrowser
              active={open && view === "add-targets"}
              // Start beside the workspace, not inside it: anything under the
              // root is already reachable and gets rejected, while sibling
              // projects are what people actually link.
              initialPath={linkBrowseStart}
              value={targetPath}
              onValueChange={setTargetPath}
              multiple
              selectedPaths={pickedTargets}
              onToggleSelected={toggleTarget}
              pathInputAction={
                nativePickerAvailable ? (
                  <NativePickerButton
                    onPick={handleNativeTargetPick}
                    disabled={previewing}
                  />
                ) : null
              }
            />
            <DialogFooter>
              <Button
                variant="outline"
                type="button"
                onClick={() => {
                  setPickedTargets([])
                  setView("links")
                }}
              >
                <ArrowLeft className="size-4" />
                {t("back")}
              </Button>
              <Button
                type="button"
                onClick={handleAddPicked}
                disabled={
                  previewing ||
                  (pickedTargets.length === 0 && !targetPath.trim())
                }
              >
                {previewing ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : null}
                {pickedTargets.length > 1
                  ? t("addSelectedCount", { count: pickedTargets.length })
                  : t("addSelected")}
              </Button>
            </DialogFooter>
          </>
        ) : null}

        {view === "links" ? (
          <>
            <fieldset
              disabled={submitting}
              className="flex min-h-0 flex-col gap-3 overflow-y-auto"
            >
              <div className="flex items-center gap-2 rounded-md border bg-muted/40 px-3 py-2">
                <FolderOpen className="size-4 shrink-0 text-muted-foreground" />
                <span
                  className="min-w-0 flex-1 truncate font-mono text-xs"
                  title={rootFolder?.path}
                >
                  {rootFolder?.path ?? ""}
                </span>
                {!manageMode ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    type="button"
                    className="h-6 shrink-0 px-2 text-xs"
                    onClick={() => setView("pick-root")}
                  >
                    {t("change")}
                  </Button>
                ) : null}
              </div>

              <ScrollArea className="min-h-0 flex-1 rounded-md border">
                <div className="divide-y">
                  {loadingLinks &&
                  links.length === 0 &&
                  pending.length === 0 ? (
                    <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
                      <Loader2 className="size-3.5 animate-spin" />
                      {tBrowser("loading")}
                    </div>
                  ) : null}

                  {links.map((link) => (
                    <LinkRow
                      key={link.id}
                      link={{ ...link, name: renames[link.id] ?? link.name }}
                      busy={submitting}
                      removed={removals.has(link.id)}
                      repairPending={repairs.has(link.id)}
                      editing={renamingId === link.id}
                      renameValue={renameValue}
                      renameIssue={renameIssue}
                      onRenameChange={setRenameValue}
                      onStartRename={() => {
                        setRenamingId(link.id)
                        setRenameValue(renames[link.id] ?? link.name)
                      }}
                      onCancelRename={() => setRenamingId(null)}
                      onCommitRename={commitRename}
                      onRemove={() => toggleRemoval(link.id)}
                      onRepair={() => toggleRepair(link.id)}
                    />
                  ))}

                  {pending.map((item, index) => (
                    <PendingRow
                      key={item.targetPath}
                      item={item}
                      issue={pendingIssues.get(item.targetPath) ?? null}
                      onNameChange={(name) =>
                        setPending((prev) =>
                          prev.map((p, i) => (i === index ? { ...p, name } : p))
                        )
                      }
                      onRemove={() =>
                        setPending((prev) => prev.filter((_, i) => i !== index))
                      }
                    />
                  ))}

                  {skipped.map((plan) => (
                    <div
                      key={plan.targetPath}
                      className="flex items-start gap-2 px-3 py-2 text-xs text-muted-foreground"
                    >
                      <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-amber-500" />
                      <div className="min-w-0">
                        <div className="truncate font-mono">
                          {plan.targetPath}
                        </div>
                        <div>
                          {plan.rejection === "already_linked" &&
                          plan.existingLinkName
                            ? t("rejection.alreadyLinkedAs", {
                                name: plan.existingLinkName,
                              })
                            : t(`rejection.${plan.rejection ?? "not_found"}`)}
                        </div>
                      </div>
                    </div>
                  ))}

                  {!loadingLinks &&
                  links.length === 0 &&
                  pending.length === 0 &&
                  skipped.length === 0 ? (
                    <div className="p-6 text-center text-sm text-muted-foreground">
                      {t("noLinks")}
                    </div>
                  ) : null}
                </div>
              </ScrollArea>

              <div className="flex items-center justify-between gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  type="button"
                  onClick={() => {
                    setPickedTargets([])
                    setTargetPath("")
                    setView("add-targets")
                  }}
                >
                  <Plus className="size-4" />
                  {t("addFolders")}
                </Button>
                {pending.length > 0 ? (
                  <label className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground">
                    <Checkbox
                      checked={gitExclude}
                      onCheckedChange={(v) => setGitExclude(v === true)}
                    />
                    {t("gitExclude")}
                  </label>
                ) : null}
              </div>
            </fieldset>
          </>
        ) : null}
        {rootFolder && (view === "links" || view === "client-config") && (
          <div className="shrink-0 space-y-3">
            {submitError && (
              <div role="alert" className="space-y-1 text-sm text-destructive">
                <p>{submitError.message}</p>
                {submitError.kind === "server" && submitError.uncertain && (
                  <p>{tConfiguration("saveUnconfirmed")}</p>
                )}
                <p>{tConfiguration("partialSaveNotice")}</p>
              </div>
            )}
            <DialogFooter className="flex-wrap">
              <Button
                variant="outline"
                disabled={submitting}
                onClick={() => close(false)}
              >
                {tBrowser("cancel")}
              </Button>
              {submitError?.kind === "server" && (
                <Button
                  variant="outline"
                  disabled={submitting}
                  onClick={() => void submit(true)}
                >
                  {tConfiguration("discardServerAndFinish")}
                </Button>
              )}
              <Button
                disabled={
                  submitting ||
                  loadingLinks ||
                  pendingIssues.size > 0 ||
                  renamingId !== null ||
                  (manageMode &&
                    !localDirty &&
                    !configuration.dirty &&
                    !submitError)
                }
                onClick={() => void submit()}
              >
                {submitting && <Loader2 className="size-4 animate-spin" />}
                {submitError &&
                (localDirty || configuration.dirty || reconcileNeeded)
                  ? tConfiguration("retry")
                  : manageMode
                    ? tConfiguration("save")
                    : t("done")}
              </Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

/**
 * Native-picker shortcut that lives at the trailing edge of the path box.
 * Icon-only — the label moves into a tooltip, so the affordance sits next to
 * the thing it fills in instead of competing with the footer's real actions.
 */
function NativePickerButton({
  onPick,
  disabled,
}: {
  onPick: () => void
  disabled: boolean
}) {
  const t = useTranslations("Folder.workspaceDialog")

  return (
    // The app mounts no global tooltip provider — each surface brings its own,
    // and Radix throws without one.
    <TooltipProvider delayDuration={200}>
      <Tooltip>
        <TooltipTrigger asChild>
          <InputGroupButton
            size="icon-xs"
            variant="ghost"
            type="button"
            onClick={onPick}
            disabled={disabled}
            aria-label={t("useSystemPicker")}
          >
            <MonitorDot className="size-3.5" />
          </InputGroupButton>
        </TooltipTrigger>
        <TooltipContent>{t("useSystemPicker")}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

interface PendingLink {
  targetPath: string
  name: string
  baseName: string
  renamed: boolean
  collidesWithExistingEntry: boolean
}

function statusTone(status: FolderLinkStatus) {
  switch (status) {
    case "ok":
      return "text-muted-foreground"
    case "missing":
    case "broken":
      return "text-amber-500"
    case "conflicted":
      return "text-destructive"
  }
}

function LinkRow({
  link,
  busy,
  removed,
  repairPending,
  editing,
  renameValue,
  renameIssue,
  onRenameChange,
  onStartRename,
  onCancelRename,
  onCommitRename,
  onRemove,
  onRepair,
}: {
  link: FolderLinkDetail
  busy: boolean
  removed: boolean
  repairPending: boolean
  editing: boolean
  renameValue: string
  renameIssue: LinkNameIssue | null
  onRenameChange: (value: string) => void
  onStartRename: () => void
  onCancelRename: () => void
  onCommitRename: () => void
  onRemove: () => void
  onRepair: () => void
}) {
  const t = useTranslations("Folder.workspaceDialog")
  const ime = useImeGuard()
  const tConfiguration = useTranslations("CerebroFolder")

  return (
    <div
      className={cn(
        "flex items-center gap-2 px-3 py-2",
        removed && "opacity-60"
      )}
    >
      <Link2
        className={cn("size-4 shrink-0", statusTone(link.status))}
        aria-hidden
      />
      <div className="min-w-0 flex-1">
        {editing ? (
          <div className="flex items-center gap-1">
            <Input
              value={renameValue}
              onChange={(e) => onRenameChange(e.target.value)}
              {...ime.props}
              onKeyDown={(e) => {
                if (ime.isComposing(e)) return
                if (e.key === "Enter") onCommitRename()
                if (e.key === "Escape") onCancelRename()
              }}
              className={cn(
                "h-7 text-sm",
                renameIssue && "border-destructive focus-visible:ring-0"
              )}
              autoFocus
            />
            <Button
              size="icon"
              variant="ghost"
              className="size-7 shrink-0"
              type="button"
              onClick={onCommitRename}
              disabled={!!renameIssue || busy}
              title={tConfiguration("applyName")}
            >
              <Check className="size-3.5" />
            </Button>
            <Button
              size="icon"
              variant="ghost"
              className="size-7 shrink-0"
              type="button"
              onClick={onCancelRename}
              title={t("cancelRename")}
            >
              <X className="size-3.5" />
            </Button>
          </div>
        ) : (
          <div className="truncate text-sm font-medium">{link.name}</div>
        )}
        {editing && renameIssue ? (
          <div className="mt-0.5 text-xs text-destructive">
            {t(`nameIssue.${renameIssue}`)}
          </div>
        ) : (
          <div
            className="truncate font-mono text-xs text-muted-foreground"
            title={link.targetPath}
          >
            {link.targetPath}
          </div>
        )}
        {removed && (
          <p className="text-xs">{tConfiguration("removalPending")}</p>
        )}
        {repairPending && !removed && (
          <p className="text-xs">{tConfiguration("repairPending")}</p>
        )}
        {link.status !== "ok" ? (
          <div className={cn("mt-0.5 text-xs", statusTone(link.status))}>
            {t(`status.${link.status}`)}
          </div>
        ) : null}
      </div>

      {!editing ? (
        // The app mounts no global tooltip provider — each surface brings its
        // own, and Radix throws without one.
        <TooltipProvider delayDuration={200}>
          <div className="flex shrink-0 items-center gap-0.5">
            {!removed &&
            (link.status === "missing" || link.status === "broken") ? (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="size-7"
                    type="button"
                    onClick={onRepair}
                    disabled={busy}
                    aria-label={
                      repairPending ? tConfiguration("undoRepair") : t("repair")
                    }
                  >
                    <RefreshCw className="size-3.5" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>{t("repair")}</TooltipContent>
              </Tooltip>
            ) : null}
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="icon"
                  variant="ghost"
                  className="size-7"
                  type="button"
                  onClick={onStartRename}
                  disabled={busy || removed}
                  aria-label={t("rename")}
                >
                  <Pencil className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t("rename")}</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="icon"
                  variant="ghost"
                  className="size-7 text-muted-foreground hover:text-destructive"
                  type="button"
                  onClick={onRemove}
                  disabled={busy}
                  aria-label={
                    removed ? tConfiguration("undoRemoval") : t("unlink")
                  }
                >
                  {busy ? (
                    <Loader2 className="size-3.5 animate-spin" />
                  ) : (
                    <Link2Off className="size-3.5" />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t("unlink")}</TooltipContent>
            </Tooltip>
          </div>
        </TooltipProvider>
      ) : null}
    </div>
  )
}

function PendingRow({
  item,
  issue,
  onNameChange,
  onRemove,
}: {
  item: PendingLink
  issue: LinkNameIssue | null
  onNameChange: (name: string) => void
  onRemove: () => void
}) {
  const t = useTranslations("Folder.workspaceDialog")

  return (
    <div className="flex items-center gap-2 bg-primary/5 px-3 py-2">
      <Plus className="size-4 shrink-0 text-primary" aria-hidden />
      <div className="min-w-0 flex-1 space-y-1">
        <Input
          value={item.name}
          onChange={(e) => onNameChange(e.target.value)}
          className={cn(
            "h-7 text-sm",
            issue && "border-destructive focus-visible:ring-0"
          )}
        />
        <div
          className="truncate font-mono text-xs text-muted-foreground"
          title={item.targetPath}
        >
          {item.targetPath}
        </div>
        {issue ? (
          <div className="text-xs text-destructive">
            {t(`nameIssue.${issue}`)}
          </div>
        ) : item.renamed ? (
          <div className="text-xs text-amber-600 dark:text-amber-500">
            {item.collidesWithExistingEntry
              ? t("renamedForExistingEntry", { base: item.baseName })
              : t("renamedForDuplicate", { base: item.baseName })}
          </div>
        ) : (
          <div className="text-xs text-muted-foreground">
            {t("willAppearAs", { name: basenameOf(item.targetPath) })}
          </div>
        )}
      </div>
      <Button
        size="icon"
        variant="ghost"
        className="size-7 shrink-0"
        type="button"
        onClick={onRemove}
        title={t("removePending")}
      >
        <Trash2 className="size-3.5" />
      </Button>
    </div>
  )
}
