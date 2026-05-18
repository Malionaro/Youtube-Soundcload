import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { open, confirm } from "@tauri-apps/plugin-dialog";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { openUrl, openPath } from "@tauri-apps/plugin-opener";

// ─── Utilities ───────────────────────────────────────────────────────────────
function debounce<T extends (...args: any[]) => any>(fn: T, ms: number): (...args: Parameters<T>) => void {
  let timer: number | undefined;
  return (...args) => {
    clearTimeout(timer);
    timer = window.setTimeout(() => fn(...args), ms);
  };
}

// ─── i18n ────────────────────────────────────────────────────────────────────
import en from "./i18n/en.json";
import de from "./i18n/de.json";
import pl from "./i18n/pl.json";

const translations: Record<string, Record<string, string>> = { en, de, pl };
let currentLang = "de";
let t = translations[currentLang];

function _(key: string, vars?: Record<string, string | number>): string {
  let text = t[key] || translations["en"][key] || key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      text = text.replace(`{${k}}`, String(v));
    }
  }
  return text;
}

// ─── State ───────────────────────────────────────────────────────────────────
interface AppConfig {
  download_folder: string;
  cookies_path: string;
  language: string;
  disable_changelog: boolean;
  auto_url_detection: boolean;
  discord_rpc: boolean;
  accent_color?: string;
  custom_background?: string;
  format?: string;
  quality?: string;
  auto_scroll_log?: boolean;
  auto_scroll_tracks?: boolean;
  eco_mode?: boolean;
  auto_tagging?: boolean;
}

interface PlaylistEntry {
  title: string;
  url: string;
  thumbnail: string | null;
  duration: number | null;
  index: number;
}

interface PlaylistInfo {
  title: string;
  entries: PlaylistEntry[];
  total: number;
}

interface DownloadProgress {
  status: string;
  percent: number;
  speed: string;
  eta: string;
  title: string;
  current: number;
  total: number;
  filename: string;
}

let config: AppConfig = {
  download_folder: "",
  cookies_path: "",
  language: "de",
  disable_changelog: false,
  auto_url_detection: true,
  discord_rpc: false,
  auto_scroll_log: true,
  auto_scroll_tracks: true,
  eco_mode: true,
  auto_tagging: true,
};

let isDownloading = false;
let totalTracks = 0;
let completedTracks = 0;
let startTime = 0;
let playlistEntries: PlaylistEntry[] = [];
let clipboardWatchTimer: number | null = null;
let lastDetectedUrl = "";
let pendingDetectedUrl = "";
let appVersion = "0.0.0";
let selectedConversionFiles: string[] = [];
let logScrollPending = false;
let isWindowVisible = true;
let ecoModeActive = false;
let isTvMode = false;
let currentTranslations: Record<string, string> = {};

// ─── DOM Elements ────────────────────────────────────────────────────────────
const $ = (id: string) => (document.getElementById(id) || document.createElement("div")) as HTMLElement;
const maybeElement = (id: string) => document.getElementById(id);

// Globale Elemente sicher abrufen
const getEl = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

let urlInput: HTMLInputElement;
let folderInput: HTMLInputElement;
let cookiesInput: HTMLInputElement;
let formatSelect: HTMLSelectElement;
let qualitySelect: HTMLSelectElement;
let audioQualities: HTMLElement;
let videoQualities: HTMLElement;
let downloadBtn: HTMLButtonElement;
let cancelBtn: HTMLButtonElement;
let logOutput: HTMLElement;
let trackList: HTMLElement;
let downloadProgress: HTMLElement;
let convertProgress: HTMLElement;
let totalProgress: HTMLElement;

function setupElements() {
  try {
    urlInput = getEl("url-input");
    folderInput = getEl("folder-input");
    cookiesInput = getEl("cookies-input");
    formatSelect = getEl("format-select");
    qualitySelect = getEl("quality-select");
    audioQualities = getEl("audio-qualities");
    videoQualities = getEl("video-qualities");
    downloadBtn = getEl("download-btn");
    cancelBtn = getEl("cancel-btn");
    logOutput = getEl("log-output");
    trackList = getEl("track-list");
    downloadProgress = getEl("download-progress");
    convertProgress = getEl("convert-progress");
    totalProgress = getEl("total-progress");
  } catch (e) {
    console.error("Kritischer Fehler: Wichtige UI-Elemente fehlen!", e);
  }
}

// ─── Tabs ────────────────────────────────────────────────────────────────────
function setupTabs() {
  const tabs = document.querySelectorAll(".tab-btn");
  const indicator = document.getElementById("nav-indicator");

  function updateIndicator(tab: HTMLElement) {
    if (!indicator) return;
    
    // Position relativ zum padding des nav-tabs (da nav-tabs position: relative hat)
    const offsetLeft = tab.offsetLeft;
    indicator.style.transform = `translateX(${offsetLeft}px)`;
    indicator.style.width = `${tab.offsetWidth}px`;
  }

  tabs.forEach(tab => {
    tab.addEventListener("click", () => {
      const targetId = tab.getAttribute("data-tab");
      if (!targetId) return;

      // Update buttons
      tabs.forEach(t => t.classList.remove("active"));
      tab.classList.add("active");
      updateIndicator(tab as HTMLElement);

      // Update content
      document.querySelectorAll(".tab-content").forEach(content => {
        content.classList.remove("active");
      });
      $(targetId).classList.add("active");

      // Special actions on tab switch
      if (targetId === "trending-tab" && $(targetId).querySelector(".loading-state")) {
        loadTrending();
      }
    });
  });

  // Initialize indicator position
  setTimeout(() => {
    const activeTab = document.querySelector(".tab-btn.active") as HTMLElement;
    if (activeTab) updateIndicator(activeTab);
  }, 50);

  // Update on window resize
  window.addEventListener("resize", () => {
    const activeTab = document.querySelector(".tab-btn.active") as HTMLElement;
    if (activeTab) updateIndicator(activeTab);
  });
}



// ─── Init ────────────────────────────────────────────────────────────────────

function on(id: string, event: string, handler: (e: any) => void) {
  const el = document.getElementById(id);
  if (el) {
    el.addEventListener(event, handler);
  }
}

function updateQualityOptions() {
  const isVideo = (formatSelect.options[formatSelect.selectedIndex].parentNode as HTMLOptGroupElement)?.label === "Video";
  if (isVideo) {
    audioQualities.style.display = "none";
    videoQualities.style.display = "block";
    const currentVal = qualitySelect.value;
    if (["best", "good", "worst"].includes(currentVal) && currentVal !== "best" && currentVal !== "worst") {
      qualitySelect.value = "1080p"; // sensible fallback
    }
  } else {
    audioQualities.style.display = "block";
    videoQualities.style.display = "none";
    const currentVal = qualitySelect.value;
    if (["best", "4k", "1080p", "720p", "480p", "worst"].includes(currentVal) && currentVal !== "best" && currentVal !== "worst") {
      qualitySelect.value = "best"; // sensible fallback
    }
  }
}

