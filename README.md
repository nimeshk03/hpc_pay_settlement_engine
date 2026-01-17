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
