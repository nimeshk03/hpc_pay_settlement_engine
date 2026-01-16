-- Create Netting Positions table
CREATE TABLE netting_positions (
    batch_id UUID NOT NULL REFERENCES settlement_batches(id),
    participant_id UUID NOT NULL REFERENCES accounts(id),
    currency VARCHAR(3) NOT NULL,
    gross_receivable DECIMAL(19, 4) NOT NULL DEFAULT 0,
    gross_payable DECIMAL(19, 4) NOT NULL DEFAULT 0,
    net_position DECIMAL(19, 4) NOT NULL DEFAULT 0,
    transaction_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    PRIMARY KEY (batch_id, participant_id, currency)
);

CREATE INDEX idx_netting_batch ON netting_positions(batch_id);
CREATE INDEX idx_netting_participant ON netting_positions(participant_id);
