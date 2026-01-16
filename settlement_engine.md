# Module 2: Settlement & Clearing Engine

## Overview

A real-time settlement processor that handles transaction batching, netting, and reconciliation with ACID-compliant ledger operations and double-entry bookkeeping.

## Timeline
**Week 2**

## Why This Module Matters

- **Financial Core**: Every payment platform requires settlement infrastructure
- **Blockchain Alternative**: Traditional settlement reduced from days to seconds
- **Domain Expertise**: Demonstrates understanding of financial systems and accounting
- **Regulatory Compliance**: Audit trails and balance verification are mandatory

## Technical Features

### 1. ACID-Compliant Transaction Ledger

Using PostgreSQL with Write-Ahead Logging (WAL):

```sql
-- Transaction isolation for financial operations
SET SESSION TRANSACTION ISOLATION LEVEL SERIALIZABLE;

BEGIN;
-- Ledger operations here
COMMIT;
```

**Isolation Level: SERIALIZABLE**
- All statements see rows committed before first query
- Concurrent transactions that would create anomalies are rolled back
- Prevents phantom reads, dirty reads, and non-repeatable reads
- Essential for financial accuracy

**WAL (Write-Ahead Log)**:
- All changes written to log before data files
- Crash recovery guaranteed
- Point-in-time recovery support
- Replication foundation

### 2. Netting Engine

Reduces settlement volume by offsetting mutual obligations:

```
Before Netting:
  Bank A owes Bank B: $100,000
  Bank B owes Bank A: $75,000
  Total movements: $175,000

After Netting:
  Bank A owes Bank B: $25,000
  Total movements: $25,000
  Reduction: 85.7%
```

**Netting Types**:
- **Bilateral**: Between two parties
- **Multilateral**: Across multiple parties (more complex, higher savings)
- **Real-time**: Continuous netting as transactions arrive
- **Batch**: Periodic netting windows

### 3. Double-Entry Bookkeeping

Every transaction creates balanced entries:

```
Transaction: Payment of $100 from Customer to Merchant

Entries:
  DEBIT   Customer Liability Account    $100
  CREDIT  Merchant Asset Account        $100

Verification:
  SUM(DEBITS) = SUM(CREDITS) -- Always true
```

**Account Types**:
| Type | Normal Balance | Increases | Decreases |
|------|----------------|-----------|-----------|
| Asset | Debit | Debit | Credit |
| Liability | Credit | Credit | Debit |
| Revenue | Credit | Credit | Debit |
| Expense | Debit | Debit | Credit |

### 4. Idempotency Handling

Prevent duplicate processing:

```
Idempotency Strategy:
1. Generate idempotency_key from transaction attributes
2. Check existence in processed_transactions table
3. If exists: return cached result
4. If not: process and store result atomically

Key Generation:
  key = hash(merchant_id + amount + currency + timestamp_window + reference)
```

**Storage Options**:
- PostgreSQL with unique constraint
- Redis with TTL for temporary deduplication
- Bloom filters for fast negative lookups

### 5. Batch Settlement Optimization

```
Settlement Windows:
  - Real-time: Individual transaction settlement (higher cost)
  - Micro-batch: Every 1-5 minutes
  - Hourly: Aggregated hourly settlements
  - Daily: End-of-day batch processing

Optimization Factors:
  - Transaction fees (batch reduces per-tx fees)
  - Liquidity requirements
  - Merchant preferences
  - Regulatory requirements
```

## Core Components

### Ledger Entry Structure

```
LedgerEntry:
  id: UUID
  transaction_id: UUID
  account_id: UUID
  entry_type: enum [DEBIT, CREDIT]
  amount: decimal(19,4)
  currency: ISO4217
  balance_after: decimal(19,4)
  created_at: timestamp
  effective_date: date
  metadata: jsonb
```

### Settlement Batch

```
SettlementBatch:
  id: UUID
  status: enum [PENDING, PROCESSING, COMPLETED, FAILED]
  settlement_date: date
  cut_off_time: timestamp
  total_transactions: integer
  gross_amount: decimal
  net_amount: decimal
  fee_amount: decimal
  currency: string
  created_at: timestamp
  completed_at: timestamp
  participants: list<ParticipantSummary>
```

### Account Balance

```
AccountBalance:
  account_id: UUID
  currency: string
  available_balance: decimal
  pending_balance: decimal
  reserved_balance: decimal
  last_updated: timestamp
  version: integer  -- Optimistic locking
```

## Settlement Flow

```
1. Transaction Received (from Module 1)
   |
2. Validation
   ├── Idempotency check
   ├── Account existence
   └── Sufficient funds
   |
3. Ledger Entry Creation
   ├── Create DEBIT entry
   ├── Create CREDIT entry
   └── Update balances atomically
   |
4. Batch Assignment
   ├── Assign to settlement batch
   └── Update batch totals
   |
5. Netting (at batch close)
   ├── Calculate net positions
   └── Generate settlement instructions
   |
6. Settlement Execution
   ├── Execute bank transfers
   └── Update final status
   |
7. Confirmation
   └── Notify participants
```

## Data Models

### Transaction Record

```
TransactionRecord:
  id: UUID
  external_id: string
  type: enum [PAYMENT, REFUND, CHARGEBACK, TRANSFER, FEE]
  status: enum [PENDING, SETTLED, FAILED, REVERSED]
  source_account_id: UUID
  destination_account_id: UUID
  amount: decimal
  currency: string
  fee_amount: decimal
  net_amount: decimal
  settlement_batch_id: UUID
  idempotency_key: string
  created_at: timestamp
  settled_at: timestamp
  metadata: jsonb
```