async function init() {
  setupElements();
  appVersion = await getVersion();
  try {
    const savedLang = localStorage.getItem("language") || "de";
    ($("language-select") as HTMLSelectElement).value = savedLang;
    await loadTranslations(savedLang);
    
    config = await invoke<AppConfig>("load_config");
    if (config.download_folder) folderInput.value = config.download_folder;
    if (config.cookies_path) cookiesInput.value = config.cookies_path;
    if (config.auto_url_detection === undefined) config.auto_url_detection = true;
    if (config.discord_rpc === undefined) config.discord_rpc = true;
    if (config.format) formatSelect.value = config.format;
    
    updateQualityOptions();
    if (config.quality) qualitySelect.value = config.quality;

    // Apply defaults for new settings
    if (config.auto_scroll_log === undefined) config.auto_scroll_log = true;
    if (config.auto_scroll_tracks === undefined) config.auto_scroll_tracks = true;
    if (config.eco_mode === undefined) config.eco_mode = true;

    applyTheme();
    if (config.accent_color) {
        ($("accent-color-picker") as HTMLInputElement).value = config.accent_color;
    }
    
    // Auto-tagging sync
    if (config.auto_tagging === undefined) config.auto_tagging = true;
    const taggingToggle = document.getElementById("auto-tagging-toggle") as HTMLInputElement;
    if (taggingToggle) taggingToggle.checked = config.auto_tagging;

    updateRemoteStatus();
    setupEcoMode();
    
    // Discord RPC Initialisierung
    updateDiscordPresence("Ready to download", "Waiting for URLs...");
    
    if (config.language) {
      currentLang = config.language;
      t = translations[currentLang];
      (document.getElementById("language-select") as HTMLSelectElement).value = currentLang;
    }
  } catch (e) {
    log("⚠️ Config could not be loaded, using defaults", "warning");
  }
  updateUI();
  setupEventListeners();
  setupTauriListeners();
  setupTabs();
  checkSystem();
  log(`🎵 SoundSync Downloader v${appVersion} ready`, "success");
  
  // Auto-check for updates on startup
  setTimeout(() => checkForUpdates(false), 2000);
}

// ─── Search & Trending ───────────────────────────────────────────────────────
async function performSearch() {
  const input = getEl<HTMLInputElement>("search-input");
  const query = input.value.trim();
  if (!query) return;

  const resultsGrid = $("search-results");
  resultsGrid.innerHTML = `<div class="loading-state"><div class="spinner"></div><span>Suche nach "${query}"...</span></div>`;

  try {
    const info = await invoke<PlaylistInfo>("search_videos", { query });
    renderResults(info.entries, resultsGrid);
  } catch (e) {
    resultsGrid.innerHTML = `<div class="empty-state"><p class="status-text error">Fehler bei der Suche: ${e}</p></div>`;
  }
}

async function loadTrending() {
  const resultsGrid = $("trending-results");
  resultsGrid.innerHTML = `<div class="loading-state"><div class="spinner"></div><span>Top-Charts werden geladen...</span></div>`;

  try {
    const info = await invoke<PlaylistInfo>("get_trending_videos");
    renderResults(info.entries, resultsGrid);
  } catch (e) {
    resultsGrid.innerHTML = `<div class="empty-state">
      <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--danger)" stroke-width="1.5" opacity="0.8">
        <circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>
      </svg>
      <p class="status-text error" style="max-width: 400px; word-break: break-word;">Fehler beim Laden der Trends: ${e}</p>
    </div>`;
  }
}

function renderResults(entries: PlaylistEntry[], container: HTMLElement) {
  if (!entries || entries.length === 0) {
    container.innerHTML = `<div class="empty-state"><p>Keine Ergebnisse gefunden.</p></div>`;
    return;
  }

  container.innerHTML = "";
  entries.forEach((entry, idx) => {
    const card = document.createElement("div");
    card.className = "result-card";
    card.style.animationDelay = `${idx * 0.05}s`;

    const durationStr = entry.duration ? formatDuration(entry.duration) : "";
    
    card.innerHTML = `
      <div class="result-thumb-container">
        <img src="${entry.thumbnail || '/src/assets/logo.png'}" class="result-thumb" loading="lazy" />
        ${durationStr ? `<span class="result-duration">${durationStr}</span>` : ""}
      </div>
      <div class="result-info">
        <h3 class="result-title" title="${entry.title}">${entry.title}</h3>
        <span class="result-channel">YouTube</span>
      </div>
      <div class="result-actions">
        <button class="btn btn-small btn-primary btn-full download-result-btn" data-url="${entry.url}">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          Download
        </button>
      </div>
    `;

    card.querySelector(".download-result-btn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      const url = (e.currentTarget as HTMLElement).getAttribute("data-url");
      if (url) {
        urlInput.value = url;
        updateDownloadBtnState();
        // Switch to downloader tab
        document.querySelector<HTMLElement>('.tab-btn[data-tab="downloader-tab"]')?.click();
        log(`🎯 Track ausgewählt: ${entry.title}`, "info");
      }
    });

    container.appendChild(card);
  });
}

function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
  return `${m}:${s.toString().padStart(2, '0')}`;
}

async function updateDiscordPresence(details: string, state_msg: string) {
  try {
    await invoke("update_discord_presence", { details, stateMsg: state_msg });
  } catch (e) {
    // Ignore RPC errors (e.g. Discord not running)
  }
}

async function checkForUpdates(manual: boolean = false) {
  const tag = $("update-status-tag");
  const btn = $("check-update-btn") as HTMLButtonElement;
  
  const currentVersion = await getVersion();
  
  if (manual) {
    tag.textContent = "Prüfe...";
    tag.className = "status-tag checking";
    btn.disabled = true;
  }

  try {
    const response = await fetch("https://api.github.com/repos/Malionaro/Youtube-Soundcload/releases/latest");
    if (!response.ok) throw new Error("GitHub API unreachable");
    
    const data = await response.json();
    const latestVersion = data.tag_name.replace(/^v/i, "");

    // Simple semver check
    const isNewer = (v1: string, v2: string) => {
      const parts1 = v1.split('.').map(p => parseInt(p, 10));
      const parts2 = v2.split('.').map(p => parseInt(p, 10));
      for (let i = 0; i < Math.max(parts1.length, parts2.length); i++) {
        const a = isNaN(parts1[i]) ? 0 : parts1[i];
        const b = isNaN(parts2[i]) ? 0 : parts2[i];
        if (a > b) return true;
        if (a < b) return false;
      }
      return false;
    };
    
    if (isNewer(latestVersion, currentVersion)) {
      tag.textContent = "Update verfügbar!";
      tag.className = "status-tag update-available";
      
      // Always prompt if there's an update, regardless of `manual`
      // Find the installer asset (.msi or .exe)
      const assets = data.assets || [];
      const installerAsset = assets.find((a: any) => 
        a.name.endsWith(".msi") || a.name.endsWith(".exe")
      );

      if (installerAsset) {
        const confirmed = await confirm(
          `Version ${latestVersion} ist verfügbar!\n\nMöchtest du das Update jetzt herunterladen und installieren?\n\nDatei: ${installerAsset.name}`,
          { title: "Update verfügbar", kind: "info" }
        );
        
        if (confirmed) {
          tag.textContent = "Lade herunter...";
          tag.className = "status-tag checking";
          if (btn) {
            btn.textContent = "⏬ Wird heruntergeladen...";
            btn.disabled = true;
          }
          log(`📥 Update ${latestVersion} wird heruntergeladen...`, "info");

          try {
            await invoke("download_and_install_update", {
              downloadUrl: installerAsset.browser_download_url,
              filename: installerAsset.name,
            });
            
            tag.textContent = "Installiert!";
            tag.className = "status-tag update-available";
            log("✅ Installer gestartet – App wird geschlossen...", "success");
          } catch (installErr) {
            tag.textContent = "Fehler";
            tag.className = "status-tag error";
            log(`❌ Installation fehlgeschlagen: ${installErr}`, "error");
            
            // Fallback: Open download page
            const fallback = await confirm(
              "Automatische Installation fehlgeschlagen. Möchtest du die Download-Seite öffnen?",
              { title: "Fehler", kind: "warning" }
            );
            if (fallback) {
              await openUrl("https://github.com/Malionaro/Youtube-Soundcload/releases/latest");
            }
          }
        }
      } else {
        // No installer asset found, fallback to opening the page
        const confirmed = await confirm(
          `Version ${latestVersion} ist verfügbar. Kein Installer gefunden – möchtest du die Download-Seite öffnen?`,
          { title: "Update verfügbar", kind: "info" }
        );
        if (confirmed) {
          await openUrl("https://github.com/Malionaro/Youtube-Soundcload/releases/latest");
        }
      }
    } else {
      tag.textContent = "Aktuell";
      tag.className = "status-tag";
      if (manual) log("✅ SoundSync ist auf dem neuesten Stand", "success");
    }
  } catch (e) {
    console.warn("GitHub Update Check failed:", e);
    if (manual) {
      tag.textContent = "Fehler";
      tag.className = "status-tag error";
      log(`ℹ️ Update-Prüfung: Verbindung zu GitHub fehlgeschlagen`, "warning");
    }
  } finally {
    btn.disabled = false;
    btn.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M23 4v6h-6M1 20v-6h6"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg> Auf Updates prüfen`;
  }
}

