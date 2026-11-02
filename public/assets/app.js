const signInForm = document.querySelector("#signin-form");
const registerForm = document.querySelector("#register-form");
const authMessage = document.querySelector("#auth-message");

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

function openWorkspace(user) {
  const destinations = {
    admin: "/admin",
    provider: "/provider",
    customer: "/store",
  };
  const destination = destinations[user.role];
  if (!destination) {
    showMessage(authMessage, "This account does not have a supported workspace.", "error");
    return;
  }
  window.location.assign(destination);
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
    openWorkspace(result.user);
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
        role: form.get("role"),
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
    showMessage(authMessage, error.message, "error");
  }
});

async function loadCurrentUser() {
  try {
    const result = await api("/api/auth/me");
    if (result.user) openWorkspace(result.user);
  } catch (error) {
    showMessage(authMessage, error.message, "error");
  }
}

loadCurrentUser();
