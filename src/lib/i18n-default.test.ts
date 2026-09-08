import { describe, expect, it } from "vitest"
import {
  DEFAULT_LANGUAGE_SETTINGS,
  normalizeLanguageSettings,
  resolveAppLocale,
} from "./i18n"

describe("new client language", () => {
  it("starts in simplified Chinese even on an English system", () => {
    expect(resolveAppLocale(DEFAULT_LANGUAGE_SETTINGS, ["en-US"])).toBe("zh_cn")
    expect(resolveAppLocale(normalizeLanguageSettings(null), ["en-US"])).toBe(
      "zh_cn"
    )
  })
  it("keeps an existing manual choice and explicit follow-system choice", () => {
    expect(
      resolveAppLocale(
        normalizeLanguageSettings({ mode: "manual", language: "en" }),
        ["zh-CN"]
      )
    ).toBe("en")
    expect(
      resolveAppLocale(
        normalizeLanguageSettings({ mode: "system", language: "en" }),
        ["ja-JP"]
      )
    ).toBe("ja")
  })
})
