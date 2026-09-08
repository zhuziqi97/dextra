"use client"

import { useCallback, useEffect, useState } from "react"
import {
  Check,
  Copy,
  ExternalLink,
  Link2,
  Loader2,
  RefreshCw,
  Unlink,
  X,
} from "lucide-react"
import { useTranslations } from "next-intl"

import { SettingCard, SettingRow } from "@/components/shared/setting-card"
import {
  SettingsError,
  SettingsSection,
} from "@/components/shared/settings-section"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  cancelCerebroPairing,
  forgetCerebroRunner,
  getCerebroAuthState,
  getCerebroStorageSettings,
  selectCerebroStorage,
  importCerebroCredential,
  pollCerebroPairing,
  refreshCerebroAccessToken,
  startCerebroPairing,
} from "@/lib/api"
import { extractAppCommandError, toErrorMessage } from "@/lib/app-error"
import { openUrl } from "@/lib/platform"
import type {
  CerebroAuthState,
  CerebroPairingStart,
  CerebroStorageSettings,
  CerebroStorageMode,
} from "@/lib/types"
import { copyTextToClipboard } from "@/lib/utils"
import { useCopiedFlag } from "@/hooks/use-copied-flag"

const EMPTY_STATE: CerebroAuthState = {
  paired: false,
  cerebroBaseUrl: null,
  runnerId: null,
  pairing: null,
}

function cerebroErrorMessage(cause: unknown): string {
  return extractAppCommandError(cause)?.message ?? toErrorMessage(cause)
}

