# Stripe integration reference

This is a local, project-specific index to Stripe's official documentation, reviewed on 2026-11-01. It records the current marketplace integration and decisions to revisit for future payment features. Stripe's online documentation is authoritative and can change; consult the linked pages again before implementing or upgrading an integration. This file intentionally summarizes the guidance instead of copying Stripe's documentation.

## Current marketplace integration

- The marketplace creates one server-side PaymentIntent per reserved order and uses Stripe.js Payment Element. Card details go directly to Stripe; the application does not handle or store them.
- Signed webhooks confirm payments, refresh Express connected-account status, create provider transfers, and reverse transfers proportionally for full or partial refunds. Configure `STRIPE_WEBHOOK_SECRET` for both platform and connected-account events; the [README](README.md) includes Stripe CLI setup.
- Orders, item snapshots, stock reservations, Connect account IDs, 90/10 provider allocations, transfer IDs, reversal records, payment identifiers, fee estimates, and webhook event IDs are persisted in SQLite. Migration `0003_stripe_connect.sql` adds the Connect records while preserving legacy rows.
- Providers connect US Stripe Express accounts through Stripe-hosted onboarding. Stripe collects identity and payout information; the application does not collect or store it. Products are listed in the customer catalog only after the account's transfer capability is active.
- The platform charges the full order, then transfers 90% of each provider's gross order value, using the original charge as the source transaction. The platform retains 10% and pays Stripe fees. Disputes and manual reconciliation remain the platform's responsibility.
- The Rust marketplace and webhook flow lives in `src/marketplace.rs`; environment configuration is in `src/state.rs`.

### Current integration documentation

- [Stripe getting started](https://docs.stripe.com/get-started)
- [Development environment and Stripe CLI](https://docs.stripe.com/get-started/development-environment)
- [API keys](https://docs.stripe.com/keys) — secret keys belong on the server and must not be committed or shared in chat.
- [Test your integration](https://docs.stripe.com/testing)
- [Payment Intents](https://docs.stripe.com/payments/payment-intents)
- [Payment Element with Checkout Sessions](https://docs.stripe.com/payments/checkout) and [hosted Checkout quickstart](https://docs.stripe.com/checkout/quickstart)
- [Radar fraud prevention rules](https://docs.stripe.com/radar/rules) and [card-testing prevention](https://docs.stripe.com/disputes/prevention/card-testing) — built-in Radar checks apply to transactions; review risk controls and any custom rules in the Dashboard. The application intentionally does not collect card numbers.

**Future review:** Stripe's current Payment Intents guidance recommends Checkout Sessions with the Payment Element for most integrations. Before expanding the custom PaymentIntent flow, compare Checkout Sessions and its fulfillment model against the marketplace's needs; it is not a drop-in API substitution.

## Future payments: installments and BNPL

Connected-account revenue sharing is implemented above. Installments can mean either a lender paying the seller up front or the seller collecting a fixed number of monthly charges. These are different business and Stripe flows. Choose based on who receives funds, who bears collection risk, and the countries, currencies, and payment methods involved; do not model them as one generic payment option.

### Current marketplace fund flow

The implementation uses [separate charges and transfers](https://docs.stripe.com/connect/separate-charges-and-transfers) because one cart can contain multiple providers. Stripe receives the full PaymentIntent on the platform account; after `payment_intent.succeeded`, the app creates one transfer per provider with `source_transaction` and an order-level transfer group. Checkout distinguishes a successful customer payment from completion of all provider transfers; webhook retries reuse deterministic idempotency keys.

The platform is responsible for Stripe processing fees, refunds, and disputes. On `charge.refunded`, the app reverses each connected account's proportional share of its transfer. Disputes do not yet trigger automated transfer reversals or dispute workflows and require platform-side handling.

Providers use US Stripe Express accounts and hosted onboarding. Stripe collects identity and payout information; the application does not collect or store it.

### Letting a customer pay over time

Stripe documents distinct options:

1. **Fixed number of seller-collected payments:** [Installment plans in recurring payments](https://docs.stripe.com/recurring-payments#installment-plans) describes charging a limited number of recurring partial payments, and points to [Subscription Schedules](https://docs.stripe.com/billing/subscriptions/subscription-schedules) for implementing the plan. Define the installment count, cadence, total price, due dates, cancellation/refund policy, and handling of failed or late installments before implementation.
2. **Buy now, pay later (BNPL):** [Stripe BNPL methods](https://docs.stripe.com/payments/buy-now-pay-later) let supported providers offer customers repayment over time. Provider availability and eligibility depend on business/customer location, currency, transaction amount, and provider rules; verify the target markets rather than assuming a method is available.
3. **Card-network installments:** Some options are country-specific; for example, Stripe documents [Mexico card installments](https://docs.stripe.com/payments/mx-installments). This is not a universal card feature.

Decide whether the business needs the seller to collect each installment or wants an eligible third-party BNPL provider to collect from the customer. Those choices affect checkout, payment state, accounting, refunds, and fulfillment.

## Reliability before production use

- Use [webhooks](https://docs.stripe.com/webhooks) for asynchronous and authoritative payment lifecycle updates. Verify signatures, handle duplicate and out-of-order delivery safely, and persist event processing so retries are idempotent.
- Use [idempotency keys](https://docs.stripe.com/api/idempotent_requests) for server-side create operations that may be retried.
- Keep secrets in local environment configuration or a production secret manager. Use test-mode keys and test events while developing; never paste secret keys into chat or commit them.
- Consult current Stripe API versions, supported regions, currencies, and method-specific requirements at implementation time.

## Marketplace allocation policy

Each provider receives 90% of its gross order total and the platform retains 10%. Stripe's actual fee is read from the charge's expanded balance transaction when available; otherwise the dashboard estimates it at 3.99% + USD $0.39. Fees are paid by the platform and do not reduce provider transfers. Full and partial refunds reverse the corresponding proportional transfer amount. The platform's estimated net is its commission less Stripe fees and its 10% share of refunds.

## Suggested reading order for the next payment feature

1. Define installment count, cadence, total price, cancellation/refund policy, and handling of failed or late installments.
2. Confirm the countries, currencies, and payment methods required for the customer and provider.
3. Review the matching Stripe product docs above, then model payment attempts and schedules before changing checkout.
4. Re-run the flow with Stripe test keys and CLI/webhook test events before considering live-mode setup.
