const { api, formatUsd, logout, showMessage } = window.Marketplace;
const cartForm = document.querySelector("#catalog-filters");
const productList = document.querySelector("#product-list");
const cartItems = document.querySelector("#cart-items");
const cartMessage = document.querySelector("#cart-message");
const catalogMessage = document.querySelector("#catalog-message");
const productCache = new Map();
let cartStorageKey;
let checkoutStorageKey;
let cart = [];

function loadCart() {
  try {
    const saved = JSON.parse(localStorage.getItem(cartStorageKey) || "[]");
    if (!Array.isArray(saved)) throw new Error("Saved cart data is invalid.");
    return saved.filter((item) =>
      Number.isSafeInteger(item.product_id) &&
      item.product_id > 0 &&
      Number.isSafeInteger(item.quantity) &&
      item.quantity > 0
    );
  } catch (error) {
    showMessage(cartMessage, "Your saved cart could not be read. Start a new cart to continue.", "error");
    return [];
  }
}

function saveCart() {
  try {
    localStorage.setItem(cartStorageKey, JSON.stringify(cart));
  } catch (error) {
    showMessage(cartMessage, "The cart could not be saved in this browser.", "error");
  }
}

function parsePrice(value) {
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (!/^\d{1,6}(?:\.\d{1,2})?$/.test(trimmed)) return Number.NaN;
  const [whole, fraction = ""] = trimmed.split(".");
  return Number(whole) * 100 + Number(fraction.padEnd(2, "0"));
}

function renderProducts(products) {
  productList.replaceChildren();
  for (const product of products) productCache.set(product.id, product);
  if (products.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = "No available products match these filters.";
    productList.append(empty);
  }

  for (const product of products) {
    const row = document.createElement("article");
    row.className = "catalogue-record";
    const sku = document.createElement("p");
    sku.className = "record-code";
    sku.textContent = product.sku;

    const details = document.createElement("div");
    const name = document.createElement("h3");
    name.textContent = product.name;
    const description = document.createElement("p");
    description.className = "quiet-copy";
    description.textContent = product.description;
    const provider = document.createElement("p");
    provider.className = "provider-attribution";
    provider.textContent = `Provided by ${product.provider_name}`;
    details.append(name, description, provider);

    const buy = document.createElement("div");
    buy.className = "catalogue-buy";
    const price = document.createElement("strong");
    price.textContent = formatUsd(product.price_cents);
    const stock = document.createElement("span");
    stock.textContent = `${product.stock_available} available`;
    const controls = document.createElement("div");
    controls.className = "quantity-add";
    const quantity = document.createElement("input");
    quantity.type = "number";
    quantity.min = "1";
    quantity.max = String(product.stock_available);
    quantity.value = "1";
    quantity.setAttribute("aria-label", `Quantity for ${product.name}`);
    const add = document.createElement("button");
    add.type = "button";
    add.className = "secondary-button";
    add.textContent = "ADD";
    add.addEventListener("click", () => {
      const amount = Number(quantity.value);
      if (!Number.isSafeInteger(amount) || amount < 1 || amount > product.stock_available) {
        showMessage(catalogMessage, `Choose a quantity between 1 and ${product.stock_available}.`, "error");
        return;
      }
      const existing = cart.find((item) => item.product_id === product.id);
      const nextQuantity = (existing?.quantity || 0) + amount;
      if (nextQuantity > product.stock_available) {
        showMessage(catalogMessage, `Only ${product.stock_available} units are available for ${product.name}.`, "error");
        return;
      }
      if (existing) existing.quantity = nextQuantity;
      else cart.push({ product_id: product.id, quantity: amount });
      catalogMessage.hidden = true;
      cartMessage.hidden = true;
      saveCart();
      renderCart();
    });
    controls.append(quantity, add);
    buy.append(price, stock, controls);
    row.append(sku, details, buy);
    productList.append(row);
  }
  document.querySelector("#catalog-count").textContent =
    `${products.length} ${products.length === 1 ? "product" : "products"} listed`;
}

