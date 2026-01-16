-- Create Settlement Batches table
CREATE TYPE batch_status AS ENUM ('PENDING', 'PROCESSING', 'COMPLETED', 'FAILED');

CREATE TABLE settlement_batches (
    id UUID PRIMARY KEY,
    status batch_status NOT NULL DEFAULT 'PENDING',
    settlement_date DATE NOT NULL,
    cut_off_time TIMESTAMP WITH TIME ZONE NOT NULL,
    total_transactions INTEGER NOT NULL DEFAULT 0,
    gross_amount DECIMAL(19, 4) NOT NULL DEFAULT 0,
    net_amount DECIMAL(19, 4) NOT NULL DEFAULT 0,
    fee_amount DECIMAL(19, 4) NOT NULL DEFAULT 0,
    currency VARCHAR(3) NOT NULL,
    metadata JSONB,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMP WITH TIME ZONE
);

CREATE INDEX idx_batches_status ON settlement_batches(status);
CREATE INDEX idx_batches_date ON settlement_batches(settlement_date);
