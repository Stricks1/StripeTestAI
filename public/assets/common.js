window.Marketplace = (() => {
  async function api(url, options = {}) {
    const response = await fetch(url, {
      ...options,
      headers: { "Content-Type": "application/json", ...options.headers },
    });
    let payload = {};
    if (response.status !== 204) {
      const contentType = response.headers.get("content-type") || "";
      if (!contentType.includes("application/json")) {
        throw new Error("The server returned an unexpected response. Reload and try again.");
      }
      payload = await response.json();
    }
    if (!response.ok) throw new Error(payload.error || "The request could not be completed.");
    return payload;
  }

  function formatUsd(cents) {
    return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(cents / 100);
  }

  function formatBalance(entries) {
    if (!Array.isArray(entries) || entries.length === 0) return formatUsd(0);
    const zeroDecimalCurrencies = new Set([
      "BIF", "CLP", "DJF", "GNF", "ISK", "JPY", "KMF", "KRW", "MGA",
      "PYG", "RWF", "UGX", "VND", "VUV", "XAF", "XOF", "XPF",
    ]);
    const threeDecimalCurrencies = new Set(["BHD", "JOD", "KWD", "OMR", "TND"]);
    return entries.map((entry) => {
      const currency = String(entry.currency || "usd").toUpperCase();
      const divisor = zeroDecimalCurrencies.has(currency)
        ? 1
        : threeDecimalCurrencies.has(currency)
          ? 1000
          : 100;
      const amount = new Intl.NumberFormat("en-US", {
        style: "currency",
        currency,
      }).format((Number(entry.amount) || 0) / divisor);
      return amount;
    }).join(" · ");
  }

  function showMessage(element, message, kind = "info") {
    element.textContent = message;
    element.dataset.kind = kind;
    element.hidden = false;
  }

  async function logout(messageElement) {
    try {
      await api("/api/auth/logout", { method: "POST", body: "{}" });
      window.location.assign("/");
    } catch (error) {
      showMessage(messageElement, error.message, "error");
    }
  }

  function appendCell(row, value, className = "") {
    const cell = document.createElement("td");
    cell.textContent = value;
    if (className) cell.className = className;
    row.append(cell);
    return cell;
  }

  function renderSalesChart(container, points) {
    container.replaceChildren();
    if (!Array.isArray(points) || points.length === 0) {
      const empty = document.createElement("p");
      empty.className = "empty-state";
      empty.textContent = "No sales in this period.";
      container.append(empty);
      return;
    }

    const width = 720;
    const height = 220;
    const padding = { top: 16, right: 12, bottom: 34, left: 12 };
    const plotWidth = width - padding.left - padding.right;
    const plotHeight = height - padding.top - padding.bottom;
    const maxValue = Math.max(...points.map((point) => point.gross_amount_cents), 1);
    const slot = plotWidth / points.length;
    const barWidth = Math.max(2, Math.min(20, slot * 0.62));
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
    svg.setAttribute("role", "img");
    svg.setAttribute("aria-label", "Gross sales by day");

    const baseline = document.createElementNS(svg.namespaceURI, "line");
    baseline.setAttribute("x1", String(padding.left));
    baseline.setAttribute("x2", String(width - padding.right));
    baseline.setAttribute("y1", String(padding.top + plotHeight));
    baseline.setAttribute("y2", String(padding.top + plotHeight));
    baseline.setAttribute("class", "chart-baseline");
    svg.append(baseline);

    points.forEach((point, index) => {
      const value = Math.max(0, point.gross_amount_cents);
      const barHeight = (value / maxValue) * plotHeight;
      const bar = document.createElementNS(svg.namespaceURI, "rect");
      bar.setAttribute("x", String(padding.left + index * slot + (slot - barWidth) / 2));
      bar.setAttribute("y", String(padding.top + plotHeight - barHeight));
      bar.setAttribute("width", String(barWidth));
      bar.setAttribute("height", String(Math.max(1, barHeight)));
      bar.setAttribute("rx", "1");
      bar.setAttribute("class", index % 4 === 1 ? "chart-bar chart-bar-amber" : "chart-bar");
      const title = document.createElementNS(svg.namespaceURI, "title");
      title.textContent = `${point.day}: ${formatUsd(value)}`;
      bar.append(title);
      svg.append(bar);
    });

    const labelIndexes = points.length === 1
      ? [0]
      : points.length === 2
        ? [0, 1]
        : [0, Math.floor((points.length - 1) / 2), points.length - 1];
    const labels = labelIndexes.map((index) => points[index]);
    labels.forEach((point, index) => {
      const text = document.createElementNS(svg.namespaceURI, "text");
      const x = index === 0 ? padding.left : index === 2 ? width - padding.right : width / 2;
      text.setAttribute("x", String(x));
      text.setAttribute("y", String(height - 8));
      text.setAttribute("text-anchor", index === 0 ? "start" : index === 2 ? "end" : "middle");
      text.setAttribute("class", "chart-label");
      text.textContent = new Date(`${point.day}T00:00:00Z`).toLocaleDateString("en-US", {
        month: "short",
        day: "numeric",
        timeZone: "UTC",
      });
      svg.append(text);
    });
    container.append(svg);
  }

  function formatDate(unixSeconds) {
    if (!Number.isFinite(unixSeconds)) return "—";
    return new Date(unixSeconds * 1000).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  }

  return Object.freeze({
    api,
    appendCell,
    formatBalance,
    formatDate,
    formatUsd,
    logout,
    renderSalesChart,
    showMessage,
  });
})();
