const verifyButton = document.querySelector("#verify-button");
const verifyMessage = document.querySelector("#verify-message");
const token = new URLSearchParams(window.location.search).get("token");

verifyButton.addEventListener("click", async () => {
  if (!token) {
    showVerifyMessage("This confirmation link is missing its token. Request a new one.", "error");
    return;
  }
  verifyButton.disabled = true;
  try {
    const response = await fetch("/api/auth/verify", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ token }),
    });
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || "Email confirmation failed.");
    showVerifyMessage(result.message, "info");
    verifyButton.hidden = true;
  } catch (error) {
    showVerifyMessage(error.message, "error");
    verifyButton.disabled = false;
  }
});

function showVerifyMessage(message, kind) {
  verifyMessage.textContent = message;
  verifyMessage.dataset.kind = kind;
  verifyMessage.hidden = false;
}
