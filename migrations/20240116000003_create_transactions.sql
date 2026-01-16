-- Create Transactions table
CREATE TYPE transaction_type AS ENUM ('PAYMENT', 'REFUND', 'CHARGEBACK', 'TRANSFER', 'FEE');
CREATE TYPE transaction_status AS ENUM ('PENDING', 'SETTLED', 'FAILED', 'REVERSED');

CREATE TABLE transactions (
    id UUID PRIMARY KEY,
    external_id VARCHAR(255) NOT NULL,
    type transaction_type NOT NULL,
    status transaction_status NOT NULL DEFAULT 'PENDING',
    source_account_id UUID NOT NULL REFERENCES accounts(id),
    destination_account_id UUID NOT NULL REFERENCES accounts(id),
    amount DECIMAL(19, 4) NOT NULL,
    currency VARCHAR(3) NOT NULL,
    fee_amount DECIMAL(19, 4) NOT NULL DEFAULT 0,
    net_amount DECIMAL(19, 4) NOT NULL,
    settlement_batch_id UUID REFERENCES settlement_batches(id),
    idempotency_key VARCHAR(255) NOT NULL,
    metadata JSONB,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    settled_at TIMESTAMP WITH TIME ZONE
);

CREATE UNIQUE INDEX idx_transactions_external_id ON transactions(external_id);
CREATE UNIQUE INDEX idx_transactions_idempotency ON transactions(idempotency_key);
CREATE INDEX idx_transactions_batch ON transactions(settlement_batch_id);
CREATE INDEX idx_transactions_source ON transactions(source_account_id);
CREATE INDEX idx_transactions_dest ON transactions(destination_account_id);