// ─── UI Update ───────────────────────────────────────────────────────────────
function updateUI() {
  $("url-label").textContent = _("url_label");
  urlInput.placeholder = _("url_placeholder");
  $("folder-label").textContent = _("folder_label");
  folderInput.placeholder = _("folder_placeholder");
  $("format-label").textContent = _("format_label");
  $("cookies-label").textContent = _("cookies_label");
  cookiesInput.placeholder = _("cookies_placeholder");
  $("download-btn-text").textContent = _("start_download");
  $("cancel-btn-text").textContent = _("cancel");
  $("convert-btn-text").textContent = _("convert_file");
  $("browse-btn").textContent = _("browse");
  $("cookies-btn").textContent = _("select");
  $("progress-label").textContent = _("ready_to_start");
  $("convert-label").textContent = _("conversion_waiting");
  $("total-progress-label").textContent = _("total_progress");
  $("log-title").textContent = _("activity_log");
  $("clear-log-btn").textContent = _("clear_log");
  $("sidebar-title").textContent = _("downloaded_tracks");
  $("scroll-to-current-btn").textContent = _("scroll_to_current");
  $("status-text").textContent = _("ready");
  $("footer-version").textContent = _("version", { version: appVersion });
  $("app-version-badge").textContent = `v${appVersion}`;
  if (maybeElement("update-current-version")) {
    $("update-current-version").textContent = appVersion;
  }
  $("no-downloads-text").textContent = _("no_downloads_yet");
  const dragDropText = maybeElement("drag-drop-text");
  if (dragDropText) dragDropText.textContent = _("drag_drop_hint");
  $("log-search").setAttribute("placeholder", _("search_placeholder"));
  const autoUrlToggle = document.getElementById("auto-url-toggle") as HTMLInputElement;
  if (autoUrlToggle) autoUrlToggle.checked = config.auto_url_detection;
}

