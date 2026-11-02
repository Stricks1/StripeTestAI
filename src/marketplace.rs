use crate::{auth::require_role, error::AppError, state::AppState};
use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::{FromRow, Row, Sqlite, Transaction};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower_sessions::Session;
use uuid::Uuid;

const MIN_PRODUCT_PRICE_CENTS: i64 = 50;
const MAX_PRODUCT_PRICE_CENTS: i64 = 50_000;
const MAX_ORDER_AMOUNT_CENTS: i64 = 500_000;
const MAX_CART_LINES: usize = 50;
const MAX_ITEM_QUANTITY: i64 = 1_000;
const INVENTORY_RESERVATION_SECONDS: i64 = 30 * 60;
const PLATFORM_ALLOCATION_BPS: i64 = 1_000;
const STRIPE_ESTIMATED_RATE_BPS: i64 = 399;
const STRIPE_ESTIMATED_FIXED_FEE_CENTS: i64 = 39;
const WEBHOOK_TOLERANCE_SECONDS: i64 = 5 * 60;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Deserialize)]
pub struct ProductInput {
    name: String,
    description: String,
    price_cents: i64,
    stock_quantity: i64,
}

#[derive(Debug, Deserialize)]
pub struct ProductSearch {
    q: Option<String>,
    min_price_cents: Option<i64>,
    max_price_cents: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateOrderInput {
    checkout_key: String,
    items: Vec<CartItem>,
}

#[derive(Debug, Deserialize)]
pub struct CartItem {
    product_id: i64,
    quantity: i64,
}

#[derive(Debug, Deserialize)]
pub struct AdminDashboardQuery {
    days: Option<i64>,
    product_id: Option<i64>,
    provider_id: Option<i64>,
}

#[derive(Debug, FromRow, Serialize)]
struct AdminProviderOption {
    id: i64,
    username: String,
}

#[derive(Debug, FromRow, Serialize)]
struct AdminProductOption {
    id: i64,
    name: String,
    provider_name: String,
}

pub async fn store_page(
    State(state): State<AppState>,
    session: Session,
) -> Result<Html<&'static str>, AppError> {
    require_role(&session, &state.pool, "customer").await?;
    Ok(Html(include_str!("../public/store.html")))
}

pub async fn provider_page(
    State(state): State<AppState>,
    session: Session,
) -> Result<Html<&'static str>, AppError> {
    require_role(&session, &state.pool, "provider").await?;
    Ok(Html(include_str!("../public/provider.html")))
}

pub async fn admin_page(
    State(state): State<AppState>,
    session: Session,
) -> Result<Html<&'static str>, AppError> {
    require_role(&session, &state.pool, "admin").await?;
    Ok(Html(include_str!("../public/admin.html")))
}

pub async fn checkout_page(
    State(state): State<AppState>,
    session: Session,
) -> Result<Html<&'static str>, AppError> {
    require_role(&session, &state.pool, "customer").await?;
    Ok(Html(include_str!("../public/checkout.html")))
}

#[derive(Debug, FromRow, Serialize)]
struct ProductView {
    id: i64,
    sku: String,
    name: String,
    description: String,
    price_cents: i64,
    stock_available: i64,
    provider_id: i64,
    provider_name: String,
}

#[derive(Debug, FromRow, Serialize)]
struct ProviderProductView {
    id: i64,
    sku: String,
    name: String,
    description: String,
    price_cents: i64,
    stock_quantity: i64,
    reserved_quantity: i64,
    visible: bool,
}

#[derive(Debug, FromRow)]
struct CheckoutProduct {
    id: i64,
    sku: String,
    name: String,
    base_price_cents: i64,
    stock_quantity: i64,
    reserved_quantity: i64,
    provider_user_id: i64,
}

#[derive(Debug, FromRow)]
struct ProviderGross {
    provider_user_id: i64,
    gross_amount_cents: i64,
}

#[derive(Debug, FromRow)]
struct OrderPayment {
    id: i64,
    stripe_payment_intent_id: String,
    amount_cents: i64,
    currency: String,
}

#[derive(Debug, FromRow)]
struct ReservationRow {
    product_id: i64,
    quantity: i64,
}

#[derive(Debug, FromRow)]
struct ExpiredOrder {
    id: i64,
}

#[derive(Debug, FromRow, Serialize)]
struct SalesSeriesPoint {
    day: String,
    gross_amount_cents: i64,
}

#[derive(Debug, FromRow, Serialize)]
struct ProviderProductSales {
    product_id: i64,
    product_name: String,
    units_sold: i64,
    gross_amount_cents: i64,
}

#[derive(Debug, FromRow, Serialize)]
struct ProviderRecentOrder {
    order_id: i64,
    paid_at: i64,
    item_count: i64,
    gross_amount_cents: i64,
    new_since_last_visit: bool,
}

#[derive(Debug, FromRow, Serialize)]
struct AdminProductSales {
    product_id: i64,
    product_name: String,
    provider_id: i64,
    provider_name: String,
    units_sold: i64,
    gross_amount_cents: i64,
}

#[derive(Debug, FromRow, Serialize)]
struct AdminProviderSales {
    provider_id: i64,
    provider_name: String,
    products_sold: i64,
    units_sold: i64,
    gross_amount_cents: i64,
    platform_commission_cents: i64,
}

#[derive(Debug, FromRow)]
struct OrderForStripe {
    id: i64,
    total_amount_cents: i64,
    currency: String,
    status: String,
}

#[derive(Debug, FromRow)]
struct WebhookPaymentRecord {
    id: i64,
    order_id: i64,
    amount_cents: i64,
    currency: String,
    stripe_charge_id: Option<String>,
    refunded_amount_cents: i64,
}

pub async fn customer_products(
    State(state): State<AppState>,
    session: Session,
    Query(search): Query<ProductSearch>,
) -> Result<impl IntoResponse, AppError> {
    require_role(&session, &state.pool, "customer").await?;
    let query = search.q.as_deref().unwrap_or("").trim();
    if query.len() > 100 {
        return Err(AppError::bad_request(
            "Product search must be 100 characters or fewer.",
        ));
    }
    if search.min_price_cents.is_some_and(|value| value < 0)
        || search.max_price_cents.is_some_and(|value| value < 0)
        || matches!(
            (search.min_price_cents, search.max_price_cents),
            (Some(minimum), Some(maximum)) if minimum > maximum
        )
    {
        return Err(AppError::bad_request("Enter a valid product price range."));
    }
    let pattern = format!("%{}%", escape_like(query));
    let products = sqlx::query_as::<_, ProductView>(
        "SELECT p.id, p.sku, p.name, p.description,
                p.base_price_cents AS price_cents,
                p.stock_quantity - p.reserved_quantity AS stock_available,
                p.provider_user_id AS provider_id,
                u.username AS provider_name
         FROM products p
         JOIN users u ON u.id = p.provider_user_id AND u.role = 'provider'
         WHERE p.visible = 1 AND p.active = 1
           AND p.stock_quantity > p.reserved_quantity
           AND p.name LIKE ? ESCAPE '\\' COLLATE NOCASE
           AND (? IS NULL OR p.base_price_cents >= ?)
           AND (? IS NULL OR p.base_price_cents <= ?)
         ORDER BY p.created_at DESC, p.id DESC
         LIMIT 200",
    )
    .bind(pattern)
    .bind(search.min_price_cents)
    .bind(search.min_price_cents)
    .bind(search.max_price_cents)
    .bind(search.max_price_cents)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(products))
}

pub async fn provider_products(
    State(state): State<AppState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    let provider_id = require_role(&session, &state.pool, "provider").await?;
    let products = sqlx::query_as::<_, ProviderProductView>(
        "SELECT id, sku, name, description, base_price_cents AS price_cents,
                stock_quantity, reserved_quantity, visible
         FROM products
         WHERE provider_user_id = ?
         ORDER BY created_at DESC, id DESC",
    )
    .bind(provider_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(products))
}

