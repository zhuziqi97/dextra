//! Cerebro MCP 的单次 Agent 启动配置。

use sacp::schema::{HttpHeader, McpServer, McpServerHttp};

use crate::app_error::AppCommandError;

use super::identity;

const CEREBRO_MCP_SERVER_NAME: &str = "cerebro";

pub async fn server_for_binding(
    binding_id: &str,
    session_id: &str,
) -> Result<McpServer, AppCommandError> {
    let principal = identity::issue_mcp_session_principal(binding_id, session_id).await?;
    let server = McpServerHttp::new(CEREBRO_MCP_SERVER_NAME, principal.mcp_url).headers(vec![
        HttpHeader::new(
            "Authorization",
            format!("Bearer {}", principal.access_token),
        ),
    ]);
    Ok(McpServer::Http(server))
}

pub async fn server_for_task(task_id: &str) -> Result<McpServer, AppCommandError> {
    let principal = identity::issue_mcp_task_principal(task_id).await?;
    let server = McpServerHttp::new(CEREBRO_MCP_SERVER_NAME, principal.mcp_url).headers(vec![
        HttpHeader::new(
            "Authorization",
            format!("Bearer {}", principal.access_token),
        ),
    ]);
    Ok(McpServer::Http(server))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cerebro::CerebroMcpPrincipal;

    #[test]
    fn principal_decodes_cerebro_snake_case_response() {
        let principal: CerebroMcpPrincipal = serde_json::from_value(serde_json::json!({
            "mcp_url": "http://cerebro/mcp/stream",
            "access_token": "short-lived",
            "token_type": "bearer",
            "expires_in": 600
        }))
        .unwrap();

        assert_eq!(principal.mcp_url, "http://cerebro/mcp/stream");
        assert_eq!(principal.access_token, "short-lived");
    }

    #[test]
    fn principal_maps_to_streamable_http_header_shape() {
        let server = McpServerHttp::new(CEREBRO_MCP_SERVER_NAME, "http://cerebro/mcp/stream")
            .headers(vec![HttpHeader::new("Authorization", "Bearer short-lived")]);
        let serialized = serde_json::to_value(McpServer::Http(server)).unwrap();

        assert_eq!(serialized["type"], "http");
        assert_eq!(serialized["url"], "http://cerebro/mcp/stream");
        assert_eq!(serialized["headers"][0]["name"], "Authorization");
        assert_eq!(serialized["headers"][0]["value"], "Bearer short-lived");
    }
}
