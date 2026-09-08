"use client"

import { useCallback, useEffect, useRef } from "react"
import { getFolderConversation, resolveCerebroTarget } from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import { toast } from "sonner"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useTabStore, useTabActions } from "@/contexts/tab-context"
import type { AgentType } from "@/lib/types"

/**
 * Handles `/workspace?folderId=X&conversationId=Y&agent=Z` URLs.
 * Runs once after both folders and tabs have hydrated.
 */
export function DeepLinkBootstrap() {
  const foldersHydrated = useAppWorkspaceStore((s) => s.foldersHydrated)
  const tabsHydrated = useTabStore((s) => s.tabsHydrated)
  const { openTab, openNewConversationTab } = useTabActions()
  const ranRef = useRef(false)

  useEffect(() => {
    if (ranRef.current) return
    if (!foldersHydrated || !tabsHydrated) return
    ranRef.current = true

    if (typeof window === "undefined") return

    const params = new URLSearchParams(window.location.search)
    const targetId = params.get("target")
    const rawFolderId = params.get("folderId")
    const rawConversationId = params.get("conversationId")
    const rawAgent = params.get("agent") as AgentType | null

    if (!rawFolderId && !rawConversationId && !targetId) return

    const clearUrl = () => {
      try {
        const url = new URL(window.location.href)
        for (const key of ["folderId", "conversationId", "agent", "target"])
          url.searchParams.delete(key)
        window.history.replaceState({}, "", url.pathname + url.search)
      } catch {
        /* ignore */
      }
    }

    void (async () => {
      try {
        const folderId = targetId
          ? await resolveCerebroTarget(targetId)
          : rawFolderId
            ? Number(rawFolderId)
            : null
        const conversationId = rawConversationId
          ? Number(rawConversationId)
          : null

        if (folderId == null || !Number.isFinite(folderId)) return

        // Read at run time: this effect fires once when hydration completes,
        // and getState() sees exactly the lists as of that moment.
        const { folders, addFolderToWorkspaceById, conversations } =
          useAppWorkspaceStore.getState()

        let folder = folders.find((f) => f.id === folderId)
        if (!folder) {
          try {
            folder = await addFolderToWorkspaceById(folderId)
          } catch (err) {
            console.error("[DeepLinkBootstrap] open folder failed:", err)
            toast.error(toErrorMessage(err))
            return
          }
        }

        if (conversationId == null) {
          if (targetId) {
            // 模块入口定位目录并接续已有会话，不额外创建草稿或启动另一个 Agent。
            await useAppWorkspaceStore.getState().refreshConversations()
            const latest = useAppWorkspaceStore
              .getState()
              .conversations.filter(
                (conversation) => conversation.folder_id === folderId
              )
              .sort((left, right) =>
                right.updated_at.localeCompare(left.updated_at)
              )[0]
            if (latest) {
              openTab(folderId, latest.id, latest.agent_type, true)
              return
            }
          }
          openNewConversationTab(folderId, folder.path)
          return
        }
        if (!Number.isFinite(conversationId) || !rawAgent) return
        // 文件夹与会话列表分别加载；深链不把尚未加载的侧栏当作会话不存在。
        const conversation =
          conversations.find((c) => c.id === conversationId) ??
          (await getFolderConversation(conversationId, { tailTurns: 1 }))
            .summary
        if (
          conversation.folder_id !== folderId ||
          conversation.agent_type !== rawAgent
        ) {
          toast.error("Linked conversation not found")
          return
        }

        openTab(folderId, conversationId, rawAgent, true)
      } catch (error) {
        toast.error(toErrorMessage(error))
      } finally {
        clearUrl()
      }
    })()
  }, [foldersHydrated, tabsHydrated, openTab, openNewConversationTab])

  return null
}

type FocusRequest = {
  folderId: number
  conversationId: number
  agent: AgentType
}

/**
 * Live counterpart to {@link DeepLinkBootstrap}: listens for the pet panel's
 * `workspace://focus-conversation` request (emitted by the `focus_conversation`
 * command after bringing the main window forward) and opens the conversation
 * via `openTab` — no URL reload, so in-memory tab/session state survives.
 *
 * Latest workspace state is held in a ref so the single subscription always
 * sees fresh state without re-subscribing on every change. A request that
 * arrives before folders/tabs hydrate is queued and replayed.
 */
export function PetFocusBridge() {
  const foldersHydrated = useAppWorkspaceStore((s) => s.foldersHydrated)
  const tabsHydrated = useTabStore((s) => s.tabsHydrated)
  const { openTab } = useTabActions()

  // Workspace state is read via getState() at attempt time; only the tab
  // half still needs a ref mirror (it lives in a context, not a store).
  const stateRef = useRef({ tabsHydrated, openTab })
  useEffect(() => {
    stateRef.current = { tabsHydrated, openTab }
  }, [tabsHydrated, openTab])

  // Holds the latest focus request until the workspace has hydrated. The event
  // is one-shot, so a pet-panel click during startup/reload (before folders &
  // tabs hydrate) must not be dropped — replay it once hydration completes.
  const pendingRef = useRef<FocusRequest | null>(null)

  const attempt = useCallback(() => {
    const req = pendingRef.current
    if (!req) return
    const workspace = useAppWorkspaceStore.getState()
    if (!workspace.foldersHydrated || !stateRef.current.tabsHydrated) return // wait for hydration
    // One-shot after hydration (mirrors DeepLinkBootstrap): clear before the
    // async work so a later state change can't double-open.
    pendingRef.current = null
    void (async () => {
      // Ensure the folder is in the workspace so the tab has a home.
      if (!workspace.folders.some((f) => f.id === req.folderId)) {
        try {
          await workspace.addFolderToWorkspaceById(req.folderId)
        } catch (err) {
          console.error("[PetFocusBridge] open folder failed:", err)
          return
        }
      }
      // The event is backend-originated for a live session, so the conversation
      // exists; open the tab directly and let its title/content hydrate. We do
      // NOT gate on the conversations list — it loads independently of folders,
      // and waiting on it (without a ready flag) would drop the request.
      stateRef.current.openTab(
        req.folderId,
        req.conversationId,
        req.agent,
        true
      )
    })()
  }, [])

  // Replay a queued request once hydration flips ready.
  useEffect(() => {
    attempt()
  }, [foldersHydrated, tabsHydrated, attempt])

  useEffect(() => {
    let dispose: (() => void) | null = null
    let cancelled = false

    void (async () => {
      try {
        const { getTransport } = await import("@/lib/transport")
        const off = await getTransport().subscribe<{
          folderId?: number
          conversationId?: number
          agent?: string
        }>("workspace://focus-conversation", (payload) => {
          const folderId = Number(payload?.folderId)
          const conversationId = Number(payload?.conversationId)
          const agent = payload?.agent as AgentType | undefined
          if (
            !Number.isFinite(folderId) ||
            !Number.isFinite(conversationId) ||
            !agent
          ) {
            return
          }
          pendingRef.current = { folderId, conversationId, agent }
          attempt()
        })
        if (cancelled) off()
        else dispose = off
      } catch (err) {
        console.warn("[PetFocusBridge] subscription failed:", err)
      }
    })()

    return () => {
      cancelled = true
      if (dispose) dispose()
    }
  }, [attempt])

  return null
}
