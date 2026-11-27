import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { open, confirm } from "@tauri-apps/plugin-dialog";
import { isPermissionGranted, onAction, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { openUrl, openPath } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import logoImg from "./assets/logo.png";

const DEFAULT_ACCENT_COLOR = "#8e7cf3";
const THEME_STORAGE_KEY = "soundsync_theme_v1";
const BATCH_PLACEHOLDER = "Sammelkorb Batch-Download";

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
let currentLang = "en";
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

function translatedOrFallback(key: string, fallback: string, vars?: Record<string, string | number>): string {
  const text = _(key, vars);
  return text === key ? fallback : text;
}

function diagnoseDownloadError(error: unknown) {
  const raw = String(error || "").trim();
  const lower = raw.toLowerCase();

  if (lower.includes("failed to start yt-dlp") || lower.includes("yt-dlp not found") || lower.includes("program not found")) {
    return { titleKey: "error_title_ytdlp_missing", helpKey: "error_help_ytdlp_missing", raw };
  }
  if (lower.includes("ffmpeg") && (lower.includes("not found") || lower.includes("no such file") || lower.includes("cannot find"))) {
    return { titleKey: "error_title_ffmpeg_missing", helpKey: "error_help_ffmpeg_missing", raw };
  }
  if (lower.includes("cookies") || lower.includes("sign in") || lower.includes("login") || lower.includes("private video") || lower.includes("confirm your age")) {
    return { titleKey: "error_title_cookies", helpKey: "error_help_cookies", raw };
  }
  if (lower.includes("unsupported url") || lower.includes("no suitable extractor") || lower.includes("invalid url")) {
    return { titleKey: "error_title_invalid_url", helpKey: "error_help_invalid_url", raw };
  }
  if (lower.includes("403") || lower.includes("forbidden") || lower.includes("unavailable") || lower.includes("copyright") || lower.includes("removed")) {
    return { titleKey: "error_title_unavailable", helpKey: "error_help_unavailable", raw };
  }
  if (lower.includes("timed out") || lower.includes("temporary failure") || lower.includes("connection") || lower.includes("network") || lower.includes("dns")) {
    return { titleKey: "error_title_network", helpKey: "error_help_network", raw };
  }
  if (lower.includes("permission denied") || lower.includes("access is denied") || lower.includes("zugriff verweigert")) {
    return { titleKey: "error_title_folder_permission", helpKey: "error_help_folder_permission", raw };
  }

  return { titleKey: "error_title_unknown", helpKey: "error_help_unknown", raw };
}

function logErrorDetails(error: unknown, context?: string) {
  const diagnosis = diagnoseDownloadError(error);
  const prefix = context ? `${context}: ` : "";
  log(`❌ ${prefix}${_(diagnosis.titleKey)}`, "error");
  log(`💡 ${_(diagnosis.helpKey)}`, "warning");
  if (diagnosis.raw) {
    const compactRaw = diagnosis.raw.split("\n").slice(-6).join(" | ");
    log(`Details: ${compactRaw}`, "info");
  }
  showErrorPanel(diagnosis.titleKey, diagnosis.helpKey);
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
  after_download?: string;
  sponsorblock_mode?: string;
  sponsorblock_categories?: string;
  gpu_encoder?: string;
}

interface PlaylistEntry {
  title: string;
  url: string;
  thumbnail: string | null;
  duration: number | null;
  index: number;
  queueId?: string;
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
  language: "en",
  disable_changelog: false,
  auto_url_detection: true,
  discord_rpc: true,
  auto_scroll_log: true,
  auto_scroll_tracks: true,
  eco_mode: true,
  auto_tagging: true,
  after_download: "nothing",
};

let isDownloading = false;
let totalTracks = 0;
let completedTracks = 0;
let discordRpcLogged = false;
let downloadRunId = 0;
let startTime = 0;
let playlistEntries: PlaylistEntry[] = [];
let clipboardWatchTimer: number | null = null;
let clipboardCheckInFlight = false;
let lastDetectedUrl = "";
let pendingDetectedUrl = "";
let pendingDetectedUrls: string[] = [];
let appVersion = "0.0.0";
let selectedConversionFiles: string[] = [];
let isConvertingBatch = false;
let logScrollPending = false;
let trackScrollPending = false;
let isWindowVisible = true;
let ecoModeActive = false;
let isTvMode = false;
let currentTranslations: Record<string, string> = {};
let notificationActionListenerReady = false;
let trackProgressByIndex = new Map<number, number>();
let convertingTracks = new Set<number>();
let isDownloadPaused = false;
let lastFailedAction: (() => void | Promise<void>) | null = null;

let isBatchQueueMode = false;
interface QueueItem {
  id: string;
  url: string;
  title: string;
  source: "clipboard" | "extension" | "manual";
  sourceType: "youtube" | "soundcloud" | "spotify" | "apple_music" | "other";
  status: "queued" | "downloading" | "done" | "error";
  addedAt: number;
}
let clipboardQueue: QueueItem[] = [];
let queueSearchInput: HTMLInputElement;
let queueSortSelect: HTMLSelectElement;
let queueRetryFailedBtn: HTMLButtonElement;
const QUEUE_STORAGE_KEY = "soundsync_clipboard_queue_v2";

// ─── Download History (Verlauf) ──────────────────────────────────────────────
interface HistoryEntry {
  id: string;
  title: string;
  url: string;
  format: string;
  quality: string;
  status: "done" | "error";
  finishedAt: number;
}
let downloadHistory: HistoryEntry[] = [];
let historyList: HTMLElement;
let historyClearBtn: HTMLButtonElement;
const HISTORY_STORAGE_KEY = "soundsync_history_v1";
const HISTORY_MAX_ENTRIES = 200;

// ─── DOM Elements ────────────────────────────────────────────────────────────
const $ = (id: string) => {
  const el = document.getElementById(id);
  if (!el) {
    console.warn(`[DOM] #${id} fehlt – Dummy wird verwendet (bitte index.html prüfen)`);
    return document.createElement("div") as HTMLElement;
  }
  return el as HTMLElement;
};
const $req = (id: string) => {
  const el = document.getElementById(id);
  if (!el) throw new Error(`[DOM] Pflicht-Element #${id} fehlt in index.html`);
  return el as HTMLElement;
};
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
let pauseResumeBtn: HTMLButtonElement;
let logOutput: HTMLElement;
let trackList: HTMLElement;
let downloadProgress: HTMLElement;
let convertProgress: HTMLElement;
let totalProgress: HTMLElement;

let queueBadge: HTMLElement;
let queueList: HTMLElement;
let queueManualInput: HTMLInputElement;
let queueManualAddBtn: HTMLButtonElement;
let queueDownloadAllBtn: HTMLButtonElement;
let queueClearAllBtn: HTMLButtonElement;

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
    pauseResumeBtn = getEl("pause-resume-btn");
    logOutput = getEl("log-output");
    trackList = getEl("track-list");
    downloadProgress = getEl("download-progress");
    convertProgress = getEl("convert-progress");
    totalProgress = getEl("total-progress");

    queueBadge = getEl("queue-badge");
    queueList = getEl("queue-list");
    queueManualInput = getEl("queue-manual-input") as HTMLInputElement;
    queueManualAddBtn = getEl("queue-manual-add-btn") as HTMLButtonElement;
    queueDownloadAllBtn = getEl("queue-download-all-btn") as HTMLButtonElement;
    queueClearAllBtn = getEl("queue-clear-all-btn") as HTMLButtonElement;
    queueSearchInput = getEl("queue-search-input") as HTMLInputElement;
    queueSortSelect = getEl("queue-sort-select") as HTMLSelectElement;
    queueRetryFailedBtn = getEl("queue-retry-failed-btn") as HTMLButtonElement;
    historyList = getEl("history-list");
    historyClearBtn = getEl("history-clear-btn") as HTMLButtonElement;
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

async function chooseDownloadFolder() {
  const folder = await open({
    directory: true,
    multiple: false,
    title: _("select_download_folder"),
  });
  if (folder) {
    config.download_folder = folder as string;
    folderInput.value = config.download_folder;
    await saveConfigImmediate();
    updateDownloadBtnState();
    logKey("log_folder_set", "success", { path: config.download_folder });
  }
}

function showErrorPanel(titleKey: string, helpKey: string) {
  const panel = maybeElement("ux-error-panel");
  if (!panel) return;
  $("ux-error-title").textContent = _(titleKey);
  $("ux-error-help").textContent = _(helpKey);
  panel.style.display = "flex";

  const cookiesBtn = maybeElement("ux-error-cookies-btn") as HTMLButtonElement | null;
  if (cookiesBtn) {
    cookiesBtn.style.display = titleKey === "error_title_cookies" ? "" : "none";
  }
}

function hideErrorPanel() {
  const panel = maybeElement("ux-error-panel");
  if (panel) panel.style.display = "none";
}

function showOnboardingIfNeeded() {
  if (localStorage.getItem("soundsync_onboarding_done_v1") === "1") return;
  const modal = maybeElement("onboarding-modal");
  if (modal) modal.style.display = "flex";
}

function closeOnboarding() {
  localStorage.setItem("soundsync_onboarding_done_v1", "1");
  const modal = maybeElement("onboarding-modal");
  if (modal) modal.style.display = "none";
}

let lastTrayUpdate = 0;
function updateTrayStatus(status: string, force = false) {
  const now = Date.now();
  if (!force && now - lastTrayUpdate < 1500) return;
  lastTrayUpdate = now;
  void invoke("update_tray_status", { status }).catch(() => {});
}

function setOptGroupActive(group: HTMLElement, active: boolean) {
  group.style.display = active ? "" : "none";
  group.hidden = !active;
  if (group instanceof HTMLOptGroupElement) {
    group.disabled = !active;
  }
  group.querySelectorAll("option").forEach((option) => {
    option.disabled = !active;
    option.hidden = !active;
  });
}

function updateQualityOptions() {
  if (!formatSelect || formatSelect.selectedIndex < 0 || !formatSelect.options[formatSelect.selectedIndex]) return;
  const opt = formatSelect.options[formatSelect.selectedIndex] as HTMLOptionElement;
  const group = opt.parentElement as HTMLOptGroupElement | null;
  const isVideo = group?.tagName === "OPTGROUP" && group.label === "Video";
  const currentVal = qualitySelect.value;

  if (isVideo) {
    setOptGroupActive(audioQualities, false);
    setOptGroupActive(videoQualities, true);
    const matchingOpt = Array.from(videoQualities.querySelectorAll("option")).find((o) => o.value === currentVal);
    if (matchingOpt) {
      matchingOpt.selected = true;
    } else {
      const fallback = Array.from(videoQualities.querySelectorAll("option")).find((o) => o.value === "1080p")
        || videoQualities.querySelector("option");
      if (fallback) fallback.selected = true;
    }
  } else {
    setOptGroupActive(videoQualities, false);
    setOptGroupActive(audioQualities, true);
    const matchingOpt = Array.from(audioQualities.querySelectorAll("option")).find((o) => o.value === currentVal);
    if (matchingOpt) {
      matchingOpt.selected = true;
    } else {
      const fallback = Array.from(audioQualities.querySelectorAll("option")).find((o) => o.value === "best")
        || audioQualities.querySelector("option");
      if (fallback) fallback.selected = true;
    }
  }
}

async function init() {
  setupElements();
  document.addEventListener("contextmenu", (event) => event.preventDefault());
  // Version schnell, Rest parallel wo möglich (Instant-Start: UI zuerst)
  try {
    appVersion = await getVersion();
  } catch { appVersion = "0.0.0"; }
  try {
    const savedLang = localStorage.getItem("language") || "de";
    const langSel = document.getElementById("language-select") as HTMLSelectElement | null;
    if (langSel) langSel.value = savedLang;
    await loadTranslations(savedLang);
    
    config = await invoke<AppConfig>("load_config");
    if (config.after_download === undefined) config.after_download = "nothing";
    const afterDownloadSelect = document.getElementById("after-download-select") as HTMLSelectElement | null;
    if (afterDownloadSelect) afterDownloadSelect.value = config.after_download;
    if (config.download_folder) folderInput.value = config.download_folder;
    if (config.cookies_path) cookiesInput.value = config.cookies_path;
    if (config.auto_url_detection === undefined) config.auto_url_detection = true;
    if (config.discord_rpc === undefined) config.discord_rpc = true;
    if (config.format) formatSelect.value = config.format;
    
    updateQualityOptions();
    if (config.quality) {
      const activeGroup = formatSelect.options[formatSelect.selectedIndex]?.parentElement?.getAttribute("label") === "Video"
        ? videoQualities
        : audioQualities;
      const optMatch = Array.from(activeGroup.querySelectorAll("option")).find((o) => o.value === config.quality);
      if (optMatch) optMatch.selected = true;
    }
    loadQueue();
    loadHistory();

    // Restore persisted light/dark mode
    const savedTheme = localStorage.getItem(THEME_STORAGE_KEY);
    if (savedTheme === "light" || savedTheme === "dark") {
      applyColorScheme(savedTheme);
    }

    // Apply defaults for new settings
    if (config.auto_scroll_log === undefined) config.auto_scroll_log = true;
    if (config.auto_scroll_tracks === undefined) config.auto_scroll_tracks = true;
    if (config.eco_mode === undefined) config.eco_mode = true;
    if (config.sponsorblock_mode === undefined) config.sponsorblock_mode = "off";
    if (config.sponsorblock_categories === undefined) config.sponsorblock_categories = "sponsor,selfpromo,interaction";
    if (config.gpu_encoder === undefined) config.gpu_encoder = "auto";

    applyTheme();
    if (config.accent_color) {
        ($("accent-color-picker") as HTMLInputElement).value = config.accent_color;
    }
    
    // Auto-tagging sync
    if (config.auto_tagging === undefined) config.auto_tagging = true;
    const taggingToggle = document.getElementById("auto-tagging-toggle") as HTMLInputElement;
    if (taggingToggle) taggingToggle.checked = config.auto_tagging;

    // SponsorBlock / GPU sync (neu verdrahtet)
    const sbMode = document.getElementById("sponsorblock-mode-select") as HTMLSelectElement | null;
    if (sbMode) sbMode.value = config.sponsorblock_mode || "off";
    const sbCats = document.getElementById("sponsorblock-categories-input") as HTMLInputElement | null;
    if (sbCats) sbCats.value = config.sponsorblock_categories || "sponsor,selfpromo,interaction";
    const gpuSel = document.getElementById("gpu-encoder-select") as HTMLSelectElement | null;
    if (gpuSel) gpuSel.value = config.gpu_encoder || "auto";
    
    if (config.language) {
      await loadTranslations(config.language);
      const langSel2 = document.getElementById("language-select") as HTMLSelectElement | null;
      if (langSel2) langSel2.value = currentLang;
    }
  } catch (e) {
    log("⚠️ Config could not be loaded, using defaults", "warning");
  }
  updateUI();
  setupNotificationClickHandler();
  setupEventListeners();
  setupTauriListeners();
  setupTabs();
  renderQueue();
  renderHistory();
  // UI sofort bereit melden – Rest lazy im Hintergrund (Instant-Start)
  log(`🎵 SoundSync Downloader v${appVersion} ready`, "success");
  updateTrayStatus(_("ready"));
  window.setTimeout(showOnboardingIfNeeded, 700);

  const idle = (fn: () => void, ms: number) => {
    const w = window as Window & { requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => void };
    if (typeof w.requestIdleCallback === "function") {
      w.requestIdleCallback(fn, { timeout: ms + 2000 });
    } else {
      globalThis.setTimeout(fn, ms);
    }
  };
  // Schwere Hintergrundtasks erst nach Idle – kein Startup-Block mehr
  idle(() => { void syncRemoteQueue(); }, 1500);
  idle(() => { void updateRemoteStatus(); }, 2000);
  idle(() => { setupEcoMode(); }, 2500);
  idle(() => {
    if (config.discord_rpc) void updateDiscordPresence("Ready to download", "Waiting for URLs...");
  }, 3000);
  idle(() => { void checkSystem(false); }, 3500);
  // Update-Checks nur noch manuell/opt-in: kein Auto-Download beim Start.
  // checkForUpdates(false) läuft nur wenn Changelog nicht deaktiviert ist, stark verzögert.
  if (!config.disable_changelog) {
    window.setTimeout(() => checkForUpdates(false), 15000);
  }
}

async function autoUpdateYtdlp() {
  log(_("log_checking_ytdlp_update"), "info");
  try {
    const result = await invoke<string>("update_ytdlp");
    if (result.includes("is up to date")) {
      log(_("log_ytdlp_up_to_date"), "success");
    } else {
      log(`🔄 yt-dlp: ${result}`, "success");
      await checkSystem(false);
    }
  } catch (e) {
    log(_("log_ytdlp_update_failed", { error: String(e) }), "warning");
  }
}

// ─── Search & Trending ───────────────────────────────────────────────────────
async function performSearch() {
  const input = getEl<HTMLInputElement>("search-input");
  const typeSelect = getEl<HTMLSelectElement>("search-type-select");
  const query = input.value.trim();
  if (!query) return;

  const resultsGrid = $("search-results");
  const searchType = typeSelect?.value === "playlists" ? "playlists" : "videos";
  resultsGrid.innerHTML = `<div class="loading-state"><div class="spinner"></div><span>${escapeHtml(_("search_button"))} … ${escapeHtml(query)}</span></div>`;

  try {
    const command = searchType === "playlists" ? "search_playlists" : "search_videos";
    const info = await invoke<PlaylistInfo>(command, { query });
    renderResults(info.entries, resultsGrid, searchType);
  } catch (e) {
    resultsGrid.innerHTML = `<div class="empty-state"><p class="status-text error">${escapeHtml(_("error_occurred"))}: ${escapeHtml(String(e))}</p></div>`;
  }
}

async function loadTrending() {
  const resultsGrid = $("trending-results");
  resultsGrid.innerHTML = `<div class="loading-state"><div class="spinner"></div><span>${escapeHtml(_("loading_charts"))}</span></div>`;

  try {
    const info = await invoke<PlaylistInfo>("get_trending_videos");
    renderResults(info.entries, resultsGrid, "videos");
  } catch (e) {
    resultsGrid.innerHTML = `<div class="empty-state">
      <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--danger)" stroke-width="1.5" opacity="0.8">
        <circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>
      </svg>
      <p class="status-text error" style="max-width: 400px; word-break: break-word;">${escapeHtml(_("error_occurred"))}: ${escapeHtml(String(e))}</p>
    </div>`;
  }
}

function renderResults(entries: PlaylistEntry[], container: HTMLElement, resultType: "videos" | "playlists" = "videos") {
  if (!entries || entries.length === 0) {
    container.innerHTML = `<div class="empty-state"><p>${escapeHtml(_("queue_no_results"))}</p></div>`;
    return;
  }

  container.innerHTML = "";
  entries.forEach((entry, idx) => {
    const card = document.createElement("div");
    card.className = "result-card";
    card.style.animationDelay = `${Math.min(idx, 12) * 0.03}s`;

    const durationStr = entry.duration ? formatDuration(entry.duration) : "";
    const isPlaylist = resultType === "playlists";
    const safeThumb = entry.thumbnail && /^https?:\/\//i.test(entry.thumbnail)
      ? escapeAttribute(entry.thumbnail)
      : escapeAttribute(logoImg);
    
    card.innerHTML = `
      <div class="result-thumb-container">
        <img src="${safeThumb}" class="result-thumb" loading="lazy" />
        ${durationStr ? `<span class="result-duration">${escapeHtml(durationStr)}</span>` : ""}
        ${isPlaylist ? `<span class="result-duration">Playlist</span>` : ""}
      </div>
      <div class="result-info">
        <h3 class="result-title" title="${escapeAttribute(entry.title)}">${escapeHtml(entry.title)}</h3>
        <span class="result-channel">${isPlaylist ? "YouTube Playlist" : "YouTube"}</span>
      </div>
      <div class="result-actions">
        <button class="btn btn-small btn-primary btn-full download-result-btn" data-url="${escapeAttribute(entry.url)}">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          ${escapeHtml(isPlaylist ? _("result_add_to_queue") : _("result_download"))}
        </button>
      </div>
    `;

    const imgEl = card.querySelector<HTMLImageElement>(".result-thumb");
    if (imgEl) {
      imgEl.onerror = () => {
        if (imgEl.src !== logoImg) imgEl.src = logoImg;
      };
    }

    card.querySelector(".download-result-btn")?.addEventListener("click", async (e) => {
      e.stopPropagation();
      const url = (e.currentTarget as HTMLElement).getAttribute("data-url");
      if (url) {
        if (isPlaylist) {
          await importPlaylist(url, "manual");
          return;
        }
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

function logKey(key: string, type: string = "info", vars: Record<string, string> = {}) {
  log(_(key, vars), type);
}

let lastDiscordUpdate = 0;
async function updateDiscordPresence(details: string, state_msg: string, force = false) {
  // Throttle: max 1 Update / 5s (Progress-Events feuern mehrmals pro Sekunde x3 parallel)
  const now = Date.now();
  const important = force || /complete|finished|error|fehler|✅|❌/i.test(details + " " + state_msg);
  if (!important && now - lastDiscordUpdate < 5000) return;
  lastDiscordUpdate = now;
  try {
    await invoke("update_discord_presence", { details, stateMsg: state_msg });
    if (config.discord_rpc && !discordRpcLogged) {
      discordRpcLogged = true;
      logKey("log_discord_rpc_updated", "info");
    }
  } catch (e) {
    logKey("log_discord_rpc_error", "info", { error: String(e) });
  }
}

function selectPlatformInstallerAsset(assets: any[]): any | undefined {
  const ua = (navigator.userAgent || "").toLowerCase();
  if (ua.includes("mac")) {
    return assets.find((a: any) => typeof a?.name === "string" && a.name.toLowerCase().endsWith(".dmg"));
  }
  if (ua.includes("linux")) {
    return (
      assets.find((a: any) => typeof a?.name === "string" && a.name.toLowerCase().endsWith(".appimage")) ||
      assets.find((a: any) => typeof a?.name === "string" && a.name.toLowerCase().endsWith(".deb")) ||
      assets.find((a: any) => typeof a?.name === "string" && a.name.toLowerCase().endsWith(".rpm"))
    );
  }
  return assets.find(
    (a: any) => typeof a?.name === "string" && (a.name.toLowerCase().endsWith(".msi") || a.name.toLowerCase().endsWith(".exe")),
  );
}

async function checkForUpdates(manual: boolean = false) {
  const tag = $("update-status-tag");
  const btn = $("check-update-btn") as HTMLButtonElement;
  const btnText = maybeElement("check-update-btn-text");
  
  const currentVersion = await getVersion();
  
  if (manual) {
    tag.textContent = _("update_checking");
    tag.className = "status-tag checking";
    btn.disabled = true;
  }

  const controller = new AbortController();
  const timeoutId = window.setTimeout(() => controller.abort(), 10000);

  try {
    const response = await fetch("https://api.github.com/repos/Malionaro/Youtube-Soundcload/releases/latest", {
      signal: controller.signal,
    });
    if (!response.ok) throw new Error("GitHub API unreachable");
    
    const data = await response.json();
    const latestVersion = String(data.tag_name || "").replace(/^v/i, "");

    const isNewer = (v1: string, v2: string) => {
      const parts1 = v1.split(".").map((p) => parseInt(p, 10));
      const parts2 = v2.split(".").map((p) => parseInt(p, 10));
      for (let i = 0; i < Math.max(parts1.length, parts2.length); i++) {
        const a = isNaN(parts1[i]) ? 0 : parts1[i];
        const b = isNaN(parts2[i]) ? 0 : parts2[i];
        if (a > b) return true;
        if (a < b) return false;
      }
      return false;
    };
    
    if (latestVersion && isNewer(latestVersion, currentVersion)) {
      tag.textContent = _("update_available");
      tag.className = "status-tag update-available";
      
      const assets = Array.isArray(data.assets) ? data.assets : [];
      const installerAsset = selectPlatformInstallerAsset(assets);

      if (installerAsset) {
        const confirmed = await confirm(
          _("update_confirm_download", { version: latestVersion, file: installerAsset.name }),
          { title: _("update_confirm_title"), kind: "info" }
        );
        
        if (confirmed) {
          tag.textContent = _("update_downloading");
          tag.className = "status-tag checking";
          if (btn) btn.disabled = true;
          if (btnText) btnText.textContent = _("update_downloading_btn");
          log(`📥 Update ${latestVersion}: ${_("update_downloading")}`, "info");

          try {
            const msg = await invoke<string>("download_and_install_update", {
              downloadUrl: installerAsset.browser_download_url,
              filename: installerAsset.name,
            });
            
            tag.textContent = _("update_installed");
            tag.className = "status-tag update-available";
            log(msg || _("update_installer_started"), "success");
          } catch (installErr) {
            tag.textContent = _("update_error");
            tag.className = "status-tag error";
            log(`❌ ${_("install_failed")}: ${installErr}`, "error");
            
            const fallback = await confirm(
              _("update_install_failed_fallback"),
              { title: _("update_error"), kind: "warning" }
            );
            if (fallback) {
              await openUrl("https://github.com/Malionaro/Youtube-Soundcload/releases/latest");
            }
          }
        }
      } else {
        const confirmed = await confirm(
          _("update_confirm_open_page", { version: latestVersion }),
          { title: _("update_confirm_title"), kind: "info" }
        );
        if (confirmed) {
          await openUrl("https://github.com/Malionaro/Youtube-Soundcload/releases/latest");
        }
      }
    } else {
      tag.textContent = _("up_to_date");
      tag.className = "status-tag";
      if (manual) logKey("log_app_up_to_date", "success");
    }
  } catch (e) {
    console.warn("GitHub Update Check failed:", e);
    if (manual) {
      tag.textContent = _("update_error");
      tag.className = "status-tag error";
      logKey("log_update_check_failed", "warning");
    }
  } finally {
    window.clearTimeout(timeoutId);
    if (btn && document.contains(btn)) {
      btn.disabled = false;
      if (btnText) btnText.textContent = _("check_updates");
    }
  }
}

// ─── UI Update ───────────────────────────────────────────────────────────────
function updateUI() {
  $("tab-downloader-label").textContent = _("tab_downloader");
  $("tab-search-label").textContent = _("tab_search");
  $("tab-trending-label").textContent = _("tab_trending");
  $("tab-queue-label").textContent = _("tab_queue");
  $("tab-history-label").textContent = _("tab_history");
  $("url-label").textContent = _("url_label");
  urlInput.placeholder = _("url_placeholder");
  $("folder-label").textContent = _("folder_label");
  folderInput.placeholder = _("folder_placeholder");
  $("format-label").textContent = _("format_label");
  $("cookies-label").textContent = _("cookies_label");
  cookiesInput.placeholder = _("cookies_placeholder");
  $("download-btn-text").textContent = _("start_download");
  $("cancel-btn-text").textContent = _("cancel");
  $("pause-resume-btn-text").textContent = isDownloadPaused ? _("resume_download") : _("pause_download");
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
  $("log-search").setAttribute("placeholder", _("search_placeholder"));
  const autoUrlToggle = document.getElementById("auto-url-toggle") as HTMLInputElement;
  if (autoUrlToggle) autoUrlToggle.checked = config.auto_url_detection;
  refreshActiveTabIndicator();
}

function refreshActiveTabIndicator() {
  const indicator = document.getElementById("nav-indicator");
  const activeTab = document.querySelector(".tab-btn.active") as HTMLElement | null;
  if (!indicator || !activeTab) return;
  indicator.style.transform = `translateX(${activeTab.offsetLeft}px)`;
  indicator.style.width = `${activeTab.offsetWidth}px`;
}

// ─── Event Listeners ─────────────────────────────────────────────────────────
function setupEventListeners() {
  on("url-input", "input", updateDownloadBtnState);
  on("auto-url-toggle", "change", (e) => {
    config.auto_url_detection = (e.target as HTMLInputElement).checked;
    if (config.auto_url_detection) {
      startClipboardWatcher();
      logKey("log_auto_url_enabled", "success");
    } else {
      stopClipboardWatcher();
      hideDetectedUrlPrompt();
      logKey("log_auto_url_disabled", "warning");
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
    ($("accent-color-picker") as HTMLInputElement).value = config.accent_color || DEFAULT_ACCENT_COLOR;
    const afterDownloadSelect = document.getElementById("after-download-select") as HTMLSelectElement | null;
    if (afterDownloadSelect) afterDownloadSelect.value = config.after_download || "nothing";
    const sbMode = document.getElementById("sponsorblock-mode-select") as HTMLSelectElement | null;
    if (sbMode) sbMode.value = config.sponsorblock_mode || "off";
    const sbCats = document.getElementById("sponsorblock-categories-input") as HTMLInputElement | null;
    if (sbCats) sbCats.value = config.sponsorblock_categories || "sponsor,selfpromo,interaction";
    const gpuSel = document.getElementById("gpu-encoder-select") as HTMLSelectElement | null;
    if (gpuSel) gpuSel.value = config.gpu_encoder || "auto";
  });

  on("accent-color-picker", "input", (e) => {
    config.accent_color = (e.target as HTMLInputElement).value;
    applyTheme();
  });

  on("accent-color-picker", "change", () => {
    saveConfig();
    logKey("log_accent_saved", "success");
  });

  on("reset-theme-btn", "click", () => {
    config.accent_color = DEFAULT_ACCENT_COLOR;
    config.custom_background = "";
    const picker = document.getElementById("accent-color-picker") as HTMLInputElement;
    if (picker) picker.value = DEFAULT_ACCENT_COLOR;
    applyTheme();
    saveConfig();
    logKey("log_theme_reset", "info");
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
      logKey("log_background_updated", "success");
    }
  });

  on("settings-modal-close", "click", () => {
    ($("settings-modal") as HTMLElement).style.display = "none";
  });
  on("settings-modal", "click", (e) => {
    if (e.target === $("settings-modal")) $("settings-modal").style.display = "none";
  });
  on("onboarding-modal", "click", (e) => {
    if (e.target === $("onboarding-modal")) closeOnboarding();
  });
  on("github-link", "click", (e) => {
    e.preventDefault();
    void openUrl("https://github.com/Malionaro/Youtube-Soundcload").catch(() => {});
  });

  on("save-settings-btn", "click", async () => {
    config.discord_rpc = ($("discord-rpc-toggle") as HTMLInputElement).checked;
    config.disable_changelog = ($("disable-changelog-toggle") as HTMLInputElement).checked;
    config.auto_scroll_log = ($("auto-scroll-toggle") as HTMLInputElement).checked;
    config.auto_scroll_tracks = ($("auto-scroll-tracks-toggle") as HTMLInputElement).checked;
    config.eco_mode = ($("eco-mode-toggle") as HTMLInputElement).checked;
    config.auto_tagging = ($("auto-tagging-toggle") as HTMLInputElement).checked;
    const afterDownloadSelect = document.getElementById("after-download-select") as HTMLSelectElement | null;
    if (afterDownloadSelect) {
      config.after_download = afterDownloadSelect.value;
    }
    const sbMode = document.getElementById("sponsorblock-mode-select") as HTMLSelectElement | null;
    if (sbMode) config.sponsorblock_mode = sbMode.value;
    const sbCats = document.getElementById("sponsorblock-categories-input") as HTMLInputElement | null;
    if (sbCats) config.sponsorblock_categories = sbCats.value.trim() || "sponsor,selfpromo,interaction";
    const gpuSel = document.getElementById("gpu-encoder-select") as HTMLSelectElement | null;
    if (gpuSel) config.gpu_encoder = gpuSel.value;
    
    // Save configuration first to prevent race condition with Discord Rich Presence
    await saveConfigImmediate();

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
    
    ($("settings-modal") as HTMLElement).style.display = "none";
    logKey("log_settings_saved", "success");
  });

  on("check-update-btn", "click", () => checkForUpdates(true));
  on("ux-error-dismiss-btn", "click", hideErrorPanel);
  on("ux-error-system-btn", "click", () => {
    hideErrorPanel();
    $("system-modal").style.display = "flex";
    checkSystem();
  });
  on("ux-error-cookies-btn", "click", () => {
    hideErrorPanel();
    getEl<HTMLButtonElement>("cookies-btn").click();
  });
  on("ux-error-retry-btn", "click", () => {
    hideErrorPanel();
    void lastFailedAction?.();
  });
  on("onboarding-close", "click", closeOnboarding);
  on("onboarding-done-btn", "click", closeOnboarding);
  on("onboarding-folder-btn", "click", async () => {
    await chooseDownloadFolder();
  });
  on("onboarding-system-btn", "click", () => {
    closeOnboarding();
    $("system-modal").style.display = "flex";
    checkSystem();
  });

  on("perform-search-btn", "click", performSearch);
  on("search-input", "keypress", (e: KeyboardEvent) => {
    if (e.key === "Enter") performSearch();
  });
  on("refresh-trending-btn", "click", loadTrending);

  if (config.auto_url_detection) {
      startClipboardWatcher();
      logKey("log_auto_url_enabled", "success");
    } else {
      stopClipboardWatcher();
      hideDetectedUrlPrompt();
      logKey("log_auto_url_disabled", "warning");
  }

  on("detected-url-use", "click", useDetectedUrl);
  on("detected-url-add", "click", () => {
    void addDetectedUrlsToQueue();
  });
  on("detected-url-dismiss", "click", hideDetectedUrlPrompt);

  on("queue-manual-input", "input", checkPlaylistUrlInput);
  on("history-clear-btn", "click", clearHistory);
  on("queue-search-input", "input", debounce(renderQueue, 120));
  on("queue-sort-select", "change", renderQueue);
  on("queue-retry-failed-btn", "click", retryFailedQueueItems);

  on("queue-manual-add-btn", "click", async () => {
    const url = queueManualInput.value.trim();
    if (url) {
      const isPlaylist = queueManualAddBtn.classList.contains("btn-import-active");
      if (isPlaylist) {
        await importPlaylist(url);
      } else {
        await addToQueue(url, "manual");
        queueManualInput.value = "";
      }
    }
  });

  on("queue-manual-input", "keypress", async (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      const url = queueManualInput.value.trim();
      if (url) {
        const isPlaylist = queueManualAddBtn.classList.contains("btn-import-active");
        if (isPlaylist) {
          await importPlaylist(url);
        } else {
          await addToQueue(url, "manual");
          queueManualInput.value = "";
        }
      }
    }
  });

  on("queue-download-all-btn", "click", startBatchDownload);
  on("queue-clear-all-btn", "click", clearQueue);
  
  on("clear-url-btn", "click", () => {
    if (isDownloading) return;
    urlInput.value = "";
    updateDownloadBtnState();
    logKey("log_url_list_cleared", "info");
    trackList.querySelectorAll(".track-card").forEach(el => el.remove());
    resetProgress();
    setStatus(_("ready"), "info");
    const emptyState = document.getElementById("empty-state");
    if (emptyState) emptyState.style.display = "flex";
  });

  // Browse folder
  on("browse-btn", "click", chooseDownloadFolder);

  // Open folder
  on("open-folder-btn", "click", async () => {
    if (config.download_folder) {
      try {
        await openPath(config.download_folder);
      } catch (e) {
        logKey("log_open_folder_failed", "error", { error: String(e) });
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
      logKey("log_cookies_loaded", "info", { path: config.cookies_path });
    }
  });

  // Download
  if (downloadBtn) downloadBtn.addEventListener("click", startDownload);
  if (cancelBtn) cancelBtn.addEventListener("click", cancelDownload);
  if (pauseResumeBtn) pauseResumeBtn.addEventListener("click", toggleDownloadPause);

  // Clear log
  on("clear-log-btn", "click", () => {
    logOutput.innerHTML = "";
    logKey("log_cleared", "info");
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

  // Log search (debounced)
  const applyLogFilter = debounce(() => {
    const query = (maybeElement("log-search") as HTMLInputElement | null)?.value.toLowerCase() || "";
    logOutput.querySelectorAll(".log-line").forEach((el) => {
      (el as HTMLElement).style.display =
        !query || el.textContent?.toLowerCase().includes(query) ? "" : "none";
    });
  }, 120);
  on("log-search", "input", applyLogFilter);

  // Theme toggle
  on("theme-toggle", "click", toggleTheme);

  // Language
  on("language-select", "change", async (e) => {
    const lang = (e.target as HTMLSelectElement).value;
    if (translations[lang]) {
      config.language = lang;
      localStorage.setItem("language", lang);
      await loadTranslations(lang);
      void saveConfig();
      updateUI();
      logKey("log_language_changed", "info", { lang });
    }
  });

  // Convert modal
  on("convert-btn", "click", () => {
    $("convert-modal").style.display = "flex";
  });
  
  // TV Mode Toggle
  on("tv-mode-btn", "click", () => {
    isTvMode = !isTvMode;
    if (isTvMode) {
      document.body.classList.add("tv-mode");
      logKey("log_tv_mode_enabled", "info");
      // Focus first input
      setTimeout(() => urlInput?.focus(), 100);
    } else {
      document.body.classList.remove("tv-mode");
      logKey("log_tv_mode_disabled", "info");
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
      logKey("log_tv_mode_disabled", "info");
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
    if (!(await confirmInstall("FFmpeg", "Gyan.FFmpeg"))) return;

    const status = $("ffmpeg-status");
    btn.disabled = true;
    btn.textContent = _("installing_with_winget");
    status.textContent = _("install_status_running", { tool: "FFmpeg", package: "Gyan.FFmpeg" });
    log(_("install_log_start", { tool: "FFmpeg", package: "Gyan.FFmpeg" }), "info");
    log(_("install_log_wait"), "info");
    try {
      const res = await invoke<string>("install_ffmpeg");
      log(`✅ ${res}`, "success");
      await checkSystem();
      logKey("log_restarting_for_path", "info");
      setTimeout(async () => {
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      }, 1500);
    } catch (err) {
      status.textContent = _("install_failed");
      log(`❌ ${err}`, "error");
    } finally {
      btn.disabled = false;
      btn.textContent = _("install_ffmpeg_btn");
    }
  });

  on("install-ytdlp-btn", "click", async (e) => {
    const btn = e.currentTarget as HTMLButtonElement;
    if (!(await confirmInstall("yt-dlp", "yt-dlp.yt-dlp"))) return;

    const status = $("ytdlp-status");
    btn.disabled = true;
    btn.textContent = _("installing_with_winget");
    status.textContent = _("install_status_running", { tool: "yt-dlp", package: "yt-dlp.yt-dlp" });
    log(_("install_log_start", { tool: "yt-dlp", package: "yt-dlp.yt-dlp" }), "info");
    log(_("install_log_wait"), "info");
    try {
      const res = await invoke<string>("install_ytdlp");
      log(`✅ ${res}`, "success");
      await checkSystem();
      logKey("log_restarting_for_path", "info");
      setTimeout(async () => {
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      }, 1500);
    } catch (err) {
      status.textContent = _("install_failed");
      log(`❌ ${err}`, "error");
    } finally {
      btn.disabled = false;
      btn.textContent = _("install_ytdlp_btn");
    }
  });

  on("update-ytdlp-btn", "click", async (e) => {
    const btn = e.currentTarget as HTMLButtonElement;
    const status = $("ytdlp-status");
    btn.disabled = true;
    btn.textContent = _("updating_ytdlp");
    status.textContent = _("updating_ytdlp");
    log(_("log_updating_ytdlp_manual"), "info");
    try {
      const res = await invoke<string>("update_ytdlp");
      log(`✅ ${res}`, "success");
      await checkSystem(false);
    } catch (err) {
      status.textContent = _("ytdlp_update_failed");
      log(`❌ ${err}`, "error");
    } finally {
      btn.disabled = false;
      btn.textContent = _("update_ytdlp_btn");
    }
  });

  on("install-pot-provider-btn", "click", async (e) => {
    const btn = e.currentTarget as HTMLButtonElement;
    const confirmed = await confirm(_("install_pot_confirm"), {
      title: _("ytdlp_pot_title"),
      kind: "info",
    });
    if (!confirmed) return;

    const status = $("pot-provider-status");
    btn.disabled = true;
    btn.textContent = _("installing_with_winget");
    status.textContent = _("pot_provider_installing");
    log(_("pot_provider_log_start"), "info");
    log(_("pot_provider_log_detail"), "info");
    try {
      const res = await invoke<string>("install_pot_provider");
      log(`✅ ${res}`, "success");
      await checkSystem(false);
    } catch (err) {
      status.textContent = _("pot_provider_failed");
      log(`❌ ${err}`, "error");
    } finally {
      btn.disabled = false;
      btn.textContent = _("install_pot_provider_btn");
    }
  });

  // Scroll to current
  on("scroll-to-current-btn", "click", () => {
    const active = trackList.querySelector(".track-card.active");
    if (active) active.scrollIntoView({ behavior: "smooth", block: "center" });
  });

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
      logKey("log_cleared_shortcut", "info");
    }

    // Open Settings: Ctrl + ,
    if (e.ctrlKey && e.key === ",") {
      e.preventDefault();
      getEl("settings-btn").click();
    }

    // Close Modals: Escape
    if (e.key === "Escape") {
      const onboarding = maybeElement("onboarding-modal");
      if (onboarding && onboarding.style.display !== "none") {
        closeOnboarding();
      }
      const modals = document.querySelectorAll(".modal-overlay");
      modals.forEach(m => (m as HTMLElement).style.display = "none");
      hideDetectedUrlPrompt();
    }
  });
}

// ─── Tauri Event Listeners ───────────────────────────────────────────────────
function setupTauriListeners() {
  listen("tray-pause-resume", () => {
    if (isDownloading) void toggleDownloadPause();
  });

  listen("tray-cancel-download", () => {
    if (isDownloading) void cancelDownload();
  });

  listen<DownloadProgress>("download-progress", (event) => {
    if (!isDownloading) return;
    const p = event.payload;
    
    // Add active animation class if downloading
    downloadProgress.classList.add("active");
    totalProgress.classList.add("active");

    // Global progress
    if (p.status === "downloading") {
      convertingTracks.delete(p.current);
      const overallPercent = updateTotalProgress(p.current, p.percent);
      const clampedTrackPct = Math.max(0, Math.min(100, p.percent));
      downloadProgress.style.width = `${clampedTrackPct}%`;
      $("progress-percent").textContent = `${clampedTrackPct.toFixed(1)}%`;
      $("progress-label").textContent = _("progress_downloading", {
        percent: clampedTrackPct.toFixed(0),
        speed: p.speed || "...",
        eta: p.eta || "...",
      });
      if (convertingTracks.size === 0) {
        convertProgress.classList.remove("active");
        convertProgress.style.width = "0%";
        $("convert-label").textContent = _("conversion_waiting");
      }
      updateTrayStatus(`${overallPercent.toFixed(0)}% • ${p.title}`);
      updateDiscordPresence(`Downloading: ${clampedTrackPct.toFixed(0)}%`, `${p.current}/${p.total} | ${p.title}`);
    } else if (p.status === "converting") {
      convertingTracks.add(p.current);
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
        const clampedTrackPct = Math.max(0, Math.min(100, p.percent));
        if (bar) {
          bar.style.width = `${clampedTrackPct}%`;
          bar.style.background = "var(--accent)";
        }
        if (stat) stat.textContent = `${clampedTrackPct.toFixed(0)}%`;
      } else if (p.status === "converting") {
        if (bar) {
          bar.style.width = "100%";
          bar.style.background = "var(--warning)";
        }
        if (stat) stat.textContent = "🔄";
      }
    }
  });

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

  // Conversion events
  listen<{ status: string; percent: number; filename: string }>("convert-progress", (event) => {
    const p = event.payload;
    const bar = $("modal-convert-progress") as HTMLElement;
    const clamped = Math.max(0, Math.min(100, p.percent));
    bar.style.width = `${clamped}%`;
    $("convert-status-text").textContent = `${_("converting")} ${clamped.toFixed(0)}% (${p.filename})`;
    $("convert-status-text").className = "status-text converting";
    updateDiscordPresence(`Converting: ${clamped.toFixed(0)}%`, p.filename);
  });

  listen<string>("convert-finished", (event) => {
    if (!isConvertingBatch) {
      $("convert-status-text").textContent = `${_("conversion_complete")} → ${event.payload}`;
      $("convert-status-text").className = "status-text success";
      $("modal-progress-container").style.display = "none";
    }
    log(`✅ Converted: ${event.payload}`, "success");
  });

  listen<string>("convert-error", (event) => {
    if (!isConvertingBatch) {
      $("convert-status-text").textContent = _("conversion_failed");
      $("convert-status-text").className = "status-text error";
      $("modal-progress-container").style.display = "none";
    }
    logErrorDetails(event.payload, "Conversion");
  });

  listen<{ url: string; format?: string; autoStart?: boolean }>("remote-url-received", (event) => {
    const p = event.payload;
    console.log("Remote URL received:", p.url);

    if (p.autoStart) {
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
      logKey("log_remote_link_received", "success", { url: p.url });
      if (!downloadBtn.disabled && !isDownloading) {
        logKey("log_auto_download_started", "info");
        startDownload();
      } else {
        logKey("log_auto_start_cancelled", "warning");
      }
    } else {
      void addToQueue(p.url, "extension");
      logKey("log_extension_link_queued", "success");
      
      // Native notification that it was added
      try {
        isPermissionGranted().then(granted => {
          if (granted) {
            sendNotification({
              title: "SoundSync Browser-Extension",
              body: `Link erfolgreich in Sammelkorb gelegt: ${p.url}`,
            });
          }
        });
      } catch {
        // Ignore notification errors
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
  if (!config.auto_url_detection || isDownloading || clipboardCheckInFlight) return;
  clipboardCheckInFlight = true;

  try {
    const text = await invoke<string>("read_clipboard_text");
    const urls = extractMediaUrls(text).filter(url => url !== urlInput.value.trim());
    if (urls.length === 0) return;

    const detectionSignature = urls.join("\n");
    if (detectionSignature === lastDetectedUrl) return;

    lastDetectedUrl = detectionSignature;
    pendingDetectedUrls = urls;
    pendingDetectedUrl = urls[0];
    showDetectedUrlPrompt(urls);
    await notifyDetectedUrl(urls);
  } catch {
    // Clipboard can be temporarily locked by another app; ignore and try again.
  } finally {
    clipboardCheckInFlight = false;
  }
}

async function openAppWindowFromNotification() {
  try {
    const appWindow = getCurrentWindow();
    await appWindow.show();
    await appWindow.unminimize();
    await appWindow.setFocus();
  } catch (e) {
    console.warn("Could not focus app window from notification:", e);
  }
}

async function setupNotificationClickHandler() {
  if (notificationActionListenerReady) return;
  notificationActionListenerReady = true;

  try {
    await onAction(() => {
      void openAppWindowFromNotification();
    });
  } catch (e) {
    notificationActionListenerReady = false;
    console.warn("Could not register notification click handler:", e);
  }
}

function extractMediaUrl(text: string): string | null {
  return extractMediaUrls(text)[0] || null;
}

function extractMediaUrls(text: string): string[] {
  const matches = text.match(/https?:\/\/[^\s"'<>]+/gi);
  if (!matches) return [];

  const urls: string[] = [];
  const seen = new Set<string>();
  for (const rawUrl of matches) {
    const url = rawUrl.replace(/[),.;]+$/, "");
    if (seen.has(url)) continue;

    if (isSupportedMediaUrl(url)) {
      seen.add(url);
      urls.push(url);
    }
  }

  return urls;
}

function matchesDomain(host: string, domain: string): boolean {
  return host === domain || host.endsWith(`.${domain}`);
}

function isSupportedMediaUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return false;
    const host = parsed.hostname.toLowerCase();
    return (
      host === "youtu.be" ||
      matchesDomain(host, "youtube.com") ||
      matchesDomain(host, "soundcloud.com") ||
      host === "snd.sc" ||
      matchesDomain(host, "spotify.com") ||
      host === "spotify.link" ||
      matchesDomain(host, "music.apple.com")
    );
  } catch {
    return false;
  }
}

// fetchSpotifyMetadata removed – was unused dead code

function showDetectedUrlPrompt(urls: string[]) {
  const prompt = $("detected-url-prompt-text");
  prompt.textContent = urls.length > 1
    ? _("use_urls_prompt").replace("{count}", String(urls.length))
    : _("use_url_prompt");

  $("detected-url-text").textContent = urls.length > 1
    ? `${urls[0]} +${urls.length - 1}`
    : urls[0];
  $("detected-url-banner").style.display = "flex";
}

function hideDetectedUrlPrompt() {
  pendingDetectedUrl = "";
  pendingDetectedUrls = [];
  $("detected-url-banner").style.display = "none";
}

async function useDetectedUrl() {
  if (!pendingDetectedUrl) return;
  const first = pendingDetectedUrl;
  const rest = pendingDetectedUrls.filter((u) => u !== first);

  // Läuft gerade ein Download? Dann das Feld nicht überschreiben → alles in den Korb
  if (isDownloading) {
    for (const url of [first, ...rest]) {
      await addToQueue(url, "clipboard");
    }
    logKey("log_clipboard_urls_queued", "success", { count: String(rest.length + 1) });
    hideDetectedUrlPrompt();
    return;
  }

  // Erste URL ins Download-Feld einsetzen …
  urlInput.value = first;
  updateDownloadBtnState();
  // … und Downloader-Tab anzeigen, damit man das Feld auch sieht
  document.querySelector<HTMLElement>('.tab-btn[data-tab="downloader-tab"]')?.click();
  logKey("log_url_accepted", "success", { url: first });
  hideDetectedUrlPrompt();

  // Übrige erkannte URLs gehen nicht verloren → ab in den Korb
  for (const url of rest) {
    await addToQueue(url, "clipboard");
  }
  if (rest.length > 0) {
    logKey("log_clipboard_urls_queued", "success", { count: String(rest.length) });
  }
}

async function addDetectedUrlsToQueue() {
  const urls = pendingDetectedUrls.length > 0 ? pendingDetectedUrls : [pendingDetectedUrl].filter(Boolean);
  if (urls.length === 0) return;

  for (const url of urls) {
    await addToQueue(url, "clipboard");
  }

  logKey("log_clipboard_urls_queued", "success", { count: String(urls.length) });
  hideDetectedUrlPrompt();
}

async function notifyDetectedUrl(urls: string[]) {
  try {
    let permissionGranted = await isPermissionGranted();
    if (!permissionGranted) {
      permissionGranted = (await requestPermission()) === "granted";
    }
    if (permissionGranted) {
      sendNotification({
        title: translatedOrFallback("notification_url_detected_title", "SoundSync URL erkannt"),
        body: urls.length > 1
          ? translatedOrFallback(
              "notification_urls_detected_body",
              `${urls.length} YouTube/SoundCloud-Links gefunden. In SoundSync sammeln?`,
              { count: String(urls.length) },
            )
          : translatedOrFallback(
              "notification_url_detected_body",
              `YouTube/SoundCloud-Link gefunden. In der App übernehmen? ${urls[0]}`,
              { url: urls[0] },
            ),
      });
    }
  } catch {
    // In-app prompt is enough if native notifications are unavailable.
  }
}

// ─── Clipboard Queue (Sammelkorb) Logic ──────────────────────────────────────
function isPlaylistQueueUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    const host = parsed.hostname.toLowerCase();
    const path = parsed.pathname.toLowerCase();
    return (
      ((host === "youtu.be" || matchesDomain(host, "youtube.com")) && parsed.searchParams.has("list")) ||
      (matchesDomain(host, "spotify.com") && (path.includes("/playlist/") || path.includes("/album/"))) ||
      (matchesDomain(host, "music.apple.com") &&
        (path.includes("/playlist/") || (path.includes("/album/") && !parsed.searchParams.has("i")))) ||
      (matchesDomain(host, "soundcloud.com") && path.includes("/sets/"))
    );
  } catch {
    return false;
  }
}

async function addToQueue(url: string, source: "clipboard" | "extension" | "manual", title?: string) {
  // Simple validation to ensure it looks like a URL
  if (!url.startsWith("http://") && !url.startsWith("https://")) {
    logKey("log_invalid_link", "warning", { url });
    return;
  }

  if (isPlaylistQueueUrl(url) && !title) {
    await importPlaylist(url, source);
    return;
  }

  // Check duplicate – non-blocking Tauri dialog statt window.prompt (Webview-sicher)
  const duplicateIndex = clipboardQueue.findIndex(item => item.url === url);
  if (duplicateIndex !== -1) {
    try {
      const addAnyway = await confirm(
        `${_("duplicate_queue_prompt")}\n\n${url}`,
        { title: _("queue_title"), kind: "warning", okLabel: _("duplicate_add"), cancelLabel: _("duplicate_skip") }
      );
      if (!addAnyway) {
        logKey("log_link_already_queued", "info", { url });
        return;
      }
      // Altes Duplikat ersetzen, dann neu anhängen (kein Doppel-Eintrag)
      clipboardQueue.splice(duplicateIndex, 1);
    } catch {
      logKey("log_link_already_queued", "info", { url });
      return;
    }
  }

  // Optimistisch hinzufügen (kein Block durch Metadaten-Lookup), Titel async nachladen
  const newItem: QueueItem = {
    id: createQueueId(),
    url,
    title: title || fallbackQueueTitle(url),
    source,
    sourceType: detectQueueSourceType(url),
    status: "queued",
    addedAt: Date.now(),
  };
  clipboardQueue.push(newItem);
  logKey("log_link_added_queue", "success", { title: newItem.title });
  
  // Play dynamic scale animation on queue badge
  if (queueBadge) {
    queueBadge.textContent = clipboardQueue.length.toString();
    queueBadge.style.display = "flex";
    
    // Trigger pop animation
    queueBadge.style.animation = "none";
    queueBadge.offsetHeight; // force reflow
    queueBadge.style.animation = "badgePop 0.3s cubic-bezier(0.34, 1.56, 0.64, 1) forwards";
  }

  saveQueue();
  renderQueue();

  // Titel im Hintergrund nachladen (nur wenn kein expliziter Titel übergeben wurde)
  if (!title) {
    const itemId = newItem.id;
    void fetchQueueTitle(url).then((fetched) => {
      const item = clipboardQueue.find((q) => q.id === itemId);
      if (item && item.title === newItem.title && fetched && fetched !== item.title) {
        item.title = fetched;
        saveQueue();
        renderQueue();
      }
    });
  }
}

function fallbackQueueTitle(url: string): string {
  try {
    const parsed = new URL(url);
    const host = parsed.hostname.toLowerCase();
    if (host === "youtu.be" || matchesDomain(host, "youtube.com")) return "YouTube Video / Musik";
    if (matchesDomain(host, "soundcloud.com") || host === "snd.sc") return "SoundCloud Track";
    if (matchesDomain(host, "spotify.com") || host === "spotify.link") return "Spotify Link";
    if (matchesDomain(host, "music.apple.com")) return "Apple Music Link";
    return _("source_other");
  } catch {
    return _("source_other");
  }
}

async function fetchQueueTitle(url: string): Promise<string> {
  try {
    const info = await invoke<PlaylistInfo>("get_playlist_info", {
      url,
      cookiesPath: config.cookies_path || null,
    });
    const title = info.entries?.[0]?.title || info.title;
    if (title && title !== "Downloads") return title;
  } catch {
    // Fallback below keeps adding links responsive even when metadata lookup fails.
  }

  return fallbackQueueTitle(url);
}

function renderQueue() {
  if (!queueList) return;

  const visibleItems = getVisibleQueueItems();

  if (clipboardQueue.length === 0) {
    queueList.innerHTML = `
      <div class="empty-state">
         <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" opacity="0.3">
           <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
           <line x1="9" y1="9" x2="15" y2="9"/>
           <line x1="9" y1="13" x2="15" y2="13"/>
           <line x1="9" y1="17" x2="15" y2="17"/>
         </svg>
         <p id="queue-empty-state-text" data-i18n="queue_empty_state">${_("queue_empty_state")}</p>
      </div>
    `;
    
    queueDownloadAllBtn.disabled = true;
    queueClearAllBtn.disabled = true;
    if (queueRetryFailedBtn) queueRetryFailedBtn.disabled = true;
    if (queueBadge) {
      queueBadge.style.display = "none";
    }
    return;
  }

  queueDownloadAllBtn.disabled = false;
  queueClearAllBtn.disabled = false;
  if (queueRetryFailedBtn) queueRetryFailedBtn.disabled = !clipboardQueue.some(item => item.status === "error");
  
  if (queueBadge) {
    queueBadge.textContent = clipboardQueue.length.toString();
    queueBadge.style.display = "flex";
  }

  if (visibleItems.length === 0) {
    queueList.innerHTML = `<div class="empty-state"><p>${_("queue_no_results")}</p></div>`;
    return;
  }

  queueList.innerHTML = "";
  visibleItems.forEach((item, idx) => {
    const realIdx = clipboardQueue.findIndex(queueItem => queueItem.id === item.id);
    const card = document.createElement("div");
    card.className = `queue-card status-${item.status}`;
    card.style.animationDelay = `${Math.min(idx, 12) * 0.03}s`;

    let sourceClass: string = item.sourceType;
    let sourceLabel = _("source_other");
    let sourceIcon = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>`;
    
    if (item.sourceType === "youtube") {
      sourceClass = "youtube";
      sourceLabel = "YouTube";
      sourceIcon = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22.54 6.42a2.78 2.78 0 0 0-1.94-2C18.88 4 12 4 12 4s-6.88 0-8.6.46a2.78 2.78 0 0 0-1.94 2A29 29 0 0 0 1 11.75a29 29 0 0 0 .46 5.33A2.78 2.78 0 0 0 3.4 19c1.72.46 8.6.46 8.6.46s6.88 0 8.6-.46a2.78 2.78 0 0 0 1.94-2 29 29 0 0 0 .46-5.25a29 29 0 0 0-.46-5.33z"/><polygon points="9.75 15.02 15.5 11.75 9.75 8.48 9.75 15.02"/></svg>`;
    } else if (item.sourceType === "soundcloud") {
      sourceClass = "soundcloud";
      sourceLabel = "SoundCloud";
      sourceIcon = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M17.5 19H9a7 7 0 1 1 6.71-9h.79a4.5 4.5 0 1 1 1 8.89Z"/></svg>`;
    } else if (item.sourceType === "spotify") {
      sourceClass = "spotify";
      sourceLabel = "Spotify";
      sourceIcon = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M8 10c2.7-.8 5.3-.6 8 .8"/><path d="M8.7 13c2-.5 4-.4 6.1.6"/><path d="M9.4 15.8c1.4-.3 2.8-.2 4.3.4"/></svg>`;
    } else if (item.sourceType === "apple_music") {
      sourceClass = "apple-music";
      sourceLabel = "Apple Music";
      sourceIcon = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 18V5l10-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="16" cy="16" r="3"/></svg>`;
    }

    card.innerHTML = `
      <div class="queue-card-source ${sourceClass}">
         ${sourceIcon}
      </div>
       <div class="queue-card-info">
         <input type="text" class="queue-card-title-input" value="${escapeAttribute(item.title)}" placeholder="${escapeAttribute(_("queue_manual_input_placeholder"))}" />
         <div class="queue-card-meta">
            <span class="queue-source-badge ${sourceClass}">${sourceLabel}</span>
            <span class="queue-status-badge ${item.status}">${_("queue_status_" + item.status)}</span>
            <span>${new Date(item.addedAt).toLocaleString()}</span>
         </div>
         <span class="queue-card-url" title="${escapeAttribute(item.url)}">${escapeHtml(item.url)}</span>
      </div>
      <div class="queue-card-actions">
         <button class="queue-card-remove" title="${escapeAttribute(_("queue_remove_title"))}" data-idx="${realIdx}">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
         </button>
      </div>
    `;

    // Handle Title edits in real-time
    const titleInput = card.querySelector(".queue-card-title-input") as HTMLInputElement;
    titleInput.addEventListener("input", (e) => {
      item.title = (e.target as HTMLInputElement).value;
      saveQueue();
    });

    // Handle single item deletion
    card.querySelector(".queue-card-remove")?.addEventListener("click", () => {
      if (realIdx >= 0) clipboardQueue.splice(realIdx, 1);
      logKey("log_link_removed_queue", "info");
      saveQueue();
      renderQueue();
    });

    queueList.appendChild(card);
  });
}

function clearQueue() {
  clipboardQueue = [];
  logKey("log_queue_cleared", "info");
  saveQueue();
  renderQueue();
}

function startBatchDownload() {
  if (isDownloading) {
    logKey("log_auto_start_cancelled", "warning");
    return;
  }
  if (clipboardQueue.length === 0) return;
  if (!config.download_folder) {
    log(_("folder_placeholder") || "Bitte zuerst einen Zielordner wählen", "warning");
    return;
  }

  const itemsToDownload = clipboardQueue.filter(item => item.status === "queued" || item.status === "error");
  if (itemsToDownload.length === 0) return;
  logKey("log_batch_started", "info", { count: String(itemsToDownload.length) });
  itemsToDownload.forEach(item => item.status = "downloading");
  saveQueue();
  renderQueue();

  // Map to playlist entries – mit queueId für eindeutiges Status-Mapping (Duplikat-URLs!)
  playlistEntries = itemsToDownload.map((item, idx) => ({
    title: item.title,
    url: item.url,
    thumbnail: null,
    duration: null,
    index: idx + 1,
    queueId: item.id,
  }));

  // Set mode flag and visual settings
  isBatchQueueMode = true;
  urlInput.value = BATCH_PLACEHOLDER;
  updateDownloadBtnState();

  // Visually switch to the downloader tab
  const downloaderTabBtn = document.querySelector('.tab-btn[data-tab="downloader-tab"]') as HTMLElement;
  if (downloaderTabBtn) {
    downloaderTabBtn.click();
  }

  // Trigger download process!
  void startDownload();
}

function retryFailedQueueItems() {
  const failed = clipboardQueue.filter(item => item.status === "error");
  failed.forEach(item => item.status = "queued");
  if (failed.length > 0) {
    saveQueue();
    renderQueue();
    startBatchDownload();
  }
}

function checkPlaylistUrlInput() {
  const url = queueManualInput.value.trim();
  const textSpan = document.getElementById("queue-manual-add-btn-text");
  if (!textSpan) return;

  const isPlaylist = isPlaylistQueueUrl(url);

  if (isPlaylist) {
    queueManualAddBtn.classList.add("btn-import-active");
    textSpan.textContent = _("queue_import_playlist");
  } else {
    queueManualAddBtn.classList.remove("btn-import-active");
    textSpan.textContent = _("queue_manual_add");
  }
}

async function importPlaylist(url: string, source: "clipboard" | "extension" | "manual" = "manual") {
  const textSpan = document.getElementById("queue-manual-add-btn-text");
  if (!textSpan) return;
  const isManualImport = source === "manual" && queueManualInput.value.trim() === url;

  try {
    if (isManualImport) {
      queueManualInput.disabled = true;
      queueManualAddBtn.disabled = true;
      textSpan.innerHTML = `<span class="btn-spinner"></span>${_("queue_loading_playlist")}`;
    }
    logKey("log_importing_playlist", "info", { url });

    const playlistInfo = await invoke<PlaylistInfo>("get_playlist_info", {
      url,
      cookiesPath: config.cookies_path || null,
    });
    let count = 0;
    for (const entry of playlistInfo.entries) {
      if (!clipboardQueue.some(item => item.url === entry.url)) {
        clipboardQueue.push({
          id: createQueueId(),
          url: entry.url,
          title: entry.title,
          source,
          sourceType: detectQueueSourceType(entry.url),
          status: "queued",
          addedAt: Date.now() + count,
        });
        count++;
      }
    }
    renderQueue();
    saveQueue();
    logKey("log_playlist_imported", "success", {
      count: String(count),
      total: String(playlistInfo.total),
      title: playlistInfo.title,
    });
    if (isManualImport) queueManualInput.value = "";
  } catch (e) {
    logKey("log_playlist_import_failed", "error", { error: String(e) });
  } finally {
    if (isManualImport) {
      queueManualInput.disabled = false;
      queueManualAddBtn.disabled = false;
      checkPlaylistUrlInput();
    }
  }
}

// ─── Download Logic ──────────────────────────────────────────────────────────
async function startDownload() {
  if (isDownloading) return;
  const url = urlInput.value.trim();
  if (!url || !config.download_folder) {
    // Kein hängendes Batch-Flag bei Early-Return
    isBatchQueueMode = false;
    return;
  }
  const wasBatchMode = isBatchQueueMode;
  if (wasBatchMode) {
    lastFailedAction = () => startBatchDownload();
  } else {
    lastFailedAction = () => startDownload();
  }
  const runId = ++downloadRunId;

  setDownloadUiState(true);
  setDownloadPaused(false);
  completedTracks = 0;
  let succeededTracks = 0;
  let failedTracks = 0;
  trackProgressByIndex = new Map();
  convertingTracks = new Set();
  startTime = Date.now();
  
  if (!wasBatchMode) {
    playlistEntries = [];
    totalTracks = 0;
  } else {
    totalTracks = playlistEntries.length;
  }

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

    if (!wasBatchMode) {
      // Step 1: Get playlist info
      const info = await invoke<PlaylistInfo>("get_playlist_info", {
        url,
        cookiesPath: config.cookies_path || null,
      });

      if (!isDownloading || runId !== downloadRunId) return;

      totalTracks = info.total;
      playlistEntries = info.entries;
    }

    if (!isDownloading || runId !== downloadRunId) return;
    
    // Reset batch queue mode flag
    isBatchQueueMode = false;

    if (playlistEntries.length === 0) {
      log(`⚠️ ${_("no_tracks_found")}`, "warning");
      setStatus(_("no_tracks_found"), "warning");
      setDownloadUiState(false);
      await invoke("finish_download").catch(() => {});
      if (urlInput.value === BATCH_PLACEHOLDER) {
        urlInput.value = "";
        updateDownloadBtnState();
      }
      return;
    }

    log(_("tracks_found", { count: String(totalTracks) }), "success");
    $("progress-label").textContent = _("tracks_found", { count: String(totalTracks) });

    // Add track cards
    $("empty-state").style.display = "none";
    for (const entry of playlistEntries) {
      addTrackCard(entry);
    }

    // Step 2: Start download (Concurrent execution)
    const concurrencyLimit = 3; // Number of simultaneous downloads
    const executing = new Set<Promise<void>>();

    for (let i = 0; i < playlistEntries.length; i++) {
      if (!isDownloading || runId !== downloadRunId) break; // User cancelled
      
      const entry = playlistEntries[i];
      const trackNum = i + 1;
      const card = document.getElementById(`track-card-${trackNum}`);
      
      // Create the task promise and self-clean inside finally BEFORE settling
      let task!: Promise<void>;
      task = (async () => {
        if (!isDownloading || runId !== downloadRunId) return;
        if (card) {
          card.classList.add("active");
          if (config.auto_scroll_tracks !== false) scrollTrackIntoView(card);
        }

        try {
          await invoke("download_track", {
            request: {
              url: entry.url,
              format: formatSelect.value,
              folder: config.download_folder,
              cookies_path: config.cookies_path || null,
              quality: qualitySelect.value,
            },
            trackIndex: trackNum,
            totalTracks: totalTracks,
            trackTitle: entry.title
          });

          if (runId !== downloadRunId) return;
          succeededTracks++;

          if (card) {
            const bar = card.querySelector(".track-progress-bar") as HTMLElement;
            const stat = card.querySelector(".track-stat") as HTMLElement;
            if (bar) { bar.style.width = "100%"; bar.style.background = "var(--success)"; }
            if (stat) stat.textContent = "✅";
          }
          const queueItem = clipboardQueue.find(item =>
            (entry.queueId && item.id === entry.queueId) || (!entry.queueId && item.url === entry.url)
          );
          if (queueItem) {
            queueItem.status = "done";
            saveQueue();
            renderQueue();
          }
          recordHistory({ title: entry.title, url: entry.url, format: formatSelect.value, quality: qualitySelect.value, status: "done" });
        } catch (e) {
          if (!isDownloading || runId !== downloadRunId) return;
          failedTracks++;
          logErrorDetails(e, entry.title);
          recordHistory({ title: entry.title, url: entry.url, format: formatSelect.value, quality: qualitySelect.value, status: "error" });
          const queueItem = clipboardQueue.find(item =>
            (entry.queueId && item.id === entry.queueId) || (!entry.queueId && item.url === entry.url)
          );
          if (queueItem) {
            queueItem.status = "error";
            saveQueue();
            renderQueue();
          } else {
            clipboardQueue.push({
              id: createQueueId(),
              url: entry.url,
              title: entry.title,
              source: "manual",
              sourceType: detectQueueSourceType(entry.url),
              status: "error",
              addedAt: Date.now(),
            });
            saveQueue();
            renderQueue();
          }
          if (card) {
            const bar = card.querySelector(".track-progress-bar") as HTMLElement;
            const stat = card.querySelector(".track-stat") as HTMLElement;
            if (bar) bar.style.background = "var(--danger)";
            if (stat) stat.textContent = "❌";
          }
        } finally {
          executing.delete(task);
          convertingTracks.delete(trackNum);
          if (convertingTracks.size === 0) {
            convertProgress.classList.remove("active");
            convertProgress.style.width = "0%";
            $("convert-label").textContent = _("conversion_waiting");
          }
          if (card) card.classList.remove("active");
          if (isDownloading && runId === downloadRunId) {
            trackProgressByIndex.delete(trackNum);
            completedTracks++;
            updateTotalProgress();
          }
        }
      })();

      executing.add(task);

      // If we hit the limit, wait for at least one task to finish
      if (executing.size >= concurrencyLimit) {
        await Promise.race(executing);
      }
    }

    // Wait for all remaining tasks to finish
    await Promise.all(executing);

    if (isDownloading && runId === downloadRunId) {
      setDownloadUiState(false);
      await invoke("finish_download").catch(() => {});
      if (urlInput.value === BATCH_PLACEHOLDER) {
        urlInput.value = "";
        updateDownloadBtnState();
      }

      downloadProgress.classList.remove("active");
      convertProgress.classList.remove("active");
      totalProgress.classList.remove("active");
      $("convert-label").textContent = _("conversion_waiting");
      convertProgress.style.width = "0%";

      if (failedTracks === 0) {
        downloadProgress.style.width = "100%";
        totalProgress.style.width = "100%";
        setStatus(_("download_complete"), "success");
        $("progress-label").textContent = _("download_complete");
        log(`🎉 ${_("download_complete")} ${succeededTracks}/${totalTracks}`, "success");
        updateDiscordPresence(_("download_complete"), `${succeededTracks}/${totalTracks} Tracks`, true);
      } else {
        const partialMsg = _("download_complete_partial", {
          succeeded: String(succeededTracks),
          total: String(totalTracks),
          failed: String(failedTracks),
        });
        setStatus(partialMsg, succeededTracks > 0 ? "warning" : "error");
        $("progress-label").textContent = partialMsg;
        log(partialMsg, succeededTracks > 0 ? "warning" : "error");
        updateDiscordPresence(partialMsg, `${succeededTracks}/${totalTracks} Tracks`, true);
      }

      if (succeededTracks > 0 && config.after_download && config.after_download !== "nothing") {
        logKey("log_executing_action", "info", { action: config.after_download });
        await invoke("execute_after_download_action", { action: config.after_download });
      }
    }
  } catch (e) {
    if (runId !== downloadRunId) return;
    isBatchQueueMode = false;
    logErrorDetails(e);
    setStatus(_("error_occurred"), "error");
    setDownloadUiState(false);
    await invoke("finish_download").catch(() => {});
    if (urlInput.value === BATCH_PLACEHOLDER) {
      urlInput.value = "";
      updateDownloadBtnState();
    }
  }
}

function createQueueId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

function detectQueueSourceType(url: string): QueueItem["sourceType"] {
  try {
    const host = new URL(url).hostname.toLowerCase();
    if (host === "youtu.be" || matchesDomain(host, "youtube.com")) return "youtube";
    if (matchesDomain(host, "soundcloud.com") || host === "snd.sc") return "soundcloud";
    if (matchesDomain(host, "spotify.com") || host === "spotify.link") return "spotify";
    if (matchesDomain(host, "music.apple.com")) return "apple_music";
  } catch {}
  return "other";
}

function saveQueue() {
  try {
    localStorage.setItem(QUEUE_STORAGE_KEY, JSON.stringify(clipboardQueue));
  } catch (e) {
    console.warn("Could not persist queue:", e);
  }
  void syncRemoteQueue();
}

function loadQueue() {
  try {
    const raw = localStorage.getItem(QUEUE_STORAGE_KEY);
    if (!raw) return;
    const loaded = JSON.parse(raw) as Partial<QueueItem>[];
    let repaired = 0;
    clipboardQueue = loaded
      .filter(item => item.url && item.title)
      .map(item => {
        // Abgebrochene/neu gestartete Sessions: downloading → queued (kein ewiges Hängen)
        let status = item.status || "queued";
        if (status === "downloading") {
          status = "queued";
          repaired++;
        }
        return {
          id: item.id || createQueueId(),
          url: item.url!,
          title: item.title!,
          source: item.source || "manual",
          sourceType: item.sourceType || detectQueueSourceType(item.url!),
          status,
          addedAt: item.addedAt || Date.now(),
        };
      });
    if (repaired > 0) {
      try {
        localStorage.setItem(QUEUE_STORAGE_KEY, JSON.stringify(clipboardQueue));
      } catch { /* ignore */ }
    }
  } catch (e) {
    console.warn("Could not load queue:", e);
    clipboardQueue = [];
  }
}

async function syncRemoteQueue() {
  try {
    await invoke("update_remote_queue", {
      queue: clipboardQueue.map(item => ({
        id: item.id,
        title: item.title,
        url: item.url,
        source: item.sourceType,
        status: item.status,
        added_at: item.addedAt,
      })),
    });
  } catch {
    // Remote server may not be ready yet; local queue persistence is still enough.
  }
}

// ─── Download History (Verlauf) ──────────────────────────────────────────────
function saveHistory() {
  try {
    localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(downloadHistory));
  } catch (e) {
    console.warn("Could not persist history:", e);
  }
}

function loadHistory() {
  try {
    const raw = localStorage.getItem(HISTORY_STORAGE_KEY);
    if (!raw) return;
    const loaded = JSON.parse(raw) as Partial<HistoryEntry>[];
    downloadHistory = loaded
      .filter(item => item.url && item.title)
      .map((item): HistoryEntry => ({
        id: item.id || createQueueId(),
        url: item.url!,
        title: item.title!,
        format: item.format || "mp3",
        quality: item.quality || "best",
        status: item.status === "error" ? "error" : "done",
        finishedAt: item.finishedAt || Date.now(),
      }))
      .slice(0, HISTORY_MAX_ENTRIES);
  } catch (e) {
    console.warn("Could not load history:", e);
    downloadHistory = [];
  }
}

function recordHistory(entry: Omit<HistoryEntry, "id" | "finishedAt">) {
  downloadHistory.unshift({
    ...entry,
    id: createQueueId(),
    finishedAt: Date.now(),
  });
  if (downloadHistory.length > HISTORY_MAX_ENTRIES) {
    downloadHistory.length = HISTORY_MAX_ENTRIES;
  }
  saveHistory();
  renderHistory();
}

function clearHistory() {
  downloadHistory = [];
  saveHistory();
  renderHistory();
  logKey("log_history_cleared", "info");
}

function renderHistory() {
  if (!historyList) return;

  if (downloadHistory.length === 0) {
    historyList.innerHTML = `
      <div class="empty-state">
         <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" opacity="0.3">
           <polyline points="1 4 1 10 7 10"/><path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10"/>
         </svg>
         <p data-i18n="history_empty">${_("history_empty")}</p>
      </div>
    `;
    if (historyClearBtn) historyClearBtn.disabled = true;
    return;
  }

  if (historyClearBtn) historyClearBtn.disabled = false;

  historyList.innerHTML = "";
  downloadHistory.forEach((item) => {
    const card = document.createElement("div");
    card.className = `queue-card status-${item.status}`;
    const detectedSource = detectQueueSourceType(item.url);
    const sourceClass = detectedSource === "apple_music" ? "apple-music" : detectedSource;

    card.innerHTML = `
      <div class="queue-card-source ${sourceClass}">
         <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="1 4 1 10 7 10"/><path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10"/></svg>
      </div>
      <div class="queue-card-info">
         <span class="queue-card-url" title="${escapeAttribute(item.title)}" style="font-weight:600">${escapeHtml(item.title)}</span>
         <div class="queue-card-meta">
            <span class="queue-source-badge">${escapeHtml(item.format)} · ${escapeHtml(item.quality)}</span>
            <span class="queue-status-badge ${item.status}">${_("queue_status_" + item.status)}</span>
            <span>${new Date(item.finishedAt).toLocaleString()}</span>
         </div>
         <span class="queue-card-url" title="${escapeAttribute(item.url)}">${escapeHtml(item.url)}</span>
      </div>
      <div class="queue-card-actions">
         <button class="queue-card-redownload" title="${escapeAttribute(_("history_redownload"))}" data-id="${escapeAttribute(item.id)}">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
         </button>
         <button class="queue-card-remove" title="${escapeAttribute(_("history_remove"))}" data-id="${escapeAttribute(item.id)}">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
         </button>
      </div>
    `;

    card.querySelector(".queue-card-redownload")?.addEventListener("click", () => {
      urlInput.value = item.url;
      updateDownloadBtnState();
      document.querySelector<HTMLElement>('.tab-btn[data-tab="downloader-tab"]')?.click();
      logKey("log_url_accepted", "success", { url: item.url });
    });

    card.querySelector(".queue-card-remove")?.addEventListener("click", () => {
      downloadHistory = downloadHistory.filter((h) => h.id !== item.id);
      saveHistory();
      renderHistory();
    });

    historyList.appendChild(card);
  });
}

function getVisibleQueueItems(): QueueItem[] {

  const query = (queueSearchInput?.value || "").trim().toLowerCase();
  const sort = queueSortSelect?.value || "added_desc";
  let items = clipboardQueue.map((item, index) => ({ item, index }));
  if (query) {
    items = items.filter(({ item }) =>
      item.title.toLowerCase().includes(query) ||
      item.url.toLowerCase().includes(query) ||
      item.sourceType.toLowerCase().includes(query) ||
      item.status.toLowerCase().includes(query)
    );
  }
  items.sort((a, b) => {
    if (sort === "added_asc") return a.item.addedAt - b.item.addedAt;
    if (sort === "title_asc") return a.item.title.localeCompare(b.item.title);
    if (sort === "source_asc") return a.item.sourceType.localeCompare(b.item.sourceType);
    if (sort === "status_asc") return a.item.status.localeCompare(b.item.status);
    return b.item.addedAt - a.item.addedAt;
  });
  return items.map(({ item }) => item);
}

async function cancelDownload() {
  downloadRunId++;
  // Hängende Queue-Einträge zurücksetzen
  let queueRepaired = false;
  clipboardQueue.forEach(item => {
    if (item.status === "downloading") {
      item.status = "queued";
      queueRepaired = true;
    }
  });
  if (queueRepaired) {
    saveQueue();
    renderQueue();
  }
  isBatchQueueMode = false;
  
  // UI sofort zurücksetzen für besseres Feedback
  setDownloadUiState(false);
  setDownloadPaused(false);
  if (urlInput.value === BATCH_PLACEHOLDER) {
    urlInput.value = "";
    updateDownloadBtnState();
  }
  
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

async function toggleDownloadPause() {
  if (!isDownloading || !pauseResumeBtn) return;

  pauseResumeBtn.disabled = true;
  try {
    if (isDownloadPaused) {
      await invoke("resume_download");
      setDownloadPaused(false);
      setStatus(_("download_resumed"), "info");
      log("▶ " + _("download_resumed"), "info");
    } else {
      await invoke("pause_download");
      setDownloadPaused(true);
      setStatus(_("download_paused"), "warning");
      log("⏸ " + _("download_paused"), "warning");
    }
  } catch (e) {
    log(`⚠️ ${e}`, "warning");
  } finally {
    pauseResumeBtn.disabled = !isDownloading;
  }
}

// ─── Conversion ──────────────────────────────────────────────────────────────
async function startConversion() {
  const inputPath = ($("convert-file-input") as HTMLInputElement).value;
  const outputFormat = ($("convert-format-select") as HTMLSelectElement).value;
  const quality = ($("convert-quality-select") as HTMLSelectElement).value;
  const filesToConvert = selectedConversionFiles.length > 0 ? selectedConversionFiles : [inputPath].filter(Boolean);

  if (filesToConvert.length === 0) return;
  lastFailedAction = () => startConversion();

  isConvertingBatch = filesToConvert.length > 1;
  $("modal-progress-container").style.display = "block";
  ($("modal-convert-progress") as HTMLElement).style.width = "0%";
  $("convert-status-text").textContent = _("converting");
  $("convert-status-text").className = "status-text converting";
  (getEl("start-convert-btn") as HTMLButtonElement).disabled = true;

  let succeeded = 0;
  let lastError: unknown = null;
  try {
    for (let i = 0; i < filesToConvert.length; i++) {
      const file = filesToConvert[i];
      const base = file.split(/[\\/]/).pop() || file;
      ($("modal-convert-progress") as HTMLElement).style.width = "0%";
      $("convert-status-text").textContent = filesToConvert.length > 1
        ? `${_("converting")} (${i + 1}/${filesToConvert.length}) ${base}`
        : `${_("converting")} ${base}`;
      try {
        await invoke("convert_file", {
          request: { input_path: file, output_format: outputFormat, quality },
        });
        succeeded++;
      } catch (fileErr) {
        lastError = fileErr;
      }
    }

    if (isConvertingBatch) {
      if (succeeded === filesToConvert.length) {
        $("convert-status-text").textContent = _("conversion_summary", {
          succeeded: String(succeeded),
          total: String(filesToConvert.length),
        });
        $("convert-status-text").className = "status-text success";
      } else {
        $("convert-status-text").textContent = `${_("conversion_failed")} (${succeeded}/${filesToConvert.length})`;
        $("convert-status-text").className = "status-text error";
      }
    } else if (lastError) {
      $("convert-status-text").textContent = `❌ ${lastError}`;
      $("convert-status-text").className = "status-text error";
    }
  } finally {
    isConvertingBatch = false;
    $("modal-progress-container").style.display = "none";
    (getEl("start-convert-btn") as HTMLButtonElement).disabled = false;
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
      pot_provider_installed: boolean;
      pot_provider_status: string;
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
      $("update-ytdlp-btn").style.display = "inline-flex";
    } else {
      ytdlpIcon.textContent = "❌";
      ytdlpIcon.classList.remove("loading");
      ytdlpStatus.textContent = _("ytdlp_missing");
      $("install-ytdlp-btn").style.display = "block";
      $("update-ytdlp-btn").style.display = "none";
      isMissingDependencies = true;
    }

    if (result.ffmpeg_installed && result.ytdlp_installed) {
      log(_("all_systems_go"), "success");
    } else if (autoShowModal && isMissingDependencies) {
      // Automatically show the system modal if things are missing
      $("system-modal").style.display = "flex";
      log("⚠️ " + _("system_dependencies_missing"), "warning");
    }

    const potStatus = maybeElement("pot-provider-status");
    const potButton = maybeElement("install-pot-provider-btn") as HTMLButtonElement | null;
    if (potStatus) {
      potStatus.textContent = result.pot_provider_installed
        ? _("pot_provider_ready")
        : result.pot_provider_status || _("pot_provider_missing");
    }
    if (potButton) {
      potButton.style.display = result.pot_provider_installed ? "none" : "inline-flex";
    }
  } catch (e) {
    logKey("log_system_check_failed", "warning", { error: String(e) });
  }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────
async function confirmInstall(tool: string, packageId: string): Promise<boolean> {
  try {
    return await confirm(_("install_confirm", { tool, package: packageId }), {
      title: _("system_check"),
      kind: "info",
    });
  } catch {
    return false;
  }
}

function log(message: string, type: string = "info") {
  const line = document.createElement("div");
  line.className = `log-line ${type}`;
  const timestamp = new Date().toLocaleTimeString();
  const fullText = `[${timestamp}] ${message}`;
  line.textContent = fullText;
  const activeQuery = (maybeElement("log-search") as HTMLInputElement | null)?.value.toLowerCase() || "";
  if (activeQuery && !fullText.toLowerCase().includes(activeQuery)) {
    line.style.display = "none";
  }
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
  updateTrayStatus(text, true);
}

function updateDownloadBtnState() {
  const hasUrl = urlInput.value.trim().length > 0;
  const hasFolder = config.download_folder.length > 0;
  downloadBtn.disabled = !hasUrl || !hasFolder || isDownloading;
}

function setDownloadUiState(downloading: boolean) {
  isDownloading = downloading;
  cancelBtn.disabled = !downloading;
  pauseResumeBtn.disabled = !downloading;
  formatSelect.disabled = downloading;
  (getEl("convert-btn") as HTMLButtonElement).disabled = downloading;
  if (!downloading) setDownloadPaused(false);
  updateDownloadBtnState();
  updateTrayStatus(downloading ? _("analyzing_url") : _("ready"), true);
}

function setDownloadPaused(paused: boolean) {
  isDownloadPaused = paused;
  const text = maybeElement("pause-resume-btn-text");
  const pauseIcon = maybeElement("pause-resume-icon-pause");
  const playIcon = maybeElement("pause-resume-icon-play");
  if (text) text.textContent = paused ? _("resume_download") : _("pause_download");
  if (pauseIcon) pauseIcon.style.display = paused ? "none" : "";
  if (playIcon) playIcon.style.display = paused ? "" : "none";
  if (isDownloading) updateTrayStatus(paused ? _("download_paused") : _("downloading", { title: urlInput?.value || "…", current: completedTracks, total: totalTracks || "…" }), true);
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
  if (totalTracks <= 0) return 0;
  if (currentTrackIndex !== undefined && trackPercent !== undefined) {
    trackProgressByIndex.set(currentTrackIndex, Math.max(0, Math.min(100, trackPercent)));
  }
  
  // Real-time overall value: completed tracks plus partial progress from all active parallel downloads.
  const activeProgressValue = Array.from(trackProgressByIndex.values()).reduce(
    (sum, percent) => sum + (percent / 100),
    0,
  );
  const currentOverallValue = Math.min(totalTracks, completedTracks + activeProgressValue);

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
  return pct;
}

function scrollTrackIntoView(track: HTMLElement) {
  if (trackScrollPending) return;
  trackScrollPending = true;

  requestAnimationFrame(() => {
    const listRect = trackList.getBoundingClientRect();
    const trackRect = track.getBoundingClientRect();
    const padding = 12;

    if (trackRect.top < listRect.top + padding) {
      trackList.scrollTo({
        top: trackList.scrollTop - (listRect.top + padding - trackRect.top),
        behavior: "smooth",
      });
    } else if (trackRect.bottom > listRect.bottom - padding) {
      trackList.scrollTo({
        top: trackList.scrollTop + (trackRect.bottom - listRect.bottom + padding),
        behavior: "smooth",
      });
    }

    window.setTimeout(() => {
      trackScrollPending = false;
    }, 120);
  });
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
  card.style.animationDelay = `${Math.min(entry.index - 1, 12) * 0.03}s`;

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
  return div.innerHTML.replace(/"/g, "&quot;").replace(/'/g, "&#39;");
}

function escapeAttribute(text: string): string {
  return String(text)
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function applyColorScheme(theme: "dark" | "light") {
  document.documentElement.setAttribute("data-theme", theme);
  $("theme-icon-dark").style.display = theme === "dark" ? "block" : "none";
  $("theme-icon-light").style.display = theme === "light" ? "block" : "none";
}

function toggleTheme() {
  const isDark = document.documentElement.getAttribute("data-theme") === "dark";
  const nextTheme: "dark" | "light" = isDark ? "light" : "dark";
  applyColorScheme(nextTheme);
  try {
    localStorage.setItem(THEME_STORAGE_KEY, nextTheme);
  } catch {
    // ignore storage errors
  }
}

function applyTheme() {
  const root = document.documentElement;
  const rawAccent = (config.accent_color || "").trim();
  const accent = /^#[0-9a-fA-F]{6}$/.test(rawAccent) ? rawAccent : DEFAULT_ACCENT_COLOR;
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
    let bgUrl = config.custom_background;
    if (!bgUrl.startsWith("http") && !bgUrl.startsWith("asset:")) {
      bgUrl = convertFileSrc(bgUrl);
    }
    const escapedUrl = bgUrl.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    overlay.style.backgroundImage = `url("${escapedUrl}")`;
  } else if (overlay) {
    overlay.remove();
  }
}

function setRemoteStatusBadge(online: boolean) {
  const pill = maybeElement("remote-status-pill");
  const dot = maybeElement("remote-status-dot");
  const label = maybeElement("remote-status-label");
  if (!pill || !dot || !label) return;
  if (online) {
    pill.style.background = "rgba(0,225,186,0.1)";
    pill.style.borderColor = "rgba(0,225,186,0.2)";
    pill.style.color = "var(--primary)";
    dot.style.background = "var(--primary)";
    dot.style.boxShadow = "0 0 6px var(--primary)";
    label.textContent = _("remote_status_online");
  } else {
    pill.style.background = "rgba(255,94,87,0.12)";
    pill.style.borderColor = "rgba(255,94,87,0.3)";
    pill.style.color = "var(--danger)";
    dot.style.background = "var(--danger)";
    dot.style.boxShadow = "0 0 6px var(--danger)";
    label.textContent = _("remote_status_offline");
  }
}

async function updateRemoteStatus() {
  try {
    const ip = await invoke<string>("ss_get_local_ip");
    const remoteUrl = `http://${ip}:3030`;
    setRemoteStatusBadge(true);
    const qrPlaceholder = $("qr-placeholder");
    if (qrPlaceholder) {
      qrPlaceholder.innerHTML = `
        <div style="background: white; padding: 12px; border-radius: 18px; box-shadow: 0 10px 30px rgba(0,0,0,0.5); display: flex; flex-direction: column; align-items: center; gap: 10px; width: 100%;">
          <img id="remote-qr-img" src="https://api.qrserver.com/v1/create-qr-code/?size=120x120&data=${encodeURIComponent(remoteUrl)}" 
               alt="QR Code" style="width:120px;height:120px;display:block;" />
          <div id="remote-qr-fallback" style="display:none;font-size:10px;color:#555;text-align:center;padding:8px 4px;">${escapeHtml(_("qr_offline_fallback"))}</div>
          <div style="display: flex; align-items: center; gap: 8px; background: #f5f5f7; padding: 4px 8px; border-radius: 8px; border: 1px solid #eee; width: 100%; justify-content: center;">
            <code style="font-size:10px;color:#333;font-weight:700;font-family:monospace;white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">${escapeHtml(ip)}:3030</code>
            <button id="copy-remote-url" class="btn-icon-copy" title="${escapeAttribute(_("copy_url"))}">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#555" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
            </button>
          </div>
        </div>
      `;

      const qrImg = maybeElement("remote-qr-img") as HTMLImageElement | null;
      const qrFallback = maybeElement("remote-qr-fallback");
      if (qrImg && qrFallback) {
        qrImg.onerror = () => {
          qrImg.style.display = "none";
          qrFallback.style.display = "block";
        };
      }

      on("copy-remote-url", "click", async () => {
        try {
          await navigator.clipboard.writeText(remoteUrl);
          const btn = $("copy-remote-url");
          const originalSVG = btn.innerHTML;
          btn.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#22c55e" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>`;
          setTimeout(() => btn.innerHTML = originalSVG, 2000);
          logKey("log_link_copied", "success");
        } catch (e) {
          console.error("Clipboard error", e);
          logKey("log_copy_failed", "error");
        }
      });
    }
  } catch (e) {
    console.error("Failed to get local IP", e);
    setRemoteStatusBadge(false);
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
    logKey("log_config_save_failed", "warning", { error: String(e) });
  }
}

const saveConfig = debounce(saveConfigImmediate, 500);

// ─── Translations ────────────────────────────────────────────────────────────

async function loadTranslations(lang: string) {
  const nextTranslations = translations[lang] || translations.en;
  currentLang = translations[lang] ? lang : "en";
  t = nextTranslations;
  currentTranslations = nextTranslations;
  applyTranslations();
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

// ─── Team-Modus / P2P ────────────────────────────────────────────────────────

interface TeamMember {
  node_id: string;
  name: string;
  addr: string;
  last_seen: number;
  blocked: boolean;
  online: boolean;
}

interface TeamTask {
  id: string;
  url: string;
  title: string;
  origin: string;
  source: string;
  status: string;
  assigned_to: string;
  attempts: number;
  last_update: number;
  percent: number;
  error: string;
  file_name: string;
}

interface TeamState {
  active: boolean;
  role: "none" | "host" | "member";
  node_id: string;
  node_name: string;
  token: string;
  team_id: string;
  self_addr: string;
  host_addr: string;
  members: TeamMember[];
  tasks: TeamTask[];
  format: string;
  quality: string;
  folder: string;
  share_files: boolean;
  blocked_until: number;
  worker_running: boolean;
  last_error: string;
}

interface TeamInvite {
  link: string;
  token: string;
  addr: string;
  team_id: string;
  node_id: string;
}

let teamState: TeamState | null = null;
let teamPollTimer: number | null = null;

function teamEl<T extends HTMLElement = HTMLElement>(id: string): T | null {
  return document.getElementById(id) as T | null;
}

function teamShowError(msg: string) {
  const el = teamEl("team-error");
  if (el) el.textContent = msg;
}

async function initTeam() {
  const createBtn = teamEl<HTMLButtonElement>("team-create-btn");
  const joinBtn = teamEl<HTMLButtonElement>("team-join-btn");
  const joinInput = teamEl<HTMLInputElement>("team-join-input");
  const copyBtn = teamEl<HTMLButtonElement>("team-copy-link-btn");
  const addBtn = teamEl<HTMLButtonElement>("team-add-playlist-btn");
  const addInput = teamEl<HTMLInputElement>("team-add-playlist-input");
  const startBtn = teamEl<HTMLButtonElement>("team-start-btn");
  const stopBtn = teamEl<HTMLButtonElement>("team-stop-btn");
  const pullBtn = teamEl<HTMLButtonElement>("team-pull-btn");
  const leaveBtn = teamEl<HTMLButtonElement>("team-leave-btn");
  if (!createBtn) return; // UI nicht vorhanden

  const nameInput = () => teamEl<HTMLInputElement>("team-name-input")?.value.trim() || undefined;

  createBtn.addEventListener("click", async () => {
    try {
      teamShowError("");
      const invite = await invoke<TeamInvite>("team_create", {
        name: nameInput(),
        format: (document.getElementById("format-select") as HTMLSelectElement | null)?.value,
        quality: (document.getElementById("quality-select") as HTMLSelectElement | null)?.value,
        folder: config.download_folder || undefined,
        shareFiles: true,
        advertisedAddr: teamEl<HTMLInputElement>("team-advertised-input")?.value.trim() || undefined,
      });
      const linkBox = teamEl("team-link-box");
      const linkOut = teamEl<HTMLInputElement>("team-link-output");
      if (linkBox) linkBox.style.display = "flex";
      if (linkOut) linkOut.value = invite.link;
      await refreshTeam();
    } catch (e) {
      teamShowError(String(e));
    }
  });

  joinBtn?.addEventListener("click", async () => {
    const link = joinInput?.value.trim();
    if (!link) return;
    try {
      teamShowError("");
      await invoke<TeamState>("team_join", {
        link,
        name: nameInput(),
        format: (document.getElementById("format-select") as HTMLSelectElement | null)?.value,
        quality: (document.getElementById("quality-select") as HTMLSelectElement | null)?.value,
        folder: config.download_folder || undefined,
        shareFiles: true,
      });
      if (joinInput) joinInput.value = "";
      await refreshTeam();
    } catch (e) {
      teamShowError(String(e));
    }
  });

  copyBtn?.addEventListener("click", async () => {
    const linkOut = teamEl<HTMLInputElement>("team-link-output");
    if (!linkOut?.value) return;
    try {
      await navigator.clipboard.writeText(linkOut.value);
    } catch {
      linkOut.select();
      document.execCommand("copy");
    }
    const old = copyBtn.textContent;
    copyBtn.textContent = _("team_copied");
    setTimeout(() => { copyBtn.textContent = old; }, 1200);
  });

  addBtn?.addEventListener("click", async () => {
    const url = addInput?.value.trim();
    if (!url) return;
    try {
      teamShowError("");
      const n = await invoke<number>("team_add_playlist", {
        url,
        cookiesPath: config.cookies_path || null,
      });
      log(_("team_add_playlist") + `: +${n}`, "success");
      if (addInput) addInput.value = "";
      await refreshTeam();
    } catch (e) {
      teamShowError(String(e));
    }
  });

  startBtn?.addEventListener("click", async () => {
    try {
      teamShowError("");
      await invoke("team_start");
      await refreshTeam();
    } catch (e) {
      teamShowError(String(e));
    }
  });

  stopBtn?.addEventListener("click", async () => {
    try { await invoke("team_stop"); await refreshTeam(); } catch (e) { teamShowError(String(e)); }
  });

  pullBtn?.addEventListener("click", async () => {
    try {
      const n = await invoke<number>("team_pull_missing");
      log(_("team_pull_missing") + `: ${n}`, "success");
    } catch (e) { teamShowError(String(e)); }
  });

  leaveBtn?.addEventListener("click", async () => {
    try {
      await invoke("team_leave");
      const linkBox = teamEl("team-link-box");
      if (linkBox) linkBox.style.display = "none";
      teamState = null;
      await refreshTeam();
    } catch (e) { teamShowError(String(e)); }
  });

  // Selbstgehosteten Relay-Server verbinden
  const relayBtn = teamEl<HTMLButtonElement>("team-connect-relay-btn");
  const relayUrlEl = teamEl<HTMLInputElement>("team-relay-url");
  const relayTokenEl = teamEl<HTMLInputElement>("team-relay-token");
  // Default vom Backend
  try {
    const def = await invoke<string>("team_default_relay");
    if (relayUrlEl && def) relayUrlEl.value = def;
  } catch { /* ignore */ }
  relayBtn?.addEventListener("click", async () => {
    const token = relayTokenEl?.value.trim();
    if (!token) { teamShowError(_("team_relay_token_placeholder")); return; }
    try {
      teamShowError("");
      await invoke<TeamState>("team_connect_relay", {
        relayUrl: relayUrlEl?.value.trim() || undefined,
        token,
        name: nameInput(),
        format: (document.getElementById("format-select") as HTMLSelectElement | null)?.value,
        quality: (document.getElementById("quality-select") as HTMLSelectElement | null)?.value,
        folder: config.download_folder || undefined,
        shareFiles: true,
      });
      await refreshTeam();
    } catch (e) { teamShowError(String(e)); }
  });

  // Live-Updates aus dem Backend
  try {
    await listen<TeamState>("team-state-changed", (event) => {
      teamState = event.payload;
      renderTeam();
    });
    await listen<{ node_id: string; reason: string }>("team-blocked", (event) => {
      log(_("team_blocked"), "error");
      teamShowError(_("team_blocked") + (event.payload?.reason ? `: ${event.payload.reason}` : ""));
    });
  } catch { /* ignore */ }

  // Polling-Fallback
  await refreshTeam();
  if (teamPollTimer) clearInterval(teamPollTimer);
  teamPollTimer = window.setInterval(() => { void refreshTeam(); }, 1000);
}

async function refreshTeam() {
  try {
    teamState = await invoke<TeamState>("team_status");
  } catch { /* ignore */ }
  renderTeam();
}

function renderTeam() {
  const badge = teamEl("team-role-badge");
  const activeBox = teamEl("team-active-box");
  const membersEl = teamEl("team-members");
  const progressEl = teamEl("team-progress");
  const tasksEl = teamEl("team-tasks");
  const startBtn = teamEl<HTMLButtonElement>("team-start-btn");
  const stopBtn = teamEl<HTMLButtonElement>("team-stop-btn");
  if (!badge) return;

  const t = teamState;
  const active = !!t?.active;

  if (!active) {
    badge.textContent = _("team_off");
    badge.style.color = "var(--text-secondary)";
    if (activeBox) activeBox.style.display = "none";
    return;
  }

  const roleLabel = t!.role === "host" ? _("team_role_host") : _("team_role_member");
  badge.textContent = roleLabel;
  badge.style.color = "var(--primary)";
  if (activeBox) activeBox.style.display = "flex";

  // Mitglieder
  if (membersEl) {
    const memberName = (m: TeamMember) =>
      `${m.name || m.node_id}${m.node_id === t!.node_id ? " (du)" : ""}` +
      `${m.online ? "" : " · offline"}${m.blocked ? " · gesperrt" : ""}`;
    const list = (t!.members || []).map(m => memberName(m));
    membersEl.innerHTML =
      `<strong>${_("team_members")} (${list.length}):</strong> ` +
      (list.length ? list.map(escapeHtmlTeam).join(", ") : _("team_no_members"));
  }

  // Fortschritt
  const tasks = t!.tasks || [];
  const done = tasks.filter(x => x.status === "done").length;
  const failed = tasks.filter(x => x.status === "failed").length;
  const activeN = tasks.filter(x => x.status === "assigned" || x.status === "downloading").length;
  if (progressEl) {
    progressEl.textContent =
      _("team_progress", { done: String(done), total: String(tasks.length), active: String(activeN) }) +
      (failed ? ` · ${failed} ${_("team_task_failed")}` : "") +
      (t!.blocked_until && Date.now() < t!.blocked_until ? ` · ${_("team_blocked")}` : "");
  }

  if (startBtn) startBtn.style.display = t!.worker_running ? "none" : "";
  if (stopBtn) stopBtn.style.display = t!.worker_running ? "" : "none";

  // Task-Liste
  if (tasksEl) {
    const statusLabel = (s: string) => {
      switch (s) {
        case "done": return _("team_task_done");
        case "failed": return _("team_task_failed");
        case "downloading":
        case "assigned": return _("team_task_downloading");
        default: return _("team_task_queued");
      }
    };
    const icon = (s: string) =>
      s === "done" ? "✅" : s === "failed" ? "❌" : (s === "downloading" || s === "assigned") ? "⏳" : "•";
    tasksEl.innerHTML = tasks.slice(0, 200).map(task => {
      const who = task.assigned_to ? ` · ${escapeHtmlTeam(shortNode(t!, task.assigned_to))}` : "";
      const pct = task.status === "downloading" ? ` ${Math.round(task.percent)}%` : "";
      return `<div style="display:flex; justify-content:space-between; gap:8px; padding:4px 8px; background:var(--bg-hover); border-radius:6px;">
        <span style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">${icon(task.status)} ${escapeHtmlTeam(task.title)}</span>
        <span style="flex-shrink:0; color:var(--text-secondary);">${statusLabel(task.status)}${pct}${who}</span>
      </div>`;
    }).join("");
  }
}

function shortNode(t: TeamState, nodeId: string): string {
  const m = (t.members || []).find(x => x.node_id === nodeId);
  return m?.name || nodeId.slice(0, 6);
}

function escapeHtmlTeam(s: string): string {
  const map: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
  return s.replace(/[&<>"']/g, c => map[c] || c);
}

document.addEventListener("DOMContentLoaded", initTeam);
