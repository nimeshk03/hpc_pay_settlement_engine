use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Entry type for double-entry bookkeeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "entry_type", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntryType {
    /// Debit entry - increases assets/expenses, decreases liabilities/revenue.
    Debit,
    /// Credit entry - decreases assets/expenses, increases liabilities/revenue.
    Credit,
}

impl EntryType {
    /// Returns the opposite entry type.
    pub fn opposite(&self) -> Self {
        match self {
            EntryType::Debit => EntryType::Credit,
            EntryType::Credit => EntryType::Debit,
        }
    }

    /// Returns the sign multiplier for balance calculations.
    /// Debit = +1, Credit = -1 (from the perspective of asset accounts).
    pub fn sign(&self) -> i32 {
        match self {
            EntryType::Debit => 1,
            EntryType::Credit => -1,
        }
    }
}

/// Represents a single entry in the ledger.
/// Every transaction creates at least two entries (debit and credit) that must balance.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LedgerEntry {
    pub id: Uuid,
    pub transaction_id: Uuid,
    pub account_id: Uuid,
    pub entry_type: EntryType,
    /// Amount of the entry (always positive, sign determined by entry_type).
    pub amount: Decimal,
    pub currency: String,
    /// Account balance after this entry was applied.
    pub balance_after: Decimal,
    /// Date when the entry becomes effective for reporting.
    pub effective_date: NaiveDate,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl LedgerEntry {
    /// Creates a new debit entry.
    pub fn debit(
        transaction_id: Uuid,
        account_id: Uuid,
        amount: Decimal,
        currency: String,
        balance_after: Decimal,
        effective_date: NaiveDate,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            transaction_id,
            account_id,
            entry_type: EntryType::Debit,
            amount,
            currency,
            balance_after,
            effective_date,
            metadata: None,
            created_at: Utc::now(),
        }
    }

    /// Creates a new credit entry.
    pub fn credit(
        transaction_id: Uuid,
        account_id: Uuid,
        amount: Decimal,
        currency: String,
        balance_after: Decimal,
        effective_date: NaiveDate,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            transaction_id,
            account_id,
            entry_type: EntryType::Credit,
            amount,
            currency,
            balance_after,
            effective_date,
            metadata: None,
            created_at: Utc::now(),
        }
    }

    /// Adds metadata to the entry.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Returns the signed amount based on entry type.
    /// Positive for debit, negative for credit.
    pub fn signed_amount(&self) -> Decimal {
        match self.entry_type {
            EntryType::Debit => self.amount,
            EntryType::Credit => -self.amount,
        }
    }
}

/// A pair of ledger entries representing a complete double-entry transaction.
#[derive(Debug, Clone)]
pub struct LedgerEntryPair {
    pub debit: LedgerEntry,
    pub credit: LedgerEntry,
}

impl LedgerEntryPair {
    /// Creates a new entry pair. Validates that amounts match.
    pub fn new(debit: LedgerEntry, credit: LedgerEntry) -> Result<Self, LedgerEntryError> {
        if debit.entry_type != EntryType::Debit {
            return Err(LedgerEntryError::InvalidEntryType(
                "Debit entry must have Debit type".to_string(),
            ));
        }
        if credit.entry_type != EntryType::Credit {
            return Err(LedgerEntryError::InvalidEntryType(
                "Credit entry must have Credit type".to_string(),
            ));
        }
        if debit.amount != credit.amount {
            return Err(LedgerEntryError::UnbalancedEntries {
                debit_amount: debit.amount,
                credit_amount: credit.amount,
            });
        }
        if debit.currency != credit.currency {
            return Err(LedgerEntryError::CurrencyMismatch {
                debit_currency: debit.currency.clone(),
                credit_currency: credit.currency.clone(),
            });
        }
        Ok(Self { debit, credit })
    }

    /// Returns the transaction amount.
    pub fn amount(&self) -> Decimal {
        self.debit.amount
    }

    /// Returns the currency.
    pub fn currency(&self) -> &str {
        &self.debit.currency
    }
}

#[derive(Debug, Clone)]
pub enum LedgerEntryError {
    InvalidEntryType(String),
    UnbalancedEntries {
        debit_amount: Decimal,
        credit_amount: Decimal,
    },
    CurrencyMismatch {
        debit_currency: String,
        credit_currency: String,
    },
}

