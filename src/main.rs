mod auth;
mod error;
mod marketplace;
mod state;

use anyhow::{Context, Result};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Router,
    routing::{get, patch, post},
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
    seed_admin(&pool).await?;
    if env::var("APP_ENV").unwrap_or_else(|_| "development".to_owned()) == "development" {
        seed_provider3(&pool).await?;
    }

    let state = state::AppState::from_env(pool.clone())?;
    let secure_cookie = state.app_env != "development";
    let session_layer = SessionManagerLayer::new(MemoryStore::default())
        .with_secure(secure_cookie)
        .with_same_site(SameSite::Lax);

    tokio::spawn(marketplace::expire_reservations_forever(state.clone()));

    let app = Router::new()
        .route("/", get(auth::index_page))
        .route("/verify", get(auth::verification_page))
        .route("/store", get(marketplace::store_page))
        .route("/provider", get(marketplace::provider_page))
        .route("/admin", get(marketplace::admin_page))
        .route("/checkout", get(marketplace::checkout_page))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/verify", post(auth::verify_email))
        .route(
            "/api/marketplace/products",
            get(marketplace::customer_products),
        )
        .route(
            "/api/provider/products",
            get(marketplace::provider_products).post(marketplace::create_product),
        )
        .route(
            "/api/provider/products/{product_id}",
            patch(marketplace::update_product).delete(marketplace::archive_product),
        )
        .route(
            "/api/provider/dashboard",
            get(marketplace::provider_dashboard),
        )
        .route(
            "/api/provider/stripe/status",
            get(marketplace::provider_stripe_status),
        )
        .route(
            "/api/provider/stripe/connect",
            post(marketplace::provider_stripe_connect),
        )
        .route(
            "/api/provider/stripe/dashboard-login",
            post(marketplace::provider_stripe_dashboard_login),
        )
        .route("/api/admin/filters", get(marketplace::admin_filter_options))
        .route("/api/admin/dashboard", get(marketplace::admin_dashboard))
        .route("/api/orders", post(marketplace::create_order))
        .route("/api/orders/{order_id}", get(marketplace::order_status))
        .route("/api/stripe/webhook", post(marketplace::stripe_webhook))
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
        "INSERT INTO users (username, email, password_hash, email_verified, is_admin, role)
         VALUES ('admin', 'admin@localhost', ?, 1, 1, 'admin')",
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

async fn seed_provider3(pool: &SqlitePool) -> Result<()> {
    let existing: Option<(i64, String)> =
        sqlx::query_as("SELECT id, role FROM users WHERE username = 'provider3' COLLATE NOCASE")
            .fetch_optional(pool)
            .await
            .context("Could not check for the development provider account")?;
    let provider_id = if let Some((id, role)) = existing {
        if role != "provider" {
            anyhow::bail!("The development provider3 account exists with a non-provider role");
        }
        id
    } else {
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = Argon2::default()
            .hash_password(b"provider3", &salt)
            .map_err(|error| {
                anyhow::anyhow!("Could not hash the development provider password: {error}")
            })?
            .to_string();
        sqlx::query(
            "INSERT INTO users
                (username, email, password_hash, email_verified, role)
             VALUES ('provider3', 'provider3@example.com', ?, 1, 'provider')",
        )
        .bind(password_hash)
        .execute(pool)
        .await
        .context("Could not create the development provider account")?
        .last_insert_rowid()
    };

    let product_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM products WHERE sku = 'PROVIDER3-001' AND provider_user_id = ?
         )",
    )
    .bind(provider_id)
    .fetch_one(pool)
    .await
    .context("Could not check the development provider product")?;
    if !product_exists {
        sqlx::query(
            "INSERT INTO products
                (sku, name, description, base_price_cents, provider_user_id,
                 visible, active, stock_quantity)
             VALUES ('PROVIDER3-001', 'A DOLLAR',
                     'A one-dollar product for Stripe Connect test checkout.',
                     100, ?, 1, 1, 1000)",
        )
        .bind(provider_id)
        .execute(pool)
        .await
        .context("Could not create the development provider product")?;
    }
    tracing::info!("Development provider account ready (provider3/provider3)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;

    #[tokio::test]
    async fn marketplace_migration_preserves_legacy_payment_history() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(":memory:")
                    .foreign_keys(true),
            )
            .await
            .expect("in-memory database should connect");

        sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
            .execute(&pool)
            .await
            .expect("initial migration should apply");
        sqlx::query(
            "INSERT INTO users (username, email, password_hash, email_verified, is_admin)
             VALUES ('admin', 'admin@localhost', 'hash', 1, 1)",
        )
        .execute(&pool)
        .await
        .expect("legacy admin should insert");
        sqlx::query(
            "INSERT INTO products (sku, name, description, base_price_cents)
             VALUES ('TEST-001', 'Legacy test product', '', 1000)",
        )
        .execute(&pool)
        .await
        .expect("legacy product should insert");
        sqlx::query(
            "INSERT INTO payments (user_id, product_id, amount_cents, status)
             VALUES (1, 1, 1000, 'succeeded')",
        )
        .execute(&pool)
        .await
        .expect("legacy payment should insert");

        sqlx::raw_sql(include_str!("../migrations/0002_marketplace.sql"))
            .execute(&pool)
            .await
            .expect("marketplace migration should apply");
        sqlx::raw_sql(include_str!("../migrations/0003_stripe_connect.sql"))
            .execute(&pool)
            .await
            .expect("Stripe Connect migration should apply");
        seed_provider3(&pool)
            .await
            .expect("development provider fixture should seed");

        let admin_role: String = sqlx::query_scalar("SELECT role FROM users WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("legacy admin role should be available");
        let product_visible: bool = sqlx::query_scalar("SELECT visible FROM products WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("legacy product visibility should be available");
        let legacy_payments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payments")
            .fetch_one(&pool)
            .await
            .expect("legacy payment history should remain available");

        assert_eq!(admin_role, "admin");
        assert!(!product_visible);
        assert_eq!(legacy_payments, 1);

        let provider: (String, String, bool) = sqlx::query_as(
            "SELECT username, role, email_verified FROM users WHERE username = 'provider3'",
        )
        .fetch_one(&pool)
        .await
        .expect("development provider should be available");
        let product: (String, i64, i64) = sqlx::query_as(
            "SELECT name, base_price_cents, stock_quantity
             FROM products WHERE sku = 'PROVIDER3-001'",
        )
        .fetch_one(&pool)
        .await
        .expect("development provider product should be available");
        assert_eq!(
            provider,
            ("provider3".to_owned(), "provider".to_owned(), true)
        );
        assert_eq!(product, ("A DOLLAR".to_owned(), 100, 1000));
    }
}