pub async fn create_product(
    State(state): State<AppState>,
    session: Session,
    Json(input): Json<ProductInput>,
) -> Result<impl IntoResponse, AppError> {
    let provider_id = require_role(&session, &state.pool, "provider").await?;
    validate_product_input(&input)?;
    let sku = format!("PRD-{}", Uuid::new_v4().simple());
    let result = sqlx::query(
        "INSERT INTO products
            (sku, name, description, base_price_cents, provider_user_id,
             visible, active, stock_quantity)
         VALUES (?, ?, ?, ?, ?, 1, 1, ?)",
    )
    .bind(sku)
    .bind(input.name.trim())
    .bind(input.description.trim())
    .bind(input.price_cents)
    .bind(provider_id)
    .bind(input.stock_quantity)
    .execute(&state.pool)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "product_id": result.last_insert_rowid() })),
    ))
}

pub async fn update_product(
    State(state): State<AppState>,
    session: Session,
    Path(product_id): Path<i64>,
    Json(input): Json<ProductInput>,
) -> Result<impl IntoResponse, AppError> {
    let provider_id = require_role(&session, &state.pool, "provider").await?;
    validate_product_input(&input)?;
    let result = sqlx::query(
        "UPDATE products
         SET name = ?, description = ?, base_price_cents = ?, stock_quantity = ?
         WHERE id = ? AND provider_user_id = ? AND stock_quantity - reserved_quantity >= 0
           AND ? >= reserved_quantity",
    )
    .bind(input.name.trim())
    .bind(input.description.trim())
    .bind(input.price_cents)
    .bind(input.stock_quantity)
    .bind(product_id)
    .bind(provider_id)
    .bind(input.stock_quantity)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "The product was not found or its stock is reserved by an active checkout.",
        ));
    }
    Ok(Json(json!({ "message": "Product updated." })))
}

pub async fn archive_product(
    State(state): State<AppState>,
    session: Session,
    Path(product_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let provider_id = require_role(&session, &state.pool, "provider").await?;
    let result = sqlx::query(
        "UPDATE products SET visible = 0, active = 0
         WHERE id = ? AND provider_user_id = ?",
    )
    .bind(product_id)
    .bind(provider_id)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            "Product was not found.",
        ));
    }
    Ok(Json(json!({
        "message": "Product archived. Historical orders and Stripe records are preserved."
    })))
}

pub async fn provider_dashboard(
    State(state): State<AppState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    let provider_id = require_role(&session, &state.pool, "provider").await?;
    let mut transaction = state.pool.begin().await?;
    let cutoff: Option<i64> = sqlx::query_scalar(
        "SELECT provider_unseen_since FROM users WHERE id = ? AND role = 'provider'",
    )
    .bind(provider_id)
    .fetch_one(&mut *transaction)
    .await?;
    let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&mut *transaction)
        .await?;
    let cutoff = cutoff.unwrap_or(now);
    let unseen_orders: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT o.id)
         FROM orders o
         JOIN order_items i ON i.order_id = o.id
         WHERE i.provider_user_id = ? AND o.status IN ('paid', 'partially_refunded', 'refunded')
           AND o.paid_at >= ? AND o.paid_at < ?",
    )
    .bind(provider_id)
    .bind(cutoff)
    .bind(now)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query("UPDATE users SET provider_unseen_since = ? WHERE id = ?")
        .bind(now)
        .bind(provider_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    let gross_sales_cents: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(opt.gross_amount_cents), 0)
         FROM order_provider_totals opt
         JOIN orders o ON o.id = opt.order_id
         WHERE opt.provider_user_id = ? AND o.status IN ('paid', 'partially_refunded', 'refunded')",
    )
    .bind(provider_id)
    .fetch_one(&state.pool)
    .await?;
    let live_products: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM products
         WHERE provider_user_id = ? AND visible = 1 AND active = 1",
    )
    .bind(provider_id)
    .fetch_one(&state.pool)
    .await?;
    let series = sqlx::query_as::<_, SalesSeriesPoint>(
        "SELECT strftime('%Y-%m-%d', o.paid_at, 'unixepoch') AS day,
                COALESCE(SUM(i.line_total_cents), 0) AS gross_amount_cents
         FROM orders o
         JOIN order_items i ON i.order_id = o.id
         WHERE i.provider_user_id = ? AND o.status IN ('paid', 'partially_refunded', 'refunded')
           AND o.paid_at >= unixepoch() - 2592000
         GROUP BY day ORDER BY day",
    )
    .bind(provider_id)
    .fetch_all(&state.pool)
    .await?;
    let products = sqlx::query_as::<_, ProviderProductSales>(
        "SELECT p.id AS product_id, p.name AS product_name,
                COALESCE(SUM(CASE WHEN o.id IS NOT NULL THEN i.quantity ELSE 0 END), 0) AS units_sold,
                COALESCE(SUM(CASE WHEN o.id IS NOT NULL THEN i.line_total_cents ELSE 0 END), 0) AS gross_amount_cents
         FROM products p
         LEFT JOIN order_items i ON i.product_id = p.id AND i.provider_user_id = p.provider_user_id
         LEFT JOIN orders o ON o.id = i.order_id
             AND o.status IN ('paid', 'partially_refunded', 'refunded')
         WHERE p.provider_user_id = ?
         GROUP BY p.id, p.name
         ORDER BY gross_amount_cents DESC, p.name COLLATE NOCASE",
    )
    .bind(provider_id)
    .fetch_all(&state.pool)
    .await?;
    let recent_orders = sqlx::query_as::<_, ProviderRecentOrder>(
        "SELECT o.id AS order_id, o.paid_at,
                COALESCE(SUM(i.quantity), 0) AS item_count,
                SUM(i.line_total_cents) AS gross_amount_cents,
                (o.paid_at >= ? AND o.paid_at < ?) AS new_since_last_visit
         FROM orders o
         JOIN order_items i ON i.order_id = o.id
         WHERE i.provider_user_id = ? AND o.status IN ('paid', 'partially_refunded', 'refunded')
         GROUP BY o.id ORDER BY o.paid_at DESC LIMIT 50",
    )
    .bind(cutoff)
    .bind(now)
    .bind(provider_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!({
        "gross_sales_cents": gross_sales_cents,
        "live_products": live_products,
        "unseen_purchase_count": unseen_orders,
        "sales_series": series,
        "products": products,
        "recent_orders": recent_orders,
        "accounting_note": "Sales are gross. Stripe Connect and provider payouts are not enabled in this version."
    })))
}

pub async fn create_order(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    session: Session,
    Json(input): Json<CreateOrderInput>,
) -> Result<impl IntoResponse, AppError> {
    let customer_id = require_role(&session, &state.pool, "customer").await?;
    stripe_test_keys(&state)?;
    if !state
        .stripe_webhook_secret
        .as_deref()
        .is_some_and(|secret| secret.starts_with("whsec_"))
    {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Configure a Stripe webhook signing secret before accepting checkout.",
        ));
    }
    if let Err(retry_after_secs) = state.payment_rate_limiter.check_ip(peer.ip()).await {
        return Err(AppError::too_many_requests(retry_after_secs));
    }
    if let Err(retry_after_secs) = state.payment_rate_limiter.check_user(customer_id).await {
        return Err(AppError::too_many_requests(retry_after_secs));
    }
    let checkout_key = Uuid::parse_str(&input.checkout_key)
        .map_err(|_| AppError::bad_request("The checkout key is invalid."))?
        .to_string();
    let cart = normalize_cart(input.items)?;
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM orders WHERE customer_user_id = ? AND checkout_key = ?")
            .bind(customer_id)
            .bind(&checkout_key)
            .fetch_optional(&state.pool)
            .await?;
    let order_id = match existing {
        Some(order_id) => order_id,
        None => reserve_order(&state, customer_id, &checkout_key, &cart).await?,
    };
    let payment = ensure_order_payment(&state, order_id).await?;
    state
        .payment_rate_limiter
        .record_small_order(customer_id, order_id, payment.amount_cents)
        .await;
    Ok(Json(json!({
        "order_id": order_id,
        "payment_id": payment.id,
        "client_secret": payment.client_secret,
        "publishable_key": payment.publishable_key,
        "amount_cents": payment.amount_cents,
        "currency": "usd",
        "reservation_expires_in_seconds": INVENTORY_RESERVATION_SECONDS
    })))
}

