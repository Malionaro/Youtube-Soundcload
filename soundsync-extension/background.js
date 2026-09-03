// Background Service Worker für SoundSync Extension

const MEDIA_URL_PATTERNS = [
  "*://*.youtube.com/*",
  "*://youtu.be/*",
  "*://*.soundcloud.com/*",
  "*://*.spotify.com/*",
  "*://*.music.apple.com/*"
];

function isValidMediaUrl(u) {
  if (!u || typeof u !== "string") return false;
  try {
    const host = new URL(u).hostname.toLowerCase();
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

// Kontextmenüs beim Installieren/Updaten sauber neu erstellen (kein duplicate-id)
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "send-current-page",
      title: "Aktuelle Seite an SoundSync senden",
      contexts: ["page"],
      documentUrlPatterns: MEDIA_URL_PATTERNS
    });

    chrome.contextMenus.create({
      id: "send-link",
      title: "Link an SoundSync senden",
      contexts: ["link"],
      targetUrlPatterns: MEDIA_URL_PATTERNS
    });
  });
});

// Klick-Handler für das Kontextmenü
chrome.contextMenus.onClicked.addListener((info, tab) => {
  let url = "";

  if (info.menuItemId === "send-current-page") {
    url = info.pageUrl;
  } else if (info.menuItemId === "send-link") {
    url = info.linkUrl;
  }

  if (url && isValidMediaUrl(url)) {
    sendUrlToSoundSync(url);
  } else if (url) {
    console.warn("SoundSync: URL wird nicht unterstützt:", url);
  }
});

async function postToServer(path, body) {
  const hosts = ["http://localhost:3030", "http://127.0.0.1:3030"];
  let lastError = null;
  for (const host of hosts) {
    try {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 3000);
      const response = await fetch(host + path, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: controller.signal
      });
      clearTimeout(timeout);
      return response;
    } catch (e) {
      lastError = e;
    }
  }
  throw lastError || new Error("unreachable");
}

// Hilfsfunktion zum Senden der URL an den lokalen HTTP-Server
function sendUrlToSoundSync(url) {
  postToServer("/send", { url, format: null, autoStart: false })
    .then(response => {
      if (response && response.ok) {
        console.log("Erfolgreich an SoundSync gesendet:", url);
        setBadgeFeedback(true);
      } else {
        console.error("Fehler beim Senden an SoundSync:", response && response.statusText);
        setBadgeFeedback(false);
      }
    })
    .catch(error => {
      console.error("Verbindungsfehler zu SoundSync (ist die App geöffnet?):", error);
      setBadgeFeedback(false);
    });
}

function setBadgeFeedback(ok) {
  try {
    if (!chrome.action || !chrome.action.setBadgeText) return;
    chrome.action.setBadgeText({ text: ok ? "✓" : "!" });
    chrome.action.setBadgeBackgroundColor({ color: ok ? "#00b894" : "#ff7675" });
    setTimeout(() => chrome.action.setBadgeText({ text: "" }), 2500);
  } catch {
    // Badge nicht verfügbar – still weitermachen
  }
}
