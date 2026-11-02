const { api, formatUsd, showMessage } = window.Marketplace;
const paymentForm = document.querySelector("#payment-form");
const paymentElementContainer = document.querySelector("#payment-element");
const checkoutMessage = document.querySelector("#checkout-message");
const payButton = document.querySelector("#pay-button");
const summary = document.querySelector("#checkout-summary");
let stripe;
let elements;
let activeOrderId;
let activeCheckoutKey;
let cartStorageKey;
let checkoutStorageKey;

function showCheckoutMessage(message, kind = "info") {
  showMessage(checkoutMessage, message, kind);
}

function renderOrder(order) {
  summary.replaceChildren();
  const heading = document.createElement("h2");
  heading.textContent = "Order summary";
  const list = document.createElement("div");
  list.className = "checkout-lines";
  for (const item of order.items) {
    const line = document.createElement("div");
    line.className = "checkout-line";
    const itemLabel = document.createElement("span");
    itemLabel.textContent = `${item.quantity} × ${item.product_name}`;
    const amount = document.createElement("strong");
    amount.textContent = formatUsd(item.line_total_cents);
    line.append(itemLabel, amount);
    list.append(line);
  }
  const total = document.createElement("div");
  total.className = "checkout-total";
  const totalLabel = document.createElement("span");
  totalLabel.textContent = "Total";
  const totalAmount = document.createElement("strong");
  totalAmount.textContent = formatUsd(order.total_amount_cents);
  total.append(totalLabel, totalAmount);
  summary.append(heading, list, total);
}

async function loadCheckout() {
  showCheckoutMessage("Preparing your order and secure payment form.");
  const { user } = await api("/api/auth/me");
  if (!user || user.role !== "customer") {
    window.location.replace("/");
    return;
  }
  const storageIdentity = encodeURIComponent(user.username.toLowerCase());
  cartStorageKey = `marketplace-cart:${storageIdentity}`;
  checkoutStorageKey = `marketplace-checkout:${storageIdentity}`;
  if (typeof window.Stripe !== "function") {
    throw new Error("Stripe.js did not load. Check your connection and reload checkout.");
  }
  const queryOrderId = new URLSearchParams(window.location.search).get("order_id");
  if (queryOrderId) {
    if (!/^\d+$/.test(queryOrderId)) throw new Error("The order reference is invalid.");
    activeOrderId = Number(queryOrderId);
    activeCheckoutKey = new URLSearchParams(window.location.search).get("checkout_key");
    await pollOrderStatus();
    return;
  }

  let checkout;
  try {
    checkout = JSON.parse(localStorage.getItem(checkoutStorageKey) || "null");
  } catch (error) {
    throw new Error("The saved checkout could not be read. Return to the catalog and try again.");
  }
  if (!checkout || !Array.isArray(checkout.items) || checkout.items.length === 0) {
    throw new Error("Your cart is empty. Return to the catalog and select products first.");
  }
  activeCheckoutKey = checkout.checkout_key;
  const payment = await api("/api/orders", {
    method: "POST",
    body: JSON.stringify(checkout),
  });
  activeOrderId = payment.order_id;
  const order = await api(`/api/orders/${encodeURIComponent(activeOrderId)}`);
  renderOrder(order);
  checkoutMessage.hidden = true;
  stripe = Stripe(payment.publishable_key);
  elements = stripe.elements({
    clientSecret: payment.client_secret,
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
  elements.create("payment", { layout: "tabs" }).mount("#payment-element");
  payButton.textContent = `PAY ${formatUsd(payment.amount_cents)}`;
  payButton.disabled = false;
}

async function pollOrderStatus() {
  paymentForm.hidden = true;
  showCheckoutMessage("Checking the payment status with the marketplace.");
  let lastOrder;
  for (let attempt = 0; attempt < 20; attempt += 1) {
    lastOrder = await api(`/api/orders/${encodeURIComponent(activeOrderId)}`);
    renderOrder(lastOrder);
    if (lastOrder.status === "paid") {
      checkoutMessage.hidden = true;
      showCheckoutMessage("Payment confirmed by Stripe. Your order is complete.");
      try {
        const savedCheckout = JSON.parse(localStorage.getItem(checkoutStorageKey) || "null");
        if (activeCheckoutKey && savedCheckout?.checkout_key === activeCheckoutKey) {
          localStorage.removeItem(cartStorageKey);
          localStorage.removeItem(checkoutStorageKey);
        }
      } catch (error) {
        showCheckoutMessage("The order is complete, but the saved cart could not be cleared.", "error");
        return;
      }
      return;
    }
    if (["canceled", "failed", "refunded"].includes(lastOrder.status)) {
      showCheckoutMessage(`This order is ${lastOrder.status}. Return to the catalog to start a new checkout.`, "error");
      return;
    }
    await new Promise((resolve) => window.setTimeout(resolve, 1500));
  }
  if (lastOrder) {
    showCheckoutMessage(
      "Stripe is still confirming this order. Keep this page open or check again shortly."
    );
  }
}

paymentForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!stripe || !elements || !activeOrderId) {
    payButton.disabled = true;
    try {
      await loadCheckout();
    } catch (error) {
      showCheckoutMessage(error.message, "error");
      payButton.textContent = "RETRY CHECKOUT";
      payButton.disabled = false;
    }
    return;
  }
  payButton.disabled = true;
  checkoutMessage.hidden = true;
  try {
    const result = await stripe.confirmPayment({
      elements,
      confirmParams: {
        return_url: `${window.location.origin}/checkout?order_id=${encodeURIComponent(activeOrderId)}&checkout_key=${encodeURIComponent(activeCheckoutKey || "")}`,
      },
      redirect: "if_required",
    });
    if (result.error) {
      showCheckoutMessage(result.error.message || "Stripe could not confirm this payment.", "error");
      return;
    }
    await pollOrderStatus();
  } catch (error) {
    showCheckoutMessage(error.message, "error");
  } finally {
    payButton.disabled = false;
  }
});

loadCheckout().catch((error) => {
  showCheckoutMessage(error.message, "error");
  payButton.textContent = "RETRY CHECKOUT";
  payButton.disabled = false;
});