pub async fn order_status(
    State(state): State<AppState>,
    session: Session,
    Path(order_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let customer_id = require_role(&session, &state.pool, "customer").await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM orders WHERE id = ? AND customer_user_id = ?")
            .bind(order_id)
            .bind(customer_id)
            .fetch_optional(&state.pool)
            .await?;
    let Some(status) = status else {
        return Err(AppError::new(StatusCode::NOT_FOUND, "Order was not found."));
    };
    let items = sqlx::query(
        "SELECT product_name, quantity, unit_price_cents, line_total_cents
         FROM order_items WHERE order_id = ? ORDER BY id",
    )
    .bind(order_id)
    .fetch_all(&state.pool)
    .await?;
    let items = items
        .iter()
        .map(|row| {
            Ok(json!({
                "product_name": row.try_get::<String, _>("product_name")?,
                "quantity": row.try_get::<i64, _>("quantity")?,
                "unit_price_cents": row.try_get::<i64, _>("unit_price_cents")?,
                "line_total_cents": row.try_get::<i64, _>("line_total_cents")?
            }))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    let total_amount_cents: i64 =
        sqlx::query_scalar("SELECT total_amount_cents FROM orders WHERE id = ?")
            .bind(order_id)
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(json!({
        "order_id": order_id,
        "status": status,
        "total_amount_cents": total_amount_cents,
        "currency": "usd",
        "items": items
    })))
}

pub async fn admin_dashboard(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<AdminDashboardQuery>,
) -> Result<impl IntoResponse, AppError> {
    require_role(&session, &state.pool, "admin").await?;
    let days = query.days.unwrap_or(30);
    if !(1..=365).contains(&days)
        || query.product_id.is_some_and(|value| value < 1)
        || query.provider_id.is_some_and(|value| value < 1)
    {
        return Err(AppError::bad_request("Dashboard filters are invalid."));
    }
    let cutoff = days * 86_400;
    let (
        gross_sales_cents,
        actual_fees_cents,
        estimated_fees_cents,
        total_refunds_cents,
        commission_cents,
    ): (i64, i64, i64, i64, i64) = sqlx::query_as(
        "WITH filtered_items AS (
             SELECT i.order_id, i.provider_user_id, i.product_id, i.line_total_cents
             FROM order_items i
             JOIN orders o ON o.id = i.order_id
             WHERE o.status IN ('paid', 'partially_refunded', 'refunded')
               AND o.paid_at >= unixepoch() - ?
               AND (? IS NULL OR i.product_id = ?)
               AND (? IS NULL OR i.provider_user_id = ?)
         ),
         scoped_order_gross AS (
             SELECT order_id, SUM(line_total_cents) AS gross_amount_cents
             FROM filtered_items GROUP BY order_id
         ),
         scoped_provider_orders AS (
             SELECT provider_user_id, order_id, SUM(line_total_cents) AS gross_amount_cents
             FROM filtered_items GROUP BY provider_user_id, order_id
         ),
         financials AS (
             SELECT SUM(s.gross_amount_cents) AS gross_sales_cents,
                    SUM(CASE WHEN sp.stripe_fee_cents IS NULL THEN 0
                        WHEN s.gross_amount_cents >= sp.amount_cents THEN sp.stripe_fee_cents
                        ELSE (sp.stripe_fee_cents * s.gross_amount_cents + sp.amount_cents / 2)
                             / sp.amount_cents END) AS actual_fees_cents,
                    SUM(CASE WHEN sp.stripe_fee_cents IS NOT NULL THEN 0
                        WHEN s.gross_amount_cents >= sp.amount_cents THEN sp.estimated_fee_cents
                        ELSE (sp.estimated_fee_cents * s.gross_amount_cents + sp.amount_cents / 2)
                             / sp.amount_cents END) AS estimated_fees_cents,
                    SUM(CASE WHEN sp.id IS NULL THEN 0
                        WHEN s.gross_amount_cents >= sp.amount_cents THEN sp.refunded_amount_cents
                        ELSE (sp.refunded_amount_cents * s.gross_amount_cents + sp.amount_cents / 2)
                             / sp.amount_cents END) AS total_refunds_cents
             FROM scoped_order_gross s
             LEFT JOIN stripe_payments sp ON sp.order_id = s.order_id
         ),
         commissions AS (
             SELECT SUM(CASE
                 WHEN f.gross_amount_cents >= opt.gross_amount_cents
                     THEN opt.platform_commission_cents
                 ELSE (f.gross_amount_cents * opt.platform_commission_cents
                       + opt.gross_amount_cents / 2) / opt.gross_amount_cents
                 END) AS commission_cents
             FROM scoped_provider_orders f
             JOIN order_provider_totals opt
               ON opt.order_id = f.order_id AND opt.provider_user_id = f.provider_user_id
         )
         SELECT COALESCE(f.gross_sales_cents, 0),
                COALESCE(f.actual_fees_cents, 0),
                COALESCE(f.estimated_fees_cents, 0),
                COALESCE(f.total_refunds_cents, 0),
                COALESCE(c.commission_cents, 0)
         FROM financials f CROSS JOIN commissions c",
    )
    .bind(cutoff)
    .bind(query.product_id)
    .bind(query.product_id)
    .bind(query.provider_id)
    .bind(query.provider_id)
    .fetch_one(&state.pool)
    .await?;
    let product_sales = sqlx::query_as::<_, AdminProductSales>(
        "SELECT p.id AS product_id, i.product_name,
                i.provider_user_id AS provider_id, u.username AS provider_name,
                SUM(i.quantity) AS units_sold, SUM(i.line_total_cents) AS gross_amount_cents
         FROM order_items i
         JOIN orders o ON o.id = i.order_id
         JOIN users u ON u.id = i.provider_user_id
         JOIN products p ON p.id = i.product_id
         WHERE o.status IN ('paid', 'partially_refunded', 'refunded')
           AND o.paid_at >= unixepoch() - ?
           AND (? IS NULL OR p.id = ?)
           AND (? IS NULL OR i.provider_user_id = ?)
         GROUP BY p.id, i.product_name, i.provider_user_id, u.username
         ORDER BY gross_amount_cents DESC LIMIT 200",
    )
    .bind(cutoff)
    .bind(query.product_id)
    .bind(query.product_id)
    .bind(query.provider_id)
    .bind(query.provider_id)
    .fetch_all(&state.pool)
    .await?;
    let provider_sales = sqlx::query_as::<_, AdminProviderSales>(
        "WITH filtered_items AS (
             SELECT i.provider_user_id, i.order_id, i.product_id, i.quantity, i.line_total_cents
             FROM order_items i
             JOIN orders o ON o.id = i.order_id
             WHERE o.status IN ('paid', 'partially_refunded', 'refunded')
               AND o.paid_at >= unixepoch() - ?
               AND (? IS NULL OR i.product_id = ?)
               AND (? IS NULL OR i.provider_user_id = ?)
         ),
         sales_by_provider AS (
             SELECT provider_user_id, COUNT(DISTINCT product_id) AS products_sold,
                    SUM(quantity) AS units_sold, SUM(line_total_cents) AS gross_amount_cents
             FROM filtered_items
             GROUP BY provider_user_id
         ),
         commission_by_provider AS (
             SELECT f.provider_user_id,
                    SUM(CASE
                        WHEN f.gross_amount_cents >= opt.gross_amount_cents
                            THEN opt.platform_commission_cents
                        ELSE (f.gross_amount_cents * opt.platform_commission_cents
                              + opt.gross_amount_cents / 2) / opt.gross_amount_cents
                    END) AS platform_commission_cents
             FROM (
                 SELECT provider_user_id, order_id, SUM(line_total_cents) AS gross_amount_cents
                 FROM filtered_items
                 GROUP BY provider_user_id, order_id
             ) f
             JOIN order_provider_totals opt
               ON opt.order_id = f.order_id AND opt.provider_user_id = f.provider_user_id
             GROUP BY f.provider_user_id
         )
         SELECT s.provider_user_id AS provider_id, u.username AS provider_name,
                s.products_sold, s.units_sold, s.gross_amount_cents,
                COALESCE(c.platform_commission_cents, 0) AS platform_commission_cents
         FROM sales_by_provider s
         JOIN users u ON u.id = s.provider_user_id
         LEFT JOIN commission_by_provider c ON c.provider_user_id = s.provider_user_id
         ORDER BY s.gross_amount_cents DESC LIMIT 200",
    )
    .bind(cutoff)
    .bind(query.product_id)
    .bind(query.product_id)
    .bind(query.provider_id)
    .bind(query.provider_id)
    .fetch_all(&state.pool)
    .await?;
    let series = sqlx::query_as::<_, SalesSeriesPoint>(
        "SELECT strftime('%Y-%m-%d', o.paid_at, 'unixepoch') AS day,
                COALESCE(SUM(i.line_total_cents), 0) AS gross_amount_cents
         FROM orders o
         JOIN order_items i ON i.order_id = o.id
         WHERE o.status IN ('paid', 'partially_refunded', 'refunded')
           AND o.paid_at >= unixepoch() - ?
           AND (? IS NULL OR i.product_id = ?)
           AND (? IS NULL OR i.provider_user_id = ?)
         GROUP BY day ORDER BY day",
    )
    .bind(cutoff)
    .bind(query.product_id)
    .bind(query.product_id)
    .bind(query.provider_id)
    .bind(query.provider_id)
    .fetch_all(&state.pool)
    .await?;
    let (stripe_balance, stripe_balance_error) = match fetch_stripe_balance(&state).await {
        Ok(Some(balance)) => (Some(balance), None),
        Ok(None) => (
            None,
            Some("Stripe test keys are not configured.".to_owned()),
        ),
        Err(error) => (None, Some(error.message)),
    };
    let net_after_fee_and_refunds_cents =
        gross_sales_cents - actual_fees_cents - estimated_fees_cents - total_refunds_cents;
    Ok(Json(json!({
        "period_days": days,
        "summary": {
            "gross_sales_cents": gross_sales_cents,
            "actual_stripe_fees_cents": actual_fees_cents,
            "estimated_stripe_fees_cents": estimated_fees_cents,
            "refunds_cents": total_refunds_cents,
            "platform_commission_cents": commission_cents,
            "net_after_fees_and_refunds_cents": net_after_fee_and_refunds_cents
        },
        "stripe_balance": stripe_balance,
        "stripe_balance_error": stripe_balance_error,
        "balance_note": "Stripe balance is the current platform test-account balance and is not limited to this report period.",
        "sales_series": series,
        "products": product_sales,
        "providers": provider_sales,
        "commission_policy": "The platform allocation is capped at 10% of gross. Actual Stripe fees are counted first; the platform absorbs fees above that cap."
    })))
}

