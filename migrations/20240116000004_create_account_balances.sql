-- Create Account Balances table
CREATE TABLE account_balances (
    account_id UUID NOT NULL REFERENCES accounts(id),
    currency VARCHAR(3) NOT NULL,
    available_balance DECIMAL(19, 4) NOT NULL DEFAULT 0,
    pending_balance DECIMAL(19, 4) NOT NULL DEFAULT 0,
    reserved_balance DECIMAL(19, 4) NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 1,
    last_updated TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    PRIMARY KEY (account_id, currency)
);

CREATE UNIQUE INDEX idx_balance_account_currency ON account_balances(account_id, currency);