// ─── Event Listeners ─────────────────────────────────────────────────────────
function setupEventListeners() {
  on("url-input", "input", updateDownloadBtnState);
  on("auto-url-toggle", "change", (e) => {
    config.auto_url_detection = (e.target as HTMLInputElement).checked;
    if (config.auto_url_detection) {
      startClipboardWatcher();
      log("Automatische URL-Erkennung aktiviert", "success");
    } else {
      stopClipboardWatcher();
      hideDetectedUrlPrompt();
      log("Automatische URL-Erkennung deaktiviert", "warning");
    }
    saveConfig();
  });

  // ─── Settings Modal ────────────────────────────────────────────────────────
  on("settings-btn", "click", () => {
    ($("settings-modal") as HTMLElement).style.display = "flex";
    // Sync checkboxes with current config
    ($("discord-rpc-toggle") as HTMLInputElement).checked = config.discord_rpc;
    ($("disable-changelog-toggle") as HTMLInputElement).checked = config.disable_changelog;
    ($("auto-scroll-toggle") as HTMLInputElement).checked = config.auto_scroll_log !== false;
    ($("auto-scroll-tracks-toggle") as HTMLInputElement).checked = config.auto_scroll_tracks !== false;
    ($("eco-mode-toggle") as HTMLInputElement).checked = config.eco_mode !== false;
    ($("auto-tagging-toggle") as HTMLInputElement).checked = config.auto_tagging !== false;
    ($("accent-color-picker") as HTMLInputElement).value = config.accent_color || "#6c5ce7";
  });

  on("accent-color-picker", "input", (e) => {
    config.accent_color = (e.target as HTMLInputElement).value;
    applyTheme();
  });

  on("accent-color-picker", "change", () => {
    saveConfig();
    log("🎨 Akzentfarbe gespeichert", "success");
  });

  on("reset-theme-btn", "click", () => {
    config.accent_color = "#6c5ce7";
    config.custom_background = "";
    const picker = document.getElementById("accent-color-picker") as HTMLInputElement;
    if (picker) picker.value = "#6c5ce7";
    applyTheme();
    saveConfig();
    log("🔄 Design zurückgesetzt", "info");
  });

  on("bg-browse-btn", "click", async () => {
    const file = await open({
      title: _("select_bg"),
      filters: [{ name: "Images", extensions: ["jpg", "png", "webp", "jpeg"] }],
    });
    if (file) {
      config.custom_background = file as string; // Store raw path, not asset URL
      applyTheme();
      saveConfig();
      log("🖼️ Hintergrundbild aktualisiert", "success");
    }
  });

  on("settings-modal-close", "click", () => {
    ($("settings-modal") as HTMLElement).style.display = "none";
  });

  on("save-settings-btn", "click", async () => {
    config.discord_rpc = ($("discord-rpc-toggle") as HTMLInputElement).checked;
    config.disable_changelog = ($("disable-changelog-toggle") as HTMLInputElement).checked;
    config.auto_scroll_log = ($("auto-scroll-toggle") as HTMLInputElement).checked;
    config.auto_scroll_tracks = ($("auto-scroll-tracks-toggle") as HTMLInputElement).checked;
    config.eco_mode = ($("eco-mode-toggle") as HTMLInputElement).checked;
    config.auto_tagging = ($("auto-tagging-toggle") as HTMLInputElement).checked;
    
    // Apply Eco Mode immediately
    if (config.eco_mode) {
      if (!isWindowVisible || !document.hasFocus()) {
        enterEcoMode();
      }
    } else {
      exitEcoMode();
    }
    
    // Apply Discord RPC immediately
    if (config.discord_rpc) {
      updateDiscordPresence("Ready to download", "Settings saved");
    } else {
      invoke("update_discord_presence", { details: "", stateMsg: "" });
    }
    
    await saveConfigImmediate();
    ($("settings-modal") as HTMLElement).style.display = "none";
    log("⚙️ Einstellungen gespeichert", "success");
  });

  on("check-update-btn", "click", () => checkForUpdates(true));

  on("perform-search-btn", "click", performSearch);
  on("search-input", "keypress", (e: KeyboardEvent) => {
    if (e.key === "Enter") performSearch();
  });
  on("refresh-trending-btn", "click", loadTrending);

  if (config.auto_url_detection) {
      startClipboardWatcher();
      log("Automatische URL-Erkennung aktiviert", "success");
    } else {
      stopClipboardWatcher();
      hideDetectedUrlPrompt();
      log("Automatische URL-Erkennung deaktiviert", "warning");
  }

  on("detected-url-use", "click", useDetectedUrl);
  on("detected-url-dismiss", "click", hideDetectedUrlPrompt);
  
  on("clear-url-btn", "click", () => {
    if (isDownloading) return;
    urlInput.value = "";
    updateDownloadBtnState();
    log("🧹 URL und Liste geleert", "info");
    trackList.querySelectorAll(".track-card").forEach(el => el.remove());
    resetProgress();
    setStatus(_("ready"), "info");
    const emptyState = document.getElementById("empty-state");
    if (emptyState) emptyState.style.display = "flex";
  });

  // Browse folder
  on("browse-btn", "click", async () => {
    const folder = await open({ directory: true, title: _("folder_label") });
    if (folder) {
      config.download_folder = folder as string;
      folderInput.value = config.download_folder;
      void saveConfig();
      log(`✅ Folder set: ${config.download_folder}`, "success");
      updateDownloadBtnState();
    }
  });

  // Open folder
  on("open-folder-btn", "click", async () => {
    if (config.download_folder) {
      try {
        await openPath(config.download_folder);
      } catch (e) {
        log("Fehler beim Öffnen des Ordners: " + e, "error");
      }
    }
  });

  // Browse cookies – only YouTube-compatible cookies.txt (Netscape format)
  on("cookies-btn", "click", async () => {
    const file = await open({
      title: "YouTube Cookies (cookies.txt / Netscape-Format)",
      filters: [{ name: "YouTube Cookies (*.txt)", extensions: ["txt"] }],
    });
    if (file) {
      config.cookies_path = file as string;
      cookiesInput.value = config.cookies_path;
      void saveConfig();
      log(`🍪 YouTube Cookies geladen: ${config.cookies_path}`, "info");
    }
  });

  // Download
  if (downloadBtn) downloadBtn.addEventListener("click", startDownload);
  if (cancelBtn) cancelBtn.addEventListener("click", cancelDownload);

  // Clear log
  on("clear-log-btn", "click", () => {
    logOutput.innerHTML = "";
    log("🧹 Log cleared", "info");
  });

  // Format and Quality changes
  on("format-select", "change", () => {
    config.format = formatSelect.value;
    updateQualityOptions();
    config.quality = qualitySelect.value;
    void saveConfig();
  });
  
  on("quality-select", "change", () => {
    config.quality = qualitySelect.value;
    void saveConfig();
  });

  // Log search
  on("log-search", "input", (e) => {
    const query = (e.target as HTMLInputElement).value.toLowerCase();
    logOutput.querySelectorAll(".log-line").forEach((el) => {
      (el as HTMLElement).style.display =
        el.textContent?.toLowerCase().includes(query) ? "" : "none";
    });
  });

  // Theme toggle
  on("theme-toggle", "click", toggleTheme);

  // Language
  on("language-select", "change", (e) => {
    const lang = (e.target as HTMLSelectElement).value;
    if (translations[lang]) {
      currentLang = lang;
      t = translations[lang];
      config.language = lang;
      void saveConfig();
      updateUI();
      log(`🌐 Language: ${lang}`, "info");
    }
  });

  // Convert modal
  on("convert-btn", "click", () => {
    $("convert-modal").style.display = "flex";
  });
  
  // Language Select
  on("language-select", "change", async (e: Event) => {
    const lang = (e.target as HTMLSelectElement).value;
    localStorage.setItem("language", lang);
    await loadTranslations(lang);
  });
  
  // TV Mode Toggle
  on("tv-mode-btn", "click", () => {
    isTvMode = !isTvMode;
    if (isTvMode) {
      document.body.classList.add("tv-mode");
      log("📺 TV-Modus aktiviert", "info");
      // Focus first input
      setTimeout(() => urlInput?.focus(), 100);
    } else {
      document.body.classList.remove("tv-mode");
      log("📺 TV-Modus deaktiviert", "info");
    }
  });

  // Basic Keyboard Navigation for TV Mode
  document.addEventListener("keydown", (e) => {
    if (!isTvMode) return;
    
    const focusable = Array.from(document.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
    )).filter(el => {
      const rect = el.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0 && window.getComputedStyle(el).visibility !== 'hidden';
    });
    
    if (focusable.length === 0) return;
    
    const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
    
    if (e.key === "ArrowDown" || e.key === "ArrowRight") {
      e.preventDefault();
      const nextIndex = currentIndex + 1 < focusable.length ? currentIndex + 1 : 0;
      focusable[nextIndex]?.focus();
    } else if (e.key === "ArrowUp" || e.key === "ArrowLeft") {
      e.preventDefault();
      const prevIndex = currentIndex - 1 >= 0 ? currentIndex - 1 : focusable.length - 1;
      focusable[prevIndex]?.focus();
    } else if (e.key === "Escape") {
      isTvMode = false;
      document.body.classList.remove("tv-mode");
      log("📺 TV-Modus deaktiviert", "info");
    }
  });

  on("convert-modal-close", "click", () => {
    $("convert-modal").style.display = "none";
  });
  on("convert-modal", "click", (e) => {
    if (e.target === $("convert-modal")) $("convert-modal").style.display = "none";
  });

  // Convert browse
  on("convert-browse-btn", "click", async () => {
    const files = await open({ 
      title: _("select_file"),
      multiple: true 
    });
    if (files && Array.isArray(files)) {
      selectedConversionFiles = files;
      ($("convert-file-input") as HTMLInputElement).value = _("files_selected", { count: files.length });
      $("convert-status-text").textContent = _("ready_to_convert");
      $("convert-status-text").className = "status-text success";
    } else if (files) {
      selectedConversionFiles = [files as string];
      ($("convert-file-input") as HTMLInputElement).value = files as string;
      $("convert-status-text").textContent = _("ready_to_convert");
      $("convert-status-text").className = "status-text success";
    }
  });

  // Start conversion
  on("start-convert-btn", "click", startConversion);

  // System check modal
  on("system-check-btn", "click", () => {
    $("system-modal").style.display = "flex";
    checkSystem();
  });
  on("system-modal-close", "click", () => {
    $("system-modal").style.display = "none";
  });
  on("system-modal", "click", (e) => {
    if (e.target === $("system-modal")) $("system-modal").style.display = "none";
  });

  // Install buttons
  on("install-ffmpeg-btn", "click", async (e) => {
    const btn = e.currentTarget as HTMLButtonElement;
    btn.disabled = true;
    btn.textContent = "⌛ " + _("installing");
    log("🔧 Installing FFmpeg...", "info");
    try {
      const res = await invoke<string>("install_ffmpeg");
      log(`✅ ${res}`, "success");
      await checkSystem();
      if (res.includes("already installed")) {
        log("ℹ️ FFmpeg ist bereits installiert, wird aber nicht im System-Pfad gefunden. Bitte starte die App neu.", "warning");
      }
    } catch (err) {
      log(`❌ ${err}`, "error");
    } finally {
      btn.disabled = false;
      btn.textContent = "FFmpeg installieren";
    }
  });

  on("install-ytdlp-btn", "click", async (e) => {
    const btn = e.currentTarget as HTMLButtonElement;
    btn.disabled = true;
    btn.textContent = "⌛ " + _("installing");
    log("🔧 Installing yt-dlp...", "info");
    try {
      const res = await invoke<string>("install_ytdlp");
      log(`✅ ${res}`, "success");
      await checkSystem();
    } catch (err) {
      log(`❌ ${err}`, "error");
    } finally {
      btn.disabled = false;
      btn.textContent = "yt-dlp installieren";
    }
  });

  // Scroll to current
  on("scroll-to-current-btn", "click", () => {
    const active = trackList.querySelector(".track-card.active");
    if (active) active.scrollIntoView({ behavior: "smooth", block: "center" });
  });
  // Drag and drop
  const dropZone = maybeElement("drag-drop-zone");
  if (dropZone) {
    dropZone.addEventListener("dragover", (e) => {
      e.preventDefault();
      dropZone.classList.add("drag-over");
    });
    dropZone.addEventListener("dragleave", () => dropZone.classList.remove("drag-over"));
    dropZone.addEventListener("drop", (e) => {
      e.preventDefault();
      dropZone.classList.remove("drag-over");
      const text = e.dataTransfer?.getData("text/plain");
      if (text && (text.includes("youtube") || text.includes("soundcloud") || text.includes("youtu.be"))) {
        urlInput.value = text;
        updateDownloadBtnState();
        log(`📋 URL dropped: ${text}`, "info");
      }
    });
  }

  // Keyboard shortcuts
  document.addEventListener("keydown", (e) => {
    // Start Download: Ctrl + Enter
    if (e.ctrlKey && e.key === "Enter" && !downloadBtn.disabled) {
      e.preventDefault();
      startDownload();
    }
    
    // Clear Log: Ctrl + L
    if (e.ctrlKey && e.key === "l") {
      e.preventDefault();
      logOutput.innerHTML = "";
      log("🧹 Log cleared via shortcut", "info");
    }

    // Open Settings: Ctrl + ,
    if (e.ctrlKey && e.key === ",") {
      e.preventDefault();
      getEl("settings-btn").click();
    }

    // Close Modals: Escape
    if (e.key === "Escape") {
      const modals = document.querySelectorAll(".modal-overlay");
      modals.forEach(m => (m as HTMLElement).style.display = "none");
      hideDetectedUrlPrompt();
    }
  });
}

