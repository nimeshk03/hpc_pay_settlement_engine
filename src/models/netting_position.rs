use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Represents a participant's netting position within a settlement batch.
/// Tracks gross receivables, payables, and the calculated net position.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct NettingPosition {
    pub batch_id: Uuid,
    pub participant_id: Uuid,
    pub currency: String,
    /// Total amount the participant should receive.
    pub gross_receivable: Decimal,
    /// Total amount the participant should pay.
    pub gross_payable: Decimal,
    /// Net position: positive = receive, negative = pay.
    pub net_position: Decimal,
    /// Number of transactions contributing to this position.
    pub transaction_count: i32,
    pub created_at: DateTime<Utc>,
}

impl NettingPosition {
    /// Creates a new netting position with zero values.
    pub fn new(batch_id: Uuid, participant_id: Uuid, currency: String) -> Self {
        Self {
            batch_id,
            participant_id,
            currency,
            gross_receivable: Decimal::ZERO,
            gross_payable: Decimal::ZERO,
            net_position: Decimal::ZERO,
            transaction_count: 0,
            created_at: Utc::now(),
        }
    }

    /// Adds a receivable amount (participant receives money).
    pub fn add_receivable(&mut self, amount: Decimal) {
        self.gross_receivable += amount;
        self.transaction_count += 1;
        self.recalculate_net();
    }

    /// Adds a payable amount (participant pays money).
    pub fn add_payable(&mut self, amount: Decimal) {
        self.gross_payable += amount;
        self.transaction_count += 1;
        self.recalculate_net();
    }

    /// Recalculates the net position from gross values.
    fn recalculate_net(&mut self) {
        self.net_position = self.gross_receivable - self.gross_payable;
    }

    /// Returns true if the participant is a net receiver.
    pub fn is_net_receiver(&self) -> bool {
        self.net_position > Decimal::ZERO
    }

    /// Returns true if the participant is a net payer.
    pub fn is_net_payer(&self) -> bool {
        self.net_position < Decimal::ZERO
    }

    /// Returns true if the position is balanced (net zero).
    pub fn is_balanced(&self) -> bool {
        self.net_position.is_zero()
    }

    /// Returns the absolute value of the net position.
    pub fn absolute_net(&self) -> Decimal {
        self.net_position.abs()
    }

    /// Returns the gross volume (total of receivables and payables).
    pub fn gross_volume(&self) -> Decimal {
        self.gross_receivable + self.gross_payable
    }

    /// Calculates the netting benefit (reduction in settlement volume).
    pub fn netting_benefit(&self) -> Decimal {
        self.gross_volume() - self.absolute_net()
    }

    /// Merges another position into this one (for aggregation).
    pub fn merge(&mut self, other: &NettingPosition) {
        self.gross_receivable += other.gross_receivable;
        self.gross_payable += other.gross_payable;
        self.transaction_count += other.transaction_count;
        self.recalculate_net();
    }
}

/// Summary of netting results for a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NettingSummary {
    pub batch_id: Uuid,
    pub currency: String,
    pub total_gross_volume: Decimal,
    pub total_net_volume: Decimal,
    pub total_transactions: i32,
    pub participant_count: i32,
    pub net_receivers: i32,
    pub net_payers: i32,
    pub balanced_participants: i32,
}

impl NettingSummary {
    /// Creates a summary from a list of netting positions.
    pub fn from_positions(batch_id: Uuid, currency: String, positions: &[NettingPosition]) -> Self {
        let total_gross_volume: Decimal = positions.iter().map(|p| p.gross_volume()).sum();
        let total_net_volume: Decimal = positions.iter().map(|p| p.absolute_net()).sum();
        let total_transactions: i32 = positions.iter().map(|p| p.transaction_count).sum();

        let net_receivers = positions.iter().filter(|p| p.is_net_receiver()).count() as i32;
        let net_payers = positions.iter().filter(|p| p.is_net_payer()).count() as i32;
        let balanced_participants = positions.iter().filter(|p| p.is_balanced()).count() as i32;

        Self {
            batch_id,
            currency,
            total_gross_volume,
            total_net_volume,
            total_transactions,
            participant_count: positions.len() as i32,
            net_receivers,
            net_payers,
            balanced_participants,
        }
    }

