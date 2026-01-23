use crate::core::crypto_types::{PublicKey, Signature};
use crate::core::user::{OneTimePreKey, PreKeyBundle, SignedPreKey};
use crate::error::{AppError, Result};
use sqlx::{Executor, PgConnection, Postgres, Row};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct KeyRepository {}

impl KeyRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn upsert_identity_key<'e, E>(
        &self,
        executor: E,
        user_id: Uuid,
        identity_key: &PublicKey,
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
        .bind(identity_key.as_bytes())
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
        public_key: &PublicKey,
        signature: &Signature,
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
        .bind(public_key.as_bytes())
        .bind(signature.as_bytes())
        .execute(executor)
        .await?;
        Ok(())
    }

    pub async fn insert_one_time_pre_keys<'e, E>(
        &self,
        executor: E,
        user_id: Uuid,
        keys: &[OneTimePreKey],
    ) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if keys.is_empty() {
            return Ok(());
        }

        let mut ids = Vec::with_capacity(keys.len());
        let mut user_ids = Vec::with_capacity(keys.len());
        let mut pub_keys = Vec::with_capacity(keys.len());

        for k in keys {
            ids.push(k.key_id);
            user_ids.push(user_id);
            pub_keys.push(k.public_key.as_bytes());
        }

        sqlx::query(
            r#"
            INSERT INTO one_time_pre_keys (id, user_id, public_key)
            SELECT * FROM UNNEST($1::int4[], $2::uuid[], $3::bytea[])
            ON CONFLICT (id, user_id) DO NOTHING
            "#,
        )
        .bind(&ids)
        .bind(&user_ids)
        .bind(&pub_keys)
        .execute(executor)
        .await?;

        Ok(())
    }

    pub async fn fetch_pre_key_bundle(
        &self,
        executor: &mut PgConnection,
        user_id: Uuid,
    ) -> Result<Option<PreKeyBundle>> {
        // Fetch identity and signed pre key
        let identity_row = sqlx::query(
            r#"
            SELECT identity_key, registration_id FROM identity_keys WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&mut *executor)
        .await?;

        let Some(identity_row) = identity_row else {
            return Ok(None);
        };
        let identity_key_bytes: Vec<u8> = identity_row.get("identity_key");
        let registration_id: i32 = identity_row.get("registration_id");

        // Convert Identity Key
        let identity_key = PublicKey::try_from(identity_key_bytes).map_err(|_| AppError::Internal)?;

        let signed_row = sqlx::query(
            r#"
            SELECT id, public_key, signature FROM signed_pre_keys WHERE user_id = $1
            ORDER BY created_at DESC LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&mut *executor)
        .await?;

        let Some(signed_row) = signed_row else {
            return Ok(None);
        };
        let pk_bytes: Vec<u8> = signed_row.get("public_key");
        let sig_bytes: Vec<u8> = signed_row.get("signature");

        let pk = PublicKey::try_from(pk_bytes).map_err(|_| AppError::Internal)?;
        let sig = Signature::try_from(sig_bytes).map_err(|_| AppError::Internal)?;

        let signed_pre_key = SignedPreKey { key_id: signed_row.get("id"), public_key: pk, signature: sig };

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
        .fetch_optional(&mut *executor)
        .await?;

        let one_time_pre_key = match otpk_row {
            Some(row) => {
                let pk_bytes: Vec<u8> = row.get("public_key");
                let pk = PublicKey::try_from(pk_bytes).map_err(|_| AppError::Internal)?;
                Some(OneTimePreKey { key_id: row.get("id"), public_key: pk })
            }
            None => None,
        };

        Ok(Some(PreKeyBundle { registration_id, identity_key, signed_pre_key, one_time_pre_key }))
    }

    pub async fn fetch_identity_key<'e, E>(&self, executor: E, user_id: Uuid) -> Result<Option<PublicKey>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let row = sqlx::query("SELECT identity_key FROM identity_keys WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(executor)
            .await?;

        match row {
            Some(r) => {
                let bytes: Vec<u8> = r.get("identity_key");
                let pk = PublicKey::try_from(bytes).map_err(|_| AppError::Internal)?;
                Ok(Some(pk))
            }
            None => Ok(None),
        }
    }

    pub async fn fetch_identity_key_for_update<'e, E>(&self, executor: E, user_id: Uuid) -> Result<Option<PublicKey>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let row = sqlx::query("SELECT identity_key FROM identity_keys WHERE user_id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(executor)
            .await?;

        match row {
            Some(r) => {
                let bytes: Vec<u8> = r.get("identity_key");
                let pk = PublicKey::try_from(bytes).map_err(|_| AppError::Internal)?;
                Ok(Some(pk))
            }
            None => Ok(None),
        }
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

    pub async fn count_one_time_pre_keys<'e, E>(&self, executor: E, user_id: Uuid) -> Result<i64>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM one_time_pre_keys WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(executor)
            .await?;
        Ok(count)
    }

    pub async fn delete_oldest_one_time_pre_keys<'e, E>(&self, executor: E, user_id: Uuid, limit: i64) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query(
            r#"
            DELETE FROM one_time_pre_keys
            WHERE user_id = $1 AND id IN (
                SELECT id FROM one_time_pre_keys
                WHERE user_id = $1
                ORDER BY created_at ASC
                LIMIT $2
            )
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .execute(executor)
        .await?;
        Ok(())
    }
}