pub async fn admin_filter_options(
    State(state): State<AppState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    require_role(&session, &state.pool, "admin").await?;
    let providers = sqlx::query_as::<_, AdminProviderOption>(
        "SELECT id, username FROM users WHERE role = 'provider' ORDER BY username COLLATE NOCASE",
    )
    .fetch_all(&state.pool)
    .await?;
    let products = sqlx::query_as::<_, AdminProductOption>(
        "SELECT p.id, p.name, u.username AS provider_name
         FROM products p JOIN users u ON u.id = p.provider_user_id
         ORDER BY p.name COLLATE NOCASE",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        json!({ "providers": providers, "products": products }),
    ))
}

pub async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let secret = state.stripe_webhook_secret.as_deref().ok_or_else(|| {
        AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Stripe webhook signing secret is not configured.",
        )
    })?;
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::bad_request("Stripe signature header is missing."))?;
    verify_stripe_signature(secret, signature, &body, unix_now()?)?;
    let event: Value = serde_json::from_slice(&body).map_err(|error| {
        tracing::warn!(error = %error, "Stripe webhook JSON is invalid");
        AppError::bad_request("Stripe webhook body is invalid.")
    })?;
    let event_id = required_json_str(&event, "id")?;
    let event_type = required_json_str(&event, "type")?;
    let object = event
        .pointer("/data/object")
        .ok_or_else(|| AppError::bad_request("Stripe webhook object is missing."))?;

    match event_type {
        "payment_intent.succeeded" => {
            apply_payment_intent_success(&state, event_id, event_type, object).await?;
        }
        "payment_intent.payment_failed" | "payment_intent.processing" => {
            apply_payment_intent_status(&state, event_id, event_type, object).await?;
        }
        "payment_intent.canceled" => {
            apply_payment_intent_canceled(&state, event_id, event_type, object).await?;
        }
        "charge.refunded" => {
            apply_charge_refund(&state, event_id, event_type, object).await?;
        }
        _ => {
            record_webhook_event(&state.pool, event_id, event_type).await?;
        }
    }
    Ok(Json(json!({ "received": true })))
}

pub async fn expire_reservations_forever(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    interval.tick().await;
    loop {
        interval.tick().await;
        if let Err(error) = expire_reservations(&state).await {
            tracing::error!(error = %error.message, "Could not expire marketplace reservations");
        }
    }
}

