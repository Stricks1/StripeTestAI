# Stripe integration reference

This is a local, project-specific index to Stripe's official documentation, reviewed on 2026-11-01. It records the current MVP integration and the decisions to revisit for future payment features. Stripe's online documentation is authoritative and can change; consult the linked pages again before implementing or upgrading an integration. This file intentionally summarizes the guidance instead of copying Stripe's documentation.

## Current MVP

- The storefront uses a server-created PaymentIntent and Stripe.js Payment Element. The server validates the USD amount, requires test-mode keys, stores the Stripe PaymentIntent ID and payment status in SQLite, and checks the intent status with Stripe before showing a result.
- Card details go directly to Stripe; they are not handled or stored by the application.
- The Rust request and response flow lives in `src/payments.rs`; environment configuration is in `src/state.rs`; persisted payment fields are in `migrations/0001_initial.sql`; local setup is in `README.md`.
- The current flow is a deliberately small prototype, not a webhook-backed fulfillment or recurring-billing system.

### Current integration documentation

- [Stripe getting started](https://docs.stripe.com/get-started)
- [Development environment and Stripe CLI](https://docs.stripe.com/get-started/development-environment)
- [API keys](https://docs.stripe.com/keys) — secret keys belong on the server and must not be committed or shared in chat.
- [Test your integration](https://docs.stripe.com/testing)
- [Payment Intents](https://docs.stripe.com/payments/payment-intents)
- [Payment Element with Checkout Sessions](https://docs.stripe.com/payments/checkout) and [hosted Checkout quickstart](https://docs.stripe.com/checkout/quickstart)

**Future review:** Stripe's current Payment Intents guidance recommends Checkout Sessions with the Payment Element for most integrations. The MVP's custom PaymentIntent flow is working as implemented, but compare the current recommendation and feature requirements before expanding it. Checkout Sessions uses a different client integration; it is not a drop-in API substitution.

## Future payments: identify the requirement first

“Split payments” can mean distributing a single charge among sellers, while “installments” can mean either a lender paying the seller up front or the seller collecting a fixed number of monthly charges. These are different business and Stripe flows. Choose based on who receives funds, who bears collection risk, and the countries, currencies, and payment methods involved; do not model them as one generic payment option.

### Distributing a payment among sellers

For a marketplace/platform paying one or more Stripe connected accounts, review [Stripe Connect](https://docs.stripe.com/connect) and [separate charges and transfers](https://docs.stripe.com/connect/separate-charges-and-transfers). Stripe describes separate charges and transfers for cases such as splitting one payment among multiple connected accounts or when the recipient is not known at charge time. Compare it with [destination charges](https://docs.stripe.com/connect/destination-charges) when one connected account receives a payment.

Before designing the schema or checkout, confirm:

- Whether the platform or a seller is the merchant responsible for the charge.
- Whether a payment can be allocated to one seller or several, and how the amounts and platform fee are calculated.
- Which account owns refunds, disputes, Stripe fees, transfer reversals, and negative-balance risk for the chosen charge type.
- Required connected-account onboarding, identity verification, country availability, currencies, and settlement timing.

Keep the original charge, each transfer/allocation, and any reversal/refund traceable to the same order. Do not treat a successful customer charge alone as proof that all seller transfers completed.

### Letting a customer pay over time

Stripe documents distinct options:

1. **Fixed number of seller-collected payments:** [Installment plans in recurring payments](https://docs.stripe.com/recurring-payments#installment-plans) describes charging a limited number of recurring partial payments, and points to [Subscription Schedules](https://docs.stripe.com/billing/subscriptions/subscription-schedules) for implementing the plan. Define the installment count, cadence, total price, due dates, cancellation/refund policy, and handling of failed or late installments before implementation.
2. **Buy now, pay later (BNPL):** [Stripe BNPL methods](https://docs.stripe.com/payments/buy-now-pay-later) let supported providers offer customers repayment over time. Provider availability and eligibility depend on business/customer location, currency, transaction amount, and provider rules; verify the target markets rather than assuming a method is available.
3. **Card-network installments:** Some options are country-specific; for example, Stripe documents [Mexico card installments](https://docs.stripe.com/payments/mx-installments). This is not a universal card feature.

Decide whether the business needs the seller to collect each installment or wants an eligible third-party BNPL provider to collect from the customer. Those choices affect checkout, payment state, accounting, refunds, and fulfillment.

## Reliability before production use

- Use [webhooks](https://docs.stripe.com/webhooks) for asynchronous and authoritative payment lifecycle updates before adding fulfillment or multi-party transfers. Verify webhook signatures, handle duplicate and out-of-order delivery safely, and persist event processing so retries are idempotent.
- Use [idempotency keys](https://docs.stripe.com/api/idempotent_requests) for server-side create operations that may be retried.
- Keep secrets in local environment configuration or a production secret manager. Use test-mode keys and test events while developing; never paste secret keys into chat or commit them.
- Consult current Stripe API versions, supported regions, currencies, and method-specific requirements at implementation time.

## Suggested reading order for the next payment feature

1. Clarify whether “split” means seller/platform fund allocation, customer installments, BNPL, or more than one of these.
2. Confirm business and customer countries, currencies, who is the seller/merchant, the number of recipients, and who bears refunds and missed-installment risk.
3. Review the matching Stripe product docs above, then model orders, payment attempts, installment schedules or transfer allocations, and webhook events before changing checkout.
4. Re-run the flow with Stripe test keys and CLI/webhook test events before considering live-mode setup.
