//! 真实 Bridge 进程经生产 IPC 帧转发协议，并随父连接撤销退出。

#[cfg(unix)]
#[tokio::test]
async fn stdio_bridge_forwards_requests_and_exits_with_parent_connection() {
    use axum::{routing::post, Json, Router};
    use codeg_lib::acp::delegation::transport::{
        read_frame, write_frame, BrokerMessage, BrokerResponse,
    };
    use serde_json::{json, Value};
    use std::{process::Stdio, sync::Arc, time::Duration};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp/stream", http.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(http, Router::new().route("/mcp/stream", post(|Json(message): Json<Value>| async move {
            Json(json!({"jsonrpc":"2.0", "id":message["id"], "result":{"echo":message["params"]}}))
        }))).await.unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("parent.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let revoked = Arc::new(tokio::sync::Notify::new());
    let watching = Arc::new(tokio::sync::Notify::new());
    let parent_revoked = revoked.clone();
    let parent_watching = watching.clone();
    let ipc = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let revoked = parent_revoked.clone();
            let watching = parent_watching.clone();
            let url = url.clone();
            tokio::spawn(async move {
                let message: BrokerMessage = read_frame(&mut socket).await.unwrap();
                let outcome = match message {
                    BrokerMessage::Credentials(request) => {
                        assert_eq!(request.token, "launch-token");
                        json!({"success":true,"data":{"mcp_url":url,"access_token":"short-lived","token_type":"bearer","expires_in":600}})
                    }
                    BrokerMessage::WatchToken(request) => {
                        assert_eq!(request.token, "launch-token");
                        watching.notify_one();
                        revoked.notified().await;
                        Value::Null
                    }
                    _ => panic!("Bridge 不应调用业务 IPC"),
                };
                write_frame(&mut socket, &BrokerResponse { outcome })
                    .await
                    .unwrap();
            });
        }
    });
    // 安装产物验收可以指定解包后的 binary；平时仍测试 Cargo 本次构建的产物。
    let binary = std::env::var_os("DEXTRA_BRIDGE_ACCEPTANCE_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_cerebro-mcp-bridge").into());
    let mut child = tokio::process::Command::new(binary)
        .args([
            "--socket-path",
            path.to_str().unwrap(),
            "--token",
            "launch-token",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
    tokio::time::timeout(Duration::from_secs(5), watching.notified())
        .await
        .unwrap();
    for (id, method) in ["initialize", "resources/read", "tools/call", "prompts/get"]
        .into_iter()
        .enumerate()
    {
        let params = json!({"sentinel":"本轮内容", "method":method});
        let line = format!(
            "{}\n",
            json!({"jsonrpc":"2.0", "id":id,"method":method,"params":params})
        );
        stdin.write_all(line.as_bytes()).await.unwrap();
        stdin.flush().await.unwrap();
        let received = tokio::time::timeout(Duration::from_secs(5), stdout.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let received: Value = serde_json::from_str(&received).unwrap();
        assert_eq!(received["result"]["echo"], params);
        assert_eq!(received["id"], id);
    }
    revoked.notify_one();
    assert!(tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
    ipc.abort();
    server.abort();
}
