//! Real relay + loopback HTTP: a paused download cannot block another stream.
use base64::{engine::general_purpose::STANDARD, Engine};
use dextra_lib::{
    app_state::AppState,
    cerebro::{
        web_flow::{Flow, Outbound, WINDOW},
        web_relay::ClientWebRelay,
    },
    db::test_helpers::fresh_in_memory_db,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn websocket_fragments_preserve_unicode_message_and_allow_native_ping() {
    let data_dir = tempfile::tempdir().unwrap();
    let static_dir = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState::new_for_test(
        fresh_in_memory_db().await,
        data_dir.path().into(),
    ));
    let relay = ClientWebRelay::new(state.clone(), static_dir.path().into());
    let (out, writer) = Outbound::new();
    let (tx, mut rx) = mpsc::channel(32);
    let sink = Box::pin(futures_util::sink::unfold(tx, |tx, msg| async move {
        tx.send(msg).await?;
        Ok::<_, mpsc::error::SendError<Message>>(tx)
    }));
    let writer_task = tokio::spawn(writer.run(sink));
    command(
        &relay,
        &out,
        "WEB_WS_OPEN",
        "ws",
        json!({"PATH":"/ws/events","PROTOCOL":"codeg-events"}),
    )
    .await;
    assert_eq!(next(&mut rx).await["TYPE"], "WEB_WS_ACCEPT");
    assert_eq!(next(&mut rx).await["TYPE"], "WEB_WS_DATA");
    command(&relay, &out, "WEB_WINDOW_UPDATE", "ws", json!({"CREDIT":1})).await;
    let payload = "终端🙂".repeat(150000);
    state.event_broadcaster.send("flow-test", &payload);
    let mut message = Vec::new();
    loop {
        let value = next(&mut rx).await;
        assert_eq!(value["TYPE"], "WEB_WS_DATA");
        assert_eq!(value["PAYLOAD"]["BINARY"], false);
        message.extend(
            STANDARD
                .decode(value["PAYLOAD"]["DATA"].as_str().unwrap())
                .unwrap(),
        );
        command(&relay, &out, "WEB_WINDOW_UPDATE", "ws", json!({"CREDIT":1})).await;
        if value["PAYLOAD"]["END"] == true {
            break;
        }
    }
    let restored: Value = serde_json::from_slice(&message).unwrap();
    assert_eq!(restored["payload"], payload);
    let ping = serde_json::to_vec(&json!({"action":"ping", "padding":payload})).unwrap();
    for (index, bytes) in ping.chunks(49152).enumerate() {
        command(
            &relay,
            &out,
            "WEB_WS_DATA",
            "ws",
            json!({"BINARY":false,"END":(index+1)*49152>=ping.len(),"DATA":STANDARD.encode(bytes)}),
        )
        .await;
        assert_eq!(next(&mut rx).await["TYPE"], "WEB_WINDOW_UPDATE");
    }
    let pong = next(&mut rx).await;
    assert_eq!(pong["TYPE"], "WEB_WS_DATA");
    let pong: Value = serde_json::from_slice(
        &STANDARD
            .decode(pong["PAYLOAD"]["DATA"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(pong["type"], "pong");
    command(&relay, &out, "WEB_CANCEL", "ws", json!({})).await;
    relay.close_all().await;
    writer_task.abort();
    let _ = writer_task.await;
}

async fn next(rx: &mut mpsc::Receiver<Message>) -> Value {
    let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(msg.to_text().unwrap()).unwrap()
}
async fn command(relay: &ClientWebRelay, tx: &Outbound, kind: &str, id: &str, payload: Value) {
    assert!(tokio::time::timeout(
        Duration::from_secs(5),
        relay.handle(
            json!({"TYPE":kind,"STREAM_ID":id,"PAYLOAD":payload}),
            tx.clone()
        )
    )
    .await
    .unwrap());
}

#[tokio::test]
async fn slow_download_does_not_block_health_and_resumes_without_lost_bytes() {
    let data_dir = tempfile::tempdir().unwrap();
    let static_dir = tempfile::tempdir().unwrap();
    let bytes: Vec<u8> = (0..2 * 1024 * 1024).map(|n| (n % 251) as u8).collect();
    std::fs::write(static_dir.path().join("large.bin"), &bytes).unwrap();
    let state = Arc::new(AppState::new_for_test(
        fresh_in_memory_db().await,
        data_dir.path().into(),
    ));
    let relay = ClientWebRelay::new(state, static_dir.path().into());
    let (out, writer) = Outbound::new();
    let (tx, mut rx) = mpsc::channel(32);
    let sink = Box::pin(futures_util::sink::unfold(tx, |tx, msg| async move {
        tx.send(msg).await?;
        Ok::<_, mpsc::error::SendError<Message>>(tx)
    }));
    let writer_task = tokio::spawn(writer.run(sink));
    command(
        &relay,
        &out,
        "WEB_HTTP_OPEN",
        "slow",
        json!({"METHOD":"GET","PATH":"/large.bin","HEADERS":[]}),
    )
    .await;
    command(&relay, &out, "WEB_HTTP_END", "slow", json!({})).await;
    assert_eq!(next(&mut rx).await["TYPE"], "WEB_HTTP_RESPONSE");
    let mut received = Vec::new();
    for _ in 0..WINDOW {
        let value = next(&mut rx).await;
        assert_eq!(value["TYPE"], "WEB_HTTP_DATA");
        received.extend(
            STANDARD
                .decode(value["PAYLOAD"]["DATA"].as_str().unwrap())
                .unwrap(),
        );
    }
    // Keep the first consumer paused while a second real HTTP request completes.
    command(
        &relay,
        &out,
        "WEB_HTTP_OPEN",
        "health",
        json!({"METHOD":"POST","PATH":"/api/health","HEADERS":[]}),
    )
    .await;
    command(&relay, &out, "WEB_HTTP_END", "health", json!({})).await;
    loop {
        let value = next(&mut rx).await;
        assert_eq!(value["STREAM_ID"], "health");
        if value["TYPE"] == "WEB_HTTP_RESPONSE" {
            assert_eq!(value["PAYLOAD"]["STATUS"], 200);
        }
        if value["TYPE"] == "WEB_HTTP_DONE" {
            break;
        }
    }
    command(
        &relay,
        &out,
        "WEB_WINDOW_UPDATE",
        "slow",
        json!({"CREDIT":WINDOW}),
    )
    .await;
    loop {
        let value = next(&mut rx).await;
        match value["TYPE"].as_str().unwrap() {
            "WEB_HTTP_DONE" => break,
            "WEB_HTTP_DATA" => {
                received.extend(
                    STANDARD
                        .decode(value["PAYLOAD"]["DATA"].as_str().unwrap())
                        .unwrap(),
                );
                command(
                    &relay,
                    &out,
                    "WEB_WINDOW_UPDATE",
                    "slow",
                    json!({"CREDIT":1}),
                )
                .await;
            }
            other => panic!("unexpected {other}: {value}"),
        }
    }
    assert_eq!(received, bytes);
    relay.close_all().await;
    writer_task.abort();
    let _ = writer_task.await;
}

#[tokio::test]
async fn independent_credit_and_close_release_waiting_producer() {
    let slow = Flow::new();
    for _ in 0..WINDOW {
        slow.take().await.unwrap();
    }
    let other = Flow::new();
    other.take().await.unwrap();
    let waiting_flow = slow.clone();
    let waiting = tokio::spawn(async move { waiting_flow.take().await });
    slow.add(1).unwrap();
    tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    slow.close();
    assert!(slow.take().await.is_err());
    assert!(other.receive(48 * 1024 + 1).is_err());
}