// ─── Tauri Event Listeners ───────────────────────────────────────────────────
function setupTauriListeners() {
  listen<DownloadProgress>("download-progress", (event) => {
    if (!isDownloading) return;
    const p = event.payload;
    
    // Add active animation class if downloading
    downloadProgress.classList.add("active");
    totalProgress.classList.add("active");

    // Global progress
    if (p.status === "downloading") {
      updateTotalProgress(p.current, p.percent);
      downloadProgress.style.width = `${p.percent}%`;
      $("progress-percent").textContent = `${p.percent.toFixed(1)}%`;
      $("progress-label").textContent = _("progress_downloading", {
        percent: p.percent.toFixed(0),
        speed: p.speed || "...",
        eta: p.eta || "...",
      });
      updateDiscordPresence(`Downloading: ${p.percent.toFixed(0)}%`, `${p.current}/${p.total} | ${p.title}`);
    } else if (p.status === "converting") {
      updateTotalProgress(p.current, 100);
      convertProgress.classList.add("active");
      convertProgress.style.width = "100%";
      $("convert-label").textContent = `🔄 ${p.title}`;
      updateDiscordPresence("Converting track...", p.title);
    }

    // Individual track progress
    const card = document.getElementById(`track-card-${p.current}`);
    if (card) {
      const bar = card.querySelector(".track-progress-bar") as HTMLElement;
      const stat = card.querySelector(".track-stat") as HTMLElement;
      if (p.status === "downloading") {
        if (bar) {
          bar.style.width = `${p.percent}%`;
          bar.style.background = "var(--accent)";
        }
        if (stat) stat.textContent = `${p.percent.toFixed(0)}%`;
      } else if (p.status === "converting") {
        if (bar) {
          bar.style.width = "100%";
          bar.style.background = "var(--warning)";
        }
        if (stat) stat.textContent = "🔄";
      }
    }
  });

    // Cleaned up unused track-complete listener

  listen<string>("download-log", (event) => {
    const line = event.payload;
    // Don't flood with every line, only meaningful ones
    if (
      line.includes("[download]") ||
      line.includes("[ExtractAudio]") ||
      line.includes("[Merger]") ||
      line.includes("ERROR") ||
      line.includes("WARNING") ||
      line.includes("Deleting")
    ) {
      const type = line.includes("ERROR") ? "error" : line.includes("WARNING") ? "warning" : "info";
      log(line, type);
    }
  });

  listen<string>("download-finished", () => {
    isDownloading = false;
    downloadBtn.disabled = false;
    cancelBtn.disabled = true;
    formatSelect.disabled = false;
    (getEl("convert-btn") as HTMLButtonElement).disabled = false;

    downloadProgress.classList.remove("active");
    totalProgress.classList.remove("active");
    convertProgress.classList.remove("active");

    // Reset conversion label
    $("convert-label").textContent = _("conversion_waiting");
    convertProgress.style.width = "0%";

    downloadProgress.style.width = "100%";
    totalProgress.style.width = "100%";
    setStatus(_("download_complete"), "success");
    $("progress-label").textContent = _("download_complete");
    log(`🎉 ${_("download_complete")} ${completedTracks}/${totalTracks}`, "success");
    updateDiscordPresence(_("download_complete"), `${completedTracks}/${totalTracks} Tracks`);
  });

  listen<string>("download-error", (event) => {
    isDownloading = false;
    downloadBtn.disabled = false;
    cancelBtn.disabled = true;
    formatSelect.disabled = false;
    downloadProgress.classList.remove("active");
    totalProgress.classList.remove("active");
    convertProgress.classList.remove("active");
    setStatus(_("error_occurred"), "error");
    log(`❌ ${event.payload}`, "error");
    updateDiscordPresence("Error occurred", event.payload);
  });

  // Conversion events
  listen<{ status: string; percent: number; filename: string }>("convert-progress", (event) => {
    const p = event.payload;
    const bar = $("modal-convert-progress") as HTMLElement;
    bar.style.width = `${p.percent}%`;
    $("convert-status-text").textContent = `${_("converting")} ${p.percent.toFixed(0)}%`;
    $("convert-status-text").className = "status-text converting";
    updateDiscordPresence(`Converting: ${p.percent.toFixed(0)}%`, p.filename);
  });

  listen<string>("convert-finished", (event) => {
    $("convert-status-text").textContent = `${_("conversion_complete")} → ${event.payload}`;
    $("convert-status-text").className = "status-text success";
    $("modal-progress-container").style.display = "none";
    log(`✅ Converted: ${event.payload}`, "success");
  });

  listen<string>("convert-error", (event) => {
    $("convert-status-text").textContent = _("conversion_failed");
    $("convert-status-text").className = "status-text error";
    $("modal-progress-container").style.display = "none";
    log(`❌ Conversion: ${event.payload}`, "error");
  });

  listen<{ url: string; format?: string; autoStart?: boolean }>("remote-url-received", (event) => {
    const p = event.payload;
    console.log("Remote URL received:", p.url);

    urlInput.value = p.url || "";
    
    if (p.format) {
      if (p.format === "audio") {
        formatSelect.value = "mp3";
      } else if (p.format === "video") {
        formatSelect.value = "mp4";
      } else {
        formatSelect.value = p.format;
      }
      formatSelect.dispatchEvent(new Event("change"));
    }

    updateDownloadBtnState();
    log(`📱 Link vom Gerät empfangen: ${p.url}`, "success");

    if (p.autoStart) {
      if (!downloadBtn.disabled && !isDownloading) {
        log(`🚀 Automatischer Download gestartet...`, "info");
        startDownload();
      } else {
        log(`⚠️ Auto-Start abgebrochen: Ordner fehlt oder Download läuft bereits.`, "warning");
      }
    }
  });
}

