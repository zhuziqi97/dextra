import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import prettier from "prettier"

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
const outputPath = path.resolve(
  scriptDirectory,
  "../src-tauri/tests/contracts/cerebro-runner-stream.schema.json"
)
const checkOnly = process.argv.includes("--check")
const sourceArgument = process.argv
  .slice(2)
  .find((argument) => argument !== "--check")

if (!sourceArgument) {
  throw new Error(
    "用法：node scripts/update-cerebro-runner-stream-contract.mjs <Cerebro runner-stream.schema.json> [--check]"
  )
}

const sourcePath = path.resolve(process.cwd(), sourceArgument)
const contract = JSON.parse(fs.readFileSync(sourcePath, "utf8"))
if (contract.schema_version !== 1 || !contract.messages || !contract.examples) {
  throw new Error("Cerebro Runner stream 合同缺少生产 schema 或样例")
}
const rendered = await prettier.format(JSON.stringify(contract), {
  parser: "json",
})

if (checkOnly) {
  const current = fs.existsSync(outputPath)
    ? fs.readFileSync(outputPath, "utf8")
    : ""
  if (current !== rendered) {
    throw new Error(
      `跨仓 Runner stream fixture 不是 ${sourcePath} 的当前生成结果`
    )
  }
} else {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true })
  fs.writeFileSync(outputPath, rendered)
}
