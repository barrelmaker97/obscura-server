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
use serde_json::json;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_device_crud_lifecycle() {
    let app = common::TestApp::spawn().await;
    let username = common::generate_username("crud_device_user");

    // 1. Register User directly (no devices yet)
    let payload = json!({
        "username": username,
        "password": "password12345",
    });

    let resp_user = app.client.post(format!("{}/v1/users", app.server_url)).json(&payload).send().await.unwrap();
    assert_eq!(resp_user.status(), StatusCode::CREATED);
    
    let json_user: serde_json::Value = resp_user.json().await.unwrap();
    let user_token = json_user["token"].as_str().unwrap().to_string();

    // 2. List devices (should be empty initially)
    let resp_list_empty = app.client.get(format!("{}/v1/devices", app.server_url))
        .header("Authorization", format!("Bearer {}", user_token))
        .send().await.unwrap();
        
    assert_eq!(resp_list_empty.status(), StatusCode::OK);
    let list_empty_res: serde_json::Value = resp_list_empty.json().await.unwrap();
    let list_empty = list_empty_res["devices"].as_array().unwrap();
    assert!(list_empty.is_empty(), "User should have no devices initially");

    // 3. Create Device 1
    let (device1_payload, _) = common::generate_device_payload(111, 5);
    let resp_device1 = app.client.post(format!("{}/v1/devices", app.server_url))
        .header("Authorization", format!("Bearer {}", user_token))
        .json(&device1_payload)
        .send().await.unwrap();
        
    assert_eq!(resp_device1.status(), StatusCode::CREATED);
    let device1_json: serde_json::Value = resp_device1.json().await.unwrap();
    let device1_id = device1_json["deviceId"].as_str().unwrap().to_string();

    // 4. Create Device 2 (with a name)
    let mut device2_payload = device1_payload.clone();
    device2_payload["registrationId"] = json!(222);
    // Overriding payload manually since common::generate_device_payload doesn't take a name
    let mut dev2 = device2_payload.as_object_mut().unwrap().clone();
    dev2.insert("name".to_string(), json!("My Phone"));
    
    let resp_device2 = app.client.post(format!("{}/v1/devices", app.server_url))
        .header("Authorization", format!("Bearer {}", user_token))
        .json(&dev2)
        .send().await.unwrap();
        
    assert_eq!(resp_device2.status(), StatusCode::CREATED);
    let device2_json: serde_json::Value = resp_device2.json().await.unwrap();
    let device2_id = device2_json["deviceId"].as_str().unwrap().to_string();

    // 5. List Devices (should show both Device 1 and Device 2)
    let resp_list = app.client.get(format!("{}/v1/devices", app.server_url))
        .header("Authorization", format!("Bearer {}", user_token))
        .send().await.unwrap();
        
    assert_eq!(resp_list.status(), StatusCode::OK);
    let list_res: serde_json::Value = resp_list.json().await.unwrap();
    let list_devices = list_res["devices"].as_array().unwrap();
    assert_eq!(list_devices.len(), 2, "User should have 2 devices");
    
    let d1 = list_devices.iter().find(|d| d["deviceId"] == device1_id).unwrap();
    assert!(d1["name"].is_null(), "Device 1 should not have a name");
    
    let d2 = list_devices.iter().find(|d| d["deviceId"] == device2_id).unwrap();
    assert_eq!(d2["name"].as_str().unwrap(), "My Phone", "Device 2 should have the specified name");

    // 6. Provide Keys & Send Message to Device 1 to verify cascade delete
    // (creating a device automatically upserts keys)
    let _dev1_token = device1_json["token"].as_str().unwrap();
    
    let sender = app.register_user(&common::generate_username("crud_sender")).await;
    app.send_message(&sender.token, Uuid::parse_str(&device1_id).unwrap(), b"Ping").await;
    
    // Verify message exists
    app.assert_message_count(Uuid::parse_str(&device1_id).unwrap(), 1).await;
    
    // 7. Delete Device 1
    let resp_delete = app.client.delete(format!("{}/v1/devices/{}", app.server_url, device1_id))
        .header("Authorization", format!("Bearer {}", user_token)) // device deletion requires user token
        .send().await.unwrap();
        
    assert_eq!(resp_delete.status(), StatusCode::OK);
    
    // 8. List devices again (should only have Device 2)
    let resp_list_after = app.client.get(format!("{}/v1/devices", app.server_url))
        .header("Authorization", format!("Bearer {}", user_token))
        .send().await.unwrap();
        
    assert_eq!(resp_list_after.status(), StatusCode::OK);
    let list_res_after: serde_json::Value = resp_list_after.json().await.unwrap();
    let list_devices_after = list_res_after["devices"].as_array().unwrap();
    assert_eq!(list_devices_after.len(), 1, "User should have 1 device remaining");
    assert_eq!(list_devices_after[0]["deviceId"].as_str().unwrap(), device2_id);
    
    // 9. Verify cascade delete (keys and messages for Device 1 should be gone)
    // Keys verification: We can query the direct DB state
    let key_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM identity_keys WHERE device_id = $1")
        .bind(Uuid::parse_str(&device1_id).unwrap())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(key_count, 0, "Identity keys should be cascade deleted");
    
    // Message verification:
    app.assert_message_count(Uuid::parse_str(&device1_id).unwrap(), 0).await;
}