// ─── Automatic URL Detection ────────────────────────────────────────────────
function startClipboardWatcher() {
  if (clipboardWatchTimer !== null) return;
  clipboardWatchTimer = window.setInterval(checkClipboardForMediaUrl, 2500);
  checkClipboardForMediaUrl();
}

function stopClipboardWatcher() {
  if (clipboardWatchTimer === null) return;
  window.clearInterval(clipboardWatchTimer);
  clipboardWatchTimer = null;
}

async function checkClipboardForMediaUrl() {
  if (!config.auto_url_detection || isDownloading) return;

  try {
    const text = await invoke<string>("read_clipboard_text");
    const url = extractMediaUrl(text);
    if (!url || url === lastDetectedUrl || url === urlInput.value.trim()) return;

    lastDetectedUrl = url;
    pendingDetectedUrl = url;
    showDetectedUrlPrompt(url);
    await notifyDetectedUrl(url);
  } catch {
    // Clipboard can be temporarily locked by another app; ignore and try again.
  }
}

function extractMediaUrl(text: string): string | null {
  const match = text.match(/https?:\/\/[^\s"'<>]+/i);
  if (!match) return null;

  const url = match[0].replace(/[),.;]+$/, "");
  try {
    const parsed = new URL(url);
    const host = parsed.hostname.toLowerCase();
    if (
      host === "youtu.be" ||
      host.endsWith("youtube.com") ||
      host.endsWith("music.youtube.com") ||
      host.endsWith("soundcloud.com") ||
      host.endsWith("spotify.com")
    ) {
      return url;
    }
  } catch {
    return null;
  }

  return null;
}

// fetchSpotifyMetadata removed – was unused dead code

function showDetectedUrlPrompt(url: string) {
  $("detected-url-text").textContent = url;
  $("detected-url-banner").style.display = "flex";
}

function hideDetectedUrlPrompt() {
  pendingDetectedUrl = "";
  $("detected-url-banner").style.display = "none";
}

function useDetectedUrl() {
  if (!pendingDetectedUrl) return;
  urlInput.value = pendingDetectedUrl;
  updateDownloadBtnState();
  log(`URL übernommen: ${pendingDetectedUrl}`, "success");
  hideDetectedUrlPrompt();
}

async function notifyDetectedUrl(url: string) {
  try {
    let permissionGranted = await isPermissionGranted();
    if (!permissionGranted) {
      permissionGranted = (await requestPermission()) === "granted";
    }
    if (permissionGranted) {
      sendNotification({
        title: "SoundSync URL erkannt",
        body: `YouTube/SoundCloud-Link gefunden. In der App übernehmen? ${url}`,
      });
    }
  } catch {
    // In-app prompt is enough if native notifications are unavailable.
  }
}

// ─── Download Logic ──────────────────────────────────────────────────────────
async function startDownload() {
  let url = urlInput.value.trim();
  if (!url || !config.download_folder) return;

  // ... (Spotify logic)

  isDownloading = true;
  downloadBtn.disabled = true;
  cancelBtn.disabled = false;
  formatSelect.disabled = true;
  (getEl("convert-btn") as HTMLButtonElement).disabled = true;
  completedTracks = 0;
  totalTracks = 0;
  startTime = Date.now();
  playlistEntries = [];

  // Clear track list (keep empty state)
  trackList.querySelectorAll(".track-card").forEach(el => el.remove());
  const emptyState = document.getElementById("empty-state");
  if (emptyState) emptyState.style.display = "flex";

  resetProgress();
  setStatus(_("analyzing_url"), "info");
  log(_("analyzing_url"), "info");
  updateDiscordPresence(_("analyzing_url"), url);

  try {
    await invoke("reset_download_cancel");

    // Step 1: Get playlist info
    const info = await invoke<PlaylistInfo>("get_playlist_info", {
      url,
      cookiesPath: config.cookies_path || null,
    });

    totalTracks = info.total;
    playlistEntries = info.entries;
    log(_("tracks_found", { count: String(totalTracks) }), "success");
    $("progress-label").textContent = _("tracks_found", { count: String(totalTracks) });

    // Add track cards
    $("empty-state").style.display = "none";
    for (const entry of info.entries) {
      addTrackCard(entry);
    }

    // Step 2: Start download (Concurrent execution)
    const concurrencyLimit = 3; // Number of simultaneous downloads
    const executing = new Set<Promise<void>>();

    for (let i = 0; i < playlistEntries.length; i++) {
      if (!isDownloading) break; // User cancelled
      
      const entry = playlistEntries[i];
      const card = document.getElementById(`track-card-${i + 1}`);
      
      // Create the task promise
      const task = (async () => {
        if (!isDownloading) return;
        if (card) card.classList.add("active");

        try {
          await invoke("download_track", {
            request: {
              url: entry.url,
              format: formatSelect.value,
              folder: config.download_folder,
              cookies_path: config.cookies_path || null,
              quality: qualitySelect.value,
            },
            trackIndex: i + 1,
            totalTracks: totalTracks,
            trackTitle: entry.title
          });

          if (card) {
            const bar = card.querySelector(".track-progress-bar") as HTMLElement;
            const stat = card.querySelector(".track-stat") as HTMLElement;
            if (bar) { bar.style.width = "100%"; bar.style.background = "var(--success)"; }
            if (stat) stat.textContent = "✅";
          }
        } catch (e) {
          if (!isDownloading) return;
          log(`❌ ${entry.title}: ${e}`, "error");
          if (card) {
            const bar = card.querySelector(".track-progress-bar") as HTMLElement;
            const stat = card.querySelector(".track-stat") as HTMLElement;
            if (bar) bar.style.background = "var(--error)";
            if (stat) stat.textContent = "❌";
          }
        } finally {
          if (card) card.classList.remove("active");
          if (isDownloading) {
            completedTracks++;
            updateTotalProgress();
          }
        }
      })();

      executing.add(task);
      task.finally(() => executing.delete(task));

      // If we hit the limit, wait for at least one task to finish
      if (executing.size >= concurrencyLimit) {
        await Promise.race(executing);
      }
    }

    // Wait for all remaining tasks to finish
    await Promise.all(executing);

    if (isDownloading) {
      isDownloading = false;
      downloadBtn.disabled = false;
      cancelBtn.disabled = true;
      formatSelect.disabled = false;
      (getEl("convert-btn") as HTMLButtonElement).disabled = false;

      downloadProgress.style.width = "100%";
      convertProgress.style.width = "100%";
      totalProgress.style.width = "100%";
      setStatus(_("download_complete"), "success");
      $("progress-label").textContent = _("download_complete");
      log(`🎉 ${_("download_complete")} ${completedTracks}/${totalTracks}`, "success");
    }
  } catch (e) {
    log(`❌ ${e}`, "error");
    setStatus(_("error_occurred"), "error");
    isDownloading = false;
    downloadBtn.disabled = false;
    cancelBtn.disabled = true;
    formatSelect.disabled = false;
    (getEl("convert-btn") as HTMLButtonElement).disabled = false;
  }
}

async function cancelDownload() {
  isDownloading = false; // Sofort auf false setzen, um JS-Schleifen zu stoppen
  
  // UI sofort zurücksetzen für besseres Feedback
  downloadBtn.disabled = false;
  cancelBtn.disabled = true;
  formatSelect.disabled = false;
  (getEl("convert-btn") as HTMLButtonElement).disabled = false;
  
  downloadProgress.classList.remove("active");
  totalProgress.classList.remove("active");
  convertProgress.classList.remove("active");
  
  setStatus(_("download_cancelled"), "error");
  log("🛑 " + _("download_cancelled"), "warning");

  try {
    await invoke("cancel_download");
  } catch (e) {
    log(`⚠️ ${e}`, "warning");
  }
}

// ─── Conversion ──────────────────────────────────────────────────────────────
async function startConversion() {
  const inputPath = ($("convert-file-input") as HTMLInputElement).value;
  const outputFormat = ($("convert-format-select") as HTMLSelectElement).value;
  const quality = ($("convert-quality-select") as HTMLSelectElement).value;

  if (!inputPath) return;

  $("modal-progress-container").style.display = "block";
  ($("modal-convert-progress") as HTMLElement).style.width = "0%";
  $("convert-status-text").textContent = _("converting");
  $("convert-status-text").className = "status-text converting";

  try {
    await invoke("convert_file", {
      request: { input_path: inputPath, output_format: outputFormat, quality },
    });
  } catch (e) {
    $("convert-status-text").textContent = `❌ ${e}`;
    $("convert-status-text").className = "status-text error";
    $("modal-progress-container").style.display = "none";
  }
}

// ─── System Check ────────────────────────────────────────────────────────────
async function checkSystem(autoShowModal: boolean = true) {
  try {
    const result = await invoke<{
      ffmpeg_installed: boolean;
      ffmpeg_version: string;
      ytdlp_installed: boolean;
      ytdlp_version: string;
    }>("check_system");

    let isMissingDependencies = false;

    // FFmpeg
    const ffmpegIcon = $("ffmpeg-check").querySelector(".check-icon")!;
    const ffmpegStatus = $("ffmpeg-status");
    if (result.ffmpeg_installed) {
      ffmpegIcon.textContent = "✅";
      ffmpegIcon.classList.remove("loading");
      ffmpegStatus.textContent = result.ffmpeg_version;
      $("install-ffmpeg-btn").style.display = "none";
    } else {
      ffmpegIcon.textContent = "❌";
      ffmpegIcon.classList.remove("loading");
      ffmpegStatus.textContent = _("ffmpeg_missing");
      $("install-ffmpeg-btn").style.display = "block";
      isMissingDependencies = true;
    }

    // yt-dlp
    const ytdlpIcon = $("ytdlp-check").querySelector(".check-icon")!;
    const ytdlpStatus = $("ytdlp-status");
    if (result.ytdlp_installed) {
      ytdlpIcon.textContent = "✅";
      ytdlpIcon.classList.remove("loading");
      ytdlpStatus.textContent = result.ytdlp_version;
      $("install-ytdlp-btn").style.display = "none";
    } else {
      ytdlpIcon.textContent = "❌";
      ytdlpIcon.classList.remove("loading");
      ytdlpStatus.textContent = _("ytdlp_missing");
      $("install-ytdlp-btn").style.display = "block";
      isMissingDependencies = true;
    }

    if (result.ffmpeg_installed && result.ytdlp_installed) {
      log(_("all_systems_go"), "success");
    } else if (autoShowModal && isMissingDependencies) {
      // Automatically show the system modal if things are missing
      $("system-modal").style.display = "flex";
      log("⚠️ " + _("system_dependencies_missing"), "warning");
    }
  } catch (e) {
    log(`⚠️ System check failed: ${e}`, "warning");
  }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────
function log(message: string, type: string = "info") {
  const line = document.createElement("div");
  line.className = `log-line ${type}`;
  const timestamp = new Date().toLocaleTimeString();
  line.textContent = `[${timestamp}] ${message}`;
  logOutput.appendChild(line);
  
  // Limit DOM nodes to prevent memory leaks
  const maxLines = 300;
  while (logOutput.children.length > maxLines) {
    logOutput.firstChild?.remove();
  }
  
  // Auto-scroll only if enabled in settings
  if (config.auto_scroll_log !== false && !logScrollPending) {
    logScrollPending = true;
    requestAnimationFrame(() => {
      logOutput.scrollTop = logOutput.scrollHeight;
      logScrollPending = false;
    });
  }
}

function setStatus(text: string, type: string = "info") {
  $("status-text").textContent = text;
  const dot = $("status-dot");
  dot.className = "status-dot";
  if (type === "error") dot.classList.add("error");
  if (type === "warning") dot.classList.add("warning");
}

function updateDownloadBtnState() {
  const hasUrl = urlInput.value.trim().length > 0;
  const hasFolder = config.download_folder.length > 0;
  downloadBtn.disabled = !hasUrl || !hasFolder || isDownloading;
}

function resetProgress() {
  downloadProgress.style.width = "0%";
  convertProgress.style.width = "0%";
  totalProgress.style.width = "0%";
  $("progress-percent").textContent = "";
  $("convert-percent").textContent = "";
  $("total-percent").textContent = "";
}

function updateTotalProgress(currentTrackIndex?: number, trackPercent?: number) {
  if (totalTracks <= 0) return;
  
  // Real-time overall value: (completed tracks + progress of current track)
  let currentOverallValue = completedTracks;
  if (currentTrackIndex !== undefined && trackPercent !== undefined) {
    if (currentTrackIndex > completedTracks) {
       currentOverallValue = (currentTrackIndex - 1) + (trackPercent / 100);
    }
  }

  const pct = (currentOverallValue / totalTracks) * 100;
  totalProgress.style.width = `${pct}%`;
  $("total-percent").textContent = `${Math.min(100, Math.floor(pct))}%`;

  const elapsed = (Date.now() - startTime) / 1000;
  const avgPerTrack = currentOverallValue > 0 ? elapsed / currentOverallValue : 0;
  const remaining = (totalTracks - currentOverallValue) * avgPerTrack;
  const eta = remaining > 0 ? formatTime(remaining) : "--:--:--";

  $("total-progress-label").textContent = _("total_progress_detail", {
    percent: Math.floor(pct).toString(),
    completed: String(completedTracks),
    total: String(totalTracks),
    eta,
  });

  // Highlight active track card efficiently
  const activeIdx = currentTrackIndex ? currentTrackIndex : completedTracks + 1;
  const prevActive = trackList.querySelector(".track-card.active");
  if (prevActive) prevActive.classList.remove("active");
  const nextActive = document.getElementById(`track-card-${activeIdx}`);
  if (nextActive) {
    nextActive.classList.add("active");
    // Auto-scroll sidebar to keep current track visible
    if (config.auto_scroll_tracks !== false) {
      nextActive.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  }
}

function formatTime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  return `${h.toString().padStart(2, "0")}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
}

function addTrackCard(entry: PlaylistEntry) {
  const card = document.createElement("div");
  card.className = "track-card";
  card.id = `track-card-${entry.index}`;

  const thumb = document.createElement("img");
  thumb.className = "track-thumb";
  thumb.alt = entry.title;
  if (entry.thumbnail) {
    thumb.src = entry.thumbnail;
    thumb.onerror = () => {
      thumb.style.display = "none";
    };
  } else {
    thumb.style.background = "var(--bg-hover)";
    thumb.style.display = "flex";
  }

  const info = document.createElement("div");
  info.className = "track-info";
  info.innerHTML = `
    <div style="display: flex; justify-content: space-between; align-items: center;">
      <span class="track-title">${escapeHtml(entry.title)}</span>
    </div>
    <div style="display: flex; justify-content: space-between; align-items: center; margin-top: 4px;">
      <span class="track-index">#${entry.index}${entry.duration ? ` • ${formatTime(entry.duration)}` : ""}</span>
      <span class="track-stat" style="font-size: 11px; opacity: 0.8; font-weight: bold;">0%</span>
    </div>
    <div class="track-progress-bg" style="width: 100%; height: 4px; background: rgba(255,255,255,0.1); border-radius: 2px; margin-top: 6px; overflow: hidden;">
       <div class="track-progress-bar" style="width: 0%; height: 100%; background: var(--accent); transition: width 0.2s ease, background 0.3s ease;"></div>
    </div>
  `;

  card.appendChild(thumb);
  card.appendChild(info);
  trackList.appendChild(card);
}



function escapeHtml(text: string): string {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

function toggleTheme() {
  const html = document.documentElement;
  const isDark = html.getAttribute("data-theme") === "dark";
  html.setAttribute("data-theme", isDark ? "light" : "dark");
  $("theme-icon-dark").style.display = isDark ? "none" : "block";
  $("theme-icon-light").style.display = isDark ? "block" : "none";
}

function applyTheme() {
  const root = document.documentElement;
  const accent = config.accent_color || "#6c5ce7";
  root.style.setProperty("--accent", accent);
  
  // Create derived colors from accent
  const r = parseInt(accent.slice(1, 3), 16);
  const g = parseInt(accent.slice(3, 5), 16);
  const b = parseInt(accent.slice(5, 7), 16);
  root.style.setProperty("--accent-glow", `rgba(${r}, ${g}, ${b}, 0.25)`);
  root.style.setProperty("--accent-hover", `rgba(${Math.min(255, r + 30)}, ${Math.min(255, g + 30)}, ${Math.min(255, b + 30)}, 1)`);
  root.style.setProperty("--primary-glow", `rgba(${r}, ${g}, ${b}, 0.3)`);
  
  // Handle custom background image
  let overlay = document.getElementById("custom-bg-overlay");
  if (config.custom_background) {
    if (!overlay) {
      overlay = document.createElement("div");
      overlay.id = "custom-bg-overlay";
      overlay.className = "custom-bg-overlay";
      document.body.prepend(overlay);
    }
    // Convert raw file path to asset URL at render time
    // Guard against already-converted URLs from old config
    let bgUrl = config.custom_background;
    if (!bgUrl.startsWith("http") && !bgUrl.startsWith("asset:")) {
      bgUrl = convertFileSrc(bgUrl);
    }
    console.log("[BG] Setting background image:", bgUrl);
    overlay.style.backgroundImage = `url('${bgUrl}')`;
  } else if (overlay) {
    overlay.remove();
  }
}

async function updateRemoteStatus() {
  try {
    const ip = await invoke<string>("ss_get_local_ip");
    const remoteUrl = `http://${ip}:3030`;
    const qrPlaceholder = $("qr-placeholder");
    if (qrPlaceholder) {
      qrPlaceholder.innerHTML = `
        <div style="background: white; padding: 12px; border-radius: 18px; box-shadow: 0 10px 30px rgba(0,0,0,0.5); display: flex; flex-direction: column; align-items: center; gap: 10px; width: 100%;">
          <img src="https://api.qrserver.com/v1/create-qr-code/?size=120x120&data=${encodeURIComponent(remoteUrl)}" 
               alt="QR Code" style="width:120px;height:120px;display:block;" />
          <div style="display: flex; align-items: center; gap: 8px; background: #f5f5f7; padding: 4px 8px; border-radius: 8px; border: 1px solid #eee; width: 100%; justify-content: center;">
            <code style="font-size:10px;color:#333;font-weight:700;font-family:monospace;white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">${ip}:3030</code>
            <button id="copy-remote-url" class="btn-icon-copy" title="Kopieren">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#555" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
            </button>
          </div>
        </div>
      `;

      on("copy-remote-url", "click", async () => {
        try {
          await navigator.clipboard.writeText(remoteUrl);
          const btn = $("copy-remote-url");
          const originalSVG = btn.innerHTML;
          btn.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#22c55e" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>`;
          setTimeout(() => btn.innerHTML = originalSVG, 2000);
          log("📋 Link kopiert!", "success");
        } catch (e) {
          console.error("Clipboard error", e);
          log("❌ Fehler beim Kopieren", "error");
        }
      });
    }
  } catch (e) {
    console.error("Failed to get local IP", e);
  }
}

// ─── Eco-Mode: Reduce resource usage when window is hidden/minimized ─────────
function setupEcoMode() {
  // Listen for window visibility changes
  document.addEventListener("visibilitychange", () => {
    isWindowVisible = document.visibilityState === "visible";
    if (config.eco_mode !== false) {
      if (!isWindowVisible) {
        enterEcoMode();
      } else {
        exitEcoMode();
      }
    }
  });

  // Also handle window blur/focus for minimized state
  window.addEventListener("blur", () => {
    if (config.eco_mode !== false && !isDownloading) {
      // Only enter full eco-mode if not downloading
      enterEcoMode();
    }
  });
  window.addEventListener("focus", () => {
    exitEcoMode();
  });
}

function enterEcoMode() {
  if (ecoModeActive) return;
  ecoModeActive = true;
  document.documentElement.classList.add("eco-mode");
  
  // Pause clipboard watcher to save CPU
  if (!isDownloading) {
    stopClipboardWatcher();
  }
  
  console.log("[ECO] Eco-Mode aktiviert – Hintergrundaktivität reduziert");
}

function exitEcoMode() {
  if (!ecoModeActive) return;
  ecoModeActive = false;
  document.documentElement.classList.remove("eco-mode");
  
  // Resume clipboard watcher if enabled
  if (config.auto_url_detection) {
    startClipboardWatcher();
  }
  
  console.log("[ECO] Eco-Mode deaktiviert – Volle Leistung");
}

async function saveConfigImmediate() {
  try {
    await invoke("save_config", { config });
  } catch (e) {
    console.error("Failed to save config:", e);
    log(`Config konnte nicht gespeichert werden: ${e}`, "warning");
  }
}

const saveConfig = debounce(saveConfigImmediate, 500);

// ─── Translations ────────────────────────────────────────────────────────────

async function loadTranslations(lang: string) {
  try {
    const response = await fetch(`/src/i18n/${lang}.json`);
    if (!response.ok) throw new Error("Failed to load translations");
    currentTranslations = await response.json();
    applyTranslations();
  } catch (error) {
    console.error("Translation load error:", error);
  }
}

function applyTranslations() {
  document.querySelectorAll("[data-i18n]").forEach(el => {
    const key = el.getAttribute("data-i18n");
    if (key && currentTranslations[key]) {
      el.textContent = currentTranslations[key];
    }
  });

  document.querySelectorAll("[data-i18n-placeholder]").forEach(el => {
    const key = el.getAttribute("data-i18n-placeholder");
    if (key && currentTranslations[key]) {
      (el as HTMLInputElement).placeholder = currentTranslations[key];
    }
  });

  document.querySelectorAll("[data-i18n-title]").forEach(el => {
    const key = el.getAttribute("data-i18n-title");
    if (key && currentTranslations[key]) {
      el.setAttribute("title", currentTranslations[key]);
    }
  });

  document.querySelectorAll("[data-i18n-label]").forEach(el => {
    const key = el.getAttribute("data-i18n-label");
    if (key && currentTranslations[key]) {
      el.setAttribute("label", currentTranslations[key]);
    }
  });
}

// ─── Boot ────────────────────────────────────────────────────────────────────
document.addEventListener("DOMContentLoaded", init);
