"use client"

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react"
import {
  CheckCircle2,
  FilePenLine,
  FileDiff,
  GitBranch,
  GitMerge,
  Loader2,
  RefreshCw,
  Save,
  TriangleAlert,
} from "lucide-react"

import { UnifiedDiffPreview } from "@/components/diff/unified-diff-preview"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Textarea } from "@/components/ui/textarea"
import {
  readFileForEdit,
  saveFileContent,
  workTaskChangedFiles,
  workTaskComplete,
  workTaskDiff,
  workTaskGet,
  workTaskMerge,
} from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import { subscribe } from "@/lib/platform"
import { CerebroWebSocketPeer } from "@/lib/transport/cerebro-websocket-peer"
import {
  clearCerebroRemoteTransport,
  configureCerebroRemoteTransport,
} from "@/lib/transport"
import type {
  FileEditContent,
  WorkTask,
  WorkTaskChangedFile,
} from "@/lib/types"

const INIT_MESSAGE_TYPE = "CEREBRO_REMOTE_WORKBENCH_INIT"
const READY_MESSAGE_TYPE = "CEREBRO_REMOTE_WORKBENCH_READY"
const TASK_CHANGED_CHANNEL = "task://changed"

interface InitMessage {
  TYPE: typeof INIT_MESSAGE_TYPE
  TICKET: string
}

interface WorkbenchState {
  workspace: string
  repository: string | null
  branch: string | null
  taskId: number
}

interface TaskData {
  task: WorkTask
  files: WorkTaskChangedFile[]
  diff: string
}

function parseInitMessage(value: unknown): InitMessage | null {
  if (!value || typeof value !== "object") return null
  const record = value as Record<string, unknown>
  if (record.TYPE !== INIT_MESSAGE_TYPE) return null
  if (typeof record.TICKET !== "string" || !record.TICKET.trim()) return null
  return { TYPE: INIT_MESSAGE_TYPE, TICKET: record.TICKET }
}

function parseTaskId(value: string | null): number {
  if (!value || !/^\d+$/.test(value)) {
    throw new Error("远程工作台未绑定本地任务")
  }
  const taskId = Number(value)
  if (!Number.isSafeInteger(taskId) || taskId <= 0) {
    throw new Error("远程工作台任务编号无效")
  }
  return taskId
}

