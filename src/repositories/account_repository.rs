use crate::error::{AppError, Result};
use crate::models::{Account, AccountStatus, AccountType};
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for Account CRUD operations.
pub struct AccountRepository {
    pool: PgPool,
}

impl AccountRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a new account in the database.
    pub async fn create(&self, account: &Account) -> Result<Account> {
        let row = sqlx::query_as::<_, Account>(
            r#"
            INSERT INTO accounts (id, external_id, name, type, status, currency, metadata, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id, external_id, name, type, status, currency, metadata, created_at, updated_at
            "#,
        )
        .bind(account.id)
        .bind(&account.external_id)
        .bind(&account.name)
        .bind(&account.account_type)
        .bind(&account.status)
        .bind(&account.currency)
        .bind(&account.metadata)
        .bind(account.created_at)
        .bind(account.updated_at)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds an account by its UUID.
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Account>> {
        let row = sqlx::query_as::<_, Account>(
            r#"
            SELECT id, external_id, name, type, status, currency, metadata, created_at, updated_at
            FROM accounts
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds an account by its external ID.
    pub async fn find_by_external_id(&self, external_id: &str) -> Result<Option<Account>> {
        let row = sqlx::query_as::<_, Account>(
            r#"
            SELECT id, external_id, name, type, status, currency, metadata, created_at, updated_at
            FROM accounts
            WHERE external_id = $1
            "#,
        )
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Lists all accounts with optional filters.
    pub async fn list(
        &self,
        account_type: Option<AccountType>,
        status: Option<AccountStatus>,
        currency: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Account>> {
        let rows = sqlx::query_as::<_, Account>(
            r#"
            SELECT id, external_id, name, type, status, currency, metadata, created_at, updated_at
            FROM accounts
            WHERE ($1::account_type IS NULL OR type = $1)
              AND ($2::account_status IS NULL OR status = $2)
              AND ($3::text IS NULL OR currency = $3)
            ORDER BY created_at DESC
            LIMIT $4 OFFSET $5
            "#,
        )
        .bind(account_type)
        .bind(status)
        .bind(currency)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Updates an account's status.
    pub async fn update_status(&self, id: Uuid, status: AccountStatus) -> Result<Option<Account>> {
        let row = sqlx::query_as::<_, Account>(
            r#"
            UPDATE accounts
            SET status = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, external_id, name, type, status, currency, metadata, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(status)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Updates an account's metadata.
    pub async fn update_metadata(
        &self,
        id: Uuid,
        metadata: serde_json::Value,
    ) -> Result<Option<Account>> {
        let row = sqlx::query_as::<_, Account>(
            r#"
            UPDATE accounts
            SET metadata = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, external_id, name, type, status, currency, metadata, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(metadata)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Deletes an account by ID (soft delete by setting status to Closed).
    pub async fn delete(&self, id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE accounts
            SET status = 'CLOSED', updated_at = NOW()
            WHERE id = $1 AND status != 'CLOSED'
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(result.rows_affected() > 0)
    }

    /// Counts accounts matching the given filters.
    pub async fn count(
        &self,
        account_type: Option<AccountType>,
        status: Option<AccountStatus>,
    ) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM accounts
            WHERE ($1::account_type IS NULL OR type = $1)
              AND ($2::account_status IS NULL OR status = $2)
            "#,
        )
        .bind(account_type)
        .bind(status)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row.0)
    }

    /// Checks if an account exists by external ID.
    pub async fn exists_by_external_id(&self, external_id: &str) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            r#"
            SELECT EXISTS(SELECT 1 FROM accounts WHERE external_id = $1)
            "#,
        )
        .bind(external_id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row.0)
    }
}
