import type { ClientConfiguration } from "@/lib/generated/cerebro/ClientConfiguration"

export function clientProjectFixture(): ClientConfiguration {
  return {
    runner_id: "device",
    runner_name: "Dextra",
    client_type: "dextra",
    client_project_id: "project-a",
    project: {
      client_project_id: "project-a",
      runner_id: "device",
      native_project_kind: "dextra_folder",
      native_project_id: "1",
      logical_project_id: null,
      display_name: "Root folder",
      execution_host_id: "local",
      availability: "AVAILABLE",
      unavailable_reason: null,
      updated_at: "2026-10-08T00:00:00Z",
    },
    targets: [],
    bindings: [],
    binding_module_details: [],
    mcp_scope_details: [],
    mcp_scope_modules: [],
    mcp_enabled: false,
    mcp_capabilities: {
      code_graph_enabled: false,
      forge_enabled: false,
      issue_enabled: false,
    },
  }
}
