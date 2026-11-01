# Product

<!-- impeccable:product-schema 1 -->

## Platform
web

## Stack
Rust with Axum and SQLite; confirmed by the user. Local development only for the first version.

## Users
A developer or tester locally signing up, logging in, and trying one test purchase.

## Product Purpose
A simple selling-site prototype for exercising account registration, login, product browsing, and a Stripe test payment.

## Positioning
No differentiated market position has been established; this is a deliberately small integration prototype.

## Operating Context
The first version runs locally and uses Stripe's test environment. No Stripe API key has been provided; credentials must be supplied through environment configuration.

## Capabilities and Constraints
- Registration collects a username, password, and email address; email is used for account confirmation.
- Users can log in and log out. The initial SQLite database includes a development-only `admin` / `admin` account.
- Logged-in users can open a products view containing one test product and begin checkout.
- Checkout accepts card details through Stripe's client-side components and an editable USD amount from $0.50 to $500.00.
- Payment creation is performed by the Rust server using Stripe test credentials; raw card details are not handled by the application server.
- Email delivery provider and production deployment requirements are undecided. The seeded admin credentials are not suitable for a publicly reachable deployment.

## Product Principles
- Keep the first release focused on one product and one test payment flow.
- Keep Stripe secret credentials server-side and out of source control.
- Validate payment amounts on the server, not only in the browser.
- Make development-only shortcuts explicit and unsuitable for production by default.

## Evidence on Hand
No brand assets, product catalog data, email provider, or Stripe credentials have been supplied. The first product is a test-only database record.
