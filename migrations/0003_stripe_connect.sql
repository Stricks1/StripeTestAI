CREATE TABLE provider_stripe_accounts (
    provider_user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE RESTRICT,
    stripe_account_id TEXT NOT NULL UNIQUE,
    details_submitted INTEGER NOT NULL DEFAULT 0 CHECK (details_submitted IN (0, 1)),
    transfers_enabled INTEGER NOT NULL DEFAULT 0 CHECK (transfers_enabled IN (0, 1)),
    payouts_enabled INTEGER NOT NULL DEFAULT 0 CHECK (payouts_enabled IN (0, 1)),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

ALTER TABLE order_provider_totals
ADD COLUMN stripe_account_id TEXT;

ALTER TABLE order_provider_totals
ADD COLUMN stripe_transfer_id TEXT;

ALTER TABLE order_provider_totals
ADD COLUMN stripe_transfer_reversed_cents INTEGER NOT NULL DEFAULT 0
CHECK (stripe_transfer_reversed_cents >= 0);

CREATE UNIQUE INDEX order_provider_totals_stripe_transfer
ON order_provider_totals(stripe_transfer_id)
WHERE stripe_transfer_id IS NOT NULL;

CREATE TABLE provider_transfer_reversals (
    order_id INTEGER NOT NULL,
    provider_user_id INTEGER NOT NULL,
    target_reversed_cents INTEGER NOT NULL CHECK (target_reversed_cents > 0),
    amount_cents INTEGER NOT NULL CHECK (amount_cents > 0),
    stripe_reversal_id TEXT UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'reversed')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (order_id, provider_user_id, target_reversed_cents),
    FOREIGN KEY (order_id, provider_user_id)
        REFERENCES order_provider_totals(order_id, provider_user_id) ON DELETE RESTRICT
);
