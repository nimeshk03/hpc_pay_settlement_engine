use crate::error::{AppError, Result};
use crate::models::{NettingPosition, NettingSummary, TransactionRecord};
use crate::repositories::{BatchNettingSummary, NettingRepository};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

/// Represents a bilateral netting pair between two participants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BilateralPair {
    pub participant_a: Uuid,
    pub participant_b: Uuid,
    pub currency: String,
    pub a_to_b_gross: Decimal,
    pub b_to_a_gross: Decimal,
    pub net_amount: Decimal,
    pub net_direction: NetDirection,
    pub transaction_count: i32,
}

/// Direction of net settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetDirection {
    AToB,
    BToA,
    Balanced,
}

impl BilateralPair {
    pub fn new(participant_a: Uuid, participant_b: Uuid, currency: String) -> Self {
        Self {
            participant_a,
            participant_b,
            currency,
            a_to_b_gross: Decimal::ZERO,
            b_to_a_gross: Decimal::ZERO,
            net_amount: Decimal::ZERO,
            net_direction: NetDirection::Balanced,
            transaction_count: 0,
        }
    }

    pub fn add_a_to_b(&mut self, amount: Decimal) {
        self.a_to_b_gross += amount;
        self.transaction_count += 1;
        self.recalculate();
    }

    pub fn add_b_to_a(&mut self, amount: Decimal) {
        self.b_to_a_gross += amount;
        self.transaction_count += 1;
        self.recalculate();
    }

    fn recalculate(&mut self) {
        let diff = self.a_to_b_gross - self.b_to_a_gross;
        if diff > Decimal::ZERO {
            self.net_amount = diff;
            self.net_direction = NetDirection::AToB;
        } else if diff < Decimal::ZERO {
            self.net_amount = diff.abs();
            self.net_direction = NetDirection::BToA;
        } else {
            self.net_amount = Decimal::ZERO;
            self.net_direction = NetDirection::Balanced;
        }
    }

    pub fn gross_volume(&self) -> Decimal {
        self.a_to_b_gross + self.b_to_a_gross
    }

    pub fn netting_benefit(&self) -> Decimal {
        self.gross_volume() - self.net_amount
    }

    pub fn netting_efficiency(&self) -> Decimal {
        if self.gross_volume().is_zero() {
            return Decimal::ZERO;
        }
        (self.netting_benefit() / self.gross_volume()) * Decimal::from(100)
    }
}

/// Settlement instruction generated from netting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementInstruction {
    pub id: Uuid,
    pub batch_id: Uuid,
    pub from_participant: Uuid,
    pub to_participant: Uuid,
    pub amount: Decimal,
    pub currency: String,
    pub instruction_type: InstructionType,
    pub status: InstructionStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionType {
    BilateralNet,
    MultilateralNet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionStatus {
    Pending,
    Executed,
    Failed,
}

impl SettlementInstruction {
    pub fn new(
        batch_id: Uuid,
        from_participant: Uuid,
        to_participant: Uuid,
        amount: Decimal,
        currency: String,
        instruction_type: InstructionType,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            batch_id,
            from_participant,
            to_participant,
            amount,
            currency,
            instruction_type,
            status: InstructionStatus::Pending,
            created_at: Utc::now(),
        }
    }
}

/// Result of bilateral netting calculation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BilateralNettingResult {
    pub batch_id: Uuid,
    pub currency: String,
    pub pairs: Vec<BilateralPair>,
    pub total_gross_volume: Decimal,
    pub total_net_volume: Decimal,
    pub netting_efficiency: Decimal,
    pub instructions: Vec<SettlementInstruction>,
}

/// Result of multilateral netting calculation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultilateralNettingResult {
    pub batch_id: Uuid,
    pub currency: String,
    pub positions: Vec<NettingPosition>,
    pub total_gross_volume: Decimal,
    pub total_net_volume: Decimal,
    pub netting_efficiency: Decimal,
    pub instructions: Vec<SettlementInstruction>,
    pub participant_count: i32,
    pub net_receivers: i32,
    pub net_payers: i32,
}