async fn expire_reservations(state: &AppState) -> Result<(), AppError> {
    let expired = sqlx::query_as::<_, ExpiredOrder>(
        "SELECT DISTINCT o.id
         FROM orders o
         JOIN inventory_reservations r ON r.order_id = o.id
         WHERE o.status = 'pending' AND r.status = 'active'
           AND r.expires_at <= unixepoch()
         ORDER BY o.id LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await?;
    for order in expired {
        match ensure_order_payment(state, order.id).await {
            Ok(payment) => {
                let secret_key = match stripe_secret_key(state) {
                    Ok(key) => key,
                    Err(error) => {
                        tracing::warn!(order_id = order.id, error = %error.message, "Reservation remains held because Stripe cannot be reached");
                        continue;
                    }
                };
                let response = state
                    .stripe_http
                    .post(format!(
                        "https://api.stripe.com/v1/payment_intents/{}/cancel",
                        payment.intent_id
                    ))
                    .bearer_auth(secret_key)
                    .send()
                    .await;
                match response {
                    Ok(response) if response.status().is_success() => {
                        release_order_reservations(state, order.id, "canceled").await?;
                    }
                    Ok(response) => {
                        tracing::warn!(
                            order_id = order.id,
                            status = %response.status(),
                            "Stripe did not confirm cancellation; inventory remains reserved"
                        );
                    }
                    Err(error) => tracing::warn!(
                        order_id = order.id,
                        error = %error,
                        "Could not cancel expired PaymentIntent; inventory remains reserved"
                    ),
                }
            }
            Err(error) => tracing::warn!(
                order_id = order.id,
                error = %error.message,
                "Could not cancel expired order; inventory remains reserved"
            ),
        }
    }
    Ok(())
}

struct EnsuredPayment {
    id: i64,
    intent_id: String,
    client_secret: String,
    publishable_key: String,
    amount_cents: i64,
}

async fn ensure_order_payment(state: &AppState, order_id: i64) -> Result<EnsuredPayment, AppError> {
    let order = sqlx::query_as::<_, OrderForStripe>(
        "SELECT id, total_amount_cents, currency, status FROM orders WHERE id = ?",
    )
    .bind(order_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "Order was not found."))?;
    if order.status != "pending" {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "This order is no longer available for payment.",
        ));
    }
    let (secret_key, publishable_key) = stripe_test_keys(state)?;
    let existing = sqlx::query_as::<_, OrderPayment>(
        "SELECT id, stripe_payment_intent_id, amount_cents, currency
         FROM stripe_payments WHERE order_id = ?",
    )
    .bind(order_id)
    .fetch_optional(&state.pool)
    .await?;
    if let Some(existing) = existing {
        let intent: Value = stripe_json(
            state
                .stripe_http
                .get(format!(
                    "https://api.stripe.com/v1/payment_intents/{}",
                    existing.stripe_payment_intent_id
                ))
                .bearer_auth(secret_key),
        )
        .await?;
        let amount = intent.get("amount").and_then(Value::as_i64);
        let currency = intent.get("currency").and_then(Value::as_str);
        if amount != Some(existing.amount_cents) || currency != Some(existing.currency.as_str()) {
            return Err(AppError::new(
                StatusCode::BAD_GATEWAY,
                "Stripe payment details did not match the stored order.",
            ));
        }
        let client_secret = required_json_str(&intent, "client_secret")?;
        return Ok(EnsuredPayment {
            id: existing.id,
            intent_id: existing.stripe_payment_intent_id,
            client_secret: client_secret.to_owned(),
            publishable_key: publishable_key.to_owned(),
            amount_cents: existing.amount_cents,
        });
    }

    let amount = order.total_amount_cents.to_string();
    let order_id_string = order.id.to_string();
    let idempotency_key = format!("marketplace-order-{}", order.id);
    let intent: Value = stripe_json(
        state
            .stripe_http
            .post("https://api.stripe.com/v1/payment_intents")
            .bearer_auth(secret_key)
            .header("Idempotency-Key", idempotency_key)
            .form(&[
                ("amount", amount.as_str()),
                ("currency", order.currency.as_str()),
                ("automatic_payment_methods[enabled]", "true"),
                (
                    "payment_method_options[card][request_three_d_secure]",
                    "any",
                ),
                ("metadata[order_id]", order_id_string.as_str()),
            ]),
    )
    .await?;
    let intent_id = required_json_str(&intent, "id")?;
    let client_secret = required_json_str(&intent, "client_secret")?;
    let stripe_amount = intent.get("amount").and_then(Value::as_i64);
    let stripe_currency = intent.get("currency").and_then(Value::as_str);
    if stripe_amount != Some(order.total_amount_cents)
        || stripe_currency != Some(order.currency.as_str())
    {
        tracing::error!(
            order_id,
            "New Stripe PaymentIntent did not match its stored order"
        );
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "Stripe returned payment details that do not match this order.",
        ));
    }
    let estimated_fee = estimate_stripe_fee(order.total_amount_cents);
    let insert = sqlx::query(
        "INSERT INTO stripe_payments
            (order_id, stripe_payment_intent_id, amount_cents, currency, status, estimated_fee_cents)
         VALUES (?, ?, ?, ?, 'pending', ?)
         ON CONFLICT(order_id) DO NOTHING",
    )
    .bind(order_id)
    .bind(intent_id)
    .bind(order.total_amount_cents)
    .bind(&order.currency)
    .bind(estimated_fee)
    .execute(&state.pool)
    .await?;
    if insert.rows_affected() == 0 {
        let stored: OrderPayment = sqlx::query_as(
            "SELECT id, stripe_payment_intent_id, amount_cents, currency
             FROM stripe_payments WHERE order_id = ?",
        )
        .bind(order_id)
        .fetch_one(&state.pool)
        .await?;
        if stored.amount_cents != order.total_amount_cents
            || stored.currency != order.currency
            || stored.stripe_payment_intent_id != intent_id
        {
            tracing::error!(
                order_id,
                "Concurrent Stripe idempotency response did not match stored payment"
            );
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "This checkout is already being prepared. Retry shortly.",
            ));
        }
        return Ok(EnsuredPayment {
            id: stored.id,
            intent_id: stored.stripe_payment_intent_id,
            client_secret: client_secret.to_owned(),
            publishable_key: publishable_key.to_owned(),
            amount_cents: stored.amount_cents,
        });
    }
    let id: i64 = sqlx::query_scalar("SELECT id FROM stripe_payments WHERE order_id = ?")
        .bind(order_id)
        .fetch_one(&state.pool)
        .await?;
    Ok(EnsuredPayment {
        id,
        intent_id: intent_id.to_owned(),
        client_secret: client_secret.to_owned(),
        publishable_key: publishable_key.to_owned(),
        amount_cents: order.total_amount_cents,
    })
}

