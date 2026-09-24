#!/usr/bin/env node
//
// Prepare Tauri sidecars before `tauri build` / `tauri dev` consume them.
//
// What it does:
//   1. Resolves the target triple — `--target <triple>` arg, or
//      `TAURI_TARGET_TRIPLE` env, or the host's `rustc -vV` host triple.
//   2. Runs `cargo build --release --no-default-features` for both sidecars
//      from `src-tauri/`. `--runner cargo-xwin` supports Windows cross builds.
//   3. Copies the produced binary to
//      `src-tauri/binaries/dextra-mcp-<triple>{.exe}` so Tauri's externalBin
//      bundler picks it up under the bare name `dextra-mcp` at install time.
//
// Why a separate script (not inline in beforeBuildCommand / GitHub Actions):
//   - Cross-compile in release.yml passes `--target <triple>` so we honour
//     the matrix triple rather than rebuilding for the host.
//   - Local `pnpm tauri dev` / `pnpm tauri build` invoke it without args and
//     get a host-triple build, so the externalBin lookup still finds a file.
//   - Skippable: set `DEXTRA_SKIP_SIDECAR=1` when iterating on the frontend
//     and you don't care about delegation.
//
// Intentionally Node-only (no shell): runs identically on macOS, Linux,
// Windows GitHub runners.

import { execFileSync } from "node:child_process"
import { existsSync, copyFileSync, mkdirSync, chmodSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import process from "node:process"

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url))
const SRC_TAURI = resolve(SCRIPT_DIR, "..")
const BINARIES_DIR = join(SRC_TAURI, "binaries")
const BIN_NAMES = ["dextra-mcp", "dextra-cerebro-mcp-bridge"]

function log(msg) {
  console.log(`[prepare-sidecars] ${msg}`)
}

function die(msg) {
  console.error(`[prepare-sidecars][ERROR] ${msg}`)
  process.exit(1)
}

function parseArgs(argv) {
  const args = { target: null, runner: "cargo" }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (a === "--target" && argv[i + 1]) {
      args.target = argv[++i]
    } else if (a.startsWith("--target=")) {
      args.target = a.slice("--target=".length)
    } else if (a === "--runner" && argv[i + 1]) {
      args.runner = argv[++i]
    } else if (a.startsWith("--runner=")) {
      args.runner = a.slice("--runner=".length)
    }
  }
  return args
}

function resolveHostTriple() {
  try {
    const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
    const line = out.split(/\r?\n/).find((l) => l.startsWith("host:"))
    if (!line) throw new Error("rustc -vV missing host: line")
    return line.replace(/^host:\s*/, "").trim()
  } catch (e) {
    die(`cannot determine host triple via rustc -vV: ${e.message}`)
  }
}

function main() {
  if (process.env.DEXTRA_SKIP_SIDECAR === "1") {
    log("DEXTRA_SKIP_SIDECAR=1 — skipping sidecar preparation")
    return
  }

  const { target: cliTarget, runner } = parseArgs(process.argv.slice(2))
  if (runner !== "cargo" && runner !== "cargo-xwin") {
    die(`unsupported cargo runner: ${runner}`)
  }
  const target =
    cliTarget || process.env.TAURI_TARGET_TRIPLE || resolveHostTriple()
  const isWindows = target.includes("windows")
  const ext = isWindows ? ".exe" : ""

  log(`target triple: ${target}`)
  log(
    `building ${BIN_NAMES.join(", ")} with ${runner} (--release --no-default-features)`
  )

  // cargo build needs to run from src-tauri so it resolves the local manifest
  // and shares the swatinem/rust-cache key with other cargo invocations.
  // `--no-default-features` keeps dextra-mcp free of the Tauri runtime deps —
  // the bin's required-features is empty, so this just enables cross-compile
  // without dragging in macOS-private-api / Linux WebKit / Windows WebView2.
  execFileSync(
    runner,
    [
      "build",
      "--release",
      ...BIN_NAMES.flatMap((name) => ["--bin", name]),
      "--no-default-features",
      "--target",
      target,
    ],
    { stdio: "inherit", cwd: SRC_TAURI }
  )

  // 尊重 Cargo 的 target_directory（包括本机共享构建缓存）。
  const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
      cwd: SRC_TAURI,
      encoding: "utf8",
    })
  )
  for (const binName of BIN_NAMES) {
    const built = join(
      metadata.target_directory,
      target,
      "release",
      `${binName}${ext}`
    )
    if (!existsSync(built)) {
      die(`expected ${built} after cargo build, but it does not exist`)
    }

    mkdirSync(BINARIES_DIR, { recursive: true })
    const dest = join(BINARIES_DIR, `${binName}-${target}${ext}`)
    copyFileSync(built, dest)
    if (!isWindows) {
      // copyFileSync preserves modes on POSIX, but be explicit for tarball
      // sources that may strip the +x bit.
      chmodSync(dest, 0o755)
    }
    log(`sidecar staged at ${dest}`)
  }
}

main()
