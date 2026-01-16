-- Create Ledger Entries table (Partitioned)
CREATE TYPE entry_type AS ENUM ('DEBIT', 'CREDIT');

CREATE TABLE ledger_entries (
    id UUID NOT NULL,
    transaction_id UUID NOT NULL REFERENCES transactions(id),
    account_id UUID NOT NULL REFERENCES accounts(id),
    entry_type entry_type NOT NULL,
    amount DECIMAL(19, 4) NOT NULL,
    currency VARCHAR(3) NOT NULL,
    balance_after DECIMAL(19, 4) NOT NULL,
    effective_date DATE NOT NULL,
    metadata JSONB,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

-- Create initial partition for current month and next month
CREATE TABLE ledger_entries_default PARTITION OF ledger_entries DEFAULT;

-- Indexes (must include partition key if unique, but here we just need search indexes)
CREATE INDEX idx_ledger_account_date ON ledger_entries(account_id, created_at);
CREATE INDEX idx_ledger_transaction ON ledger_entries(transaction_id);