    /// Returns the netting efficiency as a percentage.
    pub fn netting_efficiency(&self) -> Decimal {
        if self.total_gross_volume.is_zero() {
            return Decimal::ZERO;
        }
        let reduction = self.total_gross_volume - self.total_net_volume;
        (reduction / self.total_gross_volume) * Decimal::from(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_netting_position_creation() {
        let batch_id = Uuid::new_v4();
        let participant_id = Uuid::new_v4();
        let position = NettingPosition::new(batch_id, participant_id, "USD".to_string());

        assert_eq!(position.batch_id, batch_id);
        assert_eq!(position.participant_id, participant_id);
        assert_eq!(position.gross_receivable, Decimal::ZERO);
        assert_eq!(position.gross_payable, Decimal::ZERO);
        assert_eq!(position.net_position, Decimal::ZERO);
        assert_eq!(position.transaction_count, 0);
    }

    #[test]
    fn test_add_receivable() {
        let mut position = NettingPosition::new(Uuid::new_v4(), Uuid::new_v4(), "USD".to_string());

        position.add_receivable(dec!(100));
        assert_eq!(position.gross_receivable, dec!(100));
        assert_eq!(position.net_position, dec!(100));
        assert_eq!(position.transaction_count, 1);
        assert!(position.is_net_receiver());
    }

    #[test]
    fn test_add_payable() {
        let mut position = NettingPosition::new(Uuid::new_v4(), Uuid::new_v4(), "USD".to_string());

        position.add_payable(dec!(100));
        assert_eq!(position.gross_payable, dec!(100));
        assert_eq!(position.net_position, dec!(-100));
        assert_eq!(position.transaction_count, 1);
        assert!(position.is_net_payer());
    }

    #[test]
    fn test_net_position_calculation() {
        let mut position = NettingPosition::new(Uuid::new_v4(), Uuid::new_v4(), "USD".to_string());

        position.add_receivable(dec!(100));
        position.add_payable(dec!(75));

        assert_eq!(position.gross_receivable, dec!(100));
        assert_eq!(position.gross_payable, dec!(75));
        assert_eq!(position.net_position, dec!(25));
        assert!(position.is_net_receiver());
    }

    #[test]
    fn test_balanced_position() {
        let mut position = NettingPosition::new(Uuid::new_v4(), Uuid::new_v4(), "USD".to_string());

        position.add_receivable(dec!(100));
        position.add_payable(dec!(100));

        assert!(position.is_balanced());
        assert!(!position.is_net_receiver());
        assert!(!position.is_net_payer());
    }

    #[test]
    fn test_gross_volume() {
        let mut position = NettingPosition::new(Uuid::new_v4(), Uuid::new_v4(), "USD".to_string());

        position.add_receivable(dec!(100));
        position.add_payable(dec!(75));

        assert_eq!(position.gross_volume(), dec!(175));
    }

    #[test]
    fn test_netting_benefit() {
        let mut position = NettingPosition::new(Uuid::new_v4(), Uuid::new_v4(), "USD".to_string());

        position.add_receivable(dec!(100));
        position.add_payable(dec!(75));

        // Gross volume = 175, Net = 25, Benefit = 150
        assert_eq!(position.netting_benefit(), dec!(150));
    }

    #[test]
    fn test_merge_positions() {
        let mut position1 = NettingPosition::new(Uuid::new_v4(), Uuid::new_v4(), "USD".to_string());
        position1.add_receivable(dec!(100));
        position1.add_payable(dec!(50));

        let mut position2 = NettingPosition::new(Uuid::new_v4(), Uuid::new_v4(), "USD".to_string());
        position2.add_receivable(dec!(75));
        position2.add_payable(dec!(25));

        position1.merge(&position2);

        assert_eq!(position1.gross_receivable, dec!(175));
        assert_eq!(position1.gross_payable, dec!(75));
        assert_eq!(position1.net_position, dec!(100));
        assert_eq!(position1.transaction_count, 4);
    }

    #[test]
    fn test_netting_summary() {
        let batch_id = Uuid::new_v4();

        let mut pos1 = NettingPosition::new(batch_id, Uuid::new_v4(), "USD".to_string());
        pos1.add_receivable(dec!(100));
        pos1.add_payable(dec!(75));

        let mut pos2 = NettingPosition::new(batch_id, Uuid::new_v4(), "USD".to_string());
        pos2.add_receivable(dec!(75));
        pos2.add_payable(dec!(100));

        let positions = vec![pos1, pos2];
        let summary = NettingSummary::from_positions(batch_id, "USD".to_string(), &positions);

        assert_eq!(summary.participant_count, 2);
        assert_eq!(summary.total_transactions, 4);
        assert_eq!(summary.net_receivers, 1);
        assert_eq!(summary.net_payers, 1);
        assert_eq!(summary.balanced_participants, 0);
    }

    #[test]
    fn test_netting_efficiency() {
        let batch_id = Uuid::new_v4();

        // Bank A owes Bank B: $100,000
        // Bank B owes Bank A: $75,000
        // Gross = 175,000, Net = 25,000
        let mut pos_a = NettingPosition::new(batch_id, Uuid::new_v4(), "USD".to_string());
        pos_a.add_payable(dec!(100000));
        pos_a.add_receivable(dec!(75000));

        let mut pos_b = NettingPosition::new(batch_id, Uuid::new_v4(), "USD".to_string());
        pos_b.add_receivable(dec!(100000));
        pos_b.add_payable(dec!(75000));

        let positions = vec![pos_a, pos_b];
        let summary = NettingSummary::from_positions(batch_id, "USD".to_string(), &positions);

        // Total gross = 350,000 (175k + 175k), Total net = 50,000 (25k + 25k)
        // Efficiency = (350k - 50k) / 350k * 100 = 85.71%
        let efficiency = summary.netting_efficiency();
        assert!(efficiency > dec!(85) && efficiency < dec!(86));
    }

    #[test]
    fn test_serialization() {
        let position = NettingPosition::new(Uuid::new_v4(), Uuid::new_v4(), "USD".to_string());

        let json = serde_json::to_string(&position).unwrap();
        let deserialized: NettingPosition = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.currency, "USD");
        assert_eq!(deserialized.net_position, Decimal::ZERO);
    }
}