async fn reserve_order(
    state: &AppState,
    customer_id: i64,
    checkout_key: &str,
    cart: &BTreeMap<i64, i64>,
) -> Result<i64, AppError> {
    let mut transaction = state.pool.begin().await?;
    let mut products = Vec::with_capacity(cart.len());
    let mut total = 0_i64;
    for (&product_id, &quantity) in cart {
        let product = sqlx::query_as::<_, CheckoutProduct>(
            "SELECT p.id, p.sku, p.name, p.base_price_cents,
                    p.stock_quantity, p.reserved_quantity, p.provider_user_id
             FROM products p
             JOIN users u ON u.id = p.provider_user_id AND u.role = 'provider'
             WHERE p.id = ? AND p.visible = 1 AND p.active = 1",
        )
        .bind(product_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::bad_request("A product in your cart is no longer available."))?;
        if product.stock_quantity - product.reserved_quantity < quantity {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "A product in your cart does not have enough available stock.",
            ));
        }
        let line_total = product
            .base_price_cents
            .checked_mul(quantity)
            .ok_or_else(|| AppError::bad_request("The cart total is too large."))?;
        total = total
            .checked_add(line_total)
            .ok_or_else(|| AppError::bad_request("The cart total is too large."))?;
        products.push((product, quantity, line_total));
    }
    if !(MIN_PRODUCT_PRICE_CENTS..=MAX_ORDER_AMOUNT_CENTS).contains(&total) {
        return Err(AppError::bad_request(
            "The order total must be between USD $0.50 and USD $5,000.00.",
        ));
    }
    let order_result = sqlx::query(
        "INSERT INTO orders (customer_user_id, checkout_key, total_amount_cents, currency, status)
         VALUES (?, ?, ?, 'usd', 'pending')",
    )
    .bind(customer_id)
    .bind(checkout_key)
    .bind(total)
    .execute(&mut *transaction)
    .await?;
    let order_id = order_result.last_insert_rowid();
    let reservation_expires_at = unix_now()? + INVENTORY_RESERVATION_SECONDS;
    let mut provider_totals = BTreeMap::<i64, i64>::new();
    for (product, quantity, line_total) in &products {
        let reserved = sqlx::query(
            "UPDATE products
             SET reserved_quantity = reserved_quantity + ?
             WHERE id = ? AND visible = 1 AND active = 1
               AND stock_quantity - reserved_quantity >= ?",
        )
        .bind(quantity)
        .bind(product.id)
        .bind(quantity)
        .execute(&mut *transaction)
        .await?;
        if reserved.rows_affected() != 1 {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "Product stock changed during checkout. Refresh your cart and try again.",
            ));
        }
        sqlx::query(
            "INSERT INTO order_items
                (order_id, product_id, provider_user_id, product_name, product_sku,
                 unit_price_cents, quantity, line_total_cents)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(order_id)
        .bind(product.id)
        .bind(product.provider_user_id)
        .bind(&product.name)
        .bind(&product.sku)
        .bind(product.base_price_cents)
        .bind(quantity)
        .bind(line_total)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO inventory_reservations (order_id, product_id, quantity, expires_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(order_id)
        .bind(product.id)
        .bind(quantity)
        .bind(reservation_expires_at)
        .execute(&mut *transaction)
        .await?;
        let entry = provider_totals.entry(product.provider_user_id).or_default();
        *entry = entry
            .checked_add(*line_total)
            .ok_or_else(|| AppError::bad_request("The cart total is too large."))?;
    }
    let estimated_fee = estimate_stripe_fee(total);
    let fee_shares = allocate_cents(
        estimated_fee,
        &provider_totals
            .iter()
            .map(|(id, gross)| (*id, *gross))
            .collect::<Vec<_>>(),
    );
    for (&provider_id, &gross) in &provider_totals {
        let fee_share = fee_shares.get(&provider_id).copied().unwrap_or(0);
        let cap = percent_allocation(gross);
        let commission = platform_commission(gross, fee_share);
        sqlx::query(
            "INSERT INTO order_provider_totals
                (order_id, provider_user_id, gross_amount_cents, stripe_fee_cents,
                 platform_commission_cents, provider_net_cents)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(order_id)
        .bind(provider_id)
        .bind(gross)
        .bind(fee_share)
        .bind(commission)
        .bind(gross - cap)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(order_id)
}

fn normalize_cart(items: Vec<CartItem>) -> Result<BTreeMap<i64, i64>, AppError> {
    if items.is_empty() || items.len() > MAX_CART_LINES {
        return Err(AppError::bad_request(
            "Your cart must contain between 1 and 50 distinct products.",
        ));
    }
    let mut cart = BTreeMap::new();
    for item in items {
        if item.product_id <= 0 || !(1..=MAX_ITEM_QUANTITY).contains(&item.quantity) {
            return Err(AppError::bad_request(
                "Product IDs and quantities must be positive, and each quantity must be at most 1,000.",
            ));
        }
        let quantity = cart.entry(item.product_id).or_insert(0_i64);
        *quantity = quantity
            .checked_add(item.quantity)
            .filter(|value| *value <= MAX_ITEM_QUANTITY)
            .ok_or_else(|| AppError::bad_request("Product quantity exceeds the maximum."))?;
    }
    Ok(cart)
}

fn validate_product_input(input: &ProductInput) -> Result<(), AppError> {
    let name = input.name.trim();
    let description = input.description.trim();
    if !(2..=120).contains(&name.chars().count()) {
        return Err(AppError::bad_request(
            "Product name must contain 2 to 120 characters.",
        ));
    }
    if description.is_empty() || description.chars().count() > 2_000 {
        return Err(AppError::bad_request(
            "Product description must contain 1 to 2,000 characters.",
        ));
    }
    if !(MIN_PRODUCT_PRICE_CENTS..=MAX_PRODUCT_PRICE_CENTS).contains(&input.price_cents) {
        return Err(AppError::bad_request(
            "Product price must be between USD $0.50 and USD $500.00.",
        ));
    }
    if !(0..=1_000_000).contains(&input.stock_quantity) {
        return Err(AppError::bad_request(
            "Stock quantity must be between 0 and 1,000,000.",
        ));
    }
    Ok(())
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn estimate_stripe_fee(gross_cents: i64) -> i64 {
    (gross_cents * STRIPE_ESTIMATED_RATE_BPS + 5_000) / 10_000 + STRIPE_ESTIMATED_FIXED_FEE_CENTS
}

fn percent_allocation(gross_cents: i64) -> i64 {
    (gross_cents * PLATFORM_ALLOCATION_BPS + 5_000) / 10_000
}

fn platform_commission(gross_cents: i64, stripe_fee_cents: i64) -> i64 {
    percent_allocation(gross_cents)
        .saturating_sub(stripe_fee_cents)
        .max(0)
}

fn allocate_cents(total: i64, amounts: &[(i64, i64)]) -> BTreeMap<i64, i64> {
    let gross_total: i64 = amounts.iter().map(|(_, gross)| gross).sum();
    let mut allocations = BTreeMap::new();
    if gross_total <= 0 || total <= 0 {
        return allocations;
    }
    let mut allocated = 0_i64;
    for &(id, gross) in amounts {
        let share = total.saturating_mul(gross) / gross_total;
        allocations.insert(id, share);
        allocated += share;
    }
    let mut remainder = total - allocated;
    for &(id, _) in amounts {
        if remainder == 0 {
            break;
        }
        *allocations.entry(id).or_default() += 1;
        remainder -= 1;
    }
    allocations
}

async fn apply_payment_intent_success(
    state: &AppState,
    event_id: &str,
    event_type: &str,
    object: &Value,
) -> Result<(), AppError> {
    let intent_id = required_json_str(object, "id")?;
    let amount = object
        .get("amount")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::bad_request("PaymentIntent amount is missing."))?;
    let currency = required_json_str(object, "currency")?;
    let charge_id = object
        .get("latest_charge")
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("id").and_then(Value::as_str))
        })
        .map(str::to_owned);
    let charge_data = if let Some(charge_id) = charge_id.as_deref() {
        Some(fetch_charge_fee(state, charge_id).await?)
    } else {
        None
    };

    let mut transaction = state.pool.begin().await?;
    if !insert_webhook_event(&mut transaction, event_id, event_type).await? {
        return Ok(());
    }
    let payment = sqlx::query_as::<_, WebhookPaymentRecord>(
        "SELECT id, order_id, amount_cents, currency, stripe_charge_id, refunded_amount_cents
         FROM stripe_payments WHERE stripe_payment_intent_id = ?",
    )
    .bind(intent_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(payment) = payment else {
        tracing::warn!(
            stripe_payment_intent_id = intent_id,
            "Ignoring an unrelated Stripe PaymentIntent"
        );
        transaction.commit().await?;
        return Ok(());
    };
    if payment.amount_cents != amount || payment.currency != currency {
        tracing::error!(
            order_id = payment.order_id,
            stripe_payment_intent_id = intent_id,
            "Stripe webhook amount or currency does not match the stored order"
        );
        return Err(AppError::bad_request(
            "Stripe webhook payment details do not match the order.",
        ));
    }
    let order: OrderForStripe =
        sqlx::query_as("SELECT id, total_amount_cents, currency, status FROM orders WHERE id = ?")
            .bind(payment.order_id)
            .fetch_one(&mut *transaction)
            .await?;
    if order.total_amount_cents != amount || order.currency != currency {
        return Err(AppError::bad_request(
            "Stripe webhook payment details do not match the order.",
        ));
    }
    if order.status == "canceled" || order.status == "failed" {
        tracing::error!(
            order_id = order.id,
            "Received successful payment for a canceled marketplace order"
        );
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "The order is no longer active; payment requires manual reconciliation.",
        ));
    }
    let (charge_id, balance_transaction_id, actual_fee) = charge_data
        .map(|data| (data.charge_id, data.balance_transaction_id, data.fee_cents))
        .unwrap_or((None, None, None));
    sqlx::query(
        "UPDATE stripe_payments
         SET status = CASE
                 WHEN refunded_amount_cents >= amount_cents THEN 'refunded'
                 WHEN refunded_amount_cents > 0 THEN 'partially_refunded'
                 ELSE 'succeeded'
             END,
             stripe_charge_id = COALESCE(?, stripe_charge_id),
             stripe_balance_transaction_id = COALESCE(?, stripe_balance_transaction_id),
             stripe_fee_cents = COALESCE(?, stripe_fee_cents), updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(charge_id)
    .bind(balance_transaction_id)
    .bind(actual_fee)
    .bind(payment.id)
    .execute(&mut *transaction)
    .await?;

    if order.status != "paid" {
        sqlx::query(
            "UPDATE orders SET status = CASE WHEN status = 'pending' THEN 'paid' ELSE status END,
                paid_at = COALESCE(paid_at, unixepoch()),
                updated_at = unixepoch()
             WHERE id = ? AND status NOT IN ('canceled', 'failed')",
        )
        .bind(order.id)
        .execute(&mut *transaction)
        .await?;
        consume_order_reservations(&mut transaction, order.id).await?;
        if let Some(fee_cents) = actual_fee {
            update_provider_fee_allocations(&mut transaction, order.id, fee_cents).await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

async fn apply_payment_intent_status(
    state: &AppState,
    event_id: &str,
    event_type: &str,
    object: &Value,
) -> Result<(), AppError> {
    let intent_id = required_json_str(object, "id")?;
    let status = required_json_str(object, "status")?;
    let mut transaction = state.pool.begin().await?;
    if !insert_webhook_event(&mut transaction, event_id, event_type).await? {
        return Ok(());
    }
    sqlx::query(
        "UPDATE stripe_payments SET status = ?, updated_at = unixepoch()
         WHERE stripe_payment_intent_id = ?
           AND status NOT IN ('succeeded', 'partially_refunded', 'refunded')",
    )
    .bind(status)
    .bind(intent_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn apply_payment_intent_canceled(
    state: &AppState,
    event_id: &str,
    event_type: &str,
    object: &Value,
) -> Result<(), AppError> {
    let intent_id = required_json_str(object, "id")?;
    let order_id: Option<i64> = sqlx::query_scalar(
        "SELECT order_id FROM stripe_payments WHERE stripe_payment_intent_id = ?",
    )
    .bind(intent_id)
    .fetch_optional(&state.pool)
    .await?;
    let mut transaction = state.pool.begin().await?;
    if !insert_webhook_event(&mut transaction, event_id, event_type).await? {
        return Ok(());
    }
    sqlx::query(
        "UPDATE stripe_payments SET status = 'canceled', updated_at = unixepoch()
         WHERE stripe_payment_intent_id = ?
           AND status NOT IN ('succeeded', 'partially_refunded', 'refunded')",
    )
    .bind(intent_id)
    .execute(&mut *transaction)
    .await?;
    if let Some(order_id) = order_id {
        release_order_reservations_in_transaction(&mut transaction, order_id, "canceled").await?;
    }
    transaction.commit().await?;
    Ok(())
}

async fn apply_charge_refund(
    state: &AppState,
    event_id: &str,
    event_type: &str,
    object: &Value,
) -> Result<(), AppError> {
    let charge_id = required_json_str(object, "id")?;
    let refunded = object
        .get("amount_refunded")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::bad_request("Stripe refund amount is missing."))?;
    let amount = object
        .get("amount")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::bad_request("Stripe charge amount is missing."))?;
    if refunded < 0 {
        return Err(AppError::bad_request(
            "Stripe refund amount cannot be negative.",
        ));
    }
    let payment_intent_id = object.get("payment_intent").and_then(|value| {
        value
            .as_str()
            .or_else(|| value.get("id").and_then(Value::as_str))
    });
    let mut transaction = state.pool.begin().await?;
    if !insert_webhook_event(&mut transaction, event_id, event_type).await? {
        return Ok(());
    }
    let payments: Vec<WebhookPaymentRecord> = sqlx::query_as(
        "SELECT id, order_id, amount_cents, currency, stripe_charge_id, refunded_amount_cents
         FROM stripe_payments
         WHERE stripe_charge_id = ? OR stripe_payment_intent_id = ?
         LIMIT 2",
    )
    .bind(charge_id)
    .bind(payment_intent_id)
    .fetch_all(&mut *transaction)
    .await?;
    if payments.len() > 1 {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "Stripe charge identifies more than one stored payment.",
        ));
    }
    if let Some(payment) = payments.into_iter().next() {
        if amount != payment.amount_cents || refunded > amount {
            return Err(AppError::bad_request(
                "Stripe refund details do not match the stored payment.",
            ));
        }
        if payment
            .stripe_charge_id
            .as_deref()
            .is_some_and(|stored_id| stored_id != charge_id)
        {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "Stripe charge does not match the stored payment.",
            ));
        }
        let refunded = refunded.max(payment.refunded_amount_cents);
        let status = if refunded >= amount {
            "refunded"
        } else {
            "partially_refunded"
        };
        sqlx::query(
            "UPDATE stripe_payments
             SET stripe_charge_id = COALESCE(stripe_charge_id, ?),
                 refunded_amount_cents = ?, status = ?, updated_at = unixepoch()
             WHERE id = ?",
        )
        .bind(charge_id)
        .bind(refunded)
        .bind(status)
        .bind(payment.id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE orders SET status = ?, paid_at = COALESCE(paid_at, unixepoch()),
                updated_at = unixepoch() WHERE id = ?",
        )
        .bind(status)
        .bind(payment.order_id)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

struct ChargeFeeData {
    charge_id: Option<String>,
    balance_transaction_id: Option<String>,
    fee_cents: Option<i64>,
}

async fn fetch_charge_fee(state: &AppState, charge_id: &str) -> Result<ChargeFeeData, AppError> {
    let secret_key = stripe_secret_key(state)?;
    let charge: Value = stripe_json(
        state
            .stripe_http
            .get(format!("https://api.stripe.com/v1/charges/{charge_id}"))
            .query(&[("expand[]", "balance_transaction")])
            .bearer_auth(secret_key),
    )
    .await?;
    let balance = charge.get("balance_transaction");
    let balance_transaction_id = balance
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("id").and_then(Value::as_str))
        })
        .map(str::to_owned);
    let fee_cents = balance
        .and_then(|value| value.get("fee"))
        .and_then(Value::as_i64);
    Ok(ChargeFeeData {
        charge_id: Some(charge_id.to_owned()),
        balance_transaction_id,
        fee_cents,
    })
}

