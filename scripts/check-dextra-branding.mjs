#!/usr/bin/env node
// Run after each upstream merge: Codeg may remain only in the mobile wire
// contract or in explicit links to upstream sources/documentation.
import { execFileSync } from "node:child_process"
import { existsSync, readFileSync } from "node:fs"
import { resolve } from "node:path"

const root = resolve(import.meta.dirname, "..")
const activeFiles = new Set(
  execFileSync(
    "git",
    ["ls-files", "-z", "--cached", "--others", "--exclude-standard"],
    {
      cwd: root,
    }
  )
    .toString()
    .split("\0")
    .filter(Boolean)
)

const protocolNames = [
  "codeg-events",
  "codeg-token.",
  "/get_codeg_mcp_service_status",
  "/start_codeg_mcp_service",
  "/set_codeg_mcp_tool_group",
  "exists_in_codeg",
  "linked_to_codeg",
  "codeg.delegation",
  "codeg.codexScript",
  "codeg.codexSearchAction",
  "codeg.compactionSummary",
]
const upstreamLinks = [
  /https?:\/\/docs\.codeg\.app[^\s"'<>)]*/g,
  /https?:\/\/github\.com\/(?:xintaofei|xggz)\/codeg[^\s"'<>)]*/g,
]
const failures = []
const legacyBrand = /(?<![A-Za-z])(?:codeg|Codeg|CODEG)(?![a-z])/

function withoutProtocolAndAttribution(value) {
  let rest = value.replaceAll("[Codeg](https://github.com/xintaofei/codeg)", "")
  for (const name of protocolNames) rest = rest.replaceAll(name, "")
  for (const link of upstreamLinks) rest = rest.replace(link, "")
  for (const name of ["upstream Codeg", "Upstream Codeg", "docs.codeg.app"])
    rest = rest.replaceAll(name, "")
  return rest
}

function hasLegacyBrand(value) {
  return legacyBrand.test(withoutProtocolAndAttribution(value))
}

for (const path of activeFiles) {
  const active =
    path.startsWith("src/") ||
    path.startsWith("src-tauri/") ||
    path.startsWith("browser-agent/src/") ||
    path === "browser-agent/README.md" ||
    path === "browser-agent/probe.mjs" ||
    path === "README.md" ||
    path.startsWith("docs/readme/") ||
    path === "docs/url-scheme.md" ||
    path.startsWith("scripts/") ||
    path.startsWith(".github/") ||
    [
      "package.json",
      "Dockerfile",
      "Dockerfile.ci",
      "docker-compose.yml",
      "install.sh",
      "install.ps1",
    ].includes(path)
  if (!active || path === "scripts/check-dextra-branding.mjs") continue
  if (!existsSync(resolve(root, path))) continue
  if (/^src-tauri\/(?:experts|science|icons|binaries)\//.test(path)) continue
  if (path === "src-tauri/Cargo.lock") continue
  if (legacyBrand.test(path)) {
    failures.push(`${path}: legacy filename`)
  }
  const file = readFileSync(resolve(root, path))
  if (file.includes(0)) continue
  for (const [index, original] of file.toString().split(/\r?\n/).entries()) {
    if (hasLegacyBrand(original)) {
      failures.push(`${path}:${index + 1}: ${original.trim().slice(0, 160)}`)
    }
    if (/(?<!dextra-)cerebro-mcp-bridge/.test(original)) {
      failures.push(
        `${path}:${index + 1}: unprefixed bridge binary may collide with Codeg`
      )
    }
  }
}

const config = JSON.parse(
  readFileSync(resolve(root, "src-tauri/tauri.conf.json"))
)
if (config.productName !== "Dextra" || config.identifier !== "app.dextra") {
  failures.push("Tauri productName/identifier must be Dextra/app.dextra")
}
if (
  JSON.stringify(config.plugins?.["deep-link"]?.desktop?.schemes) !==
  '["dextra"]'
) {
  failures.push("Tauri deep-link scheme must be dextra")
}
if (
  !config.bundle.externalBin.includes("binaries/dextra-mcp") ||
  !config.bundle.externalBin.includes("binaries/dextra-cerebro-mcp-bridge")
) {
  failures.push("Tauri bundle must include both Dextra-owned sidecars")
}
const cargoManifest = readFileSync(
  resolve(root, "src-tauri/Cargo.toml"),
  "utf8"
)
const cargoLock = readFileSync(resolve(root, "src-tauri/Cargo.lock"), "utf8")
if (
  !cargoManifest.includes('name = "dextra"') ||
  /name = "codeg"/i.test(cargoLock)
) {
  failures.push("Cargo package/binary identity must be dextra")
}
const readme = readFileSync(resolve(root, "README.md"), "utf8")
if (
  !readme.startsWith("# Dextra\n") ||
  /xintaofei\/codeg\/(?:releases|main\/install)/.test(readme)
) {
  failures.push("README must identify Dextra and link to Dextra downloads")
}
const wsAuth = readFileSync(resolve(root, "src-tauri/src/web/auth.rs"), "utf8")
const wsClient = readFileSync(
  resolve(root, "src/lib/transport/ws-auth.ts"),
  "utf8"
)
for (const value of ["codeg-events", "codeg-token."]) {
  if (!wsAuth.includes(`"${value}"`) || !wsClient.includes(`"${value}"`)) {
    failures.push(`mobile WebSocket protocol ${value} changed`)
  }
}
const routes = readFileSync(
  resolve(root, "src-tauri/src/web/router.rs"),
  "utf8"
)
for (const route of protocolNames.filter((name) => name.startsWith("/"))) {
  if (!routes.includes(`"${route}"`))
    failures.push(`mobile API route ${route} changed`)
}

if (failures.length) {
  console.error(`Dextra brand/protocol check failed (${failures.length}):`)
  for (const failure of failures) console.error(`  ${failure}`)
  process.exitCode = 1
} else {
  console.log("Dextra branding and Codeg mobile protocol names are consistent")
}