/// Netting report for a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NettingReport {
    pub batch_id: Uuid,
    pub currency: String,
    pub generated_at: DateTime<Utc>,
    pub bilateral_result: Option<BilateralNettingResult>,
    pub multilateral_result: Option<MultilateralNettingResult>,
    pub total_transactions: i32,
    pub gross_volume: Decimal,
    pub net_volume: Decimal,
    pub reduction_amount: Decimal,
    pub reduction_percentage: Decimal,
}

/// Netting metrics for monitoring.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NettingMetrics {
    pub batches_processed: u64,
    pub total_transactions_netted: u64,
    pub total_gross_volume: Decimal,
    pub total_net_volume: Decimal,
    pub average_efficiency: Decimal,
}

/// The netting engine service handles all netting calculations.
pub struct NettingService {
    pool: PgPool,
    netting_repo: NettingRepository,
    metrics: std::sync::RwLock<NettingMetrics>,
}

impl NettingService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            netting_repo: NettingRepository::new(pool.clone()),
            pool,
            metrics: std::sync::RwLock::new(NettingMetrics::default()),
        }
    }

    /// Calculates bilateral netting for a set of transactions.
    pub fn calculate_bilateral_netting(
        &self,
        batch_id: Uuid,
        currency: &str,
        transactions: &[TransactionRecord],
    ) -> BilateralNettingResult {
        let mut pairs: HashMap<(Uuid, Uuid), BilateralPair> = HashMap::new();

        for tx in transactions {
            let (key, is_a_to_b) = self.normalize_pair_key(tx.source_account_id, tx.destination_account_id);

            let pair = pairs.entry(key).or_insert_with(|| {
                BilateralPair::new(key.0, key.1, currency.to_string())
            });

            if is_a_to_b {
                pair.add_a_to_b(tx.amount);
            } else {
                pair.add_b_to_a(tx.amount);
            }
        }

        let pairs_vec: Vec<BilateralPair> = pairs.into_values().collect();
        let total_gross: Decimal = pairs_vec.iter().map(|p| p.gross_volume()).sum();
        let total_net: Decimal = pairs_vec.iter().map(|p| p.net_amount).sum();

        let efficiency = if total_gross.is_zero() {
            Decimal::ZERO
        } else {
            ((total_gross - total_net) / total_gross) * Decimal::from(100)
        };

        let instructions = self.generate_bilateral_instructions(batch_id, &pairs_vec);

        BilateralNettingResult {
            batch_id,
            currency: currency.to_string(),
            pairs: pairs_vec,
            total_gross_volume: total_gross,
            total_net_volume: total_net,
            netting_efficiency: efficiency,
            instructions,
        }
    }

    fn normalize_pair_key(&self, a: Uuid, b: Uuid) -> ((Uuid, Uuid), bool) {
        if a < b {
            ((a, b), true)
        } else {
            ((b, a), false)
        }
    }

    fn generate_bilateral_instructions(
        &self,
        batch_id: Uuid,
        pairs: &[BilateralPair],
    ) -> Vec<SettlementInstruction> {
        pairs
            .iter()
            .filter(|p| p.net_direction != NetDirection::Balanced)
            .map(|p| {
                let (from, to) = match p.net_direction {
                    NetDirection::AToB => (p.participant_a, p.participant_b),
                    NetDirection::BToA => (p.participant_b, p.participant_a),
                    NetDirection::Balanced => unreachable!(),
                };
                SettlementInstruction::new(
                    batch_id,
                    from,
                    to,
                    p.net_amount,
                    p.currency.clone(),
                    InstructionType::BilateralNet,
                )
            })
            .collect()
    }

    /// Calculates multilateral netting for a set of transactions.
    pub fn calculate_multilateral_netting(
        &self,
        batch_id: Uuid,
        currency: &str,
        transactions: &[TransactionRecord],
    ) -> MultilateralNettingResult {
        let mut positions: HashMap<Uuid, NettingPosition> = HashMap::new();

        for tx in transactions {
            // Source pays
            let source_pos = positions
                .entry(tx.source_account_id)
                .or_insert_with(|| NettingPosition::new(batch_id, tx.source_account_id, currency.to_string()));
            source_pos.add_payable(tx.amount);

            // Destination receives
            let dest_pos = positions
                .entry(tx.destination_account_id)
                .or_insert_with(|| NettingPosition::new(batch_id, tx.destination_account_id, currency.to_string()));
            dest_pos.add_receivable(tx.amount);
        }

        let positions_vec: Vec<NettingPosition> = positions.into_values().collect();
        let summary = NettingSummary::from_positions(batch_id, currency.to_string(), &positions_vec);

        let instructions = self.generate_multilateral_instructions(batch_id, currency, &positions_vec);

        MultilateralNettingResult {
            batch_id,
            currency: currency.to_string(),
            positions: positions_vec,
            total_gross_volume: summary.total_gross_volume,
            total_net_volume: summary.total_net_volume,
            netting_efficiency: summary.netting_efficiency(),
            instructions,
            participant_count: summary.participant_count,
            net_receivers: summary.net_receivers,
            net_payers: summary.net_payers,
        }
    }

    fn generate_multilateral_instructions(
        &self,
        batch_id: Uuid,
        currency: &str,
        positions: &[NettingPosition],
    ) -> Vec<SettlementInstruction> {
        let mut payers: Vec<&NettingPosition> = positions
            .iter()
            .filter(|p| p.is_net_payer())
            .collect();
        let mut receivers: Vec<&NettingPosition> = positions
            .iter()
            .filter(|p| p.is_net_receiver())
            .collect();

        // Sort for deterministic matching
        payers.sort_by(|a, b| a.net_position.cmp(&b.net_position));
        receivers.sort_by(|a, b| b.net_position.cmp(&a.net_position));

        let mut instructions = Vec::new();
        let mut payer_remaining: HashMap<Uuid, Decimal> = payers
            .iter()
            .map(|p| (p.participant_id, p.net_position.abs()))
            .collect();
        let mut receiver_remaining: HashMap<Uuid, Decimal> = receivers
            .iter()
            .map(|p| (p.participant_id, p.net_position))
            .collect();

        // Match payers to receivers (greedy algorithm)
        for payer in &payers {
            let payer_id = payer.participant_id;
            while let Some(remaining) = payer_remaining.get_mut(&payer_id) {
                if remaining.is_zero() {
                    break;
                }

                // Find a receiver with remaining capacity
                let receiver = receivers.iter().find(|r| {
                    receiver_remaining
                        .get(&r.participant_id)
                        .map(|rem| *rem > Decimal::ZERO)
                        .unwrap_or(false)
                });

                if let Some(receiver) = receiver {
                    let receiver_id = receiver.participant_id;
                    let receiver_rem = receiver_remaining.get_mut(&receiver_id).unwrap();

                    let transfer_amount = (*remaining).min(*receiver_rem);

                    if transfer_amount > Decimal::ZERO {
                        instructions.push(SettlementInstruction::new(
                            batch_id,
                            payer_id,
                            receiver_id,
                            transfer_amount,
                            currency.to_string(),
                            InstructionType::MultilateralNet,
                        ));

                        *remaining -= transfer_amount;
                        *receiver_rem -= transfer_amount;
                    }
                } else {
                    break;
                }
            }
        }

        instructions
    }

    /// Persists netting positions to the database.
    pub async fn persist_positions(&self, positions: &[NettingPosition]) -> Result<Vec<NettingPosition>> {
        self.netting_repo.create_batch(positions).await
    }

    /// Gets netting positions for a batch.
    pub async fn get_batch_positions(&self, batch_id: Uuid) -> Result<Vec<NettingPosition>> {
        self.netting_repo.find_by_batch(batch_id).await
    }

    /// Gets batch netting summary.
    pub async fn get_batch_summary(&self, batch_id: Uuid) -> Result<BatchNettingSummary> {
        self.netting_repo.get_batch_summary(batch_id).await
    }

    /// Generates a complete netting report for a batch.
    pub fn generate_report(
        &self,
        batch_id: Uuid,
        currency: &str,
        transactions: &[TransactionRecord],
    ) -> NettingReport {
        let bilateral = self.calculate_bilateral_netting(batch_id, currency, transactions);
        let multilateral = self.calculate_multilateral_netting(batch_id, currency, transactions);

        let gross_volume = multilateral.total_gross_volume;
        let net_volume = multilateral.total_net_volume;
        let reduction_amount = gross_volume - net_volume;
        let reduction_percentage = if gross_volume.is_zero() {
            Decimal::ZERO
        } else {
            (reduction_amount / gross_volume) * Decimal::from(100)
        };

        // Update metrics
        self.update_metrics(transactions.len() as u64, gross_volume, net_volume);

        NettingReport {
            batch_id,
            currency: currency.to_string(),
            generated_at: Utc::now(),
            bilateral_result: Some(bilateral),
            multilateral_result: Some(multilateral),
            total_transactions: transactions.len() as i32,
            gross_volume,
            net_volume,
            reduction_amount,
            reduction_percentage,
        }
    }

    fn update_metrics(&self, transactions: u64, gross: Decimal, net: Decimal) {
        if let Ok(mut metrics) = self.metrics.write() {
            metrics.batches_processed += 1;
            metrics.total_transactions_netted += transactions;
            metrics.total_gross_volume += gross;
            metrics.total_net_volume += net;

            if metrics.total_gross_volume > Decimal::ZERO {
                let reduction = metrics.total_gross_volume - metrics.total_net_volume;
                metrics.average_efficiency = (reduction / metrics.total_gross_volume) * Decimal::from(100);
            }
        }
    }

    /// Gets current netting metrics.
    pub fn get_metrics(&self) -> NettingMetrics {
        self.metrics.read().map(|m| m.clone()).unwrap_or_default()
    }

    /// Clears netting positions for a batch.
    pub async fn clear_batch_positions(&self, batch_id: Uuid) -> Result<u64> {
        self.netting_repo.delete_by_batch(batch_id).await
    }

    /// Performs full netting for a batch and persists results.
    pub async fn process_batch_netting(
        &self,
        batch_id: Uuid,
        currency: &str,
        transactions: &[TransactionRecord],
    ) -> Result<NettingReport> {
        // Calculate multilateral netting
        let result = self.calculate_multilateral_netting(batch_id, currency, transactions);

        // Persist positions
        self.persist_positions(&result.positions).await?;

        // Generate full report
        Ok(self.generate_report(batch_id, currency, transactions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use crate::models::{TransactionType, TransactionStatus};

    fn create_test_transaction(
        source: Uuid,
        dest: Uuid,
        amount: Decimal,
        currency: &str,
    ) -> TransactionRecord {
        TransactionRecord {
            id: Uuid::new_v4(),
            external_id: format!("TX-{}", Uuid::new_v4()),
            transaction_type: TransactionType::Payment,
            status: TransactionStatus::Settled,
            source_account_id: source,
            destination_account_id: dest,
            amount,
            fee_amount: Decimal::ZERO,
            net_amount: amount,
            currency: currency.to_string(),
            idempotency_key: format!("IDEM-{}", Uuid::new_v4()),
            metadata: None,
            settlement_batch_id: None,
            created_at: Utc::now(),
            settled_at: Some(Utc::now()),
        }
    }

    #[test]
    fn test_bilateral_pair_creation() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let pair = BilateralPair::new(a, b, "USD".to_string());

        assert_eq!(pair.participant_a, a);
        assert_eq!(pair.participant_b, b);
        assert_eq!(pair.net_direction, NetDirection::Balanced);
    }

    #[test]
    fn test_bilateral_pair_netting() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut pair = BilateralPair::new(a, b, "USD".to_string());

        pair.add_a_to_b(dec!(100));
        pair.add_b_to_a(dec!(75));

        assert_eq!(pair.a_to_b_gross, dec!(100));
        assert_eq!(pair.b_to_a_gross, dec!(75));
        assert_eq!(pair.net_amount, dec!(25));
        assert_eq!(pair.net_direction, NetDirection::AToB);
        assert_eq!(pair.gross_volume(), dec!(175));
        assert_eq!(pair.netting_benefit(), dec!(150));
    }

    #[test]
    fn test_bilateral_pair_reverse_direction() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut pair = BilateralPair::new(a, b, "USD".to_string());

        pair.add_a_to_b(dec!(50));
        pair.add_b_to_a(dec!(100));

        assert_eq!(pair.net_amount, dec!(50));
        assert_eq!(pair.net_direction, NetDirection::BToA);
    }

    #[test]
    fn test_bilateral_pair_balanced() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut pair = BilateralPair::new(a, b, "USD".to_string());

        pair.add_a_to_b(dec!(100));
        pair.add_b_to_a(dec!(100));

        assert_eq!(pair.net_amount, Decimal::ZERO);
        assert_eq!(pair.net_direction, NetDirection::Balanced);
    }

    #[test]
    fn test_settlement_instruction_creation() {
        let batch_id = Uuid::new_v4();
        let from = Uuid::new_v4();
        let to = Uuid::new_v4();

        let instruction = SettlementInstruction::new(
            batch_id,
            from,
            to,
            dec!(100),
            "USD".to_string(),
            InstructionType::BilateralNet,
        );

        assert_eq!(instruction.batch_id, batch_id);
        assert_eq!(instruction.from_participant, from);
        assert_eq!(instruction.to_participant, to);
        assert_eq!(instruction.amount, dec!(100));
        assert_eq!(instruction.status, InstructionStatus::Pending);
    }

    #[test]
    fn test_bilateral_netting_calculation() {
        let batch_id = Uuid::new_v4();
        let bank_a = Uuid::new_v4();
        let bank_b = Uuid::new_v4();

        let transactions = vec![
            create_test_transaction(bank_a, bank_b, dec!(100), "USD"),
            create_test_transaction(bank_b, bank_a, dec!(75), "USD"),
        ];

        let result = calculate_bilateral_netting_standalone(batch_id, "USD", &transactions);

        assert_eq!(result.pairs.len(), 1);
        assert_eq!(result.total_gross_volume, dec!(175));
        assert_eq!(result.total_net_volume, dec!(25));
        assert!(result.netting_efficiency > dec!(85));
        assert_eq!(result.instructions.len(), 1);
    }

    #[test]
    fn test_multilateral_netting_calculation() {
        let batch_id = Uuid::new_v4();
        let bank_a = Uuid::new_v4();
        let bank_b = Uuid::new_v4();
        let bank_c = Uuid::new_v4();

        // A -> B: 100, B -> C: 80, C -> A: 60
        let transactions = vec![
            create_test_transaction(bank_a, bank_b, dec!(100), "USD"),
            create_test_transaction(bank_b, bank_c, dec!(80), "USD"),
            create_test_transaction(bank_c, bank_a, dec!(60), "USD"),
        ];

        let result = calculate_multilateral_netting_standalone(batch_id, "USD", &transactions);

        assert_eq!(result.positions.len(), 3);
        assert_eq!(result.participant_count, 3);

        // Verify positions sum to zero
        let total_net: Decimal = result.positions.iter().map(|p| p.net_position).sum();
        assert_eq!(total_net, Decimal::ZERO);
    }

    #[test]
    fn test_netting_report_generation() {
        let batch_id = Uuid::new_v4();
        let bank_a = Uuid::new_v4();
        let bank_b = Uuid::new_v4();

        let transactions = vec![
            create_test_transaction(bank_a, bank_b, dec!(100), "USD"),
            create_test_transaction(bank_b, bank_a, dec!(75), "USD"),
        ];

        let bilateral = calculate_bilateral_netting_standalone(batch_id, "USD", &transactions);
        let multilateral = calculate_multilateral_netting_standalone(batch_id, "USD", &transactions);

        assert_eq!(bilateral.batch_id, batch_id);
        assert_eq!(multilateral.participant_count, 2);
        assert!(bilateral.netting_efficiency > dec!(85));
    }

    #[test]
    fn test_netting_metrics_tracking() {
        let metrics = NettingMetrics::default();
        assert_eq!(metrics.batches_processed, 0);
        assert_eq!(metrics.total_transactions_netted, 0);
    }

    #[test]
    fn test_circular_dependency_handling() {
        let batch_id = Uuid::new_v4();
        let bank_a = Uuid::new_v4();
        let bank_b = Uuid::new_v4();
        let bank_c = Uuid::new_v4();

        // Circular: A -> B -> C -> A (each 100)
        let transactions = vec![
            create_test_transaction(bank_a, bank_b, dec!(100), "USD"),
            create_test_transaction(bank_b, bank_c, dec!(100), "USD"),
            create_test_transaction(bank_c, bank_a, dec!(100), "USD"),
        ];

        let result = calculate_multilateral_netting_standalone(batch_id, "USD", &transactions);

        // All positions should be balanced
        assert!(result.positions.iter().all(|p| p.is_balanced()));
        assert_eq!(result.total_net_volume, Decimal::ZERO);
        assert_eq!(result.netting_efficiency, dec!(100));
    }

    fn calculate_bilateral_netting_standalone(
        batch_id: Uuid,
        currency: &str,
        transactions: &[TransactionRecord],
    ) -> BilateralNettingResult {
        let mut pairs: HashMap<(Uuid, Uuid), BilateralPair> = HashMap::new();

        for tx in transactions {
            let (key, is_a_to_b) = if tx.source_account_id < tx.destination_account_id {
                ((tx.source_account_id, tx.destination_account_id), true)
            } else {
                ((tx.destination_account_id, tx.source_account_id), false)
            };

            let pair = pairs.entry(key).or_insert_with(|| {
                BilateralPair::new(key.0, key.1, currency.to_string())
            });

            if is_a_to_b {
                pair.add_a_to_b(tx.amount);
            } else {
                pair.add_b_to_a(tx.amount);
            }
        }

        let pairs_vec: Vec<BilateralPair> = pairs.into_values().collect();
        let total_gross: Decimal = pairs_vec.iter().map(|p| p.gross_volume()).sum();
        let total_net: Decimal = pairs_vec.iter().map(|p| p.net_amount).sum();

        let efficiency = if total_gross.is_zero() {
            Decimal::ZERO
        } else {
            ((total_gross - total_net) / total_gross) * Decimal::from(100)
        };

        let instructions: Vec<SettlementInstruction> = pairs_vec
            .iter()
            .filter(|p| p.net_direction != NetDirection::Balanced)
            .map(|p| {
                let (from, to) = match p.net_direction {
                    NetDirection::AToB => (p.participant_a, p.participant_b),
                    NetDirection::BToA => (p.participant_b, p.participant_a),
                    NetDirection::Balanced => unreachable!(),
                };
                SettlementInstruction::new(
                    batch_id,
                    from,
                    to,
                    p.net_amount,
                    p.currency.clone(),
                    InstructionType::BilateralNet,
                )
            })
            .collect();

        BilateralNettingResult {
            batch_id,
            currency: currency.to_string(),
            pairs: pairs_vec,
            total_gross_volume: total_gross,
            total_net_volume: total_net,
            netting_efficiency: efficiency,
            instructions,
        }
    }

    fn calculate_multilateral_netting_standalone(
        batch_id: Uuid,
        currency: &str,
        transactions: &[TransactionRecord],
    ) -> MultilateralNettingResult {
        let mut positions: HashMap<Uuid, NettingPosition> = HashMap::new();

        for tx in transactions {
            let source_pos = positions
                .entry(tx.source_account_id)
                .or_insert_with(|| NettingPosition::new(batch_id, tx.source_account_id, currency.to_string()));
            source_pos.add_payable(tx.amount);

            let dest_pos = positions
                .entry(tx.destination_account_id)
                .or_insert_with(|| NettingPosition::new(batch_id, tx.destination_account_id, currency.to_string()));
            dest_pos.add_receivable(tx.amount);
        }

        let positions_vec: Vec<NettingPosition> = positions.into_values().collect();
        let summary = NettingSummary::from_positions(batch_id, currency.to_string(), &positions_vec);

        MultilateralNettingResult {
            batch_id,
            currency: currency.to_string(),
            positions: positions_vec,
            total_gross_volume: summary.total_gross_volume,
            total_net_volume: summary.total_net_volume,
            netting_efficiency: summary.netting_efficiency(),
            instructions: Vec::new(),
            participant_count: summary.participant_count,
            net_receivers: summary.net_receivers,
            net_payers: summary.net_payers,
        }
    }
}
