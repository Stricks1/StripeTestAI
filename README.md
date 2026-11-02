# Stripe Test Store

A small, local-only Rust/Axum storefront for account confirmation and one Stripe test payment.

## Run locally

1. Copy `.env.example` to `.env`.
2. Leave `EMAIL_MODE=console` for local verification links, or configure SMTP as described below.
3. Add a Stripe **test** secret key (`sk_test_...`) and publishable key (`pk_test_...`) to `.env` when ready. The payment form stays unavailable until both test keys are configured.
4. Start the app:

   ```powershell
   cargo run
   ```

5. Open <http://127.0.0.1:3000>.

The first start creates the SQLite database and a sample product. In development, it also seeds the `admin` / `admin` account. Passwords are stored as Argon2 hashes. The local console email mode logs a one-time confirmation link and displays it after registration so the flow can be tested without an email provider.

## Email delivery

To send confirmation messages over SMTP, set `EMAIL_MODE=smtp` and configure `SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD`, and `SMTP_FROM`. The SMTP transport uses TLS. Keep provider credentials in `.env`, never in source control.

## Test payment flow

Sign in or create and confirm an account, choose **PRODUCTS**, and open the sample product's checkout. The amount is editable from USD $0.50 to $500.00 and is validated again by the server. Card entry uses Stripe.js Payment Element; card data is sent directly to Stripe and is never posted to or stored by this application. The Rust server creates a PaymentIntent and verifies its status against Stripe before displaying the result.

The payment API requires a signed-in user and allows up to 3 PaymentIntent creation attempts per client IP and per user account in separate rolling 15-minute windows. Further requests receive HTTP 429 with a `Retry-After` header. Because the IP limit is shared, users on the same home or office network share those 3 IP attempts; the separate account limit follows a user across IPs. The app requests 3D Secure for supported card payments; Stripe and the card issuer determine whether an authentication challenge can be performed. Reaching 5 PaymentIntents of USD $5.00 or less for the same account within 15 minutes writes a warning to the server log without automatically blocking the account.

Card numbers are entered through Stripe.js and are never available to this app. Stripe Radar evaluates payments using its built-in risk rules; review or configure fraud controls and any custom rules in the Stripe Dashboard's test-mode Radar settings. The application does not change Radar rules or store card numbers/fingerprints.

For Stripe's standard test card, use `4242 4242 4242 4242`, any future expiry date, and any three-digit CVC. Use Stripe test mode only.

See [STRIPE-REFERENCE.md](STRIPE-REFERENCE.md) for official Stripe documentation links, notes on this MVP's integration, and a guide to future Connect split payments versus customer installments.

## Local prototype limits

- The app binds to `127.0.0.1` by default. Do not expose it publicly with the seeded `admin` / `admin` password.
- Rate limiting uses the direct TCP peer address, not `X-Forwarded-For`, plus the authenticated user ID; both counters are in-memory, limited to a single running instance, and reset when the server restarts. A production deployment behind a proxy or using multiple instances needs trusted-proxy IP handling and shared rate-limit storage.
- Sessions are in-memory and reset when the server restarts.
- Stripe test keys are required for checkout; no key is included in this project.
- There is no production deployment, webhook-driven fulfillment, subscription billing, or admin dashboard in this prototype.
- A non-development run does not seed the default password; set a strong `BOOTSTRAP_ADMIN_PASSWORD` for the first admin account.
