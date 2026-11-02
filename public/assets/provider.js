const {
  api,
  appendCell,
  formatDate,
  formatUsd,
  logout,
  renderSalesChart,
  showMessage,
} = window.Marketplace;
const providerMessage = document.querySelector("#provider-message");
const productForm = document.querySelector("#product-form");
const productEditor = document.querySelector("#product-editor");
const stripeConnectStatus = document.querySelector("#stripe-connect-status");
const stripeConnectButton = document.querySelector("#stripe-connect-button");
const stripeDashboardButton = document.querySelector("#stripe-dashboard-button");

function renderOrders(orders) {
  const body = document.querySelector("#orders-table");
  body.replaceChildren();
  for (const order of orders) {
    const row = document.createElement("tr");
    appendCell(row, `#${order.order_id}`);
    appendCell(row, String(order.item_count));
    appendCell(row, formatUsd(order.gross_amount_cents), "numeric-cell");
    appendCell(row, formatDate(order.paid_at));
    const status = document.createElement("td");
    if (order.new_since_last_visit) {
      const badge = document.createElement("span");
      badge.className = "new-badge table-badge";
      badge.textContent = "NEW";
      status.append(badge);
    } else {
      status.textContent = "PAID";
    }
    row.append(status);
    body.append(row);
  }
  if (orders.length === 0) {
    const row = document.createElement("tr");
    const cell = appendCell(row, "No purchases yet.");
    cell.colSpan = 5;
    body.append(row);
  }
}

function renderProducts(products, sales) {
  const body = document.querySelector("#products-table");
  const empty = document.querySelector("#products-empty");
  const salesById = new Map(sales.map((product) => [product.product_id, product]));
  body.replaceChildren();
  empty.hidden = products.length > 0;
  for (const product of products) {
    const sale = salesById.get(product.id);
    const row = document.createElement("tr");
    const nameCell = appendCell(row, "");
    const productName = document.createElement("strong");
    productName.textContent = product.name;
    const sku = document.createElement("span");
    sku.className = "table-secondary";
    sku.textContent = product.sku;
    nameCell.append(productName, sku);
    appendCell(row, formatUsd(product.price_cents), "numeric-cell");
    appendCell(row, `${Math.max(0, product.stock_quantity - product.reserved_quantity)} available`, "numeric-cell");
    appendCell(row, String(sale?.units_sold || 0), "numeric-cell");
    appendCell(row, formatUsd(sale?.gross_amount_cents || 0), "numeric-cell");
    const state = appendCell(row, "");
    const stateLabel = document.createElement("span");
    stateLabel.className = product.visible ? "status-live" : "status-archived";
    stateLabel.textContent = product.visible ? "LISTED" : "ARCHIVED";
    state.append(stateLabel);
    const actions = appendCell(row, "");
    const edit = document.createElement("button");
    edit.type = "button";
    edit.className = "text-button table-action";
    edit.textContent = "Edit";
    edit.addEventListener("click", () => editProduct(product));
    actions.append(edit);
    if (product.visible) {
      const archive = document.createElement("button");
      archive.type = "button";
      archive.className = "text-button table-action archive-action";
      archive.textContent = "Archive";
      archive.addEventListener("click", () => archiveProduct(product));
      actions.append(archive);
    }
    body.append(row);
  }
}

function showEditor(product) {
  productForm.reset();
  productForm.elements.product_id.value = product ? String(product.id) : "";
  productForm.elements.name.value = product?.name || "";
  productForm.elements.description.value = product?.description || "";
  productForm.elements.price.value = product ? (product.price_cents / 100).toFixed(2) : "";
  productForm.elements.stock_quantity.value = product ? String(product.stock_quantity) : "0";
  document.querySelector("#editor-heading").textContent = product ? "Edit product" : "Add a product";
  productEditor.hidden = false;
  productEditor.scrollIntoView({ behavior: "smooth", block: "start" });
  productForm.elements.name.focus();
}

function editProduct(product) {
  showEditor(product);
}

