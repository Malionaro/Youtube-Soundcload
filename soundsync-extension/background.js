// Background Service Worker für SoundSync Extension

// Kontextmenüs beim Installieren erstellen
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "send-current-page",
    title: "Aktuelle Seite an SoundSync senden",
    contexts: ["page"]
  });

  chrome.contextMenus.create({
    id: "send-link",
    title: "Link an SoundSync senden",
    contexts: ["link"]
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

  if (url) {
    sendUrlToSoundSync(url);
  }
});

// Hilfsfunktion zum Senden der URL an den lokalen HTTP-Server
function sendUrlToSoundSync(url) {
  // CORS-gerechte POST-Anforderung an den lokalen SoundSync-Server
  fetch("http://localhost:3030/send", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify({
      url: url,
      format: null,
      autoStart: false
    })
  })
  .then(response => {
    if (response.ok) {
      console.log("Erfolgreich an SoundSync gesendet:", url);
    } else {
      console.error("Fehler beim Senden an SoundSync:", response.statusText);
    }
  })
  .catch(error => {
    console.error("Verbindungsfehler zu SoundSync (ist die App geöffnet?):", error);
  });
}
