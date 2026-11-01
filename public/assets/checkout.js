const amountForm = document.querySelector("#amount-form");
const amountInput = document.querySelector("#amount");
const checkoutMessage = document.querySelector("#checkout-message");
const paymentSection = document.querySelector("#payment-section");
const continueButton = document.querySelector("#continue-button");
const payButton = document.querySelector("#pay-button");
const editAmountButton = document.querySelector("#edit-amount");
const productId = Number(new URLSearchParams(window.location.search).get("product_id") || "1");
let stripe;
let elements;
let paymentId;

async function api(url, options = {}) {
  const response = await fetch(url, {
    ...options,
    headers: { "Content-Type": "application/json", ...options.headers },
  });
  const payload = await response.json();
  if (!response.ok) throw new Error(payload.error || "The request could not be completed.");
  return payload;
}

function showMessage(message, kind = "info") {
  checkoutMessage.textContent = message;
  checkoutMessage.dataset.kind = kind;
  checkoutMessage.hidden = false;
}

function parseCents(value) {
  const match = /^(\d{1,3})(?:\.(\d{1,2}))?$/.exec(value.trim());
  if (!match) return null;
  return Number(match[1]) * 100 + Number((match[2] || "").padEnd(2, "0"));
}

async function loadCheckout() {
  const session = await api("/api/auth/me");
  if (!session.user) {
    window.location.replace("/");
    return;
  }
  if (!Number.isSafeInteger(productId) || productId < 1) {
    showMessage("That product is unavailable.", "error");
    continueButton.disabled = true;
    return;
  }
  const products = await api("/api/products");
  const product = products.find((item) => item.id === productId);
  if (!product) {
    showMessage("That product is unavailable.", "error");
    continueButton.disabled = true;
    return;
  }
  document.querySelector("#product-name").textContent = product.name;
  document.querySelector("#product-sku").textContent = product.sku;
  document.querySelector("#product-description").textContent = product.description;
  amountInput.value = (product.base_price_cents / 100).toFixed(2);
}

amountForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (typeof window.Stripe !== "function") {
    showMessage("Stripe.js did not load. Check the connection and reload checkout.", "error");
    return;
  }
  const amountCents = parseCents(amountInput.value);
  if (amountCents === null || amountCents < 50 || amountCents > 50000) {
    showMessage("Choose an amount from USD $0.50 to USD $500.00, with no more than two decimals.", "error");
    return;
  }

  continueButton.disabled = true;
  checkoutMessage.hidden = true;
  try {
    const result = await api("/api/payment-intents", {
      method: "POST",
      body: JSON.stringify({ product_id: productId, amount_cents: amountCents }),
    });
    stripe = Stripe(result.publishable_key);
    elements = stripe.elements({
      clientSecret: result.client_secret,
      appearance: {
        theme: "stripe",
        variables: {
          colorPrimary: "#101216",
          colorText: "#101216",
          colorBackground: "#fffefa",
          borderRadius: "0px",
          fontFamily: '"Segoe UI", Arial, sans-serif',
        },
      },
    });
    elements.create("payment").mount("#payment-element");
    paymentId = result.payment_id;
    amountInput.disabled = true;
    continueButton.hidden = true;
    paymentSection.hidden = false;
    payButton.textContent = `PAY ${formatUsd(result.amount_cents)} SECURELY`;
  } catch (error) {
    showMessage(error.message, "error");
  } finally {
    continueButton.disabled = false;
  }
});

editAmountButton.addEventListener("click", () => {
  if (elements) elements.getElement("payment")?.destroy();
  elements = undefined;
  paymentId = undefined;
  amountInput.disabled = false;
  paymentSection.hidden = true;
  continueButton.hidden = false;
  checkoutMessage.hidden = true;
  amountInput.focus();
});

payButton.addEventListener("click", async () => {
  if (!stripe || !elements || !paymentId) {
    showMessage("Enter the amount and load the secure card form first.", "error");
    return;
  }
  payButton.disabled = true;
  checkoutMessage.hidden = true;
  try {
    const result = await stripe.confirmPayment({
      elements,
      confirmParams: {
        return_url: `${window.location.origin}/payment/return?payment_id=${encodeURIComponent(paymentId)}`,
      },
      redirect: "if_required",
    });
    if (result.error) {
      showMessage(result.error.message || "Stripe could not confirm the payment.", "error");
      return;
    }
    window.location.assign(`/payment/return?payment_id=${encodeURIComponent(paymentId)}`);
  } catch (error) {
    showMessage(error.message, "error");
  } finally {
    payButton.disabled = false;
  }
});

function formatUsd(cents) {
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(cents / 100);
}

loadCheckout().catch((error) => showMessage(error.message, "error"));
