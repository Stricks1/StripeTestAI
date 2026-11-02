use anyhow::{Context, Result, anyhow};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::Mailbox,
    transport::smtp::authentication::Credentials,
};
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const PAYMENT_ATTEMPT_LIMIT: usize = 3;
const PAYMENT_ATTEMPT_WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_TRACKED_PAYMENT_IDENTITIES: usize = 10_000;

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
enum PaymentRateLimitKey {
    Ip(IpAddr),
    User(i64),
}

#[derive(Clone, Default)]
pub struct PaymentRateLimiter {
    attempts: Arc<Mutex<HashMap<PaymentRateLimitKey, VecDeque<Instant>>>>,
}

impl PaymentRateLimiter {
    pub async fn check_ip(&self, ip: IpAddr) -> Result<(), u64> {
        self.check_at(PaymentRateLimitKey::Ip(ip), Instant::now())
            .await
    }

    pub async fn check_user(&self, user_id: i64) -> Result<(), u64> {
        self.check_at(PaymentRateLimitKey::User(user_id), Instant::now())
            .await
    }

    async fn check_at(&self, key: PaymentRateLimitKey, now: Instant) -> Result<(), u64> {
        let mut attempts = self.attempts.lock().await;
        if !attempts.contains_key(&key) && attempts.len() >= MAX_TRACKED_PAYMENT_IDENTITIES {
            attempts.retain(|_, timestamps| {
                prune_expired(timestamps, now);
                !timestamps.is_empty()
            });
            if attempts.len() >= MAX_TRACKED_PAYMENT_IDENTITIES {
                tracing::warn!("Payment rate limiter reached its tracked identity capacity");
                return Err(PAYMENT_ATTEMPT_WINDOW.as_secs());
            }
        }

        let timestamps = attempts.entry(key).or_default();
        prune_expired(timestamps, now);
        if timestamps.len() >= PAYMENT_ATTEMPT_LIMIT {
            let elapsed = now.saturating_duration_since(*timestamps.front().expect("non-empty"));
            let remaining = PAYMENT_ATTEMPT_WINDOW.saturating_sub(elapsed);
            return Err(remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0));
        }

        timestamps.push_back(now);
        Ok(())
    }
}