### Netting Position

```
NettingPosition:
  batch_id: UUID
  participant_id: UUID
  currency: string
  gross_receivable: decimal
  gross_payable: decimal
  net_position: decimal  -- positive = receive, negative = pay
  transaction_count: integer
```

## Potential Improvements

### 1. Event Sourcing Architecture
- **Immutable Event Log**: Store all state changes as events
- **Temporal Queries**: Query account state at any point in time
- **Audit Trail**: Complete history without separate audit tables
- **CQRS**: Separate read and write models for performance

### 2. Kafka-Based Event Streaming
Based on Apache Kafka exactly-once semantics:

```java
// Transactional processing for settlement events
producer.initTransactions();
producer.beginTransaction();
// Process settlement
producer.commitTransaction();
```

**Topics**:
- `settlement.transactions`: Incoming transactions
- `settlement.batches`: Batch lifecycle events
- `settlement.positions`: Netting position updates
- `settlement.completed`: Final settlement confirmations

### 3. Multi-Currency Support
- **Real-time FX Rates**: Integration with rate providers
- **Currency Conversion**: Automatic conversion with spread tracking
- **Multi-currency Accounts**: Hold balances in multiple currencies
- **Settlement Currency Selection**: Optimize for FX costs

### 4. Liquidity Management
- **Liquidity Forecasting**: Predict settlement funding needs
- **Intraday Credit**: Support for temporary overdrafts
- **Liquidity Pooling**: Shared liquidity across accounts
- **Auto-funding**: Automatic transfers from funding sources

### 5. Real-Time Gross Settlement (RTGS)
- **Immediate Finality**: No batch delays for critical transactions
- **Queuing Mechanism**: Handle insufficient liquidity
- **Gridlock Resolution**: Algorithms to resolve circular dependencies
- **Hybrid Mode**: RTGS for large, batch for small transactions

### 6. Distributed Ledger Option
- **Consensus Protocol**: Multi-party agreement on state
- **Smart Contracts**: Programmable settlement rules
- **Immutability**: Cryptographic proof of history
- **Interoperability**: Bridge to traditional systems

### 7. Advanced Reconciliation Hooks
- **Pre-settlement Reconciliation**: Verify before committing
- **Exception Handling**: Automated discrepancy resolution
- **Adjustment Entries**: Systematic correction process
- **Reconciliation Reports**: Daily/monthly position reports

### 8. Performance Optimizations
- **Partitioned Tables**: Partition by date for faster queries
- **Materialized Views**: Pre-computed balance summaries
- **Connection Pooling**: PgBouncer for connection management
- **Read Replicas**: Separate read traffic from writes

### 9. Compliance Features
- **Regulatory Reporting**: Automated report generation
- **Hold Management**: Regulatory and fraud holds
- **Sanction Screening**: Integration with screening services
- **Transaction Limits**: Configurable per account/merchant

## Database Schema Considerations

### Partitioning Strategy
```sql
-- Partition ledger entries by month
CREATE TABLE ledger_entries (
    id UUID PRIMARY KEY,
    created_at TIMESTAMP NOT NULL,
    ...
) PARTITION BY RANGE (created_at);

-- Create monthly partitions
CREATE TABLE ledger_entries_2026_01 
    PARTITION OF ledger_entries
    FOR VALUES FROM ('2026-01-01') TO ('2026-02-01');
```

### Indexing Strategy
```sql
-- Critical indexes for settlement queries
CREATE INDEX idx_ledger_account_date ON ledger_entries(account_id, created_at);
CREATE INDEX idx_ledger_batch ON ledger_entries(settlement_batch_id);
CREATE INDEX idx_transactions_idempotency ON transactions(idempotency_key);
CREATE UNIQUE INDEX idx_balance_account_currency ON account_balances(account_id, currency);
```

## Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Transaction Throughput | 50,000 TPS | Per settlement engine instance |
| Ledger Write Latency | < 10ms P99 | Including balance update |
| Batch Processing | < 5 minutes | For 1M transaction batch |
| Balance Query | < 1ms P99 | Real-time balance lookup |
| Netting Calculation | < 30 seconds | For 100K transactions |

## Testing Strategy

### Unit Tests
- Double-entry balance verification
- Netting calculation accuracy
- Idempotency key generation

### Integration Tests
- PostgreSQL transaction isolation
- Batch settlement end-to-end
- Failure and recovery scenarios

### Consistency Tests
- Balance invariant verification (debits = credits)
- Concurrent transaction handling
- Partial failure recovery

## Dependencies

- **Upstream**: Transaction Router (Module 1)
- **Downstream**: Transaction Coordinator (Module 3), Reconciliation (Module 4)
- **Infrastructure**: PostgreSQL, Redis, Kafka

## File Structure

```
src/settlement/
├── core/
│   ├── engine.rs           # Main settlement logic
│   ├── ledger.rs           # Ledger operations
│   └── batch.rs            # Batch management
├── accounting/
│   ├── double_entry.rs     # Double-entry bookkeeping
│   ├── accounts.rs         # Account management
│   └── balances.rs         # Balance calculations
├── netting/
│   ├── calculator.rs       # Netting algorithms
│   ├── bilateral.rs        # Bilateral netting
│   └── multilateral.rs     # Multilateral netting
├── idempotency/
│   ├── handler.rs          # Idempotency logic
│   └── storage.rs          # Key storage
├── persistence/
│   ├── repository.rs       # Database operations
│   ├── migrations/         # Schema migrations
│   └── queries.rs          # SQL queries
└── events/
    ├── producer.rs         # Kafka producer
    └── consumer.rs         # Kafka consumer
```
