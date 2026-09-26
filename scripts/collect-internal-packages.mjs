#!/usr/bin/env node

import { execFileSync } from "node:child_process"
import {
  copyFileSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs"
import { basename, join } from "node:path"
import { fileURLToPath } from "node:url"

const root = fileURLToPath(new URL("..", import.meta.url))
const [platform, target] = process.argv.slice(2)
const formats = {
  "linux-x64": ["deb", "appimage"],
  "linux-arm64": ["deb"],
  "macos-arm64": ["dmg"],
  "windows-x64": ["nsis"],
}
const extensions = {
  deb: ".deb",
  appimage: ".AppImage",
  dmg: ".dmg",
  nsis: ".exe",
}

if (!formats[platform] || !target) {
  throw new Error(`Usage: ${basename(process.argv[1])} <platform> <target>`)
}

const version = JSON.parse(
  readFileSync(join(root, "package.json"), "utf8")
).version
const commit =
  process.env.GITHUB_SHA ||
  execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: root,
    encoding: "utf8",
  }).trim()
const dest = join(root, "dist", "internal", platform)
mkdirSync(dest, { recursive: true })

const packages = formats[platform].map((format) => {
  const dir = join(
    root,
    "src-tauri",
    "target",
    target,
    "release",
    "bundle",
    format
  )
  const names = readdirSync(dir).filter(
    (name) =>
      name.startsWith("Dextra") &&
      name.includes(`_${version}_`) &&
      name.endsWith(extensions[format])
  )
  const matches = names
  if (matches.length !== 1) {
    throw new Error(
      `${format}: expected one Dextra ${version} package, got ${matches}`
    )
  }
  const name = matches[0]
  const source = join(dir, name)
  if (statSync(source).size === 0) throw new Error(`${source} is empty`)
  copyFileSync(source, join(dest, name))
  return { format, name }
})

const metadata = {
  product: "Dextra",
  version,
  platform,
  target,
  source_commit: commit,
  github_run_id: process.env.GITHUB_RUN_ID || null,
  developer_id_signed: false,
  updater_artifacts: false,
  packages,
}
writeFileSync(
  join(dest, "build-info.json"),
  `${JSON.stringify(metadata, null, 2)}\n`
)
console.log(JSON.stringify(metadata, null, 2))