impl std::fmt::Display for LedgerEntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerEntryError::InvalidEntryType(msg) => write!(f, "Invalid entry type: {}", msg),
            LedgerEntryError::UnbalancedEntries {
                debit_amount,
                credit_amount,
            } => write!(
                f,
                "Unbalanced entries: debit {} != credit {}",
                debit_amount, credit_amount
            ),
            LedgerEntryError::CurrencyMismatch {
                debit_currency,
                credit_currency,
            } => write!(
                f,
                "Currency mismatch: debit {} != credit {}",
                debit_currency, credit_currency
            ),
        }
    }
}

impl std::error::Error for LedgerEntryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_entry_type_opposite() {
        assert_eq!(EntryType::Debit.opposite(), EntryType::Credit);
        assert_eq!(EntryType::Credit.opposite(), EntryType::Debit);
    }

    #[test]
    fn test_entry_type_sign() {
        assert_eq!(EntryType::Debit.sign(), 1);
        assert_eq!(EntryType::Credit.sign(), -1);
    }

    #[test]
    fn test_debit_entry_creation() {
        let tx_id = Uuid::new_v4();
        let account_id = Uuid::new_v4();
        let entry = LedgerEntry::debit(
            tx_id,
            account_id,
            dec!(100),
            "USD".to_string(),
            dec!(500),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );

        assert_eq!(entry.transaction_id, tx_id);
        assert_eq!(entry.account_id, account_id);
        assert_eq!(entry.entry_type, EntryType::Debit);
        assert_eq!(entry.amount, dec!(100));
        assert_eq!(entry.balance_after, dec!(500));
    }

    #[test]
    fn test_credit_entry_creation() {
        let entry = LedgerEntry::credit(
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(300),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );

        assert_eq!(entry.entry_type, EntryType::Credit);
        assert_eq!(entry.amount, dec!(100));
    }

    #[test]
    fn test_signed_amount() {
        let debit = LedgerEntry::debit(
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(500),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );
        let credit = LedgerEntry::credit(
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(300),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );

        assert_eq!(debit.signed_amount(), dec!(100));
        assert_eq!(credit.signed_amount(), dec!(-100));
    }

    #[test]
    fn test_entry_pair_valid() {
        let tx_id = Uuid::new_v4();
        let debit = LedgerEntry::debit(
            tx_id,
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(500),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );
        let credit = LedgerEntry::credit(
            tx_id,
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(300),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );

        let pair = LedgerEntryPair::new(debit, credit);
        assert!(pair.is_ok());
        let pair = pair.unwrap();
        assert_eq!(pair.amount(), dec!(100));
        assert_eq!(pair.currency(), "USD");
    }

    #[test]
    fn test_entry_pair_unbalanced() {
        let tx_id = Uuid::new_v4();
        let debit = LedgerEntry::debit(
            tx_id,
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(500),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );
        let credit = LedgerEntry::credit(
            tx_id,
            Uuid::new_v4(),
            dec!(50), // Different amount
            "USD".to_string(),
            dec!(300),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );

        let pair = LedgerEntryPair::new(debit, credit);
        assert!(matches!(pair, Err(LedgerEntryError::UnbalancedEntries { .. })));
    }

    #[test]
    fn test_entry_pair_currency_mismatch() {
        let tx_id = Uuid::new_v4();
        let debit = LedgerEntry::debit(
            tx_id,
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(500),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );
        let credit = LedgerEntry::credit(
            tx_id,
            Uuid::new_v4(),
            dec!(100),
            "EUR".to_string(), // Different currency
            dec!(300),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );

        let pair = LedgerEntryPair::new(debit, credit);
        assert!(matches!(pair, Err(LedgerEntryError::CurrencyMismatch { .. })));
    }

    #[test]
    fn test_entry_with_metadata() {
        let entry = LedgerEntry::debit(
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100),
            "USD".to_string(),
            dec!(500),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        )
        .with_metadata(serde_json::json!({"note": "Test entry"}));

        assert!(entry.metadata.is_some());
    }

    #[test]
    fn test_serialization() {
        let entry = LedgerEntry::debit(
            Uuid::new_v4(),
            Uuid::new_v4(),
            dec!(100.5000),
            "USD".to_string(),
            dec!(500.2500),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
        );

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: LedgerEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.amount, dec!(100.5000));
        assert_eq!(deserialized.entry_type, EntryType::Debit);
    }
}
