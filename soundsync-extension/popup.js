// SoundSync Extension - Popup Logic

document.addEventListener("DOMContentLoaded", async () => {
  const statusDot = document.getElementById("status-dot");
  const statusText = document.getElementById("status-text");
  const urlDisplay = document.getElementById("url-display");
  const sendBtn = document.getElementById("send-btn");
  const toast = document.getElementById("toast");
  const sendBtnOriginalHtml = sendBtn ? sendBtn.innerHTML : "";

  let currentTabUrl = "";
  let connectionKnown = false;
  let connectionOnline = false;

  // 1. Aktuellen Tab ermitteln (mit Guard gegen chrome://, leere Tabs etc.)
  try {
    const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
    const tabUrl = tabs && tabs[0] && tabs[0].url ? tabs[0].url : "";
    if (!tabUrl) {
      currentTabUrl = "";
      urlDisplay.textContent = "Kein lesbarer Tab (z.B. chrome:// oder leerer Tab)";
      urlDisplay.style.color = "#ff7675";
      sendBtn.disabled = true;
      setOffline("Inaktiv");
      return;
    }
    currentTabUrl = tabUrl;
    urlDisplay.textContent = currentTabUrl;

    // Prüfen, ob die URL für SoundSync geeignet ist
    if (isValidMediaUrl(currentTabUrl)) {
      sendBtn.disabled = true; // erst nach Verbindungscheck freigeben (kein Race)
      await checkAppConnection();
    } else {
      urlDisplay.style.color = "#ff7675";
      urlDisplay.textContent = "Kein unterstützter Link:\n" + currentTabUrl;
      sendBtn.disabled = true;
      statusDot.className = "status-dot";
      statusText.textContent = "Inaktiv";
    }
  } catch (e) {
    console.warn("Tab konnte nicht gelesen werden:", e);
    sendBtn.disabled = true;
    setOffline("Inaktiv");
  }

  async function fetchWithFallback(path, options) {
    const hosts = ["http://localhost:3030", "http://127.0.0.1:3030"];
    let lastError = null;
    for (const host of hosts) {
      try {
        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), 2500);
        const res = await fetch(host + path, { ...options, signal: controller.signal });
        clearTimeout(timeout);
        return res;
      } catch (e) {
        lastError = e;
      }
    }
    throw lastError || new Error("unreachable");
  }

  // 2. Verbindung zur Desktop-App prüfen
  async function checkAppConnection() {
    try {
      const response = await fetchWithFallback("/status");
      if (response && response.ok) {
        connectionKnown = true;
        connectionOnline = true;
        statusDot.className = "status-dot online";
        statusText.textContent = "Verbunden";
        statusText.style.color = "#00b894";
        sendBtn.disabled = false;
      } else {
        throw new Error("status not ok");
      }
    } catch {
      connectionKnown = true;
      connectionOnline = false;
      setOffline("Offline");
      sendBtn.disabled = true;
    }
  }

  function setOffline(label) {
    statusDot.className = "status-dot offline";
    statusText.textContent = label;
    statusText.style.color = "#ff7675";
  }

  // 3. Sende-Klick-Handler (gegen Doppelklick geschützt)
  let sending = false;
  sendBtn.addEventListener("click", async () => {
    if (!currentTabUrl || sending) return;
    sending = true;
    sendBtn.disabled = true;
    sendBtn.textContent = "Wird gesendet...";

    try {
      const response = await fetchWithFallback("/send", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ url: currentTabUrl, format: null, autoStart: false })
      });
      if (response && response.ok) {
        showToast("Im Korb gespeichert!");
      } else {
        showToast("Fehler beim Senden!", true);
      }
    } catch {
      showToast("App-Verbindungsverlust!", true);
    } finally {
      sending = false;
      sendBtn.innerHTML = sendBtnOriginalHtml || "Link an Korb senden";
      // Button erst nach erneutem Verbindungscheck freigeben
      await checkAppConnection();
    }
  });

  // Hilfsfunktion zum Prüfen von URLs (konsistent mit background.js + App)
  function isValidMediaUrl(url) {
    if (!url) return false;
    try {
      const parsed = new URL(url);
      const host = parsed.hostname.toLowerCase();
      return (
        host === "youtu.be" ||
        host.endsWith("youtube.com") ||
        host.endsWith("soundcloud.com") ||
        host.endsWith("spotify.com") ||
        host === "music.apple.com" ||
        host.endsWith(".music.apple.com")
      );
    } catch {
      return false;
    }
  }

  // Toast-Benachrichtigung anzeigen
  function showToast(message, isError = false) {
    toast.textContent = message;
    toast.style.background = isError ? "var(--danger)" : "var(--success)";
    toast.style.boxShadow = isError ? "0 4px 12px rgba(255, 118, 117, 0.3)" : "0 4px 12px rgba(0, 184, 148, 0.3)";
    toast.classList.add("show");

    setTimeout(() => {
      toast.classList.remove("show");
    }, 2500);
  }
});