async function loadProducts(event) {
  if (event) event.preventDefault();
  catalogMessage.hidden = true;
  document.querySelector("#catalog-count").textContent = "Loading products…";
  const form = new FormData(cartForm);
  const params = new URLSearchParams();
  const search = String(form.get("q") || "").trim();
  if (search) params.set("q", search);
  const minimum = parsePrice(String(form.get("min_price") || ""));
  const maximum = parsePrice(String(form.get("max_price") || ""));
  if (Number.isNaN(minimum) || Number.isNaN(maximum) ||
      (minimum !== null && maximum !== null && minimum > maximum)) {
    showMessage(catalogMessage, "Enter a valid minimum and maximum price.", "error");
    document.querySelector("#catalog-count").textContent = "Filters need attention";
    return;
  }
  if (minimum !== null) params.set("min_price_cents", String(minimum));
  if (maximum !== null) params.set("max_price_cents", String(maximum));
  try {
    const products = await api(`/api/marketplace/products?${params}`);
    renderProducts(products);
    renderCart();
  } catch (error) {
    showMessage(catalogMessage, error.message, "error");
    document.querySelector("#catalog-count").textContent = "Catalog unavailable";
  }
}

function renderCart() {
  cartItems.replaceChildren();
  let total = 0;
  let quantityTotal = 0;
  for (const item of cart) {
    const product = productCache.get(item.product_id);
    const line = document.createElement("div");
    line.className = "cart-line";
    const description = document.createElement("div");
    const name = document.createElement("strong");
    name.textContent = product?.name || `Product ${item.product_id}`;
    const unitPrice = product?.price_cents || 0;
    const price = document.createElement("span");
    price.textContent = product
      ? `${item.quantity} × ${formatUsd(unitPrice)}`
      : `${item.quantity} units · price verified at checkout`;
    description.append(name, price);
    const controls = document.createElement("div");
    controls.className = "cart-line-controls";
    const quantity = document.createElement("input");
    quantity.type = "number";
    quantity.min = "1";
    quantity.max = String(product?.stock_available || 1000);
    quantity.value = String(item.quantity);
    quantity.setAttribute("aria-label", `Cart quantity for ${name.textContent}`);
    quantity.addEventListener("change", () => {
      const next = Number(quantity.value);
      if (!Number.isSafeInteger(next) || next < 1 || next > Number(quantity.max)) {
        showMessage(cartMessage, "Cart quantity is outside the available range.", "error");
        quantity.value = String(item.quantity);
        return;
      }
      item.quantity = next;
      cartMessage.hidden = true;
      saveCart();
      renderCart();
    });
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "text-button";
    remove.textContent = "Remove";
    remove.addEventListener("click", () => {
      cart = cart.filter((entry) => entry.product_id !== item.product_id);
      saveCart();
      renderCart();
    });
    controls.append(quantity, remove);
    line.append(description, controls);
    cartItems.append(line);
    if (product) total += unitPrice * item.quantity;
    quantityTotal += item.quantity;
  }
  document.querySelector("#cart-total").textContent = formatUsd(total);
  document.querySelector("#cart-count").textContent = String(quantityTotal);
  document.querySelector("#cart-count-label").textContent =
    `${quantityTotal} ${quantityTotal === 1 ? "ITEM" : "ITEMS"}`;
  document.querySelector("#checkout-button").disabled = cart.length === 0;
}

cartForm.addEventListener("submit", loadProducts);
document.querySelector("#checkout-button").addEventListener("click", () => {
  if (cart.length === 0) return;
  try {
    const checkout = {
      checkout_key: crypto.randomUUID(),
      items: cart.map(({ product_id, quantity }) => ({ product_id, quantity })),
    };
    localStorage.setItem(checkoutStorageKey, JSON.stringify(checkout));
    const checkoutWindow = window.open("/checkout", "_blank");
    if (!checkoutWindow) {
      showMessage(cartMessage, "A secure checkout window was blocked. Allow pop-ups and try again.", "error");
      return;
    }
    checkoutWindow.opener = null;
  } catch (error) {
    showMessage(cartMessage, "A secure checkout window could not be opened. Allow pop-ups and try again.", "error");
  }
});
document.querySelector("#logout-button").addEventListener("click", () => logout(cartMessage));

async function start() {
  const { user } = await api("/api/auth/me");
  if (!user || user.role !== "customer") {
    window.location.replace("/");
    return;
  }
  const storageIdentity = encodeURIComponent(user.username.toLowerCase());
  cartStorageKey = `marketplace-cart:${storageIdentity}`;
  checkoutStorageKey = `marketplace-checkout:${storageIdentity}`;
  cart = loadCart();
  document.querySelector("#customer-name").textContent = user.username;
  renderCart();
  await loadProducts();
}

start().catch((error) => showMessage(catalogMessage, error.message, "error"));
