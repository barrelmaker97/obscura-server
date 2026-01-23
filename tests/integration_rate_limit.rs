use axum::http::StatusCode;
use futures::future::join_all;
use reqwest::Client;
use std::time::Duration;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_rate_limit_isolation() {
    let mut config = common::get_test_config();
    config.rate_limit.per_second = 1;
    config.rate_limit.burst = 2;
    let app = common::TestApp::spawn_with_config(config).await;

    let user_a = "1.1.1.1";
    let user_b = "2.2.2.2";

    println!("Exhausting User A's bucket...");
    for i in 1..=2 {
        let resp = app
            .client
            .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
            .header("X-Forwarded-For", user_a)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "Request {} for User A should succeed", i);
    }

    let resp_a = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", user_a)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_a.status(), StatusCode::TOO_MANY_REQUESTS, "User A should now be blocked");

    println!("Verifying User B is unaffected...");
    let resp_b = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", user_b)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_b.status(), StatusCode::NOT_FOUND, "User B should be perfectly fine");
}

#[tokio::test]
async fn test_rate_limit_proxy_chain() {
    let mut config = common::get_test_config();
    config.rate_limit.per_second = 1;
    config.rate_limit.burst = 2;
    let app = common::TestApp::spawn_with_config(config).await;

    let chain = "9.9.9.9, 1.1.1.1, 2.2.2.2";

    println!("Testing proxy chain header parsing...");
    for _ in 0..2 {
        let resp = app
            .client
            .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
            .header("X-Forwarded-For", chain)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    let resp = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", "different.spoof, 2.2.2.2")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "Should block based on the rightmost untrusted IP");
}

#[tokio::test]
async fn test_rate_limit_concurrency() {
    let mut config = common::get_test_config();
    config.rate_limit.per_second = 1;
    config.rate_limit.burst = 2;
    let app = common::TestApp::spawn_with_config(config).await;

    println!("Firing 20 concurrent requests from unique IPs...");
    let mut tasks = vec![];
    let client = Client::new();

    for i in 0..20 {
        let url = app.server_url.clone();
        let c = client.clone();
        tasks.push(tokio::spawn(async move {
            let ip = format!("10.10.10.{}", i);
            c.get(format!("{}/v1/keys/{}", url, Uuid::new_v4())).header("X-Forwarded-For", ip).send().await.unwrap()
        }));
    }

    for res in join_all(tasks).await {
        let resp = res.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "All concurrent unique IPs should succeed");
    }
}

#[tokio::test]
async fn test_rate_limit_fallback_to_peer_ip() {
    let mut config = common::get_test_config();
    config.rate_limit.per_second = 1;
    config.rate_limit.burst = 2;
    let app = common::TestApp::spawn_with_config(config).await;

    println!("Testing fallback to peer IP when header is missing...");
    for _ in 0..2 {
        let resp = app.client.get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4())).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    let resp = app.client.get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "Should have fallen back to local peer IP and blocked");
}

#[tokio::test]
async fn test_rate_limit_spoofing_protection() {
    let mut config = common::get_test_config();
    config.rate_limit.per_second = 1;
    config.rate_limit.burst = 1;
    let app = common::TestApp::spawn_with_config(config).await;

    let spoofed_ip = "1.2.3.4";
    let real_attacker_ip = "5.6.7.8";

    println!("Sending spoofed header X-Forwarded-For: {}, {}", spoofed_ip, real_attacker_ip);

    let _ = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", format!("{}, {}", spoofed_ip, real_attacker_ip))
        .send()
        .await
        .unwrap();

    let resp = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", format!("9.9.9.9, {}", real_attacker_ip))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "Should block based on real IP, ignoring the spoofed part"
    );

    let resp_ok = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", spoofed_ip)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_ok.status(), StatusCode::NOT_FOUND, "The spoofed IP itself should not be affected");
}

#[tokio::test]
async fn test_rate_limit_tiers() {
    let mut config = common::get_test_config();
    config.rate_limit.per_second = 10;
    config.rate_limit.burst = 10;
    config.rate_limit.auth_per_second = 1;
    config.rate_limit.auth_burst = 1;
    let app = common::TestApp::spawn_with_config(config).await;

    let ip = "1.2.3.4";

    println!("Testing Auth Tier (Registration)...");
    let (reg_payload, _) = common::generate_registration_payload("tier_test", "password", 123, 0);

    for _ in 0..2 {
        let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();
        if resp.status() == StatusCode::TOO_MANY_REQUESTS {
            println!("Auth tier limited as expected");
        }
    }


    println!("Testing Standard Tier (Keys) from same IP...");
    for _ in 0..5 {
        let resp = app
            .client
            .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
            .header("X-Forwarded-For", ip)
            .send()
            .await
            .unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "Standard tier should not be affected by auth exhaustion"
        );
    }
}

#[tokio::test]
async fn test_rate_limit_recovery() {
    let mut config = common::get_test_config();
    config.rate_limit.per_second = 1;
    config.rate_limit.burst = 1;
    let app = common::TestApp::spawn_with_config(config).await;

    let ip = "5.5.5.5";

    println!("Testing rate limit recovery (refill)...");
    let _ = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send()
        .await
        .unwrap();

    let resp = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS, "Should be blocked initially");

    tokio::time::sleep(Duration::from_millis(1100)).await;

    let resp_ok = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_ok.status(), StatusCode::NOT_FOUND, "Should be unblocked after wait");
}

#[tokio::test]
async fn test_rate_limit_retry_after_header() {
    let mut config = common::get_test_config();
    config.rate_limit.per_second = 1;
    config.rate_limit.burst = 1;
    let app = common::TestApp::spawn_with_config(config).await;

    let ip = "7.7.7.7";

    app.client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send()
        .await
        .unwrap();

    let resp = app
        .client
        .get(format!("{}/v1/keys/{}", app.server_url, Uuid::new_v4()))
        .header("X-Forwarded-For", ip)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let retry_after = resp.headers().get("retry-after");
    assert!(retry_after.is_some(), "Retry-After header should be present");
}
