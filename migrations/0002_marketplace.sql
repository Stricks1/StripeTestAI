ALTER TABLE users
ADD COLUMN role TEXT NOT NULL DEFAULT 'customer'
CHECK (role IN ('admin', 'customer', 'provider'));

ALTER TABLE users
ADD COLUMN last_login_at INTEGER;

ALTER TABLE users
ADD COLUMN provider_unseen_since INTEGER;

UPDATE users
SET role = 'admin'
WHERE is_admin = 1;

ALTER TABLE products
ADD COLUMN provider_user_id INTEGER REFERENCES users(id) ON DELETE RESTRICT;

ALTER TABLE products
ADD COLUMN visible INTEGER NOT NULL DEFAULT 1 CHECK (visible IN (0, 1));

ALTER TABLE products
ADD COLUMN stock_quantity INTEGER NOT NULL DEFAULT 0 CHECK (stock_quantity >= 0);

ALTER TABLE products
ADD COLUMN reserved_quantity INTEGER NOT NULL DEFAULT 0 CHECK (reserved_quantity >= 0);

UPDATE products
SET visible = 0
WHERE provider_user_id IS NULL;

CREATE INDEX products_provider_visibility
ON products(provider_user_id, visible, active);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    customer_user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    checkout_key TEXT NOT NULL,
    total_amount_cents INTEGER NOT NULL CHECK (total_amount_cents > 0),
    currency TEXT NOT NULL DEFAULT 'usd',
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'paid', 'failed', 'canceled', 'partially_refunded', 'refunded')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    paid_at INTEGER
);

CREATE INDEX orders_customer_created
ON orders(customer_user_id, created_at);

CREATE INDEX orders_status_created
ON orders(status, created_at);

CREATE UNIQUE INDEX orders_customer_checkout_key
ON orders(customer_user_id, checkout_key);

CREATE TABLE order_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
    product_id INTEGER NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    provider_user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    product_name TEXT NOT NULL,
    product_sku TEXT NOT NULL,
    unit_price_cents INTEGER NOT NULL CHECK (unit_price_cents > 0),
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    line_total_cents INTEGER NOT NULL CHECK (line_total_cents = unit_price_cents * quantity)
);

CREATE INDEX order_items_order ON order_items(order_id);
CREATE INDEX order_items_provider ON order_items(provider_user_id, order_id);
CREATE INDEX order_items_product ON order_items(product_id, order_id);

CREATE TABLE inventory_reservations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
    product_id INTEGER NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    expires_at INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'consumed', 'released')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (order_id, product_id)
);

CREATE INDEX inventory_reservations_expiry
ON inventory_reservations(status, expires_at);

CREATE TABLE stripe_payments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id INTEGER NOT NULL UNIQUE REFERENCES orders(id) ON DELETE RESTRICT,
    stripe_payment_intent_id TEXT NOT NULL UNIQUE,
    stripe_charge_id TEXT UNIQUE,
    stripe_balance_transaction_id TEXT,
    amount_cents INTEGER NOT NULL CHECK (amount_cents > 0),
    currency TEXT NOT NULL DEFAULT 'usd',
    status TEXT NOT NULL DEFAULT 'pending',
    stripe_fee_cents INTEGER CHECK (stripe_fee_cents IS NULL OR stripe_fee_cents >= 0),
    refunded_amount_cents INTEGER NOT NULL DEFAULT 0 CHECK (refunded_amount_cents >= 0),
    estimated_fee_cents INTEGER NOT NULL CHECK (estimated_fee_cents >= 0),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE order_provider_totals (
    order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
    provider_user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    gross_amount_cents INTEGER NOT NULL CHECK (gross_amount_cents > 0),
    stripe_fee_cents INTEGER NOT NULL DEFAULT 0 CHECK (stripe_fee_cents >= 0),
    platform_commission_cents INTEGER NOT NULL DEFAULT 0 CHECK (platform_commission_cents >= 0),
    provider_net_cents INTEGER NOT NULL CHECK (provider_net_cents >= 0),
    PRIMARY KEY (order_id, provider_user_id)
);

CREATE INDEX order_provider_totals_provider
ON order_provider_totals(provider_user_id, order_id);

CREATE TABLE stripe_webhook_events (
    event_id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    processed_at INTEGER NOT NULL DEFAULT (unixepoch())
);
