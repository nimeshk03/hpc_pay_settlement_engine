# Settlement Engine

A real-time settlement processor built in Rust that handles transaction batching, netting, and reconciliation with ACID-compliant ledger operations and double-entry bookkeeping.

## Features

- **ACID-Compliant Ledger**: PostgreSQL with serializable isolation for financial integrity.
- **Double-Entry Bookkeeping**: Core accounting principle enforcement (Credits = Debits).
- **Settlement Batches**: Configurable windows (Real-time, Hourly, Daily).
- **Netting Engine**: Bilateral and Multilateral netting to reduce settlement volume.
- **Idempotency**: Robust duplicate handling using Redis and database constraints.
- **Event-Driven**: Kafka integration for asynchronous processing and event sourcing.

## Prerequisites

- **Rust**: Latest stable version (1.75+)
- **Docker**: For containerized dependencies (PostgreSQL, Redis, Kafka)
- **Docker Compose**: For orchestration

## Quick Start

1. **Clone the repository** (if not already done)

2. **Start Infrastructure**
   ```bash
   docker-compose up -d
   ```
   This starts:
   - PostgreSQL (port 5432)
   - Redis (port 6379)
   - Zookeeper (port 2181)
   - Kafka (port 9092)

3. **Run the Application**
   ```bash
   # Run with default logging
   cargo run

   # Run with debug logging
   RUST_LOG=debug cargo run
   ```
   
   The application will:
   - Connect to the database and run pending migrations.
   - Connect to Redis.
   - Connect to Kafka.
   - Verify system health.

## Configuration

Configuration is handled via `config` crate and supports `config/default.toml`, `config/local.toml`, and environment variables.

### Environment Variables

Prefix variables with `APP__` (double underscore).

- `APP__DATABASE__URL`: PostgreSQL connection string
- `APP__REDIS__URL`: Redis connection string
- `APP__KAFKA__BROKERS`: Kafka broker list
- `APP__APPLICATION__PORT`: HTTP port
- `APP__APPLICATION__LOG_LEVEL`: Log level (info, debug, trace)

## Project Structure

```
src/
├── accounting/     # Double-entry logic, accounts, balances
├── core/           # Engine, ledger, batch types
├── events/         # Kafka producers and consumers
├── idempotency/    # Deduplication logic
├── models/         # Domain models (Account, Transaction, LedgerEntry, etc.)
├── netting/        # Netting algorithms
├── persistence/    # Legacy persistence module
├── repositories/   # Data access layer (AccountRepository, BalanceRepository, etc.)
├── config.rs       # Configuration structs
├── error.rs        # Centralized error handling
├── lib.rs          # Library crate exports
└── main.rs         # Application entry point

tests/
├── common/         # Test utilities and setup
└── repository_tests.rs  # Integration tests for repositories
```

## Domain Models

The settlement engine implements the following core domain models:

- **Account**: Financial accounts with types (Asset, Liability, Revenue, Expense) and status management
- **AccountBalance**: Balance tracking with optimistic locking for concurrent updates
- **TransactionRecord**: Financial transactions with type (Payment, Refund, Chargeback, Transfer, Fee) and status lifecycle
- **LedgerEntry**: Double-entry bookkeeping entries (Debit/Credit) with balance tracking
- **SettlementBatch**: Batch processing with lifecycle management (Pending, Processing, Completed, Failed)
- **NettingPosition**: Participant positions for bilateral/multilateral netting
- **Currency**: ISO 4217 currency code support

## Repository Layer

Each domain model has a corresponding repository for database operations:

- **AccountRepository**: CRUD operations, status updates, filtering by type/status
- **BalanceRepository**: Atomic balance updates, optimistic locking, credit/debit/reserve operations
- **TransactionRepository**: Transaction lifecycle, idempotency key lookup, batch assignment
- **LedgerRepository**: Entry creation, balance verification, account history queries
- **BatchRepository**: Batch lifecycle management, totals tracking, ready-for-processing queries
- **NettingRepository**: Position storage, net receiver/payer queries, batch summaries

## Service Layer

Business logic services that orchestrate repository operations:

- **AccountService**: Account creation with validation, status management (freeze/activate/close), metadata updates
- **BalanceService**: Real-time balance queries, credit/debit operations, reservations, balance snapshots
- **DoubleEntryEngine**: Core double-entry bookkeeping engine with atomic transactions, balance verification, and reversal support

## Idempotency System

Robust duplicate request handling with multi-layer storage:

- **IdempotencyKeyGenerator**: SHA-256 based key generation from transaction attributes with configurable time windows
- **PostgresIdempotencyStore**: Persistent storage with unique constraints and automatic expiration
- **RedisIdempotencyCache**: Fast lookup cache with TTL for high-performance duplicate detection
- **HybridIdempotencyStore**: Combined PostgreSQL + Redis storage for reliability and speed
- **IdempotencyHandler**: Request processing with atomic check-and-process logic and metrics tracking
- **IdempotencyCleanupJob**: Background job for expired record cleanup

## Ledger Operations

Comprehensive ledger service for transaction processing:

- **LedgerService**: Main service for processing all transaction types with ACID compliance
- **TransactionStateMachine**: State machine for valid transaction status transitions (Pending -> Settled/Failed, Settled -> Reversed)
- **ValidationPipeline**: Multi-step validation including field validation, account verification, and sufficient funds checks
- **Transaction Types**: Full support for Payment, Transfer, Fee, Refund, and Chargeback transactions
- **Atomic Operations**: SERIALIZABLE isolation level for concurrent transaction safety
- **Balance Tracking**: Automatic balance_after calculation for audit trail

## Batch Settlement System

Complete batch settlement infrastructure for grouping and processing transactions:

