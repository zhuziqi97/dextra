import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import prettier from "prettier"



const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
const outputPath = path.resolve(
  scriptDirectory,
  "../src-tauri/tests/contracts/cerebro-runner.openapi.json"
)
const checkOnly = process.argv.includes("--check")
const sourceArgument = process.argv
  .slice(2)
  .find((argument) => argument !== "--check")

if (!sourceArgument) {
  throw new Error(
    "用法：node scripts/update-cerebro-runner-contract.mjs <Cerebro public-api.openapi.json> [--check]"
  )
}

const sourcePath = path.resolve(process.cwd(), sourceArgument)
const source = JSON.parse(fs.readFileSync(sourcePath, "utf8"))
const schemas = source.components?.schemas
if (!schemas || typeof schemas !== "object") {
  throw new Error("Cerebro OpenAPI 缺少 components.schemas")
}

const selectedSchemas = new Map()

function visit(value) {
  if (Array.isArray(value)) {
    value.forEach(visit)
    return
  }
  if (!value || typeof value !== "object") return

  if (typeof value.$ref === "string") {
    const prefix = "#/components/schemas/"
    if (!value.$ref.startsWith(prefix)) {
      throw new Error(`Runner 合同包含不受支持的引用：${value.$ref}`)
    }
    const name = value.$ref.slice(prefix.length)
    if (!selectedSchemas.has(name)) {
      const schema = schemas[name]
      if (!schema) throw new Error(`Runner 合同引用了缺失 schema：${name}`)
      selectedSchemas.set(name, schema)
      visit(schema)
    }
  }

  Object.values(value).forEach(visit)
}

const runnerPaths = Object.keys(source.paths ?? {}).filter((route) => route.startsWith("/api/v1/execution-runner"))
const selectedPaths = {}
for (const runnerPath of runnerPaths) {
  const pathItem = source.paths?.[runnerPath]
  if (!pathItem)
    throw new Error(`Cerebro OpenAPI 缺少 Runner endpoint：${runnerPath}`)
  selectedPaths[runnerPath] = pathItem
  visit(pathItem)
}

function resolveSchema(schema) {
  if (schema?.$ref) {
    const name = schema.$ref.slice("#/components/schemas/".length)
    return schemas[name]
  }
  return schema
}

function exampleFor(schema, stack = new Set()) {
  if (schema?.$ref) {
    const name = schema.$ref.slice("#/components/schemas/".length)
    if (stack.has(name)) throw new Error(`Runner 合同 schema 循环引用：${name}`)
    return exampleFor(schemas[name], new Set([...stack, name]))
  }
  if (Object.hasOwn(schema ?? {}, "const")) return schema.const
  if (Array.isArray(schema?.enum) && schema.enum.length > 0)
    return schema.enum[0]
  if (Array.isArray(schema?.anyOf)) {
    const branch =
      schema.anyOf.find((item) => item.type !== "null") ?? schema.anyOf[0]
    return exampleFor(branch, stack)
  }
  if (Array.isArray(schema?.oneOf)) return exampleFor(schema.oneOf[0], stack)

  if (schema?.type === "object" || schema?.properties) {
    const required = new Set(schema.required ?? [])
    return Object.fromEntries(
      Object.entries(schema.properties ?? {})
        .filter(([name]) => required.has(name))
        .map(([name, property]) => [name, exampleFor(property, stack)])
    )
  }
  if (schema?.type === "array") return [exampleFor(schema.items, stack)]
  if (schema?.type === "boolean") return false
  if (schema?.type === "integer" || schema?.type === "number") {
    return schema.default ?? schema.minimum ?? 1
  }
  if (schema?.format === "uuid") return "00000000-0000-4000-8000-000000000000"
  if (schema?.format === "date-time") return "2026-01-01T00:00:00Z"
  return "string"
}

const examples = {}
for (const runnerPath of runnerPaths) {
  const operation = selectedPaths[runnerPath].post
  const requestSchema =
    operation?.requestBody?.content?.["application/json"]?.schema
  const responseSchema =
    operation?.responses?.["200"]?.content?.["application/json"]?.schema
  if (!requestSchema || !responseSchema) {
    throw new Error(
      `Runner endpoint 缺少 JSON request/200 response：${runnerPath}`
    )
  }
  examples[runnerPath] = {
    request: exampleFor(resolveSchema(requestSchema)),
    response: exampleFor(resolveSchema(responseSchema)),
  }
}

const fixture = {
  openapi: source.openapi,
  info: source.info,
  paths: selectedPaths,
  components: {
    schemas: Object.fromEntries(
      [...selectedSchemas.entries()].sort(([left], [right]) =>
        left.localeCompare(right)
      )
    ),
  },
  "x-dextra-contract-examples": examples,
}
const rendered = await prettier.format(JSON.stringify(fixture), {
  parser: "json",
})

if (checkOnly) {
  const current = fs.existsSync(outputPath)
    ? fs.readFileSync(outputPath, "utf8")
    : ""
  if (current !== rendered) {
    throw new Error(
      `跨仓 Runner 合同 fixture 不是 ${sourcePath} 的当前生成结果`
    )
  }
} else {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true })
  fs.writeFileSync(outputPath, rendered)
}
