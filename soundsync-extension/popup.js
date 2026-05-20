// SoundSync Extension - Popup Logic

document.addEventListener("DOMContentLoaded", async () => {
  const statusDot = document.getElementById("status-dot");
  const statusText = document.getElementById("status-text");
  const urlDisplay = document.getElementById("url-display");
  const sendBtn = document.getElementById("send-btn");
  const toast = document.getElementById("toast");

  let currentTabUrl = "";

  // 1. Aktuellen Tab ermitteln
  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    if (tabs && tabs[0]) {
      currentTabUrl = tabs[0].url;
      urlDisplay.textContent = currentTabUrl;
      
      // Prüfen, ob die URL für SoundSync geeignet ist (optional, aber gut für UX)
      if (isValidMediaUrl(currentTabUrl)) {
        checkAppConnection();
      } else {
        urlDisplay.style.color = "#ff7675";
        urlDisplay.textContent = "Kein unterstützter YouTube/SoundCloud Link:\n" + currentTabUrl;
        sendBtn.disabled = true;
        statusDot.className = "status-dot";
        statusText.textContent = "Inaktiv";
      }
    }
  });

  // 2. Verbindung zur Desktop-App prüfen
  function checkAppConnection() {
    fetch("http://localhost:3030/status")
      .then(response => {
        if (response.ok) {
          statusDot.className = "status-dot online";
          statusText.textContent = "Verbunden";
          statusText.style.color = "#00b894";
          sendBtn.disabled = false;
        } else {
          throw new Error();
        }
      })
      .catch(() => {
        statusDot.className = "status-dot offline";
        statusText.textContent = "Offline";
        statusText.style.color = "#ff7675";
        sendBtn.disabled = true;
      });
  }

  // 3. Sende-Klick-Handler
  sendBtn.addEventListener("click", () => {
    if (!currentTabUrl) return;

    sendBtn.disabled = true;
    sendBtn.textContent = "Wird gesendet...";

    fetch("http://localhost:3030/send", {
      method: "POST",
      headers: {
        "Content-Type": "application/json"
      },
      body: JSON.stringify({
        url: currentTabUrl,
        format: null,
        autoStart: false
      })
    })
    .then(response => {
      if (response.ok) {
        showToast("Im Korb gespeichert!");
      } else {
        showToast("Fehler beim Senden!", true);
      }
    })
    .catch(() => {
      showToast("App-Verbindungsverlust!", true);
    })
    .finally(() => {
      sendBtn.disabled = false;
      sendBtn.innerHTML = `
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
          <line x1="22" y1="2" x2="11" y2="13"/>
          <polygon points="22 2 15 22 11 13 2 9 22 2"/>
        </svg>
        Link an Korb senden
      `;
      checkAppConnection();
    });
  });

  // Hilfsfunktion zum Prüfen von URLs
  function isValidMediaUrl(url) {
    if (!url) return false;
    try {
      const parsed = new URL(url);
      const host = parsed.hostname.toLowerCase();
      return (
        host === "youtu.be" ||
        host.endsWith("youtube.com") ||
        host.endsWith("music.youtube.com") ||
        host.endsWith("soundcloud.com") ||
        host.endsWith("spotify.com")
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