fn prune_expired(timestamps: &mut VecDeque<Instant>, now: Instant) {
    while timestamps.front().is_some_and(|timestamp| {
        now.saturating_duration_since(*timestamp) >= PAYMENT_ATTEMPT_WINDOW
    }) {
        timestamps.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allows_three_attempts_and_rejects_the_fourth_with_retry_time() {
        let limiter = PaymentRateLimiter::default();
        let ip = IpAddr::from([127, 0, 0, 1]);
        let now = Instant::now();

        assert_eq!(
            limiter.check_at(PaymentRateLimitKey::Ip(ip), now).await,
            Ok(())
        );
        assert_eq!(
            limiter.check_at(PaymentRateLimitKey::Ip(ip), now).await,
            Ok(())
        );
        assert_eq!(
            limiter.check_at(PaymentRateLimitKey::Ip(ip), now).await,
            Ok(())
        );
        assert_eq!(
            limiter.check_at(PaymentRateLimitKey::Ip(ip), now).await,
            Err(PAYMENT_ATTEMPT_WINDOW.as_secs())
        );
    }

    #[tokio::test]
    async fn expires_attempts_after_the_window_and_tracks_ips_separately() {
        let limiter = PaymentRateLimiter::default();
        let first_ip = IpAddr::from([127, 0, 0, 1]);
        let second_ip = IpAddr::from([127, 0, 0, 2]);
        let now = Instant::now();

        for _ in 0..PAYMENT_ATTEMPT_LIMIT {
            assert_eq!(
                limiter
                    .check_at(PaymentRateLimitKey::Ip(first_ip), now)
                    .await,
                Ok(())
            );
        }
        assert_eq!(
            limiter
                .check_at(PaymentRateLimitKey::Ip(second_ip), now)
                .await,
            Ok(())
        );
        assert_eq!(
            limiter
                .check_at(
                    PaymentRateLimitKey::Ip(first_ip),
                    now + PAYMENT_ATTEMPT_WINDOW
                )
                .await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn user_attempts_are_shared_across_ips_and_isolated_between_users() {
        let limiter = PaymentRateLimiter::default();
        let now = Instant::now();
        let ip = IpAddr::from([127, 0, 0, 1]);

        for _ in 0..PAYMENT_ATTEMPT_LIMIT {
            assert_eq!(
                limiter.check_at(PaymentRateLimitKey::User(42), now).await,
                Ok(())
            );
        }
        assert_eq!(
            limiter.check_at(PaymentRateLimitKey::User(42), now).await,
            Err(PAYMENT_ATTEMPT_WINDOW.as_secs())
        );
        assert_eq!(
            limiter.check_at(PaymentRateLimitKey::User(43), now).await,
            Ok(())
        );
        assert_eq!(
            limiter.check_at(PaymentRateLimitKey::Ip(ip), now).await,
            Ok(())
        );
    }
}

#[derive(Clone)]
pub enum EmailDelivery {
    Console,
    Smtp {
        host: String,
        port: u16,
        username: String,
        password: String,
        from: String,
    },
}

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub stripe_http: reqwest::Client,
    pub stripe_secret_key: Option<String>,
    pub stripe_publishable_key: Option<String>,
    pub payment_rate_limiter: PaymentRateLimiter,
    pub app_env: String,
    pub base_url: String,
    email_delivery: EmailDelivery,
}

impl AppState {
    pub fn from_env(pool: sqlx::SqlitePool) -> Result<Self> {
        let email_delivery = match std::env::var("EMAIL_MODE")
            .unwrap_or_else(|_| "console".to_owned())
            .as_str()
        {
            "console" => EmailDelivery::Console,
            "smtp" => EmailDelivery::Smtp {
                host: required_env("SMTP_HOST")?,
                port: required_env("SMTP_PORT")?
                    .parse()
                    .context("SMTP_PORT must be a valid port number")?,
                username: required_env("SMTP_USERNAME")?,
                password: required_env("SMTP_PASSWORD")?,
                from: required_env("SMTP_FROM")?,
            },
            other => {
                return Err(anyhow!(
                    "EMAIL_MODE must be either console or smtp, not {other}"
                ));
            }
        };

        Ok(Self {
            pool,
            stripe_http: reqwest::Client::new(),
            stripe_secret_key: optional_env("STRIPE_SECRET_KEY"),
            stripe_publishable_key: optional_env("STRIPE_PUBLISHABLE_KEY"),
            payment_rate_limiter: PaymentRateLimiter::default(),
            app_env: std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_owned()),
            base_url: std::env::var("APP_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_owned()),
            email_delivery,
        })
    }

    pub fn exposes_development_confirmation_link(&self) -> bool {
        matches!(&self.email_delivery, EmailDelivery::Console) && self.app_env == "development"
    }

    pub async fn send_confirmation(&self, email: &str, url: &str) -> Result<()> {
        match &self.email_delivery {
            EmailDelivery::Console => {
                tracing::info!(recipient = %email, confirmation_url = %url, "Development email confirmation link");
                Ok(())
            }
            EmailDelivery::Smtp {
                host,
                port,
                username,
                password,
                from,
            } => {
                let message = Message::builder()
                    .from(from.parse::<Mailbox>().context("SMTP_FROM must be a valid email address")?)
                    .to(email.parse::<Mailbox>().context("Recipient email is not valid")?)
                    .subject("Confirm your Test Store account")
                    .body(format!(
                        "Confirm your Test Store account by opening this link:\n\n{url}\n\nIf you did not create this account, you can ignore this message."
                    ))
                    .context("Could not create confirmation email")?;
                let transport = AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                    .context("Could not configure SMTP transport")?
                    .port(*port)
                    .credentials(Credentials::new(username.clone(), password.clone()))
                    .build();
                transport
                    .send(message)
                    .await
                    .context("SMTP server could not deliver the confirmation email")?;
                Ok(())
            }
        }
    }
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("{name} is required when EMAIL_MODE=smtp"))
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}
