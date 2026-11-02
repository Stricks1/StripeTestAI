# Stripe Test Marketplace

A local Rust/Axum and SQLite marketplace prototype with customer, provider, and admin workspaces. Stripe test-mode payments use PaymentIntents, Stripe.js Payment Element, and Stripe Connect transfers.

## Run locally

1. Copy `.env.example` to `.env`:

   ```powershell
   Copy-Item .env.example .env
   ```

2. Keep `EMAIL_MODE=console` to use local confirmation links, or configure SMTP.
3. Add your Stripe **test-mode** secret and publishable keys to `.env`. Never commit `.env` or paste secret keys into chat.
4. Start the server:

   ```powershell
   cargo run
   ```

5. Open <http://127.0.0.1:3000>.

The first development start creates the SQLite database and an `admin` / `admin` account. This is local-only; change the password and do not expose the app publicly. Passwords are stored as Argon2 hashes. Console email mode logs a one-time confirmation link after registration.

## Roles and marketplace flow

- **Admin:** sign in with the seeded development account to view platform sales, products, provider totals, and the Stripe test-account balance.
- **Provider:** register and confirm a provider account, then connect a US Stripe account from the provider overview. Stripe-hosted Express onboarding collects identity and payout details. Products appear in the customer catalog after Stripe enables transfers. Providers can then create, edit, or archive products.
- **Customer:** register and confirm a customer account, browse and filter the catalog, select quantities, and pay for the cart.

Stripe Payment Element sends card details directly to Stripe. The application server does not receive or store card numbers. The server requests 3D Secure when supported; Stripe and the card issuer decide whether a challenge is required.

## Stripe Connect split payments

The platform creates one PaymentIntent for the full customer order. After Stripe confirms payment by webhook, the server creates an idempotent transfer for **90% of each provider's gross order total** to that provider's connected account; the platform retains 10%. Carts can include products from multiple providers. Stripe processing fees are paid by the platform and do not reduce the provider's 90% share.

The platform owns the original charge. Full and partial `charge.refunded` events trigger proportional, idempotent reversals of provider transfers. Disputes and any manual reconciliation remain the platform's responsibility. Providers can open their Express dashboard from the provider workspace; each click requests a fresh, temporary login link from Stripe. The provider dashboard reports gross sales; Stripe controls when transferred funds are paid out to a provider's bank.

In development, the app seeds `provider3` / `provider3` and an `A DOLLAR` USD $1.00 product. Sign in as that provider, select **Connect with Stripe**, and complete onboarding with Stripe test data. This fixture is development-only; it does not create a connected account or bypass Stripe verification until the provider starts and completes the hosted onboarding flow.

## Stripe webhooks and inventory

Payment fulfillment, connected-account status, transfers, and refund reversals are processed from signed Stripe webhooks. For local development, install the [Stripe CLI](https://docs.stripe.com/stripe-cli), then in a second terminal run:

```powershell
stripe listen --forward-to http://127.0.0.1:3000/api/stripe/webhook --forward-connect-to http://127.0.0.1:3000/api/stripe/webhook
```

Copy the `whsec_...` signing secret printed by this combined CLI listener into `STRIPE_WEBHOOK_SECRET` in `.env`, then restart the app. Do not use a webhook signing secret from a different listener. The server verifies webhook signatures, rejects stale signatures, and records event IDs to make webhook retries idempotent. Connect must be enabled for the platform's Stripe test account.

Checkout reserves stock for 30 minutes. Confirmed cancellation releases the reservation; a failed Stripe cancellation leaves stock reserved rather than risking overselling. Refunds are recorded, but refunded stock is not automatically restocked.

## Accounting rules

The platform's commission is **10% of gross** and the provider transfer is **90% of gross**. Stripe's actual fee is used when its balance transaction is available; until then the admin dashboard estimates it at **3.99% + USD $0.39**. The platform pays that fee from its commission and absorbs any shortfall. Subscriptions and customer installment plans are not implemented. See [STRIPE-REFERENCE.md](STRIPE-REFERENCE.md) for Stripe integration notes.

## Email delivery

To send confirmation messages over SMTP, set `EMAIL_MODE=smtp` and configure `SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD`, and `SMTP_FROM`. The SMTP transport uses TLS. Keep provider credentials in `.env`, never in source control.

## Fraud controls and test cards

Checkout requires a signed-in customer and permits up to 3 order/PaymentIntent creation attempts per direct client IP and per user account in separate rolling 15-minute windows. Further requests receive HTTP 429 with `Retry-After`. Users on the same home or office network share the IP limit; the account limit follows a user across IPs. Reaching 5 PaymentIntents of USD $5.00 or less for one account within 15 minutes writes a warning to the server log but does not automatically block the account.

Stripe Radar evaluates payments using its built-in risk rules. Review or configure risk controls in the Stripe Dashboard's test-mode Radar settings. The application does not change Radar rules or store card numbers/fingerprints.

Use Stripe test mode only. For the standard test card, use `4242 4242 4242 4242`, any future expiry date, and any three-digit CVC.

## Local prototype limits

- The app binds to `127.0.0.1:3000` by default. Do not expose it publicly with the seeded `admin` / `admin` password.
- Sessions and rate limits are in-memory and reset when the server restarts. Rate limiting uses the direct TCP peer address, not `X-Forwarded-For`; production behind a proxy or multiple instances needs trusted proxy configuration and shared rate-limit storage.
- Webhook fulfillment requires `STRIPE_WEBHOOK_SECRET`; without it, webhook requests return an explicit service-unavailable response.
- In non-development environments, set a strong `BOOTSTRAP_ADMIN_PASSWORD` for the first admin account.
