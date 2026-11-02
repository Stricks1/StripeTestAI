use crate::{auth::session_user_id, error::AppError, state::AppState};
use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use sqlx::FromRow;
use std::net::SocketAddr;
use tower_sessions::Session;

const MIN_AMOUNT_CENTS: i64 = 50;
const MAX_AMOUNT_CENTS: i64 = 50_000;
const SMALL_PAYMENT_AMOUNT_CENTS: i64 = 500;
const SMALL_PAYMENT_ALERT_COUNT: i64 = 5;
const SMALL_PAYMENT_WINDOW_SECONDS: i64 = 15 * 60;

#[derive(Deserialize)]
pub struct CreatePaymentInput {
    product_id: i64,
    amount_cents: i64,
}

#[derive(Serialize, FromRow)]
struct Product {
    id: i64,
    sku: String,
    name: String,
    description: String,
    base_price_cents: i64,
}

#[derive(FromRow)]
struct PaymentRecord {
    id: i64,
    amount_cents: i64,
    currency: String,
    status: String,
    stripe_payment_intent_id: Option<String>,
}

#[derive(Deserialize)]
struct StripePaymentIntent {
    id: String,
    client_secret: String,
}

#[derive(Deserialize)]
struct StripePaymentStatus {
    id: String,
    amount: i64,
    currency: String,
    status: String,
}

pub async fn checkout_page(session: Session) -> Result<impl IntoResponse, AppError> {
    if session_user_id(&session).await?.is_none() {
        return Ok(Redirect::to("/").into_response());
    }
    Ok(Html(include_str!("../public/checkout.html")).into_response())
}

pub async fn payment_return_page(session: Session) -> Result<impl IntoResponse, AppError> {
    if session_user_id(&session).await?.is_none() {
        return Ok(Redirect::to("/").into_response());
    }
    Ok(Html(include_str!("../public/payment-result.html")).into_response())
}

pub async fn products(
    State(state): State<AppState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    require_user(&session).await?;
    let rows = sqlx::query_as::<_, Product>(
        "SELECT id, sku, name, description, base_price_cents
         FROM products WHERE active = 1 ORDER BY id LIMIT 1",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_payment_intent(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    session: Session,
    Json(input): Json<CreatePaymentInput>,
) -> Result<impl IntoResponse, AppError> {
    if let Err(retry_after_secs) = state.payment_rate_limiter.check_ip(peer.ip()).await {
        tracing::warn!(
            scope = "ip",
            retry_after_seconds = retry_after_secs,
            "Payment intent rate limit reached"
        );
        return Err(AppError::too_many_requests(retry_after_secs));
    }
    let user_id = require_user(&session).await?;
    if let Err(retry_after_secs) = state.payment_rate_limiter.check_user(user_id).await {
        tracing::warn!(
            scope = "user",
            retry_after_seconds = retry_after_secs,
            "Payment intent rate limit reached"
        );
        return Err(AppError::too_many_requests(retry_after_secs));
    }
    if !(MIN_AMOUNT_CENTS..=MAX_AMOUNT_CENTS).contains(&input.amount_cents) {
        return Err(AppError::bad_request(
            "Choose an amount from USD $0.50 to USD $500.00.",
        ));
    }

    let product = sqlx::query_as::<_, Product>(
        "SELECT id, sku, name, description, base_price_cents
         FROM products WHERE id = ? AND active = 1",
    )
    .bind(input.product_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::bad_request("That product is unavailable."))?;

    let (secret_key, publishable_key) = stripe_test_keys(&state)?;
    let amount = input.amount_cents.to_string();
    let user_id_string = user_id.to_string();
    let product_id_string = product.id.to_string();
    let payment_intent: StripePaymentIntent = stripe_json(
        state
            .stripe_http
            .post("https://api.stripe.com/v1/payment_intents")
            .bearer_auth(secret_key)
            .form(&[
                ("amount", amount.as_str()),
                ("currency", "usd"),
                ("automatic_payment_methods[enabled]", "true"),
                (
                    "payment_method_options[card][request_three_d_secure]",
                    "any",
                ),
                ("metadata[user_id]", user_id_string.as_str()),
                ("metadata[product_id]", product_id_string.as_str()),
            ]),
    )
    .await?;

    let inserted = sqlx::query(
        "INSERT INTO payments
         (user_id, product_id, stripe_payment_intent_id, amount_cents, currency, status)
         VALUES (?, ?, ?, ?, 'usd', 'pending')",
    )
    .bind(user_id)
    .bind(product.id)
    .bind(&payment_intent.id)
    .bind(input.amount_cents)
    .execute(&state.pool)
    .await?;
    let payment_id = inserted.last_insert_rowid();

    if input.amount_cents <= SMALL_PAYMENT_AMOUNT_CENTS {
        let recent_small_payments: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payments
             WHERE user_id = ? AND amount_cents <= ? AND created_at >= unixepoch() - ?",
        )
        .bind(user_id)
        .bind(SMALL_PAYMENT_AMOUNT_CENTS)
        .bind(SMALL_PAYMENT_WINDOW_SECONDS)
        .fetch_one(&state.pool)
        .await?;
        if recent_small_payments >= SMALL_PAYMENT_ALERT_COUNT {
            tracing::warn!(
                user_id,
                recent_small_payments,
                window_minutes = SMALL_PAYMENT_WINDOW_SECONDS / 60,
                "Repeated small-payment pattern detected"
            );
        }
    }

    Ok(Json(json!({
        "payment_id": payment_id,
        "client_secret": payment_intent.client_secret,
        "publishable_key": publishable_key,
        "amount_cents": input.amount_cents,
        "currency": "usd"
    })))
}