async fn consume_order_reservations(
    transaction: &mut Transaction<'_, Sqlite>,
    order_id: i64,
) -> Result<(), AppError> {
    let reservations = sqlx::query_as::<_, ReservationRow>(
        "SELECT product_id, quantity FROM inventory_reservations
         WHERE order_id = ? AND status = 'active'",
    )
    .bind(order_id)
    .fetch_all(&mut **transaction)
    .await?;
    for reservation in reservations {
        let changed = sqlx::query(
            "UPDATE products
             SET stock_quantity = stock_quantity - ?,
                 reserved_quantity = reserved_quantity - ?
             WHERE id = ? AND stock_quantity >= ? AND reserved_quantity >= ?",
        )
        .bind(reservation.quantity)
        .bind(reservation.quantity)
        .bind(reservation.product_id)
        .bind(reservation.quantity)
        .bind(reservation.quantity)
        .execute(&mut **transaction)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "Reserved product stock could not be reconciled.",
            ));
        }
    }
    sqlx::query(
        "UPDATE inventory_reservations SET status = 'consumed'
         WHERE order_id = ? AND status = 'active'",
    )
    .bind(order_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn release_order_reservations(
    state: &AppState,
    order_id: i64,
    status: &str,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    release_order_reservations_in_transaction(&mut transaction, order_id, status).await?;
    transaction.commit().await?;
    Ok(())
}

async fn release_order_reservations_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    order_id: i64,
    status: &str,
) -> Result<(), AppError> {
    let reservations = sqlx::query_as::<_, ReservationRow>(
        "SELECT product_id, quantity FROM inventory_reservations
         WHERE order_id = ? AND status = 'active'",
    )
    .bind(order_id)
    .fetch_all(&mut **transaction)
    .await?;
    for reservation in reservations {
        let changed = sqlx::query(
            "UPDATE products SET reserved_quantity = reserved_quantity - ?
             WHERE id = ? AND reserved_quantity >= ?",
        )
        .bind(reservation.quantity)
        .bind(reservation.product_id)
        .bind(reservation.quantity)
        .execute(&mut **transaction)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "Reserved product stock could not be released safely.",
            ));
        }
    }
    sqlx::query(
        "UPDATE inventory_reservations SET status = 'released'
         WHERE order_id = ? AND status = 'active'",
    )
    .bind(order_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE orders SET status = ?, updated_at = unixepoch()
         WHERE id = ? AND status = 'pending'",
    )
    .bind(status)
    .bind(order_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn update_provider_fee_allocations(
    transaction: &mut Transaction<'_, Sqlite>,
    order_id: i64,
    actual_fee_cents: i64,
) -> Result<(), AppError> {
    let providers = sqlx::query_as::<_, ProviderGross>(
        "SELECT provider_user_id, gross_amount_cents
         FROM order_provider_totals WHERE order_id = ? ORDER BY provider_user_id",
    )
    .bind(order_id)
    .fetch_all(&mut **transaction)
    .await?;
    let gross: Vec<_> = providers
        .iter()
        .map(|provider| (provider.provider_user_id, provider.gross_amount_cents))
        .collect();
    let allocations = allocate_cents(actual_fee_cents, &gross);
    for provider in providers {
        let fee = allocations
            .get(&provider.provider_user_id)
            .copied()
            .unwrap_or(0);
        let cap = percent_allocation(provider.gross_amount_cents);
        let commission = platform_commission(provider.gross_amount_cents, fee);
        sqlx::query(
            "UPDATE order_provider_totals
             SET stripe_fee_cents = ?, platform_commission_cents = ?, provider_net_cents = ?
             WHERE order_id = ? AND provider_user_id = ?",
        )
        .bind(fee)
        .bind(commission)
        .bind(provider.gross_amount_cents - cap)
        .bind(order_id)
        .bind(provider.provider_user_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn insert_webhook_event(
    transaction: &mut Transaction<'_, Sqlite>,
    event_id: &str,
    event_type: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "INSERT INTO stripe_webhook_events (event_id, event_type)
         VALUES (?, ?) ON CONFLICT(event_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(event_type)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected() == 1)
}

async fn record_webhook_event(
    pool: &sqlx::SqlitePool,
    event_id: &str,
    event_type: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO stripe_webhook_events (event_id, event_type)
         VALUES (?, ?) ON CONFLICT(event_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(event_type)
    .execute(pool)
    .await?;
    Ok(())
}

async fn fetch_stripe_balance(state: &AppState) -> Result<Option<Value>, AppError> {
    let Some(secret_key) = state.stripe_secret_key.as_deref() else {
        return Ok(None);
    };
    if !secret_key.starts_with("sk_test_") {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "The admin dashboard only accepts Stripe test keys.",
        ));
    }
    let balance: Value = stripe_json(
        state
            .stripe_http
            .get("https://api.stripe.com/v1/balance")
            .bearer_auth(secret_key),
    )
    .await?;
    let available = balance.get("available").and_then(Value::as_array);
    let pending = balance.get("pending").and_then(Value::as_array);
    if available.is_none() || pending.is_none() {
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "Stripe returned an unexpected balance response.",
        ));
    }
    Ok(Some(json!({ "available": available, "pending": pending })))
}

fn stripe_secret_key(state: &AppState) -> Result<&str, AppError> {
    let Some(secret_key) = state.stripe_secret_key.as_deref() else {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Stripe test keys are not configured.",
        ));
    };
    if !secret_key.starts_with("sk_test_") {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "This prototype accepts Stripe test keys only.",
        ));
    }
    Ok(secret_key)
}