export default function CerebroWorkbenchPage() {
  const mountedRef = useRef(true)
  const connectedRef = useRef(false)
  const taskIdRef = useRef<number | null>(null)
  const unsubscribeTaskRef = useRef<(() => void) | null>(null)
  const [phase, setPhase] = useState<
    "waiting" | "connecting" | "ready" | "error"
  >("waiting")
  const [workbench, setWorkbench] = useState<WorkbenchState | null>(null)
  const [taskData, setTaskData] = useState<TaskData | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<"merge" | "complete" | "refresh" | null>(
    null
  )
  const [notice, setNotice] = useState<string | null>(null)
  const [editingFile, setEditingFile] = useState<FileEditContent | null>(null)
  const [editingContent, setEditingContent] = useState("")
  const [fileBusy, setFileBusy] = useState<"open" | "save" | null>(null)

  const loadTask = useCallback(async (taskId: number) => {
    const [task, files, diff] = await Promise.all([
      workTaskGet(taskId),
      workTaskChangedFiles(taskId),
      workTaskDiff(taskId),
    ])
    if (mountedRef.current) setTaskData({ task, files, diff })
  }, [])

  useEffect(() => {
    mountedRef.current = true
    let disposed = false
    const connect = async (platformOrigin: string, ticket: string) => {
      setPhase("connecting")
      setError(null)
      try {
        const peer = new CerebroWebSocketPeer(platformOrigin, ticket)
        configureCerebroRemoteTransport(peer)
        await peer.waitForReady()
        const snapshot = peer.workspaceSnapshot()
        if (!snapshot) throw new Error("Dextra 未返回工作台快照")
        const taskId = parseTaskId(snapshot.workTaskId)
        taskIdRef.current = taskId
        if (mountedRef.current) {
          setWorkbench({
            workspace: snapshot.workspace,
            repository: snapshot.repository,
            branch: snapshot.branch,
            taskId,
          })
        }
        await loadTask(taskId)
        const unsubscribe = await subscribe<{ id?: number }>(
          TASK_CHANGED_CHANNEL,
          (event) => {
            if (event.id === undefined || event.id === taskId) {
              void loadTask(taskId).catch((cause) => {
                if (mountedRef.current) setError(toErrorMessage(cause))
              })
            }
          }
        )
        if (disposed) {
          unsubscribe()
          return
        }
        unsubscribeTaskRef.current = unsubscribe
        if (mountedRef.current) setPhase("ready")
      } catch (cause) {
        clearCerebroRemoteTransport()
        if (mountedRef.current) {
          setError(toErrorMessage(cause))
          setPhase("error")
        }
      }
    }

    const onMessage = (event: MessageEvent<unknown>) => {
      if (event.source !== window.parent || connectedRef.current) return
      const message = parseInitMessage(event.data)
      if (!message) return
      if (!event.origin || event.origin === "null") {
        setError("无法确认平台来源")
        setPhase("error")
        return
      }
      connectedRef.current = true
      void connect(event.origin, message.TICKET)
    }

    window.addEventListener("message", onMessage)
    window.parent.postMessage({ TYPE: READY_MESSAGE_TYPE }, "*")
    return () => {
      disposed = true
      mountedRef.current = false
      window.removeEventListener("message", onMessage)
      unsubscribeTaskRef.current?.()
      unsubscribeTaskRef.current = null
      clearCerebroRemoteTransport()
    }
  }, [loadTask])

  const refresh = useCallback(async () => {
    const taskId = taskIdRef.current
    if (!taskId) return
    setBusy("refresh")
    setError(null)
    try {
      await loadTask(taskId)
    } catch (cause) {
      setError(toErrorMessage(cause))
    } finally {
      setBusy(null)
    }
  }, [loadTask])

  const merge = useCallback(async () => {
    const taskId = taskIdRef.current
    if (!taskId) return
    setBusy("merge")
    setError(null)
    setNotice(null)
    try {
      const queued = await workTaskMerge(taskId, null, false)
      setNotice(queued ? "已进入合并队列" : "已开始合并")
      await loadTask(taskId)
    } catch (cause) {
      setError(toErrorMessage(cause))
    } finally {
      setBusy(null)
    }
  }, [loadTask])

  const complete = useCallback(async () => {
    const taskId = taskIdRef.current
    if (!taskId) return
    setBusy("complete")
    setError(null)
    setNotice(null)
    try {
      await workTaskComplete(taskId, false)
      setNotice("任务已完成")
      await loadTask(taskId)
    } catch (cause) {
      setError(toErrorMessage(cause))
    } finally {
      setBusy(null)
    }
  }, [loadTask])

  const openFile = useCallback((path: string) => {
    setFileBusy("open")
    setError(null)
    setNotice(null)
    void readFileForEdit("/workspace", path)
      .then((file) => {
        if (!mountedRef.current) return
        setEditingFile(file)
        setEditingContent(file.content)
      })
      .catch((cause) => {
        if (mountedRef.current) setError(toErrorMessage(cause))
      })
      .finally(() => {
        if (mountedRef.current) setFileBusy(null)
      })
  }, [])

  const saveFile = useCallback(async () => {
    if (!editingFile) return
    setFileBusy("save")
    setError(null)
    setNotice(null)
    try {
      const saved = await saveFileContent(
        "/workspace",
        editingFile.path,
        editingContent,
        null
      )
      setEditingFile((current) =>
        current
          ? {
              ...current,
              content: editingContent,
              etag: saved.etag,
              mtime_ms: saved.mtime_ms,
              readonly: saved.readonly,
              line_ending: saved.line_ending,
            }
          : current
      )
      setNotice(`已保存 ${editingFile.path}`)
      const taskId = taskIdRef.current
      if (taskId) await loadTask(taskId)
    } catch (cause) {
      setError(toErrorMessage(cause))
    } finally {
      setFileBusy(null)
    }
  }, [editingContent, editingFile, loadTask])

  if (phase === "waiting" || phase === "connecting") {
    return (
      <CenteredState
        icon={<Loader2 className="size-5 animate-spin" />}
        title={phase === "waiting" ? "等待 Cerebro 授权" : "正在连接 Dextra"}
        description="授权只保留在当前页面内存中。"
      />
    )
  }

  if (phase === "error" || !workbench || !taskData) {
    return (
      <CenteredState
        icon={<TriangleAlert className="size-5 text-destructive" />}
        title="远程工作台不可用"
        description={error ?? "工作台没有返回可审阅的任务。"}
      />
    )
  }

  const { task, files, diff } = taskData
  const canAccept = task.status === "review" && busy === null
  const hasChanges = files.length > 0

  return (
    <main className="flex h-screen min-h-0 flex-col bg-background text-foreground">
      <header className="flex shrink-0 items-center justify-between gap-4 border-b px-5 py-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h1 className="truncate text-base font-semibold">
              {workbench.workspace}
            </h1>
            <Badge variant="outline">{task.status}</Badge>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
            {workbench.repository && <span>{workbench.repository}</span>}
            {workbench.branch && (
              <span className="inline-flex items-center gap-1">
                <GitBranch className="size-3" />
                {workbench.branch}
              </span>
            )}
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void refresh()}
          disabled={busy !== null}
        >
          <RefreshCw className={busy === "refresh" ? "animate-spin" : ""} />
          刷新
        </Button>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-1 gap-3 p-3 lg:grid-cols-[minmax(18rem,24rem)_minmax(0,1fr)]">
        <ScrollArea className="min-h-0">
          <div className="space-y-3 pr-1">
            <Card size="sm">
              <CardHeader>
                <CardTitle>{task.title}</CardTitle>
                <CardDescription>本地任务 #{workbench.taskId}</CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div>
                  <h2 className="mb-1 text-xs font-medium text-muted-foreground">
                    结果摘要
                  </h2>
                  <p className="whitespace-pre-wrap text-sm">
                    {task.result_summary?.trim() || "任务尚未提供结果摘要。"}
                  </p>
                </div>
                <div className="grid grid-cols-3 gap-2 text-center text-xs">
                  <Metric
                    label="文件"
                    value={task.files_changed ?? files.length}
                  />
                  <Metric
                    label="新增"
                    value={task.additions ?? 0}
                    tone="positive"
                  />
                  <Metric
                    label="删除"
                    value={task.deletions ?? 0}
                    tone="negative"
                  />
                </div>
                <div>
                  <h2 className="mb-2 text-xs font-medium text-muted-foreground">
                    变更文件
                  </h2>
                  {files.length ? (
                    <ul className="space-y-1 font-mono text-xs" dir="ltr">
                      {files.map((file) => (
                        <li
                          key={file.file}
                          className="flex items-center justify-between gap-3"
                        >
                          <button
                            type="button"
                            className="min-w-0 truncate text-left hover:underline"
                            onClick={() => openFile(file.file)}
                            disabled={fileBusy !== null}
                            aria-label={`编辑 ${file.file}`}
                          >
                            {file.file}
                          </button>
                          <span className="flex shrink-0 items-center gap-2 text-muted-foreground">
                            <span>
                              <span className="text-green-600">
                                +{file.additions}
                              </span>{" "}
                              <span className="text-red-600">
                                -{file.deletions}
                              </span>
                            </span>
                            <FilePenLine className="size-3.5" />
                          </span>
                        </li>
                      ))}
                    </ul>
                  ) : (
                    <p className="text-xs text-muted-foreground">
                      没有待合并的文件变更。
                    </p>
                  )}
                </div>
              </CardContent>
              <CardFooter className="flex-wrap gap-2 border-t">
                {hasChanges ? (
                  <Button onClick={() => void merge()} disabled={!canAccept}>
                    {busy === "merge" ? (
                      <Loader2 className="animate-spin" />
                    ) : (
                      <GitMerge />
                    )}
                    合并变更
                  </Button>
                ) : (
                  <Button onClick={() => void complete()} disabled={!canAccept}>
                    {busy === "complete" ? (
                      <Loader2 className="animate-spin" />
                    ) : (
                      <CheckCircle2 />
                    )}
                    完成任务
                  </Button>
                )}
                {task.status !== "review" && (
                  <span className="text-xs text-muted-foreground">
                    当前状态无需审阅操作。
                  </span>
                )}
              </CardFooter>
            </Card>
            {(error || notice) && (
              <div
                role={error ? "alert" : "status"}
                className={
                  error
                    ? "rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
                    : "rounded-lg border border-border bg-muted/40 px-3 py-2 text-sm"
                }
              >
                {error ?? notice}
              </div>
            )}
          </div>
        </ScrollArea>

        <section className="flex min-h-0 flex-col gap-3 overflow-hidden">
          {editingFile ? (
            <div className="flex min-h-64 flex-1 flex-col overflow-hidden rounded-xl border bg-card">
              <div className="flex shrink-0 items-center justify-between gap-3 border-b px-4 py-2">
                <div className="min-w-0">
                  <div className="truncate font-mono text-sm" dir="ltr">
                    {editingFile.path}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    仓库相对路径
                  </div>
                </div>
                <Button
                  size="sm"
                  onClick={() => void saveFile()}
                  disabled={
                    fileBusy !== null ||
                    editingFile.readonly ||
                    editingContent === editingFile.content
                  }
                >
                  {fileBusy === "save" ? (
                    <Loader2 className="animate-spin" />
                  ) : (
                    <Save />
                  )}
                  保存文件
                </Button>
              </div>
              <Textarea
                aria-label="文件内容"
                className="min-h-0 flex-1 resize-none rounded-none border-0 font-mono text-xs focus-visible:ring-0"
                dir="ltr"
                value={editingContent}
                readOnly={editingFile.readonly}
                onChange={(event) => setEditingContent(event.target.value)}
              />
            </div>
          ) : null}
          <div className="flex min-h-48 flex-1 flex-col overflow-hidden rounded-xl border bg-card">
            <div className="flex shrink-0 items-center gap-2 border-b px-4 py-3 text-sm font-medium">
              <FileDiff className="size-4" />
              完整变更
            </div>
            <ScrollArea className="min-h-0 flex-1" x="scroll" dir="ltr">
              {diff.trim() ? (
                <div className="p-3">
                  <UnifiedDiffPreview diffText={diff} unbounded />
                </div>
              ) : (
                <div className="flex h-full min-h-48 items-center justify-center text-sm text-muted-foreground">
                  没有可显示的差异。
                </div>
              )}
            </ScrollArea>
          </div>
        </section>
      </div>
    </main>
  )
}

function CenteredState({
  icon,
  title,
  description,
}: {
  icon: ReactNode
  title: string
  description: string
}) {
  return (
    <main className="flex h-screen items-center justify-center bg-background p-6 text-foreground">
      <Card className="w-full max-w-md" size="sm">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            {icon}
            {title}
          </CardTitle>
          <CardDescription>{description}</CardDescription>
        </CardHeader>
      </Card>
    </main>
  )
}

function Metric({
  label,
  value,
  tone,
}: {
  label: string
  value: number
  tone?: "positive" | "negative"
}) {
  return (
    <div className="rounded-lg bg-muted/50 px-2 py-2">
      <div
        className={
          tone === "positive"
            ? "font-semibold text-green-600"
            : tone === "negative"
              ? "font-semibold text-red-600"
              : "font-semibold"
        }
      >
        {value}
      </div>
      <div className="text-muted-foreground">{label}</div>
    </div>
  )
}
