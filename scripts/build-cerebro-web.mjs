import { spawn } from "node:child_process"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const nextBin = join(projectRoot, "node_modules", "next", "dist", "bin", "next")
const child = spawn(process.execPath, [nextBin, "build"], {
  cwd: projectRoot,
  env: {
    ...process.env,
    DEXTRA_BASE_PATH: "/runner-workbench",
  },
  stdio: "inherit",
})

child.on("error", (error) => {
  console.error(error)
  process.exitCode = 1
})

child.on("exit", (code, signal) => {
  if (signal) {
    console.error(`Next build 被信号 ${signal} 中止`)
    process.exitCode = 1
    return
  }
  process.exitCode = code ?? 1
})