async function archiveProduct(product) {
  if (!window.confirm(`Archive "${product.name}"? It will disappear from the catalog; order history will remain.`)) {
    return;
  }
  try {
    await api(`/api/provider/products/${product.id}`, { method: "DELETE" });
    showMessage(providerMessage, "Product archived. Historical sales are preserved.");
    await loadProducts();
  } catch (error) {
    showMessage(providerMessage, error.message, "error");
  }
}

async function loadProducts() {
  const products = await api("/api/provider/products");
  renderProducts(products, window.providerSales || []);
  document.querySelector("#live-products").textContent =
    String(products.filter((product) => product.visible).length);
}

async function loadStripeStatus() {
  const status = await api("/api/provider/stripe/status");
  stripeDashboardButton.hidden = !status.has_stripe_account;
  if (!status.stripe_configured) {
    stripeConnectStatus.textContent =
      "Stripe test keys are not configured on the server. Ask the administrator to configure them before connecting.";
    stripeConnectButton.disabled = true;
    stripeConnectButton.textContent = "STRIPE SETUP UNAVAILABLE";
    return status;
  }
  if (status.transfers_enabled) {
    stripeConnectStatus.textContent = status.payouts_enabled
      ? "Connected. Your Stripe account can receive transfers and payouts."
      : "Connected. Your Stripe account can receive transfers; finish any remaining payout setup with Stripe.";
    stripeConnectButton.hidden = true;
    return status;
  }
  stripeConnectButton.hidden = false;
  stripeConnectButton.disabled = false;
  stripeConnectButton.textContent = status.details_submitted
    ? "CONTINUE STRIPE SETUP"
    : "CONNECT WITH STRIPE";
  stripeConnectStatus.textContent = status.details_submitted
    ? "Stripe is reviewing your details or needs more information. Continue setup to see what is required."
    : "Products appear in the customer catalog after Stripe enables transfers for your account.";
  return status;
}

stripeConnectButton.addEventListener("click", async () => {
  stripeConnectButton.disabled = true;
  stripeConnectButton.textContent = "OPENING STRIPE…";
  try {
    const result = await api("/api/provider/stripe/connect", {
      method: "POST",
      body: "{}",
    });
    const onboardingUrl = new URL(result.url);
    if (onboardingUrl.protocol !== "https:" || onboardingUrl.hostname !== "connect.stripe.com") {
      throw new Error("Stripe returned an unexpected onboarding address.");
    }
    window.location.assign(onboardingUrl.href);
  } catch (error) {
    showMessage(providerMessage, error.message, "error");
    stripeConnectButton.disabled = false;
    try {
      await loadStripeStatus();
    } catch (statusError) {
      stripeConnectStatus.textContent =
        `Could not check your Stripe connection: ${statusError.message}`;
    }
  }
});

stripeDashboardButton.addEventListener("click", async () => {
  stripeDashboardButton.disabled = true;
  stripeDashboardButton.textContent = "OPENING STRIPE…";
  providerMessage.hidden = true;
  try {
    const result = await api("/api/provider/stripe/dashboard-login", {
      method: "POST",
      body: "{}",
    });
    const loginUrl = new URL(result.url);
    if (
      loginUrl.protocol !== "https:" ||
      loginUrl.hostname !== "connect.stripe.com" ||
      !loginUrl.pathname.startsWith("/express/") ||
      loginUrl.pathname.length <= "/express/".length
    ) {
      throw new Error("Stripe returned an invalid account login link.");
    }
    window.location.assign(loginUrl.href);
  } catch {
    showMessage(providerMessage, "Could not open your Stripe dashboard. Please try again later.", "error");
    stripeDashboardButton.disabled = false;
    stripeDashboardButton.textContent = "OPEN STRIPE DASHBOARD";
  }
});

productForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const button = productForm.querySelector('button[type="submit"]');
  button.disabled = true;
  providerMessage.hidden = true;
  try {
    const form = new FormData(productForm);
    const price = String(form.get("price") || "").trim();
    if (!/^\d{1,3}(?:\.\d{1,2})?$/.test(price)) {
      throw new Error("Enter a price with up to two decimal places.");
    }
    const [whole, fraction = ""] = price.split(".");
    const body = {
      name: String(form.get("name") || ""),
      description: String(form.get("description") || ""),
      price_cents: Number(whole) * 100 + Number(fraction.padEnd(2, "0")),
      stock_quantity: Number(form.get("stock_quantity")),
    };
    const productId = String(form.get("product_id") || "");
    await api(
      productId ? `/api/provider/products/${encodeURIComponent(productId)}` : "/api/provider/products",
      { method: productId ? "PATCH" : "POST", body: JSON.stringify(body) }
    );
    productEditor.hidden = true;
    productForm.reset();
    showMessage(providerMessage, productId ? "Product updated." : "Product added to your catalog.");
    await loadProducts();
  } catch (error) {
    showMessage(providerMessage, error.message, "error");
  } finally {
    button.disabled = false;
  }
});

document.querySelector("#new-product-button").addEventListener("click", () => showEditor(null));
document.querySelector("#cancel-edit-button").addEventListener("click", () => {
  productEditor.hidden = true;
  productForm.reset();
});
document.querySelector("#logout-button").addEventListener("click", () => logout(providerMessage));

async function start() {
  const { user } = await api("/api/auth/me");
  if (!user || user.role !== "provider") {
    window.location.replace("/");
    return;
  }
  document.querySelector("#provider-name").textContent = user.username;
  const dashboard = await api("/api/provider/dashboard");
  window.providerSales = dashboard.products;
  document.querySelector("#gross-sales").textContent = formatUsd(dashboard.gross_sales_cents);
  document.querySelector("#live-products").textContent = String(dashboard.live_products);
  document.querySelector("#purchase-count").textContent = String(dashboard.unseen_purchase_count);
  document.querySelector("#accounting-note").textContent = dashboard.accounting_note;
  renderSalesChart(document.querySelector("#provider-chart"), dashboard.sales_series);
  renderOrders(dashboard.recent_orders);
  if (dashboard.unseen_purchase_count > 0) {
    const badge = document.querySelector("#new-purchase-badge");
    badge.textContent = `${dashboard.unseen_purchase_count} NEW SINCE YOUR LAST LOGIN`;
    badge.hidden = false;
  }
  const connectResult = new URLSearchParams(window.location.search).get("stripe_connect");
  if (connectResult === "refresh") {
    stripeConnectStatus.textContent = "Refreshing your secure Stripe setup link.";
    const result = await api("/api/provider/stripe/connect", {
      method: "POST",
      body: "{}",
    });
    const onboardingUrl = new URL(result.url);
    if (onboardingUrl.protocol !== "https:" || onboardingUrl.hostname !== "connect.stripe.com") {
      throw new Error("Stripe returned an unexpected onboarding address.");
    }
    window.location.replace(onboardingUrl.href);
    return;
  }
  await loadStripeStatus()
    .then((status) => {
      if (connectResult === "return") {
        stripeConnectStatus.textContent = status.transfers_enabled
          ? "Stripe setup is complete. Your account is ready to receive transfers."
          : "Stripe received your details. Transfers will be available after Stripe completes its review.";
      }
    })
    .catch((error) => {
      stripeConnectStatus.textContent = `Could not check your Stripe connection: ${error.message}`;
      stripeConnectButton.hidden = false;
      stripeConnectButton.disabled = false;
      stripeConnectButton.textContent = "RETRY STRIPE SETUP";
    });
  await loadProducts();
}

start().catch((error) => {
  showMessage(providerMessage, error.message, "error");
  stripeConnectStatus.textContent = `Stripe setup could not finish: ${error.message}`;
  stripeConnectButton.hidden = false;
  stripeConnectButton.disabled = false;
  stripeConnectButton.textContent = "RETRY STRIPE SETUP";
});
