use crate::{error::AppError, state::AppState};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
};
use email_address::EmailAddress;
use rand::rngs::OsRng;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use std::time::{SystemTime, UNIX_EPOCH};
use tower_sessions::Session;
use uuid::Uuid;

const EMAIL_TOKEN_TTL_SECONDS: i64 = 24 * 60 * 60;

#[derive(Deserialize)]
pub struct RegisterInput {
    username: String,
    email: String,
    password: String,
}

#[derive(Deserialize)]
pub struct LoginInput {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct VerifyInput {
    token: String,
}

#[derive(FromRow)]
struct UserRecord {
    id: i64,
    username: String,
    email: String,
    password_hash: String,
    email_verified: bool,
    is_admin: bool,
}

#[derive(FromRow)]
struct VerificationRecord {
    user_id: i64,
    expires_at: i64,
}

#[derive(serde::Serialize)]
struct PublicUser {
    username: String,
    email: String,
    is_admin: bool,
}

pub async fn index_page() -> Html<&'static str> {
    Html(include_str!("../public/index.html"))
}

pub async fn verification_page() -> Html<&'static str> {
    Html(include_str!("../public/verify.html"))
}

pub async fn register(
    State(state): State<AppState>,
    Json(input): Json<RegisterInput>,
) -> Result<impl IntoResponse, AppError> {
    let username = input.username.trim();
    let email = input.email.trim().to_ascii_lowercase();
    if !(3..=32).contains(&username.len())
        || !username
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(AppError::bad_request(
            "Username must be 3–32 characters and use only letters, numbers, dots, dashes, or underscores.",
        ));
    }
    if email.len() > 254 || !EmailAddress::is_valid(&email) {
        return Err(AppError::bad_request("Enter a valid email address."));
    }
    if !(8..=128).contains(&input.password.len()) {
        return Err(AppError::bad_request(
            "Password must be between 8 and 128 characters.",
        ));
    }

    let password_hash = hash_password(&input.password).map_err(|error| {
        tracing::error!(error = %error, "Could not hash registration password");
        AppError::internal("The account could not be created.")
    })?;
    let token = Uuid::new_v4().simple().to_string();
    let token_hash = hash_token(&token);
    let expires_at = unix_now()? + EMAIL_TOKEN_TTL_SECONDS;
    let mut transaction = state.pool.begin().await?;
    let inserted =
        sqlx::query("INSERT INTO users (username, email, password_hash) VALUES (?, ?, ?)")
            .bind(username)
            .bind(&email)
            .bind(password_hash)
            .execute(&mut *transaction)
            .await;
    let user_id = match inserted {
        Ok(result) => result.last_insert_rowid(),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "That username or email is already registered.",
            ));
        }
        Err(error) => return Err(error.into()),
    };

    sqlx::query(
        "INSERT INTO email_verifications (user_id, token_hash, expires_at) VALUES (?, ?, ?)",
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await?;

    let confirmation_url = format!("{}/verify?token={token}", state.base_url);
    state
        .send_confirmation(&email, &confirmation_url)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "Could not deliver confirmation email");
            AppError::internal(
                "The confirmation email could not be sent. Check the email settings and try again.",
            )
        })?;
    transaction.commit().await?;

    let mut response = json!({
        "message": "Confirm your email address before signing in."
    });
    if state.exposes_development_confirmation_link() {
        response["confirmation_url"] = json!(confirmation_url);
    }
    Ok(Json(response))
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
    Json(input): Json<LoginInput>,
) -> Result<impl IntoResponse, AppError> {
    let identifier = input.username.trim();
    let user = sqlx::query_as::<_, UserRecord>(
        "SELECT id, username, email, password_hash, email_verified, is_admin
         FROM users WHERE username = ? COLLATE NOCASE OR email = ? COLLATE NOCASE",
    )
    .bind(identifier)
    .bind(identifier)
    .fetch_optional(&state.pool)
    .await?;

    let Some(user) = user else {
        return Err(AppError::unauthorized("Username or password is incorrect."));
    };
    let parsed_hash = PasswordHash::new(&user.password_hash).map_err(|error| {
        tracing::error!(error = %error, "Stored password hash is invalid");
        AppError::internal("The account could not be accessed.")
    })?;
    if Argon2::default()
        .verify_password(input.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return Err(AppError::unauthorized("Username or password is incorrect."));
    }
    if !user.email_verified {
        return Err(AppError::forbidden(
            "Confirm your email address before signing in.",
        ));
    }

    session.cycle_id().await.map_err(|error| {
        tracing::error!(error = %error, "Could not rotate login session");
        AppError::internal("The session could not be started.")
    })?;
    session.insert("user_id", user.id).await.map_err(|error| {
        tracing::error!(error = %error, "Could not store login session");
        AppError::internal("The session could not be started.")
    })?;
    Ok(Json(json!({
        "user": PublicUser {
            username: user.username,
            email: user.email,
            is_admin: user.is_admin,
        }
    })))
}

pub async fn logout(session: Session) -> Result<impl IntoResponse, AppError> {
    session.delete().await.map_err(|error| {
        tracing::error!(error = %error, "Could not remove login session");
        AppError::internal("The session could not be ended.")
    })?;
    Ok(Json(json!({ "message": "Signed out." })))
}

pub async fn me(
    State(state): State<AppState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    let Some(user_id) = session_user_id(&session).await? else {
        return Ok(Json(json!({ "user": null })));
    };
    let user = sqlx::query_as::<_, PublicUserRecord>(
        "SELECT username, email, is_admin FROM users WHERE id = ? AND email_verified = 1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    let user = user.map(|row| PublicUser {
        username: row.username,
        email: row.email,
        is_admin: row.is_admin,
    });
    Ok(Json(json!({ "user": user })))
}

#[derive(FromRow)]
struct PublicUserRecord {
    username: String,
    email: String,
    is_admin: bool,
}

pub async fn verify_email(
    State(state): State<AppState>,
    Json(input): Json<VerifyInput>,
) -> Result<impl IntoResponse, AppError> {
    if input.token.len() != 32
        || !input
            .token
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(AppError::bad_request(
            "This confirmation link is invalid or expired.",
        ));
    }
    let record = sqlx::query_as::<_, VerificationRecord>(
        "SELECT user_id, expires_at FROM email_verifications WHERE token_hash = ?",
    )
    .bind(hash_token(&input.token))
    .fetch_optional(&state.pool)
    .await?;
    let now = unix_now()?;
    let Some(record) = record.filter(|record| record.expires_at >= now) else {
        return Err(AppError::bad_request(
            "This confirmation link is invalid or expired. Request a new one.",
        ));
    };

    let mut transaction = state.pool.begin().await?;
    sqlx::query("UPDATE users SET email_verified = 1 WHERE id = ?")
        .bind(record.user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM email_verifications WHERE user_id = ?")
        .bind(record.user_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(Json(
        json!({ "message": "Email confirmed. You can now sign in." }),
    ))
}

pub async fn session_user_id(session: &Session) -> Result<Option<i64>, AppError> {
    session.get::<i64>("user_id").await.map_err(|error| {
        tracing::error!(error = %error, "Could not read login session");
        AppError::internal("The session could not be read.")
    })
}

pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
}

fn hash_token(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
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
