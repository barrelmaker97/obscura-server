use crate::core::user::{OneTimePreKey, PreKeyBundle, SignedPreKey};
use crate::error::Result;
use sqlx::{Executor, PgPool, Postgres, Row, postgres::PgConnection};
use uuid::Uuid;

#[derive(Clone)]
pub struct KeyRepository {
    pool: PgPool,
}

impl KeyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_identity_key<'e, E>(
        &self,
        executor: E,
        user_id: Uuid,
        identity_key: &[u8],
        registration_id: i32,
    ) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query(
            r#"
            INSERT INTO identity_keys (user_id, identity_key, registration_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (user_id) DO UPDATE
            SET identity_key = $2, registration_id = $3
            "#,
        )
        .bind(user_id)
        .bind(identity_key)
        .bind(registration_id)
        .execute(executor)
        .await?;
        Ok(())
    }

    pub async fn upsert_signed_pre_key<'e, E>(
        &self,
        executor: E,
        user_id: Uuid,
        key_id: i32,
        public_key: &[u8],
        signature: &[u8],
    ) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query(
            r#"
            INSERT INTO signed_pre_keys (id, user_id, public_key, signature)
            VALUES ($2, $1, $3, $4)
            ON CONFLICT (id, user_id) DO UPDATE
            SET public_key = $3, signature = $4
            "#,
        )
        .bind(user_id)
        .bind(key_id)
        .bind(public_key)
        .bind(signature)
        .execute(executor)
        .await?;
        Ok(())
    }

    pub async fn insert_one_time_pre_keys(
        &self,
        executor: &mut PgConnection,
        user_id: Uuid,
        keys: &[(i32, Vec<u8>)],
    ) -> Result<()> {
        for (id, key) in keys {
            sqlx::query(
                r#"
                INSERT INTO one_time_pre_keys (id, user_id, public_key)
                VALUES ($2, $1, $3)
                ON CONFLICT (id, user_id) DO NOTHING
                "#,
            )
            .bind(user_id)
            .bind(*id)
            .bind(key)
            .execute(&mut *executor)
            .await?;
        }
        Ok(())
    }

    pub async fn fetch_pre_key_bundle(&self, user_id: Uuid) -> Result<Option<PreKeyBundle>> {
        // Fetch identity and signed pre key
        let identity_row = sqlx::query(
            r#"
            SELECT identity_key, registration_id FROM identity_keys WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(identity_row) = identity_row else {
            return Ok(None);
        };
        let identity_key: Vec<u8> = identity_row.get("identity_key");
        let registration_id: i32 = identity_row.get("registration_id");

        let signed_row = sqlx::query(
            r#"
            SELECT id, public_key, signature FROM signed_pre_keys WHERE user_id = $1
            ORDER BY created_at DESC LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(signed_row) = signed_row else {
            return Ok(None);
        };
        let signed_pre_key = SignedPreKey {
            key_id: signed_row.get("id"),
            public_key: signed_row.get("public_key"),
            signature: signed_row.get("signature"),
        };

        // Fetch one one-time pre key and delete it
        let otpk_row = sqlx::query(
            r#"
            DELETE FROM one_time_pre_keys
            WHERE id = (
                SELECT id FROM one_time_pre_keys WHERE user_id = $1 LIMIT 1
            ) AND user_id = $1
            RETURNING id, public_key
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        let one_time_pre_key =
            otpk_row.map(|row| OneTimePreKey { key_id: row.get("id"), public_key: row.get("public_key") });

        Ok(Some(PreKeyBundle { registration_id, identity_key, signed_pre_key, one_time_pre_key }))
    }

    pub async fn fetch_identity_key(&self, user_id: Uuid) -> Result<Option<Vec<u8>>> {
        let row = sqlx::query("SELECT identity_key FROM identity_keys WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| r.get("identity_key")))
    }

    pub async fn fetch_identity_key_for_update<'e, E>(&self, executor: E, user_id: Uuid) -> Result<Option<Vec<u8>>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let row = sqlx::query("SELECT identity_key FROM identity_keys WHERE user_id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(executor)
            .await?;

        Ok(row.map(|r| r.get("identity_key")))
    }

    pub async fn delete_all_signed_pre_keys<'e, E>(&self, executor: E, user_id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query("DELETE FROM signed_pre_keys WHERE user_id = $1").bind(user_id).execute(executor).await?;
        Ok(())
    }

    pub async fn delete_all_one_time_pre_keys<'e, E>(&self, executor: E, user_id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query("DELETE FROM one_time_pre_keys WHERE user_id = $1").bind(user_id).execute(executor).await?;
        Ok(())
    }

    pub async fn count_one_time_pre_keys(&self, user_id: Uuid) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM one_time_pre_keys WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }
}
