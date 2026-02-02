use std::time::Duration;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_graceful_websocket_shutdown() {
    let app = common::TestApp::spawn().await;
    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("shutdown_user_{}", run_id);

    // 1. Register and connect WS
    let user = app.register_user(&username).await;
    let mut ws = app.connect_ws(&user.token).await;

    // 2. Trigger Shutdown
    let _ = app.shutdown_tx.send(true);

    // 3. Assert Close Frame received with GoingAway code
    let mut close_received = false;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Some(Ok(msg)) = ws.receive_raw_timeout(Duration::from_millis(100)).await {
            if let Message::Close(Some(cf)) = msg {
                assert_eq!(cf.code, CloseCode::Away);
                assert_eq!(cf.reason, "Server shutting down");
                close_received = true;
                break;
            }
        }
    }

    assert!(close_received, "Did not receive graceful close frame within timeout");
}
