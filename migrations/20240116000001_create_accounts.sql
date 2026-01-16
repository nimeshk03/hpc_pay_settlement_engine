-- Create Accounts table
CREATE TYPE account_type AS ENUM ('ASSET', 'LIABILITY', 'REVENUE', 'EXPENSE');
CREATE TYPE account_status AS ENUM ('ACTIVE', 'FROZEN', 'CLOSED');

CREATE TABLE accounts (
    id UUID PRIMARY KEY,
    external_id VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    type account_type NOT NULL,
    status account_status NOT NULL DEFAULT 'ACTIVE',
    currency VARCHAR(3) NOT NULL,
    metadata JSONB,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_accounts_external_id ON accounts(external_id);
