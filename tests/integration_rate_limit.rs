use obscura_server::api::app_router;
use obscura_server::core::notification::InMemoryNotifier;
use std::sync::Arc;
use tokio::net::TcpListener;
use reqwest::{StatusCode, Client};
use std::time::Duration;
use futures::future::join_all;

mod common;

/// Helper to spin up a test server with custom rate limits.
/// Each test gets its own server and private rate-limit state.
async fn setup_test_server(req_per_sec: u32, burst: u32) -> String {
    let pool = common::get_test_pool().await;
    let mut config = common::get_test_config();
    config.rate_limit_per_second = req_per_sec;
    config.rate_limit_burst = burst;

    let notifier = Arc::new(InMemoryNotifier::new(config.clone()));
    let app = app_router(pool, config.clone(), notifier);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
    });

    server_url
}

#[tokio::test]
async fn test_rate_limit_isolation() {
    let server_url = setup_test_server(1, 2).await;
    let client = Client::new();
    let user_a = "1.1.1.1";
    let user_b = "2.2.2.2";

    println!("Exhausting User A's bucket...");
    for i in 1..=2 {
        let resp = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
            .header("X-Forwarded-For", user_a)
            .send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "Request {} for User A should succeed", i);
    }

    let resp_a = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", user_a)
        .send().await.unwrap();
    assert_eq!(resp_a.status(), StatusCode::TOO_MANY_REQUESTS, "User A should now be blocked");

    println!("Verifying User B is unaffected...");
    let resp_b = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", user_b)
        .send().await.unwrap();
    assert_eq!(resp_b.status(), StatusCode::NOT_FOUND, "User B should be perfectly fine");
}

#[tokio::test]
async fn test_rate_limit_proxy_chain() {
    let server_url = setup_test_server(1, 2).await;
    let client = Client::new();
    let chain = "9.9.9.9, 1.1.1.1, 2.2.2.2";

    println!("Testing proxy chain header parsing...");
    // 2.2.2.2 is not trusted, so it is treated as the client IP
    for _ in 0..2 {
        let resp = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
            .header("X-Forwarded-For", chain)
            .send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    
    // Should be blocked based on the LAST untrusted IP (2.2.2.2)
    let resp = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", "different.spoof, 2.2.2.2")
        .send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "Should block based on the rightmost untrusted IP");
}

#[tokio::test]
async fn test_rate_limit_concurrency() {
    let server_url = setup_test_server(1, 2).await;
    let client = Client::new();
    
    println!("Firing 20 concurrent requests from unique IPs...");
    let mut tasks = vec![];
    for i in 0..20 {
        let url = server_url.clone();
        let c = client.clone();
        tasks.push(tokio::spawn(async move {
            let ip = format!("10.10.10.{}", i);
            c.get(format!("{}/v1/keys/{}", url, uuid::Uuid::new_v4()))
                .header("X-Forwarded-For", ip)
                .send().await.unwrap()
        }));
    }
    
    for res in join_all(tasks).await {
        let resp = res.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "All concurrent unique IPs should succeed");
    }
}

#[tokio::test]
async fn test_rate_limit_fallback_to_peer_ip() {
    let server_url = setup_test_server(1, 2).await;
    let client = Client::new();

    println!("Testing fallback to peer IP when header is missing...");
    for _ in 0..2 {
        let resp = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
            .send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    let resp = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "Should have fallen back to local peer IP and blocked");
}

#[tokio::test]
async fn test_rate_limit_spoofing_protection() {
    // Only 127.0.0.1 is trusted by default in get_test_config()
    let server_url = setup_test_server(1, 1).await;
    let client = Client::new();
    
    let spoofed_ip = "1.2.3.4";
    let real_attacker_ip = "5.6.7.8";
    
    println!("Sending spoofed header X-Forwarded-For: {}, {}", spoofed_ip, real_attacker_ip);
    
    // First request - should be counted against 5.6.7.8, NOT 1.2.3.4
    let _ = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", format!("{}, {}", spoofed_ip, real_attacker_ip))
        .send().await.unwrap();

    // Second request with same 'real' IP but different 'spoofed' IP - should be blocked
    let resp = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", format!("9.9.9.9, {}", real_attacker_ip))
        .send().await.unwrap();
        
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "Should block based on real IP, ignoring the spoofed part");

    // Request from the 'spoofed' IP without the chain - should work (because it's a different person)
    let resp_ok = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", spoofed_ip)
        .send().await.unwrap();
    assert_eq!(resp_ok.status(), StatusCode::NOT_FOUND, "The spoofed IP itself should not be affected");
}

#[tokio::test]
async fn test_rate_limit_recovery() {
    let server_url = setup_test_server(1, 1).await;
    let client = Client::new();
    let ip = "5.5.5.5";

    println!("Testing rate limit recovery (refill)...");
    let _ = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send().await.unwrap();
    
    let resp = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "Should be blocked initially");

    // Wait for refill
    tokio::time::sleep(Duration::from_millis(1100)).await;
    
    let resp_ok = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send().await.unwrap();
    assert_eq!(resp_ok.status(), StatusCode::NOT_FOUND, "Should be unblocked after wait");
}

#[tokio::test]
async fn test_rate_limit_retry_after_header() {
    let server_url = setup_test_server(1, 1).await;
    let client = Client::new();
    let ip = "7.7.7.7";

    // First request consumes the budget
    client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send().await.unwrap();
    
    // Second request should be rate limited
    let resp = client.get(format!("{}/v1/keys/{}", server_url, uuid::Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send().await.unwrap();
    
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    
    let retry_after = resp.headers().get("retry-after");
    assert!(retry_after.is_some(), "Retry-After header should be present");
}