pub async fn payment_status(
    State(state): State<AppState>,
    session: Session,
    Path(payment_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = require_user(&session).await?;
    let payment = sqlx::query_as::<_, PaymentRecord>(
        "SELECT id, amount_cents, currency, status, stripe_payment_intent_id
         FROM payments WHERE id = ? AND user_id = ?",
    )
    .bind(payment_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "Payment was not found."))?;

    let Some(intent_id) = payment.stripe_payment_intent_id.as_deref() else {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "Payment setup is incomplete. Start checkout again.",
        ));
    };
    let (secret_key, _) = stripe_test_keys(&state)?;
    let intent: StripePaymentStatus = stripe_json(
        state
            .stripe_http
            .get(format!(
                "https://api.stripe.com/v1/payment_intents/{intent_id}"
            ))
            .bearer_auth(secret_key),
    )
    .await?;

    if intent.id != intent_id
        || intent.amount != payment.amount_cents
        || intent.currency != payment.currency
    {
        tracing::error!(
            payment_id,
            "Stripe payment details did not match the stored order"
        );
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "Stripe returned payment details that do not match this order.",
        ));
    }
    let stored_status = match intent.status.as_str() {
        "succeeded" => "succeeded",
        "canceled" => "canceled",
        "requires_payment_method" => "requires_payment_method",
        "processing" => "processing",
        _ => "pending",
    };
    if payment.status != stored_status {
        sqlx::query("UPDATE payments SET status = ? WHERE id = ? AND user_id = ?")
            .bind(stored_status)
            .bind(payment.id)
            .bind(user_id)
            .execute(&state.pool)
            .await?;
    }

    Ok(Json(json!({
        "payment_id": payment.id,
        "amount_cents": payment.amount_cents,
        "currency": payment.currency,
        "status": stored_status
    })))
}

async fn require_user(session: &Session) -> Result<i64, AppError> {
    session_user_id(session)
        .await?
        .ok_or_else(|| AppError::unauthorized("Sign in to continue."))
}

fn stripe_test_keys(state: &AppState) -> Result<(&str, &str), AppError> {
    let Some(secret_key) = state.stripe_secret_key.as_deref() else {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Stripe test keys are not configured. Add STRIPE_SECRET_KEY and STRIPE_PUBLISHABLE_KEY to your local environment.",
        ));
    };
    let Some(publishable_key) = state.stripe_publishable_key.as_deref() else {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Stripe test keys are not configured. Add STRIPE_SECRET_KEY and STRIPE_PUBLISHABLE_KEY to your local environment.",
        ));
    };
    if !secret_key.starts_with("sk_test_") || !publishable_key.starts_with("pk_test_") {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "This prototype accepts Stripe test keys only.",
        ));
    }
    Ok((secret_key, publishable_key))
}

async fn stripe_json<T: DeserializeOwned>(request: reqwest::RequestBuilder) -> Result<T, AppError> {
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
        tracing::error!(%status, "Stripe rejected the test payment request");
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
