//! Real dedicated sockets + the native loopback router, without flow-control mocks.
use dextra_lib::{
    app_state::AppState, cerebro::web_relay::ClientWebRelay, db::test_helpers::fresh_in_memory_db,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};
type Peer = WebSocketStream<tokio::net::TcpStream>;
async fn connect(relay: ClientWebRelay) -> (Peer, tokio::task::JoinHandle<Result<(), String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
            .await
            .unwrap();
        relay.serve_channel(socket).await
    });
    let (socket, _) = listener.accept().await.unwrap();
    (tokio_tungstenite::accept_async(socket).await.unwrap(), task)
}
async fn next(peer: &mut Peer) -> Message {
    tokio::time::timeout(Duration::from_secs(10), peer.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}
async fn http(peer: &mut Peer, path: &str) {
    peer.send(Message::Text(
        json!({"TYPE":"HTTP_OPEN","METHOD": if path == "/api/health" {"POST"} else {"GET"},"PATH":path,"HEADERS":[]})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    peer.send(Message::Text(json!({"TYPE":"HTTP_END"}).to_string().into()))
        .await
        .unwrap();
    let header: Value = serde_json::from_str(next(peer).await.to_text().unwrap()).unwrap();
    assert_eq!(header["STATUS"], 200);
}
async fn body(peer: &mut Peer) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        match next(peer).await {
            Message::Binary(chunk) => bytes.extend(chunk),
            Message::Text(text) => {
                assert_eq!(
                    serde_json::from_str::<Value>(&text).unwrap()["TYPE"],
                    "HTTP_DONE"
                );
                peer.close(None).await.unwrap();
                return bytes;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
#[tokio::test]
async fn paused_download_is_independent_and_resumes_byte_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let bytes: Vec<u8> = (0..32 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.path().join("large.bin"), &bytes).unwrap();
    let state = Arc::new(AppState::new_for_test(
        fresh_in_memory_db().await,
        data.path().into(),
    ));
    let relay = ClientWebRelay::new(state, dir.path().into());
    let (mut slow, slow_task) = connect(relay.clone()).await;
    http(&mut slow, "/large.bin").await;
    let (mut health, health_task) = connect(relay.clone()).await;
    http(&mut health, "/api/health").await;
    assert!(serde_json::from_slice::<Value>(&body(&mut health).await)
        .unwrap()
        .get("version")
        .is_some());
    health_task.await.unwrap().unwrap();
    assert_eq!(body(&mut slow).await, bytes);
    slow_task.await.unwrap().unwrap();
}
#[tokio::test]
async fn native_websocket_preserves_large_unicode_and_cancel_releases_task() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState::new_for_test(
        fresh_in_memory_db().await,
        data.path().into(),
    ));
    let relay = ClientWebRelay::new(state.clone(), dir.path().into());
    let (mut peer, task) = connect(relay).await;
    peer.send(Message::Text(
        json!({"TYPE":"WS_OPEN","PATH":"/ws/events","PROTOCOL":"codeg-events"})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(next(&mut peer).await.to_text().unwrap()).unwrap()["TYPE"],
        "WS_ACCEPT"
    );
    let _ready = next(&mut peer).await;
    let payload = "终端🙂".repeat(150000);
    state.event_broadcaster.send("large-message", &payload);
    assert_eq!(
        serde_json::from_str::<Value>(next(&mut peer).await.to_text().unwrap()).unwrap()["payload"],
        payload
    );
    peer.send(Message::Text(
        json!({"action":"ping","padding":payload})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(next(&mut peer).await.to_text().unwrap()).unwrap()["type"],
        "pong"
    );
    peer.close(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn early_native_response_does_not_wait_for_upload_end() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState::new_for_test(
        fresh_in_memory_db().await,
        data.path().into(),
    ));
    let relay = ClientWebRelay::new(state, dir.path().into());
    let (mut peer, task) = connect(relay).await;
    // Health is POST-only: GET returns 405 without consuming an unfinished body.
    peer.send(Message::Text(
        json!({"TYPE":"HTTP_OPEN","METHOD":"GET","PATH":"/api/health","HEADERS":[]})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    peer.send(Message::Binary(vec![42; 49152].into()))
        .await
        .unwrap();
    let header: Value = serde_json::from_str(next(&mut peer).await.to_text().unwrap()).unwrap();
    assert_eq!(header["STATUS"], 405);
    let _ = body(&mut peer).await;
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn concurrent_channels_keep_data_and_cancellation_independent() {
    use dextra_lib::cerebro::{web_relay::DataAccess, CerebroRunnerAccess};
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState::new_for_test(
        fresh_in_memory_db().await,
        data.path().into(),
    ));
    let relay = ClientWebRelay::new(state, dir.path().into());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let access = Arc::new(tokio::sync::Mutex::new(DataAccess::new(
        CerebroRunnerAccess {
            cerebro_base_url: base.clone(),
            runner_id: "test-runner".into(),
            access_token: "test-token".into(),
            token_type: "Bearer".into(),
            expires_in: 600,
        },
    )));
    let mut peers = Vec::new();
    for id in 0..70 {
        assert!(relay.handle(
            &json!({"TYPE":"OPEN_DATA_CHANNEL", "CONNECTION_ID":"owner", "REQUEST_ID":id.to_string()}),
            &base, "owner", access.clone(),
        ).await.unwrap());
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut peer = tokio_tungstenite::accept_async(stream).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(next(&mut peer).await.to_text().unwrap()).unwrap()
                ["TYPE"],
            "AUTHENTICATE"
        );
        assert_eq!(
            serde_json::from_str::<Value>(next(&mut peer).await.to_text().unwrap()).unwrap()
                ["TYPE"],
            "BIND_DATA_CHANNEL"
        );
        peers.push(peer);
    }
    http(&mut peers[69], "/api/health").await;
    assert!(serde_json::from_slice::<Value>(&body(&mut peers[69]).await)
        .unwrap()
        .get("version")
        .is_some());
    assert!(relay
        .handle(
            &json!({"TYPE":"CANCEL_DATA_CHANNEL", "CONNECTION_ID":"owner", "REQUEST_ID":"1"}),
            &base,
            "owner",
            access,
        )
        .await
        .unwrap());
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), peers[1].next())
            .await
            .unwrap(),
        None | Some(Err(_)) | Some(Ok(Message::Close(_)))
    ));
    relay.close_all().await;
}