export function CerebroSettings() {
  const t = useTranslations("CerebroSettings")
  const [storage, setStorage] = useState<CerebroStorageSettings | null>(null)
  const [storageBusy, setStorageBusy] = useState(false)
  const [authState, setAuthState] = useState<CerebroAuthState>(EMPTY_STATE)
  const [cerebroBaseUrl, setCerebroBaseUrl] = useState("")
  const [loading, setLoading] = useState(true)
  const [starting, setStarting] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const [disconnecting, setDisconnecting] = useState(false)
  const [verifying, setVerifying] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [verificationMessage, setVerificationMessage] = useState<string | null>(
    null
  )
  const [copied, markCopied] = useCopiedFlag()

  const loadAuthState = useCallback(async () => {
    const state = await getCerebroAuthState()
    setAuthState(state)
    if (state.cerebroBaseUrl) setCerebroBaseUrl(state.cerebroBaseUrl)
    return state
  }, [])

  useEffect(() => {
    let active = true
    setLoading(true)
    getCerebroStorageSettings()
      .then(async (settings) => {
        if (!active) return
        setStorage(settings)
        setLoading(false)
        await loadAuthState()
      })
      .catch((cause) => {
        if (active) setError(cerebroErrorMessage(cause))
      })
      .finally(() => {
        if (active) setLoading(false)
      })
    return () => {
      active = false
    }
  }, [loadAuthState])

  useEffect(() => {
    const pairing = authState.pairing
    if (!pairing) return

    let active = true
    let timer: ReturnType<typeof setTimeout> | null = null

    const schedule = (delaySeconds: number) => {
      timer = setTimeout(
        async () => {
          try {
            const result = await pollCerebroPairing(pairing.handle)
            if (!active) return
            if (result.status === "PAIRED") {
              setError(null)
              await loadAuthState()
              return
            }
            schedule(result.retryAfter ?? pairing.interval)
          } catch (cause) {
            if (!active) return
            setError(cerebroErrorMessage(cause))
            try {
              const state = await loadAuthState()
              if (state.paired) setError(null)
            } catch {
              setAuthState((current) => ({ ...current, pairing: null }))
            }
          }
        },
        Math.max(1, delaySeconds) * 1000
      )
    }

    schedule(pairing.interval)
    return () => {
      active = false
      if (timer) clearTimeout(timer)
    }
  }, [authState.pairing, loadAuthState])

  async function handleStartPairing() {
    setStarting(true)
    setError(null)
    setVerificationMessage(null)
    try {
      const pairing = await startCerebroPairing(cerebroBaseUrl)
      setAuthState({
        paired: false,
        cerebroBaseUrl: pairing.cerebroBaseUrl,
        runnerId: null,
        pairing,
      })
      setCerebroBaseUrl(pairing.cerebroBaseUrl)
      await openUrl(pairing.verificationUri)
    } catch (cause) {
      setError(cerebroErrorMessage(cause))
    } finally {
      setStarting(false)
    }
  }

  async function handleCancel(pairing: CerebroPairingStart) {
    setCancelling(true)
    setError(null)
    try {
      setAuthState(await cancelCerebroPairing(pairing.handle))
    } catch (cause) {
      setError(cerebroErrorMessage(cause))
    } finally {
      setCancelling(false)
    }
  }

  async function handleVerify() {
    setVerifying(true)
    setError(null)
    setVerificationMessage(null)
    try {
      await refreshCerebroAccessToken()
      setVerificationMessage(t("verificationSucceeded"))
    } catch (cause) {
      setError(cerebroErrorMessage(cause))
      try {
        await loadAuthState()
      } catch {
        // 保留原始刷新错误，下一次进入页面时会重新读取状态。
      }
    } finally {
      setVerifying(false)
    }
  }

  async function handleDisconnect() {
    setDisconnecting(true)
    setError(null)
    setVerificationMessage(null)
    try {
      setAuthState(await forgetCerebroRunner())
    } catch (cause) {
      setError(cerebroErrorMessage(cause))
    } finally {
      setDisconnecting(false)
    }
  }

  async function handleStorageChange(mode: CerebroStorageMode) {
    setStorageBusy(true)
    setError(null)
    try {
      setStorage(await selectCerebroStorage(mode))
      setAuthState(EMPTY_STATE)
      setStorageBusy(false)
      await loadAuthState()
    } catch (cause) {
      setError(cerebroErrorMessage(cause))
    } finally {
      setStorageBusy(false)
    }
  }

  async function handleImportCredential() {
    setStorageBusy(true)
    setError(null)
    try {
      await importCerebroCredential()
      await loadAuthState()
    } catch (cause) {
      setError(cerebroErrorMessage(cause))
    } finally {
      setStorageBusy(false)
    }
  }

  async function handleCopyCode(code: string) {
    if (await copyTextToClipboard(code)) markCopied()
  }

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="size-5 animate-spin text-muted-foreground" />
      </div>
    )
  }

  const pairing = authState.pairing

  return (
    <ScrollArea className="h-full">
      <div className="mx-auto max-w-3xl space-y-4 p-4 pb-10">
        <SettingsSection
          icon={Link2}
          title={t("title")}
          description={t("description")}
        >
          {error ? <SettingsError>{error}</SettingsError> : null}

          {storage ? (
            <SettingCard>
              <SettingRow title={t("credentialStorage")}>
                <Select
                  value={storage.mode}
                  onValueChange={(mode) =>
                    void handleStorageChange(mode as CerebroStorageMode)
                  }
                  disabled={storageBusy}
                >
                  <SelectTrigger className="w-48">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="FILE">{t("storageFile")}</SelectItem>
                    <SelectItem
                      value="KEYRING"
                      disabled={!storage.keyringAvailable}
                    >
                      {t("storageKeyring")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </SettingRow>
              {!authState.paired ? (
                <SettingRow title={t("importCredential")}>
                  <Button
                    variant="outline"
                    onClick={() => void handleImportCredential()}
                    disabled={storageBusy}
                  >
                    {t("importCredential")}
                  </Button>
                </SettingRow>
              ) : null}
            </SettingCard>
          ) : null}

          {authState.paired ? (
            <SettingCard>
              <SettingRow title={t("cerebroUrl")}>
                <code className="text-xs break-all select-all">
                  {authState.cerebroBaseUrl}
                </code>
              </SettingRow>
              <SettingRow title={t("runnerId")}>
                <code className="text-xs break-all select-all">
                  {authState.runnerId}
                </code>
              </SettingRow>
              <SettingRow
                title={t("connection")}
                description={
                  verificationMessage ?? t("credentialStoredLocally")
                }
                control={
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    disabled={verifying}
                    onClick={handleVerify}
                  >
                    {verifying ? (
                      <Loader2 className="size-3.5 animate-spin" />
                    ) : (
                      <RefreshCw className="size-3.5" />
                    )}
                    {t("verify")}
                  </Button>
                }
              />
              <SettingRow
                title={t("disconnectLocal")}
                description={t("disconnectHint")}
                control={
                  <Button
                    type="button"
                    size="sm"
                    variant="destructive"
                    disabled={disconnecting}
                    onClick={handleDisconnect}
                  >
                    {disconnecting ? (
                      <Loader2 className="size-3.5 animate-spin" />
                    ) : (
                      <Unlink className="size-3.5" />
                    )}
                    {t("disconnect")}
                  </Button>
                }
              />
            </SettingCard>
          ) : pairing ? (
            <SettingCard>
              <SettingRow
                title={t("userCode")}
                description={t("approvalHint")}
                control={
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => handleCopyCode(pairing.userCode)}
                  >
                    {copied ? (
                      <Check className="size-3.5" />
                    ) : (
                      <Copy className="size-3.5" />
                    )}
                    {copied ? t("copied") : t("copy")}
                  </Button>
                }
              >
                <code className="text-lg font-semibold tracking-wider select-all">
                  {pairing.userCode}
                </code>
              </SettingRow>
              <SettingRow
                title={t("approvalPage")}
                description={pairing.verificationUri}
                control={
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => openUrl(pairing.verificationUri)}
                  >
                    <ExternalLink className="size-3.5" />
                    {t("openApprovalPage")}
                  </Button>
                }
              />
              <SettingRow
                title={t("waitingForApproval")}
                description={t("pollingHint")}
                control={<Loader2 className="size-4 animate-spin" />}
              />
              <SettingRow
                title={t("cancelPairing")}
                control={
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    disabled={cancelling}
                    onClick={() => handleCancel(pairing)}
                  >
                    {cancelling ? (
                      <Loader2 className="size-3.5 animate-spin" />
                    ) : (
                      <X className="size-3.5" />
                    )}
                    {t("cancel")}
                  </Button>
                }
              />
            </SettingCard>
          ) : (
            <SettingCard>
              <SettingRow
                title={t("cerebroUrl")}
                description={t("cerebroUrlHint")}
                htmlFor="cerebro-base-url"
              >
                <Input
                  id="cerebro-base-url"
                  value={cerebroBaseUrl}
                  onChange={(event) => setCerebroBaseUrl(event.target.value)}
                  placeholder={t("cerebroUrlPlaceholder")}
                />
              </SettingRow>
              <SettingRow
                title={t("connect")}
                description={t("connectHint")}
                control={
                  <Button
                    type="button"
                    size="sm"
                    disabled={starting || !cerebroBaseUrl.trim()}
                    onClick={handleStartPairing}
                  >
                    {starting ? (
                      <Loader2 className="size-3.5 animate-spin" />
                    ) : (
                      <Link2 className="size-3.5" />
                    )}
                    {t("connect")}
                  </Button>
                }
              />
            </SettingCard>
          )}
        </SettingsSection>
      </div>
    </ScrollArea>
  )
}
