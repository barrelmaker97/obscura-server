use obscura_server::adapters::database::backup_repo::BackupRepository;
use obscura_server::adapters::storage::S3Storage;
use obscura_server::workers::BackupCleanupWorker;
use reqwest::StatusCode;
use std::sync::Arc;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_backup_lifecycle() {
    let mut config = common::get_test_config();
    config.storage.bucket = format!("test-backup-{}", &Uuid::new_v4().to_string()[..8]);
    config.backup.min_size_bytes = 0;

    let app = common::TestApp::spawn_with_config(config.clone()).await;
    common::ensure_storage_bucket(&app.s3_client, &config.storage.bucket).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("backup_user_{}", run_id)).await;

    // 1. Initial State: No backup
    let resp_404 = app
        .client
        .get(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_404.status(), StatusCode::NOT_FOUND);

    // 2. Upload (First time, If-Match: 0)
    let content = b"Backup Data v1";
    let resp_up = app
        .client
        .post(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("If-Match", "0")
        .body(content.to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(resp_up.status(), StatusCode::OK);

    // 3. Download
    let resp_down = app
        .client
        .get(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .send()
        .await
        .unwrap();

    assert_eq!(resp_down.status(), StatusCode::OK);
    let etag = resp_down.headers().get("ETag").unwrap().to_str().unwrap().to_string();
    assert_eq!(etag, "\"1\"");
    assert_eq!(resp_down.bytes().await.unwrap(), content.to_vec());

    // 4. Upload (Update, If-Match: 1)
    let content_v2 = b"Backup Data v2";
    let resp_up_v2 = app
        .client
        .post(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("If-Match", "1")
        .body(content_v2.to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(resp_up_v2.status(), StatusCode::OK);

    // 5. Download v2
    let resp_down_v2 = app
        .client
        .get(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .send()
        .await
        .unwrap();

    assert_eq!(resp_down_v2.status(), StatusCode::OK);
    let etag_v2 = resp_down_v2.headers().get("ETag").unwrap().to_str().unwrap().to_string();
    assert_eq!(etag_v2, "\"2\"");
    assert_eq!(resp_down_v2.bytes().await.unwrap(), content_v2.to_vec());

    // 6. Conflict (If-Match Mismatch)
    let resp_conflict = app
        .client
        .post(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("If-Match", "1") // Should be 2
        .body(b"Conflict".to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(resp_conflict.status(), StatusCode::PRECONDITION_FAILED);

    // 7. Head
    let resp_head = app
        .client
        .head(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .send()
        .await
        .unwrap();

    assert_eq!(resp_head.status(), StatusCode::OK);
    let etag_head = resp_head.headers().get("ETag").unwrap().to_str().unwrap();
    assert_eq!(etag_head, "\"2\"");
    let len_head = resp_head.headers().get("Content-Length").unwrap().to_str().unwrap();
    assert_eq!(len_head, content_v2.len().to_string());
}

#[tokio::test]
async fn test_backup_min_size() {
    let mut config = common::get_test_config();
    config.storage.bucket = format!("test-backup-min-{}", &Uuid::new_v4().to_string()[..8]);
    config.backup.min_size_bytes = 10;

    let app = common::TestApp::spawn_with_config(config.clone()).await;
    common::ensure_storage_bucket(&app.s3_client, &config.storage.bucket).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("backup_min_{}", run_id)).await;

    // Upload too small
    let content = b"TooSmall"; // 8 bytes
    let resp = app
        .client
        .post(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("If-Match", "0")
        .header("Content-Length", content.len().to_string())
        .body(content.to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_backup_concurrent_conflict() {
    let mut config = common::get_test_config();
    config.storage.bucket = format!("test-backup-conflict-{}", &Uuid::new_v4().to_string()[..8]);
    config.backup.min_size_bytes = 0;
    let app = common::TestApp::spawn_with_config(config).await;
    common::ensure_storage_bucket(&app.s3_client, &app.config.storage.bucket).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("backup_con_{}", run_id)).await;

    // Manually insert a "stale" UPLOADING record
    let user_id = user.user_id;
    sqlx::query("INSERT INTO backups (user_id, current_version, pending_version, state, pending_at) VALUES ($1, 0, 1, 'UPLOADING', NOW() - INTERVAL '1 hour')")
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();

    // Attempt upload (should succeed by taking over stale)
    let resp_takeover = app
        .client
        .post(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("If-Match", "0")
        .body(b"Takeover".to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(resp_takeover.status(), StatusCode::OK);

    // Verify current version is 1
    let row: (i32,) = sqlx::query_as("SELECT current_version FROM backups WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(row.0, 1);

    // Manually insert a "fresh" UPLOADING record (simulate concurrent upload)
    sqlx::query("UPDATE backups SET state = 'UPLOADING', pending_version = 2, pending_at = NOW() WHERE user_id = $1")
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();

    // Attempt upload (should conflict)
    let resp_conflict = app
        .client
        .post(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("If-Match", "1")
        .body(b"Conflict".to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(resp_conflict.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_backup_janitor_cleanup() {
    let mut config = common::get_test_config();
    config.storage.bucket = format!("test-janitor-{}", &Uuid::new_v4().to_string()[..8]);
    config.backup.stale_threshold_mins = 1; // 1 minute threshold

    let app = common::TestApp::spawn_with_config(config.clone()).await;
    common::ensure_storage_bucket(&app.s3_client, &config.storage.bucket).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("janitor_{}", run_id)).await;
    let user_id = user.user_id;

    // 1. Setup Stale DB State (2 minutes ago)
    sqlx::query(
        "INSERT INTO backups (user_id, current_version, pending_version, state, pending_at)
         VALUES ($1, 1, 2, 'UPLOADING', NOW() - INTERVAL '2 minutes')",
    )
    .bind(user_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // 2. Setup "Pending" S3 Object
    let pending_key = format!("{}{}/v2", config.backup.prefix, user_id);

    app.s3_client
        .put_object()
        .bucket(&config.storage.bucket)
        .key(&pending_key)
        .body(aws_sdk_s3::primitives::ByteStream::from(b"zombie data".to_vec()))
        .send()
        .await
        .unwrap();

    // 3. Instantiate Worker
    let storage_adapter = Arc::new(S3Storage::new(app.s3_client.clone(), config.storage.bucket.clone()));
    let worker =
        BackupCleanupWorker::new(app.pool.clone(), BackupRepository::new(), storage_adapter, config.backup.clone());

    // 4. Execution
    let cleaned_count = worker.cleanup_stale().await.expect("Janitor cleanup failed");
    assert!(cleaned_count >= 1);

    // 5. Verification: DB State for OUR specific user
    let backup: (String, Option<i32>) = sqlx::query_as("SELECT state, pending_version FROM backups WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();

    assert_eq!(backup.0, "ACTIVE");
    assert!(backup.1.is_none());

    // 6. Verification: S3 Object is GONE
    let head_res = app.s3_client.head_object().bucket(&config.storage.bucket).key(&pending_key).send().await;

    assert!(head_res.is_err(), "Pending object should have been deleted from S3");
}

#[tokio::test]
async fn test_backup_version_rotation_and_cleanup() {
    let mut config = common::get_test_config();
    config.storage.bucket = format!("test-versioning-{}", &Uuid::new_v4().to_string()[..8]);
    config.backup.min_size_bytes = 0;

    let app = common::TestApp::spawn_with_config(config.clone()).await;
    common::ensure_storage_bucket(&app.s3_client, &config.storage.bucket).await;

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    let user = app.register_user(&format!("version_{}", run_id)).await;
    let user_id = user.user_id;

    // 1. Upload Version 1
    let content_v1 = b"Version 1 Data";
    let resp_v1 = app
        .client
        .post(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("If-Match", "0")
        .body(content_v1.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp_v1.status(), StatusCode::OK);

    // Verify v1 exists in S3
    let key_v1 = format!("{}{}/v1", config.backup.prefix, user_id);
    let head_v1 = app.s3_client.head_object().bucket(&config.storage.bucket).key(&key_v1).send().await;
    assert!(head_v1.is_ok(), "v1 should exist in S3");

    // 2. Upload Version 2
    let content_v2 = b"Version 2 Data";
    let resp_v2 = app
        .client
        .post(format!("{}/v1/backup", app.server_url))
        .header("Authorization", format!("Bearer {}", user.token))
        .header("If-Match", "1")
        .body(content_v2.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp_v2.status(), StatusCode::OK);

    // Verify v2 exists in S3
    let key_v2 = format!("{}{}/v2", config.backup.prefix, user_id);
    let head_v2 = app.s3_client.head_object().bucket(&config.storage.bucket).key(&key_v2).send().await;
    assert!(head_v2.is_ok(), "v2 should exist in S3");

    // 3. Verify v1 is GONE (Cleanup)
    // The cleanup is fired in a background task, so we might need a small wait
    let mut v1_deleted = false;
    for _ in 0..10 {
        let head_v1_check = app.s3_client.head_object().bucket(&config.storage.bucket).key(&key_v1).send().await;
        if head_v1_check.is_err() {
            v1_deleted = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(v1_deleted, "Old version v1 should have been deleted from S3");

    // 4. Verify Final DB State
    let version: (i32,) = sqlx::query_as("SELECT current_version FROM backups WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(version.0, 2);
}
