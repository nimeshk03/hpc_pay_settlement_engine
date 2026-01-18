-- Create idempotency_keys table for duplicate request detection
CREATE TABLE IF NOT EXISTS idempotency_keys (
    id UUID PRIMARY KEY,
    idempotency_key VARCHAR(255) NOT NULL UNIQUE,
    client_id VARCHAR(255) NOT NULL,
    operation_type VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'PROCESSING',
    request_hash VARCHAR(64) NOT NULL,
    response_data JSONB,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    
    CONSTRAINT valid_status CHECK (status IN ('PROCESSING', 'COMPLETED', 'FAILED'))
);

-- Index for fast lookups by idempotency key
CREATE INDEX IF NOT EXISTS idx_idempotency_keys_key ON idempotency_keys(idempotency_key);

-- Index for cleanup of expired records
CREATE INDEX IF NOT EXISTS idx_idempotency_keys_expires_at ON idempotency_keys(expires_at);

-- Index for client-based queries
CREATE INDEX IF NOT EXISTS idx_idempotency_keys_client_id ON idempotency_keys(client_id);

-- Index for status-based queries
CREATE INDEX IF NOT EXISTS idx_idempotency_keys_status ON idempotency_keys(status);
