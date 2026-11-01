const heading = document.querySelector("#result-heading");
const copy = document.querySelector("#result-copy");
const amountOutput = document.querySelector("#result-amount");
const paymentId = new URLSearchParams(window.location.search).get("payment_id");

async function loadPaymentStatus() {
  if (!paymentId || !/^\d+$/.test(paymentId)) {
    showResult("Payment not found.", "The payment reference is missing or invalid.");
    return;
  }
  try {
    const response = await fetch(`/api/payments/${encodeURIComponent(paymentId)}`);
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || "The payment status could not be checked.");
    amountOutput.textContent = new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: result.currency.toUpperCase(),
    }).format(result.amount_cents / 100);

    if (result.status === "succeeded") {
      showResult("Payment complete.", "Stripe confirmed this test payment.");
    } else if (result.status === "processing") {
      showResult("Payment processing.", "Stripe is still processing the test payment. Check again shortly.");
    } else {
      showResult("Payment not completed.", "No successful payment was recorded. Return to the product and try again.");
    }
  } catch (error) {
    showResult("Payment status unavailable.", error.message);
  }
}

function showResult(title, message) {
  heading.textContent = title;
  copy.textContent = message;
}

loadPaymentStatus();