fn stripe_test_keys(state: &AppState) -> Result<(&str, &str), AppError> {
    let secret_key = stripe_secret_key(state)?;
    let Some(publishable_key) = state.stripe_publishable_key.as_deref() else {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Stripe test keys are not configured.",
        ));
    };
    if !publishable_key.starts_with("pk_test_") {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "This prototype accepts Stripe test keys only.",
        ));
    }
    Ok((secret_key, publishable_key))
}

async fn stripe_json<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T, AppError> {
    let response = request.send().await.map_err(|error| {
        tracing::error!(error = %error, "Could not reach Stripe");
        AppError::new(
            StatusCode::BAD_GATEWAY,
            "Could not reach Stripe. Check the test keys and try again.",
        )
    })?;
    let status = response.status();
    let body = response.text().await.map_err(|error| {
        tracing::error!(error = %error, "Could not read Stripe response");
        AppError::new(
            StatusCode::BAD_GATEWAY,
            "Stripe returned an unreadable response.",
        )
    })?;
    if !status.is_success() {
        tracing::error!(%status, "Stripe rejected a marketplace request");
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "Stripe could not complete this request. Check the test configuration and try again.",
        ));
    }
    serde_json::from_str(&body).map_err(|error| {
        tracing::error!(error = %error, "Stripe returned an unexpected response");
        AppError::new(
            StatusCode::BAD_GATEWAY,
            "Stripe returned an unexpected response.",
        )
    })
}

fn verify_stripe_signature(
    secret: &str,
    header: &str,
    body: &[u8],
    now: i64,
) -> Result<(), AppError> {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for component in header.split(',') {
        let Some((key, value)) = component.split_once('=') else {
            continue;
        };
        match key {
            "t" => timestamp = value.parse::<i64>().ok(),
            "v1" => signatures.push(value),
            _ => {}
        }
    }
    let timestamp =
        timestamp.ok_or_else(|| AppError::bad_request("Stripe signature timestamp is invalid."))?;
    if signatures.is_empty() || now.abs_diff(timestamp) > WEBHOOK_TOLERANCE_SECONDS as u64 {
        return Err(AppError::bad_request(
            "Stripe webhook signature is missing, invalid, or expired.",
        ));
    }
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|error| {
        tracing::error!(error = %error, "Could not initialize Stripe webhook verification");
        AppError::internal("Stripe webhook verification is not configured correctly.")
    })?;
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let expected = mac.finalize().into_bytes();
    if signatures
        .iter()
        .filter_map(|signature| decode_hex(signature))
        .any(|signature| constant_time_equal(&expected, &signature))
    {
        Ok(())
    } else {
        Err(AppError::bad_request(
            "Stripe webhook signature is missing, invalid, or expired.",
        ))
    }
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn required_json_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, AppError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::bad_request(format!("Stripe event field {key} is missing.")))
}

fn unix_now() -> Result<i64, AppError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|error| {
            tracing::error!(error = %error, "System clock is before the Unix epoch");
            AppError::internal("The server clock is not configured correctly.")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_estimated_fee_and_ten_percent_cap() {
        assert_eq!(estimate_stripe_fee(10_000), 438);
        assert_eq!(percent_allocation(10_000), 1_000);
        assert_eq!(percent_allocation(500), 50);
    }

    #[test]
    fn commission_is_zero_when_stripe_fee_exceeds_the_platform_cap() {
        assert_eq!(platform_commission(500, estimate_stripe_fee(500)), 0);
        assert_eq!(platform_commission(10_000, 1_200), 0);
        assert_eq!(platform_commission(10_000, 438), 562);
    }

    #[test]
    fn allocates_every_cent_proportionally_and_deterministically() {
        let amounts = [(1, 1_000), (2, 1_000), (3, 1_000)];
        let shares = allocate_cents(10, &amounts);
        assert_eq!(shares.values().sum::<i64>(), 10);
        assert_eq!(shares.get(&1), Some(&4));
        assert_eq!(shares.get(&2), Some(&3));
        assert_eq!(shares.get(&3), Some(&3));
    }

    #[test]
    fn verifies_only_recent_signed_stripe_payloads() {
        let secret = "whsec_unit_test";
        let body = br#"{"id":"evt_test","type":"test.event"}"#;
        let timestamp = 1_800_000_000_i64;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(timestamp.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        let signature = format!("{:x}", mac.finalize().into_bytes());
        let header = format!("t={timestamp},v1={signature}");

        assert!(verify_stripe_signature(secret, &header, body, timestamp).is_ok());
        assert!(verify_stripe_signature(secret, &header, body, timestamp + 301).is_err());
        assert!(verify_stripe_signature(secret, &header, b"tampered", timestamp).is_err());
    }
}
