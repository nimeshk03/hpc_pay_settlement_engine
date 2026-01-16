use crate::error::{AppError, Result};
use crate::models::AccountBalance;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Repository for AccountBalance operations with optimistic locking support.
pub struct BalanceRepository {
    pool: PgPool,
}

impl BalanceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a new balance record for an account.
    pub async fn create(&self, balance: &AccountBalance) -> Result<AccountBalance> {
        let row = sqlx::query_as::<_, AccountBalance>(
            r#"
            INSERT INTO account_balances (account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(balance.account_id)
        .bind(&balance.currency)
        .bind(balance.available_balance)
        .bind(balance.pending_balance)
        .bind(balance.reserved_balance)
        .bind(balance.version)
        .bind(balance.last_updated)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds a balance by account ID and currency.
    pub async fn find_by_account_and_currency(
        &self,
        account_id: Uuid,
        currency: &str,
    ) -> Result<Option<AccountBalance>> {
        let row = sqlx::query_as::<_, AccountBalance>(
            r#"
            SELECT account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            FROM account_balances
            WHERE account_id = $1 AND currency = $2
            "#,
        )
        .bind(account_id)
        .bind(currency)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Finds all balances for an account.
    pub async fn find_by_account(&self, account_id: Uuid) -> Result<Vec<AccountBalance>> {
        let rows = sqlx::query_as::<_, AccountBalance>(
            r#"
            SELECT account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            FROM account_balances
            WHERE account_id = $1
            ORDER BY currency
            "#,
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(rows)
    }

    /// Updates a balance with optimistic locking.
    /// Returns None if the version doesn't match (concurrent modification).
    pub async fn update_with_version(
        &self,
        balance: &AccountBalance,
    ) -> Result<Option<AccountBalance>> {
        let row = sqlx::query_as::<_, AccountBalance>(
            r#"
            UPDATE account_balances
            SET available_balance = $3,
                pending_balance = $4,
                reserved_balance = $5,
                version = version + 1,
                last_updated = NOW()
            WHERE account_id = $1 AND currency = $2 AND version = $6
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(balance.account_id)
        .bind(&balance.currency)
        .bind(balance.available_balance)
        .bind(balance.pending_balance)
        .bind(balance.reserved_balance)
        .bind(balance.version)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Credits an account balance atomically.
    pub async fn credit(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        let row = sqlx::query_as::<_, AccountBalance>(
            r#"
            UPDATE account_balances
            SET available_balance = available_balance + $3,
                version = version + 1,
                last_updated = NOW()
            WHERE account_id = $1 AND currency = $2
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(account_id)
        .bind(currency)
        .bind(amount)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Debits an account balance atomically.
    /// Returns an error if insufficient funds.
    pub async fn debit(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        let row = sqlx::query_as::<_, AccountBalance>(
            r#"
            UPDATE account_balances
            SET available_balance = available_balance - $3,
                version = version + 1,
                last_updated = NOW()
            WHERE account_id = $1 AND currency = $2
              AND available_balance - reserved_balance >= $3
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(account_id)
        .bind(currency)
        .bind(amount)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        row.ok_or_else(|| AppError::Validation("Insufficient funds or balance not found".to_string()))
    }

    /// Reserves an amount from available balance.
    pub async fn reserve(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        let row = sqlx::query_as::<_, AccountBalance>(
            r#"
            UPDATE account_balances
            SET available_balance = available_balance - $3,
                reserved_balance = reserved_balance + $3,
                version = version + 1,
                last_updated = NOW()
            WHERE account_id = $1 AND currency = $2
              AND available_balance >= $3
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(account_id)
        .bind(currency)
        .bind(amount)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        row.ok_or_else(|| AppError::Validation("Insufficient funds for reservation".to_string()))
    }

    /// Releases a reserved amount back to available.
    pub async fn release_reservation(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        let row = sqlx::query_as::<_, AccountBalance>(
            r#"
            UPDATE account_balances
            SET available_balance = available_balance + LEAST($3, reserved_balance),
                reserved_balance = reserved_balance - LEAST($3, reserved_balance),
                version = version + 1,
                last_updated = NOW()
            WHERE account_id = $1 AND currency = $2
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(account_id)
        .bind(currency)
        .bind(amount)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Moves amount from available to pending.
    pub async fn move_to_pending(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        let row = sqlx::query_as::<_, AccountBalance>(
            r#"
            UPDATE account_balances
            SET available_balance = available_balance - $3,
                pending_balance = pending_balance + $3,
                version = version + 1,
                last_updated = NOW()
            WHERE account_id = $1 AND currency = $2
              AND available_balance >= $3
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(account_id)
        .bind(currency)
        .bind(amount)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        row.ok_or_else(|| AppError::Validation("Insufficient funds to move to pending".to_string()))
    }

    /// Settles pending balance to available.
    pub async fn settle_pending(
        &self,
        account_id: Uuid,
        currency: &str,
        amount: Decimal,
    ) -> Result<AccountBalance> {
        let row = sqlx::query_as::<_, AccountBalance>(
            r#"
            UPDATE account_balances
            SET pending_balance = pending_balance - LEAST($3, pending_balance),
                available_balance = available_balance + LEAST($3, pending_balance),
                version = version + 1,
                last_updated = NOW()
            WHERE account_id = $1 AND currency = $2
            RETURNING account_id, currency, available_balance, pending_balance, reserved_balance, version, last_updated
            "#,
        )
        .bind(account_id)
        .bind(currency)
        .bind(amount)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)?;

        Ok(row)
    }

    /// Gets or creates a balance for an account/currency pair.
    pub async fn get_or_create(
        &self,
        account_id: Uuid,
        currency: &str,
    ) -> Result<AccountBalance> {
        let existing = self.find_by_account_and_currency(account_id, currency).await?;
        
        if let Some(balance) = existing {
            return Ok(balance);
        }

        let new_balance = AccountBalance::new(account_id, currency.to_string());
        self.create(&new_balance).await
    }
}
