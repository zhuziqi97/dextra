import { act, fireEvent, render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

vi.mock("@/lib/api", () => ({
  cancelCerebroPairing: vi.fn(),
  forgetCerebroRunner: vi.fn(),
  getCerebroAuthState: vi.fn(),
  getCerebroStorageSettings: vi.fn(),
  selectCerebroStorage: vi.fn(),
  importCerebroCredential: vi.fn(),
  pollCerebroPairing: vi.fn(),
  refreshCerebroAccessToken: vi.fn(),
  startCerebroPairing: vi.fn(),
}))

vi.mock("@/lib/platform", () => ({
  openUrl: vi.fn(async () => undefined),
}))

import { CerebroSettings } from "./cerebro-settings"
import enMessages from "@/i18n/messages/en.json"
import {
  getCerebroAuthState,
  getCerebroStorageSettings,
  selectCerebroStorage,
  pollCerebroPairing,
  refreshCerebroAccessToken,
  startCerebroPairing,
} from "@/lib/api"
import { openUrl } from "@/lib/platform"
import type { CerebroAuthState, CerebroPairingStart } from "@/lib/types"

const emptyState: CerebroAuthState = {
  paired: false,
  cerebroBaseUrl: null,
  runnerId: null,
  pairing: null,
}

const pairing: CerebroPairingStart = {
  handle: "pair-1",
  cerebroBaseUrl: "http://cerebro.internal/nested/?tenant=alpha",
  userCode: "ABCD-EFGH",
  verificationUri: "http://cerebro.internal/approve?code=ABCD-EFGH",
  expiresIn: 600,
  interval: 3,
}

const pairedState: CerebroAuthState = {
  paired: true,
  cerebroBaseUrl: pairing.cerebroBaseUrl,
  runnerId: "runner-42",
  pairing: null,
}

function renderSettings() {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <CerebroSettings />
    </NextIntlClientProvider>
  )
}

describe("CerebroSettings", () => {
  beforeEach(() => {
    vi.resetAllMocks()
    vi.useFakeTimers()
    vi.mocked(getCerebroStorageSettings).mockResolvedValue({
      mode: "FILE",
      keyringAvailable: true,
    })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it("allows local pairing after the system keychain is unavailable", async () => {
    vi.mocked(getCerebroStorageSettings).mockResolvedValue({
      mode: "KEYRING",
      keyringAvailable: true,
    })
    vi.mocked(getCerebroAuthState)
      .mockRejectedValueOnce(new Error("Keychain unavailable"))
      .mockResolvedValue(emptyState)
    vi.mocked(selectCerebroStorage).mockResolvedValue({
      mode: "FILE",
      keyringAvailable: true,
    })
    renderSettings()
    await act(async () => undefined)
    expect(screen.getByRole("alert")).toHaveTextContent("Keychain unavailable")
    fireEvent.click(screen.getByRole("combobox"))
    await act(async () => undefined)
    fireEvent.click(screen.getByRole("option", { name: "On this device" }))
    await act(async () => undefined)
    expect(selectCerebroStorage).toHaveBeenCalledWith("FILE")
    vi.mocked(startCerebroPairing).mockResolvedValue(pairing)
    fireEvent.change(screen.getByLabelText("Cerebro URL"), {
      target: { value: pairing.cerebroBaseUrl },
    })
    fireEvent.click(screen.getByRole("button", { name: "Connect to Cerebro" }))
    await act(async () => undefined)
    expect(startCerebroPairing).toHaveBeenCalledWith(pairing.cerebroBaseUrl)
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  })

  it("opens approval and follows the server polling interval until paired", async () => {
    vi.mocked(getCerebroAuthState)
      .mockResolvedValueOnce(emptyState)
      .mockResolvedValueOnce(pairedState)
    vi.mocked(startCerebroPairing).mockResolvedValue(pairing)
    vi.mocked(pollCerebroPairing)
      .mockResolvedValueOnce({
        status: "PENDING",
        runnerId: null,
        retryAfter: 2,
      })
      .mockResolvedValueOnce({
        status: "PAIRED",
        runnerId: "runner-42",
        retryAfter: null,
      })

    renderSettings()
    await act(async () => undefined)
    const urlInput = screen.getByLabelText("Cerebro URL")
    fireEvent.change(urlInput, {
      target: { value: "http://cerebro.internal/nested/?tenant=alpha" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Connect to Cerebro" }))

    await act(async () => undefined)
    expect(startCerebroPairing).toHaveBeenCalledWith(
      "http://cerebro.internal/nested/?tenant=alpha"
    )
    expect(openUrl).toHaveBeenCalledWith(pairing.verificationUri)
    expect(screen.getByText("ABCD-EFGH")).toBeInTheDocument()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_999)
    })
    expect(pollCerebroPairing).not.toHaveBeenCalled()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1)
    })
    expect(pollCerebroPairing).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_000)
    })
    expect(pollCerebroPairing).toHaveBeenCalledTimes(2)
    expect(screen.getByText("runner-42")).toBeInTheDocument()
    expect(screen.queryByText("refresh-secret")).not.toBeInTheDocument()
    expect(screen.queryByText("short-lived-access")).not.toBeInTheDocument()
  })

  it("resumes a pending pairing loaded from the backend", async () => {
    vi.mocked(getCerebroAuthState)
      .mockResolvedValueOnce({ ...emptyState, pairing })
      .mockResolvedValueOnce(pairedState)
    vi.mocked(pollCerebroPairing).mockResolvedValue({
      status: "PAIRED",
      runnerId: "runner-42",
      retryAfter: null,
    })

    renderSettings()
    await act(async () => undefined)
    expect(screen.getByText("ABCD-EFGH")).toBeInTheDocument()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3_000)
    })

    expect(pollCerebroPairing).toHaveBeenCalledWith("pair-1")
    expect(screen.getByText("runner-42")).toBeInTheDocument()
  })

  it("returns to the unpaired form after Cerebro rejects the stored credential", async () => {
    vi.mocked(getCerebroAuthState)
      .mockResolvedValueOnce(pairedState)
      .mockResolvedValueOnce(emptyState)
    vi.mocked(refreshCerebroAccessToken).mockRejectedValue({
      code: "authentication_failed",
      message: "Runner was revoked",
      detail: "RUNNER_REVOKED",
    })

    renderSettings()
    await act(async () => undefined)
    screen.getByText("runner-42")
    fireEvent.click(screen.getByRole("button", { name: "Verify connection" }))

    await act(async () => undefined)
    expect(screen.getByLabelText("Cerebro URL")).toBeInTheDocument()
    expect(screen.getByRole("alert")).toHaveTextContent("Runner was revoked")
  })
})
