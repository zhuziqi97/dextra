import { describe, expect, it } from "vitest"

import {
  buildEmbeddedReferenceUri,
  isEmbeddedReferenceUri,
  parseDextraReferenceUri,
} from "./reference-uri"

describe("parseDextraReferenceUri", () => {
  it("returns null for non-reference schemes", () => {
    expect(parseDextraReferenceUri("https://example.com", "x")).toBeNull()
    expect(parseDextraReferenceUri("data:text/plain,abc", "x")).toBeNull()
    expect(parseDextraReferenceUri("dextra://unknown/1", "x")).toBeNull()
  })

  it("parses a file uri, falling back to the basename when label is empty", () => {
    expect(
      parseDextraReferenceUri("file:///repo/deep/name.ts", "")
    ).toMatchObject({
      refType: "file",
      id: "name.ts",
      label: "name.ts",
      uri: "file:///repo/deep/name.ts",
      meta: { fileKind: "file" },
    })
  })

  it("parses an agent uri, stripping a leading @ from the label", () => {
    expect(
      parseDextraReferenceUri("dextra://agent/codex", "@Codex")
    ).toMatchObject({
      refType: "agent",
      id: "codex",
      label: "Codex",
      uri: "dextra://agent/codex",
      meta: { agentType: "codex" },
    })
  })

  it("falls back to the agent type when the agent label is empty", () => {
    expect(
      parseDextraReferenceUri("dextra://agent/claude_code", "")
    ).toMatchObject({
      refType: "agent",
      id: "claude_code",
      label: "claude_code",
      meta: { agentType: "claude_code" },
    })
  })

  it("parses a new-format session uri, recovering the agent type", () => {
    expect(
      parseDextraReferenceUri("dextra://session/codex_abc123", "My chat")
    ).toMatchObject({
      refType: "session",
      id: "codex_abc123",
      label: "My chat",
      uri: "dextra://session/codex_abc123",
      meta: { agentType: "codex" },
    })
  })

  it("never splits an agent type on its first underscore", () => {
    // claude_code / open_code / open_claw contain underscores; a naive first-`_`
    // split would yield "claude" / "open". The whole `<type>_<external_id>` is
    // the id and the full type is recovered by prefix match.
    expect(
      parseDextraReferenceUri("dextra://session/claude_code_sess-9", "")
    ).toMatchObject({
      id: "claude_code_sess-9",
      meta: { agentType: "claude_code" },
    })
    expect(
      parseDextraReferenceUri("dextra://session/open_code_x", "")?.meta
    ).toEqual({ agentType: "open_code" })
    expect(
      parseDextraReferenceUri("dextra://session/open_claw_y", "")?.meta
    ).toEqual({ agentType: "open_claw" })
  })

  it("treats a legacy numeric session id as opaque (no agent icon)", () => {
    expect(
      parseDextraReferenceUri("dextra://session/123", "Login")
    ).toMatchObject({
      refType: "session",
      id: "123",
      label: "Login",
      uri: "dextra://session/123",
      meta: null,
    })
  })

  it("treats a non-agent-prefixed token as a plain session id", () => {
    expect(
      parseDextraReferenceUri("dextra://session/randomtoken", "")
    ).toMatchObject({ refType: "session", id: "randomtoken", meta: null })
  })

  it("falls back to #id for an empty session label", () => {
    expect(parseDextraReferenceUri("dextra://session/123", "")?.label).toBe(
      "#123"
    )
  })

  it("parses a commit uri, deriving the short hash", () => {
    expect(
      parseDextraReferenceUri(
        "dextra://commit/%2Frepo@abc1234def5678",
        "abc1234"
      )
    ).toMatchObject({
      refType: "commit",
      id: "abc1234def5678",
      label: "abc1234",
      uri: "dextra://commit/%2Frepo@abc1234def5678",
      meta: { shortHash: "abc1234" },
    })
  })

  it("parses a skill uri, moving the label's leading `/`·`$` into the prefix", () => {
    expect(
      parseDextraReferenceUri("dextra://skill/review", "/review")
    ).toMatchObject({
      refType: "skill",
      id: "review",
      label: "review",
      uri: "dextra://skill/review",
      // The stripped trigger is kept so re-serializing the badge emits the
      // same `/review` token it was parsed from.
      meta: { invocationPrefix: "/" },
    })
    // The `$` prefix ($skill / Codex expert) is stripped — and kept — the same
    // way, so a `$deploy` token never re-serializes to `/deploy`.
    expect(
      parseDextraReferenceUri("dextra://skill/deploy", "$deploy")
    ).toMatchObject({
      label: "deploy",
      meta: { invocationPrefix: "$" },
    })
  })

  it("falls back to the bare id for an empty skill label", () => {
    expect(parseDextraReferenceUri("dextra://skill/deploy", "")).toMatchObject({
      label: "deploy",
      // No literal token to read a trigger from → serializer's `/` default.
      meta: null,
    })
  })

  it("parses an embedded-attachment uri as an inert file badge", () => {
    expect(
      parseDextraReferenceUri("dextra://embedded/9f3c-uuid", "report.pdf")
    ).toMatchObject({
      refType: "file",
      label: "report.pdf",
      uri: "dextra://embedded/9f3c-uuid",
      meta: { fileKind: "file" },
    })
  })

  it("falls back to a generic label for an empty embedded-attachment label", () => {
    expect(
      parseDextraReferenceUri("dextra://embedded/9f3c-uuid", "")?.label
    ).toBe("resource")
  })

  it("recognizes a freshly minted embedded reference uri", () => {
    const uri = buildEmbeddedReferenceUri()
    expect(isEmbeddedReferenceUri(uri)).toBe(true)
    expect(isEmbeddedReferenceUri("file:///dextra-embedded/real.ts")).toBe(
      false
    )
    expect(isEmbeddedReferenceUri("dextra://session/abc")).toBe(false)
  })
})
