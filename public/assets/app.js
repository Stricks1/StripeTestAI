const authView = document.querySelector("#auth-view");
const storeView = document.querySelector("#store-view");
const signInForm = document.querySelector("#signin-form");
const registerForm = document.querySelector("#register-form");
const authMessage = document.querySelector("#auth-message");
const storeMessage = document.querySelector("#store-message");
const productsView = document.querySelector("#products-view");
const productList = document.querySelector("#product-list");

async function api(url, options = {}) {
  const response = await fetch(url, {
    ...options,
    headers: { "Content-Type": "application/json", ...options.headers },
  });
  const payload = await response.json();
  if (!response.ok) throw new Error(payload.error || "The request could not be completed.");
  return payload;
}

function showMessage(element, message, kind = "info") {
  element.textContent = message;
  element.dataset.kind = kind;
  element.hidden = false;
}

function showConfirmationLink(element, payload) {
  element.replaceChildren();
  const copy = document.createElement("span");
  copy.textContent = payload.message;
  element.append(copy);
  if (payload.confirmation_url) {
    const link = document.createElement("a");
    link.href = payload.confirmation_url;
    link.textContent = " Confirm this local email address.";
    element.append(link);
  }
  element.dataset.kind = "info";
  element.hidden = false;
}

function setAuthMode(mode) {
  const signingIn = mode === "signin";
  signInForm.hidden = !signingIn;
  registerForm.hidden = signingIn;
  document.querySelector("#signin-tab").classList.toggle("is-active", signingIn);
  document.querySelector("#register-tab").classList.toggle("is-active", !signingIn);
  document.querySelector("#signin-tab").setAttribute("aria-selected", String(signingIn));
  document.querySelector("#register-tab").setAttribute("aria-selected", String(!signingIn));
  document.querySelector("#auth-heading").textContent = signingIn ? "Sign in" : "Create account";
  authMessage.hidden = true;
}

function showStore(user) {
  authView.hidden = true;
  storeView.hidden = false;
  document.querySelector("#logout-button").hidden = false;
  document.querySelector("#username-label").textContent = user.username;
}

document.querySelector("#signin-tab").addEventListener("click", () => setAuthMode("signin"));
document.querySelector("#register-tab").addEventListener("click", () => setAuthMode("register"));

signInForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const button = signInForm.querySelector("button");
  button.disabled = true;
  authMessage.hidden = true;
  try {
    const form = new FormData(signInForm);
    const result = await api("/api/auth/login", {
      method: "POST",
      body: JSON.stringify({
        username: form.get("username"),
        password: form.get("password"),
      }),
    });
    showStore(result.user);
  } catch (error) {
    showMessage(authMessage, error.message, "error");
  } finally {
    button.disabled = false;
  }
});

registerForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const button = registerForm.querySelector("button");
  button.disabled = true;
  authMessage.hidden = true;
  try {
    const form = new FormData(registerForm);
    const result = await api("/api/auth/register", {
      method: "POST",
      body: JSON.stringify({
        username: form.get("username"),
        email: form.get("email"),
        password: form.get("password"),
      }),
    });
    showConfirmationLink(authMessage, result);
  } catch (error) {
    showMessage(authMessage, error.message, "error");
  } finally {
    button.disabled = false;
  }
});

document.querySelector("#logout-button").addEventListener("click", async () => {
  try {
    await api("/api/auth/logout", { method: "POST", body: "{}" });
    window.location.reload();
  } catch (error) {
    showMessage(storeMessage, error.message, "error");
  }
});

document.querySelector("#products-button").addEventListener("click", async () => {
  productsView.hidden = false;
  productList.replaceChildren();
  showMessage(storeMessage, "Loading the test catalog.");
  try {
    const products = await api("/api/products");
    storeMessage.hidden = true;
    if (products.length === 0) {
      showMessage(storeMessage, "There are no active products yet.");
      return;
    }
    for (const product of products) {
      const article = document.createElement("article");
      article.className = "product-record";
      const sku = document.createElement("div");
      sku.className = "product-id";
      sku.textContent = product.sku;
      const details = document.createElement("div");
      const name = document.createElement("h3");
      name.textContent = product.name;
      const description = document.createElement("p");
      description.textContent = product.description;
      const price = document.createElement("div");
      price.className = "product-price";
      price.textContent = `Default amount ${formatUsd(product.base_price_cents)}`;
      details.append(name, description, price);
      const buy = document.createElement("a");
      buy.className = "primary-button buy-link";
      buy.href = `/checkout?product_id=${encodeURIComponent(product.id)}`;
      buy.target = "_blank";
      buy.rel = "noopener noreferrer";
      buy.textContent = "BUY";
      article.append(sku, details, buy);
      productList.append(article);
    }
  } catch (error) {
    showMessage(storeMessage, error.message, "error");
  }
});

function formatUsd(cents) {
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(cents / 100);
}

async function loadCurrentUser() {
  try {
    const result = await api("/api/auth/me");
    if (result.user) showStore(result.user);
  } catch (error) {
    showMessage(authMessage, error.message, "error");
  }
}

loadCurrentUser();
