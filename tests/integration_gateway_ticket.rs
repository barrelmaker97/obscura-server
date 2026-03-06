#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::todo,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    missing_debug_implementations,
    clippy::cast_precision_loss,
    clippy::clone_on_ref_ptr,
    clippy::match_same_arms,
    clippy::items_after_statements,
    unreachable_pub,
    clippy::print_stdout,
    clippy::similar_names
)]
use reqwest::StatusCode;

mod common;

#[tokio::test]
async fn test_generate_websocket_ticket() {
    let app = common::TestApp::spawn().await;
    let username = common::generate_username("ticket_user");

    // 1. Register a user to get an auth token
    let user = app.register_user(&username).await;

    // 2. Request a WebSocket ticket
    let resp = app
        .client
        .post(format!("{}/v1/gateway/ticket", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED, "Failed to create ticket");

    let body: serde_json::Value = resp.json().await.unwrap();

    assert!(body.get("ticket").is_some(), "Response missing ticket field");
    let ticket = body["ticket"].as_str().unwrap();
    assert!(!ticket.is_empty(), "Ticket should not be empty");

    let cache = obscura_server::adapters::redis::RedisCache::new(
        std::sync::Arc::clone(&app.resources.pubsub),
        "ws:ticket:".to_string(),
        30,
    );

    // 3. Verify the ticket was saved in Redis
    let redis_ticket = cache.get(ticket).await.expect("Failed to query Redis").expect("Ticket not found in Redis");

    let cached_user_id = String::from_utf8(redis_ticket).expect("Invalid UTF-8 in cached user ID");
    assert_eq!(cached_user_id, user.user_id.to_string(), "Cached user ID does not match");
}

#[tokio::test]
async fn test_generate_websocket_ticket_unauthenticated() {
    let app = common::TestApp::spawn().await;

    // Request a WebSocket ticket without an auth header
    let resp = app.client.post(format!("{}/v1/gateway/ticket", app.server_url)).send().await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "Expected unauthorized response");
}
