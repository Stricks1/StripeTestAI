const {
  api,
  appendCell,
  formatBalance,
  formatUsd,
  logout,
  renderSalesChart,
  showMessage,
} = window.Marketplace;
const adminMessage = document.querySelector("#admin-message");
const filterForm = document.querySelector("#admin-filters");

function populateSelect(select, items, labelFor) {
  const firstOption = select.options[0];
  select.replaceChildren(firstOption);
  for (const item of items) {
    const option = document.createElement("option");
    option.value = String(item.id);
    option.textContent = labelFor(item);
    select.append(option);
  }
}

function renderProductSales(products) {
  const body = document.querySelector("#product-sales-table");
  body.replaceChildren();
  for (const product of products) {
    const row = document.createElement("tr");
    appendCell(row, product.product_name);
    appendCell(row, product.provider_name);
    appendCell(row, String(product.units_sold), "numeric-cell");
    appendCell(row, formatUsd(product.gross_amount_cents), "numeric-cell");
    body.append(row);
  }
  document.querySelector("#product-sales-empty").hidden = products.length > 0;
}

function renderProviderSales(providers) {
  const body = document.querySelector("#provider-sales-table");
  body.replaceChildren();
  for (const provider of providers) {
    const row = document.createElement("tr");
    appendCell(row, provider.provider_name);
    appendCell(row, String(provider.products_sold), "numeric-cell");
    appendCell(row, String(provider.units_sold), "numeric-cell");
    appendCell(row, formatUsd(provider.gross_amount_cents), "numeric-cell");
    appendCell(row, formatUsd(provider.platform_commission_cents), "numeric-cell");
    body.append(row);
  }
  document.querySelector("#provider-sales-empty").hidden = providers.length > 0;
}

function renderSummary(summary) {
  const fees = summary.actual_stripe_fees_cents + summary.estimated_stripe_fees_cents;
  document.querySelector("#gross-sales").textContent = formatUsd(summary.gross_sales_cents);
  document.querySelector("#stripe-fees").textContent = formatUsd(fees);
  document.querySelector("#refunds").textContent = formatUsd(summary.refunds_cents);
  document.querySelector("#platform-commission").textContent = formatUsd(summary.platform_commission_cents);
  document.querySelector("#net-total").textContent = formatUsd(summary.net_after_fees_and_refunds_cents);
  const feeNote = document.querySelector("#fee-note");
  feeNote.textContent =
    `${formatUsd(summary.actual_stripe_fees_cents)} actual Stripe fees · ` +
    `${formatUsd(summary.estimated_stripe_fees_cents)} estimated while Stripe settlement data is unavailable. ` +
    "The 10% cap includes Stripe fees and platform commission. Order-level fees, refunds, and commission are prorated in product-filtered reports.";
}

function renderBalance(balance, error) {
  document.querySelector("#balance-available").textContent =
    balance ? formatBalance(balance.available) : "Unavailable";
  document.querySelector("#balance-pending").textContent =
    balance ? formatBalance(balance.pending) : "Unavailable";
  const message = document.querySelector("#balance-message");
  message.textContent = error || "Available and pending balances are reported by Stripe.";
  message.classList.toggle("balance-error", Boolean(error));
}

async function loadDashboard() {
  const form = new FormData(filterForm);
  const params = new URLSearchParams();
  for (const field of ["days", "provider_id", "product_id"]) {
    const value = String(form.get(field) || "");
    if (value) params.set(field, value);
  }
  adminMessage.hidden = true;
  const button = filterForm.querySelector('button[type="submit"]');
  button.disabled = true;
  try {
    const result = await api(`/api/admin/dashboard?${params}`);
    renderSummary(result.summary);
    renderBalance(result.stripe_balance, result.stripe_balance_error);
    renderSalesChart(document.querySelector("#admin-chart"), result.sales_series);
    renderProductSales(result.products);
    renderProviderSales(result.providers);
  } catch (error) {
    showMessage(adminMessage, error.message, "error");
  } finally {
    button.disabled = false;
  }
}

filterForm.addEventListener("submit", (event) => {
  event.preventDefault();
  loadDashboard();
});
document.querySelector("#logout-button").addEventListener("click", () => logout(adminMessage));

async function start() {
  const { user } = await api("/api/auth/me");
  if (!user || user.role !== "admin") {
    window.location.replace("/");
    return;
  }
  const options = await api("/api/admin/filters");
  populateSelect(
    filterForm.elements.provider_id,
    options.providers,
    (provider) => provider.username
  );
  populateSelect(
    filterForm.elements.product_id,
    options.products,
    (product) => `${product.name} · ${product.provider_name}`
  );
  await loadDashboard();
}

start().catch((error) => showMessage(adminMessage, error.message, "error"));