- **BatchService**: Main service for batch creation, management, and processing
- **BatchStateMachine**: State machine for batch lifecycle (Pending -> Processing -> Completed/Failed)
- **SettlementWindowConfig**: Configurable settlement windows (real-time, micro-batch, hourly, daily)
- **BatchScheduler**: Background scheduler for automatic batch processing at cut-off times
- **Transaction Assignment**: Assign settled transactions to batches with automatic totals calculation
- **Batch Processing Pipeline**: Process batches with partial failure handling and completion notifications
- **Retry Support**: Failed batches can be retried after fixing issues

## Netting Engine

High-performance netting engine for reducing settlement volumes:

- **Bilateral Netting**: Calculate net positions between pairs of participants
  - Gross receivable/payable tracking per pair
  - Net amount and direction calculation
  - Settlement instruction generation
- **Multilateral Netting**: Calculate net positions across all participants
  - Aggregate positions from all transactions
  - Optimize for minimum settlement movements
  - Handle circular dependencies (100% netting efficiency)
- **Netting Reports**: Comprehensive reports with metrics
  - Gross volume, net volume, reduction amount
  - Netting efficiency percentage (target: 85%+)
  - Participant breakdown (net receivers, net payers, balanced)
- **Position Persistence**: Store and retrieve netting positions from database
- **Metrics Tracking**: Track batches processed, transactions netted, average efficiency

## Event System (Kafka Integration)

Distributed event streaming for settlement events using Apache Kafka:

- **EventProducer**: Kafka producer with retry logic and batch support
  - Configurable compression (None, Gzip, Snappy, LZ4, Zstd)
  - Automatic retry with exponential backoff
  - Batch message production for high throughput
- **EventConsumer**: Kafka consumer with offset management
  - MessageHandler trait for custom message processing
  - Dead letter queue (DLQ) for failed messages
  - Manual offset tracking for exactly-once semantics
- **Event Types**: Strongly-typed event payloads
  - `TransactionEvent`: Transaction lifecycle events
  - `BatchEvent`: Batch creation and completion events
  - `PositionEvent`: Netting position calculations
  - `NettingEvent`: Netting completion summaries
  - `SettlementEvent`: Final settlement confirmations
- **Topics**: Predefined topic structure
  - `settlement.transactions`: Transaction events
  - `settlement.batches`: Batch lifecycle events
  - `settlement.positions`: Netting position events
  - `settlement.completed`: Settlement completion events

## HTTP API

The settlement engine exposes a RESTful HTTP API built with Axum.

### Health Endpoints
- `GET /health` - Basic health check with service status
- `GET /health/detailed` - Detailed health check with dependency latencies
- `GET /ready` - Readiness probe for Kubernetes
- `GET /live` - Liveness probe for Kubernetes
- `GET /metrics` - Prometheus metrics endpoint

### Account Endpoints
- `POST /accounts` - Create a new account
- `GET /accounts/{id}` - Get account details
- `GET /accounts/{id}/balance` - Get account balance
- `GET /accounts/{id}/ledger` - Get ledger entries for account

### Transaction Endpoints
- `POST /transactions` - Create a new transaction
- `GET /transactions` - List transactions with filters
- `GET /transactions/{id}` - Get transaction details
- `POST /transactions/{id}/reverse` - Reverse a transaction

### Batch Endpoints
- `GET /batches` - List settlement batches
- `GET /batches/{id}` - Get batch details
- `POST /batches/{id}/process` - Trigger batch processing
- `GET /batches/{id}/positions` - Get netting positions for batch

### API Response Format
All responses follow a consistent format:
```json
{
  "success": true,
  "data": { ... },
  "error": null
}
```

Error responses include:
```json
{
  "success": false,
  "data": null,
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Request validation failed",
    "details": [...]
  }
}
```

## Observability

### Structured Logging
The application uses `tracing` for structured logging with configurable output formats:
- **Pretty**: Human-readable format for development
- **JSON**: Machine-parseable format for production
- **Compact**: Minimal format for high-throughput scenarios

Set the log format via environment variable: `LOG_FORMAT=json`

### Prometheus Metrics
Available at `GET /metrics`, includes:
- **Transaction metrics**: `settlement_transactions_total`, `settlement_transactions_settled_total`
- **Latency histograms**: `settlement_ledger_write_duration_ms`, `settlement_balance_query_duration_ms`
- **Batch metrics**: `settlement_batches_processed_total`, `settlement_batch_processing_duration_ms`
- **Netting metrics**: `settlement_netting_efficiency_ratio`, `settlement_netting_calculation_duration_ms`
- **HTTP metrics**: `http_requests_total`, `http_request_duration_ms`
- **Database metrics**: `db_queries_total`, `db_query_duration_ms`

### Health Checks
- `/health` - Basic health status
- `/health/detailed` - Detailed health with per-dependency latency and status
- `/ready` - Kubernetes readiness probe (checks DB connectivity)
- `/live` - Kubernetes liveness probe (always returns 200)

### Sensitive Data Masking
The logging module includes utilities for masking sensitive data:
- `mask_sensitive()` - Masks middle characters of strings
- `mask_uuid()` - Masks UUIDs showing only first/last 4 chars
- `mask_amount()` - Shows only magnitude (K+, M+) for amounts

## Development

- **Linting**: `cargo clippy`
- **Formatting**: `cargo fmt`
- **Testing**: `cargo test`
- **Check**: `cargo check`

## Database Migrations

Migrations are managed by `sqlx`.

```bash
# Install sqlx-cli
cargo install sqlx-cli

# Create a new migration
sqlx migrate add <name>

# Run migrations manually
sqlx migrate run
```

(Note: The application automatically runs pending migrations on startup).
