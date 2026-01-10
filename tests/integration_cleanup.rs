use uuid::Uuid;
use time::{OffsetDateTime, Duration};
use obscura_server::storage::message_repo::MessageRepository;

mod common;

#[tokio::test]
async fn test_expired_message_cleanup() {
    let pool = common::get_test_pool().await;
    
    let repo = MessageRepository::new(pool.clone());

    // 1. Create a dummy user
    let user_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, 'hash')")
        .bind(user_id)
        .bind(format!("cleanup_user_{}", user_id.to_string()[..8].to_string()))
        .execute(&pool)
        .await
        .unwrap();

    // 2. Insert an expired message (1 day ago)
    let msg_id = Uuid::new_v4();
    let expired_time = OffsetDateTime::now_utc() - Duration::days(1);
    
    sqlx::query(
        "INSERT INTO messages (id, sender_id, recipient_id, content, expires_at) VALUES ($1, $2, $2, $3, $4)"
    )
    .bind(msg_id)
    .bind(user_id)
    .bind(b"expired content".to_vec())
    .bind(expired_time)
    .execute(&pool)
    .await
    .unwrap();

    // 3. Insert a non-expired message (1 day from now)
    let active_msg_id = Uuid::new_v4();
    let active_time = OffsetDateTime::now_utc() + Duration::days(1);
    
    sqlx::query(
        "INSERT INTO messages (id, sender_id, recipient_id, content, expires_at) VALUES ($1, $2, $2, $3, $4)"
    )
    .bind(active_msg_id)
    .bind(user_id)
    .bind(b"active content".to_vec())
    .bind(active_time)
    .execute(&pool)
    .await
    .unwrap();

    // Verify both exist
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE recipient_id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);

    // 4. Run cleanup
    let deleted = repo.delete_expired().await.unwrap();
    assert!(deleted >= 1);

    // 5. Verify only active one remains
    let count_after: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE recipient_id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_after, 1);

    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages WHERE id = $1)")
        .bind(active_msg_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(exists, "Active message should still exist");
}
