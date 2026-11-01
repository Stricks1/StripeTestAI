mod auth;
mod error;
mod payments;
mod state;

use anyhow::{Context, Result};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Router,
    routing::{get, post},
};
use rand::rngs::OsRng;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{env, net::SocketAddr, str::FromStr};
use tower_http::{services::ServeDir, trace::TraceLayer};
use tower_sessions::{MemoryStore, SessionManagerLayer, cookie::SameSite};

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(error) = dotenvy::dotenv() {
        if !error.not_found() {
            return Err(error).context("Could not read .env");
        }
    }
    tracing_subscriber::fmt::init();

    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://store.db?mode=rwc".to_owned());
    let sqlite_options = SqliteConnectOptions::from_str(&database_url)
        .with_context(|| "DATABASE_URL is not a valid SQLite URL")?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(sqlite_options)
        .await
        .context("Could not connect to SQLite")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Could not apply database migrations")?;
    seed_product(&pool).await?;
    seed_admin(&pool).await?;

    let state = state::AppState::from_env(pool.clone())?;
    let secure_cookie = state.app_env != "development";
    let session_layer = SessionManagerLayer::new(MemoryStore::default())
        .with_secure(secure_cookie)
        .with_same_site(SameSite::Lax);

    let app = Router::new()
        .route("/", get(auth::index_page))
        .route("/verify", get(auth::verification_page))
        .route("/checkout", get(payments::checkout_page))
        .route("/payment/return", get(payments::payment_return_page))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/verify", post(auth::verify_email))
        .route("/api/products", get(payments::products))
        .route(
            "/api/payment-intents",
            post(payments::create_payment_intent),
        )
        .route("/api/payments/{payment_id}", get(payments::payment_status))
        .fallback_service(ServeDir::new("public"))
        .with_state(state)
        .layer(session_layer)
        .layer(TraceLayer::new_for_http());

    let address = env::var("APP_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .with_context(|| format!("Could not bind to {address}"))?;
    tracing::info!("Test storefront listening at http://{address}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("HTTP server stopped")?;
    Ok(())
}

async fn seed_product(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO products (id, sku, name, description, base_price_cents, active)
         VALUES (1, 'TEST-001', 'TEST PRODUCT', 'A single sample product for Stripe test payments.', 1000, 1)",
    )
    .execute(pool)
    .await
    .context("Could not seed the test product")?;
    Ok(())
}

async fn seed_admin(pool: &SqlitePool) -> Result<()> {
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM users WHERE username = 'admin' COLLATE NOCASE")
            .fetch_optional(pool)
            .await
            .context("Could not check for the initial admin account")?;
    if existing.is_some() {
        return Ok(());
    }

    let app_env = env::var("APP_ENV").unwrap_or_else(|_| "development".to_owned());
    let password = if app_env == "development" {
        "admin".to_owned()
    } else {
        env::var("BOOTSTRAP_ADMIN_PASSWORD")
            .context("Set BOOTSTRAP_ADMIN_PASSWORD before starting outside development")?
    };
    if app_env != "development" && password.len() < 12 {
        anyhow::bail!("BOOTSTRAP_ADMIN_PASSWORD must contain at least 12 characters");
    }

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("Could not hash the initial admin password: {error}"))?
        .to_string();
    sqlx::query(
        "INSERT INTO users (username, email, password_hash, email_verified, is_admin)
         VALUES ('admin', 'admin@localhost', ?, 1, 1)",
    )
    .bind(password_hash)
    .execute(pool)
    .await
    .context("Could not create the initial admin account")?;
    tracing::warn!(
        "Development admin account created (admin/admin); do not expose this app publicly"
    );
    Ok(())
}
