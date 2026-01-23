use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_registration_matches_typescript_client_behavior() {
    // This test EXACTLY matches the @privacyresearch/libsignal-protocol-typescript implementation:
    // 1. Transmit Identity Key as 33 bytes (0x05 prefix)
    // 2. Transmit SignedPreKey as 33 bytes (0x05 prefix)
    // 3. Calculate Signature over the 33-byte WIRE format of the SignedPreKey
    
    let app = common::TestApp::spawn().await;
    let username = format!("user_ts_client_{}", &Uuid::new_v4().to_string()[..8]);

    let (reg_payload, _) = common::generate_registration_payload(&username, "password", 123, 0);

    let resp = app.client.post(format!("{}/v1/users", app.server_url)).json(&reg_payload).send().await.unwrap();
    assert_eq!(resp.status(), 201, "Server rejected the format used by the TypeScript client: {}", resp.text().await.unwrap());
}
