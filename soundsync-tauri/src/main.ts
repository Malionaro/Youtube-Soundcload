import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

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
};

let isDownloading = false;
let totalTracks = 0;
let completedTracks = 0;
let startTime = 0;
let playlistEntries: PlaylistEntry[] = [];
let clipboardWatchTimer: number | null = null;
let lastDetectedUrl = "";
let pendingDetectedUrl = "";

// ─── DOM Elements ────────────────────────────────────────────────────────────
const $ = (id: string) => document.getElementById(id)!;
const urlInput = $("url-input") as HTMLInputElement;
const folderInput = $("folder-input") as HTMLInputElement;
const cookiesInput = $("cookies-input") as HTMLInputElement;
const formatSelect = $("format-select") as HTMLSelectElement;
const downloadBtn = $("download-btn") as HTMLButtonElement;
const cancelBtn = $("cancel-btn") as HTMLButtonElement;
const logOutput = $("log-output");
const trackList = $("track-list");
const downloadProgress = $("download-progress") as HTMLElement;
const convertProgress = $("convert-progress") as HTMLElement;
const totalProgress = $("total-progress") as HTMLElement;
const maybeElement = (id: string) => document.getElementById(id);

// ─── Init ────────────────────────────────────────────────────────────────────
async function init() {
  try {
    config = await invoke<AppConfig>("load_config");
    if (config.download_folder) folderInput.value = config.download_folder;
    if (config.cookies_path) cookiesInput.value = config.cookies_path;
    config.auto_url_detection = config.auto_url_detection !== false;
    if (config.language && translations[config.language]) {
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
  if (config.auto_url_detection) startClipboardWatcher();
  checkSystem();
  log("🎵 SoundSync Downloader v2.0.1 ready", "success");
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
  $("footer-version").textContent = _("version", { version: "2.0.1" });
  $("no-downloads-text").textContent = _("no_downloads_yet");
  const dragDropText = maybeElement("drag-drop-text");
  if (dragDropText) dragDropText.textContent = _("drag_drop_hint");
  $("log-search").setAttribute("placeholder", _("search_placeholder"));
  ($("auto-url-toggle") as HTMLInputElement).checked = config.auto_url_detection;
}

// ─── Event Listeners ─────────────────────────────────────────────────────────
function setupEventListeners() {
  // URL input → enable/disable download button
  urlInput.addEventListener("input", updateDownloadBtnState);
  $("auto-url-toggle").addEventListener("change", (e) => {
    config.auto_url_detection = (e.target as HTMLInputElement).checked;
    saveConfig();
    if (config.auto_url_detection) {
      startClipboardWatcher();
      log("Automatische URL-Erkennung aktiviert", "success");
    } else {
      stopClipboardWatcher();
      hideDetectedUrlPrompt();
      log("Automatische URL-Erkennung deaktiviert", "warning");
    }
  });
  $("detected-url-use").addEventListener("click", useDetectedUrl);
  $("detected-url-dismiss").addEventListener("click", hideDetectedUrlPrompt);
  $("clear-url-btn").addEventListener("click", () => {
    urlInput.value = "";
    updateDownloadBtnState();
    log("🧹 URL cleared", "info");
  });

  // Browse folder
  $("browse-btn").addEventListener("click", async () => {
    const folder = await open({ directory: true, title: _("folder_label") });
    if (folder) {
      config.download_folder = folder as string;
      folderInput.value = config.download_folder;
      saveConfig();
      log(`✅ Folder set: ${config.download_folder}`, "success");
      updateDownloadBtnState();
    }
  });

  // Open folder
  $("open-folder-btn").addEventListener("click", () => {
    if (config.download_folder) {
      invoke("open_folder", { path: config.download_folder });
    }
  });

  // Browse cookies
  $("cookies-btn").addEventListener("click", async () => {
    const file = await open({
      title: _("cookies_label"),
      filters: [{ name: "Text", extensions: ["txt"] }],
    });
    if (file) {
      config.cookies_path = file as string;
      cookiesInput.value = config.cookies_path;
      saveConfig();
      log(`🍪 Cookies: ${config.cookies_path}`, "info");
    }
  });

  // Download
  downloadBtn.addEventListener("click", startDownload);
  cancelBtn.addEventListener("click", cancelDownload);

  // Clear log
  $("clear-log-btn").addEventListener("click", () => {
    logOutput.innerHTML = "";
    log("🧹 Log cleared", "info");
  });

  // Log search
  ($("log-search") as HTMLInputElement).addEventListener("input", (e) => {
    const query = (e.target as HTMLInputElement).value.toLowerCase();
    logOutput.querySelectorAll(".log-line").forEach((el) => {
      (el as HTMLElement).style.display =
        el.textContent?.toLowerCase().includes(query) ? "" : "none";
    });
  });

  // Theme toggle
  $("theme-toggle").addEventListener("click", toggleTheme);

  // Language
  $("language-select").addEventListener("change", (e) => {
    const lang = (e.target as HTMLSelectElement).value;
    if (translations[lang]) {
      currentLang = lang;
      t = translations[lang];
      config.language = lang;
      saveConfig();
      updateUI();
      log(`🌐 Language: ${lang}`, "info");
    }
  });

  // Convert modal
  $("convert-btn").addEventListener("click", () => {
    $("convert-modal").style.display = "flex";
  });
  $("convert-modal-close").addEventListener("click", () => {
    $("convert-modal").style.display = "none";
  });
  $("convert-modal").addEventListener("click", (e) => {
    if (e.target === $("convert-modal")) $("convert-modal").style.display = "none";
  });

  // Convert browse
  $("convert-browse-btn").addEventListener("click", async () => {
    const file = await open({ title: _("select_file") });
    if (file) {
      ($("convert-file-input") as HTMLInputElement).value = file as string;
      $("convert-status-text").textContent = _("ready_to_convert");
      $("convert-status-text").className = "status-text success";
    }
  });

  // Start conversion
  $("start-convert-btn").addEventListener("click", startConversion);

  // System check modal
  $("system-check-btn").addEventListener("click", () => {
    $("system-modal").style.display = "flex";
    checkSystem();
  });
  $("system-modal-close").addEventListener("click", () => {
    $("system-modal").style.display = "none";
  });
  $("system-modal").addEventListener("click", (e) => {
    if (e.target === $("system-modal")) $("system-modal").style.display = "none";
  });

  // Install buttons
  $("install-ffmpeg-btn").addEventListener("click", async () => {
    log("🔧 Installing FFmpeg...", "info");
    try {
      await invoke("install_ffmpeg");
      log(_("ffmpeg_installed"), "success");
      checkSystem();
    } catch (e) {
      log(`❌ ${e}`, "error");
    }
  });

  $("install-ytdlp-btn").addEventListener("click", async () => {
    log("🔧 Installing yt-dlp...", "info");
    try {
      await invoke("install_ytdlp");
      log(_("ytdlp_installed"), "success");
      checkSystem();
    } catch (e) {
      log(`❌ ${e}`, "error");
    }
  });

  // Scroll to current
  $("scroll-to-current-btn").addEventListener("click", () => {
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
    if (e.ctrlKey && e.key === "Enter" && !downloadBtn.disabled) startDownload();
    if (e.key === "Escape") {
      $("convert-modal").style.display = "none";
      $("system-modal").style.display = "none";
    }
  });
}

// ─── Tauri Event Listeners ───────────────────────────────────────────────────
function setupTauriListeners() {
  listen<DownloadProgress>("download-progress", (event) => {
    const p = event.payload;
    
    // Global progress (shows the last active)
    if (p.status === "downloading") {
      downloadProgress.style.width = `${p.percent}%`;
      $("progress-percent").textContent = `${p.percent.toFixed(1)}%`;
      $("progress-label").textContent = _("progress_downloading", {
        percent: p.percent.toFixed(0),
        speed: p.speed || "...",
        eta: p.eta || "...",
      });
    } else if (p.status === "converting") {
      convertProgress.style.width = "100%";
      $("convert-label").textContent = `🔄 ${p.title}`;
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

    downloadProgress.style.width = "100%";
    totalProgress.style.width = "100%";
    setStatus(_("download_complete"), "success");
    $("progress-label").textContent = _("download_complete");
    log(`🎉 ${_("download_complete")} ${completedTracks}/${totalTracks}`, "success");
  });

  listen<string>("download-error", (event) => {
    isDownloading = false;
    downloadBtn.disabled = false;
    cancelBtn.disabled = true;
    formatSelect.disabled = false;
    setStatus(_("error_occurred"), "error");
    log(`❌ ${event.payload}`, "error");
  });

  // Conversion events
  listen<{ status: string; percent: number; filename: string }>("convert-progress", (event) => {
    const p = event.payload;
    const bar = $("modal-convert-progress") as HTMLElement;
    bar.style.width = `${p.percent}%`;
    $("convert-status-text").textContent = `${_("converting")} ${p.percent.toFixed(0)}%`;
    $("convert-status-text").className = "status-text converting";
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
}

// ─── Automatic URL Detection ────────────────────────────────────────────────
function startClipboardWatcher() {
  if (clipboardWatchTimer !== null) return;
  clipboardWatchTimer = window.setInterval(checkClipboardForMediaUrl, 1500);
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
      host.endsWith("soundcloud.com")
    ) {
      return url;
    }
  } catch {
    return null;
  }

  return null;
}

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
  const url = urlInput.value.trim();
  if (!url || !config.download_folder) return;

  isDownloading = true;
  downloadBtn.disabled = true;
  cancelBtn.disabled = false;
  formatSelect.disabled = true;
  completedTracks = 0;
  totalTracks = 0;
  startTime = Date.now();
  playlistEntries = [];

  // Clear track list (keep empty state)
  trackList.querySelectorAll(".track-card").forEach(el => el.remove());
  if ($("empty-state")) $("empty-state").style.display = "flex";

  resetProgress();
  setStatus(_("analyzing_url"), "info");
  log(_("analyzing_url"), "info");

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
          if (!isDownloading) {
            if (card) card.classList.remove("active");
            return;
          }
          log(`❌ ${entry.title}: ${e}`, "error");
          if (card) {
            const bar = card.querySelector(".track-progress-bar") as HTMLElement;
            const stat = card.querySelector(".track-stat") as HTMLElement;
            if (bar) bar.style.background = "var(--error)";
            if (stat) stat.textContent = "❌";
          }
        }

        if (card) card.classList.remove("active");
        if (!isDownloading) return;
        completedTracks++;
        updateTotalProgress();
      })();

      executing.add(task);
      task.then(() => executing.delete(task));

      if (executing.size >= concurrencyLimit) {
        await Promise.race(executing);
      }
    }

    // Wait for remaining tasks to finish
    await Promise.all(executing);

    if (isDownloading) {
      isDownloading = false;
      downloadBtn.disabled = false;
      cancelBtn.disabled = true;
      formatSelect.disabled = false;

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
  }
}

async function cancelDownload() {
  try {
    await invoke("cancel_download");
    isDownloading = false;
    downloadBtn.disabled = false;
    cancelBtn.disabled = true;
    formatSelect.disabled = false;
    setStatus(_("download_cancelled"), "error");
    log("🛑 " + _("download_cancelled"), "warning");
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
      
      // Auto trigger installation
      if (!result.ffmpeg_installed) {
        $("install-ffmpeg-btn").click();
      }
      if (!result.ytdlp_installed) {
        $("install-ytdlp-btn").click();
      }
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
  logOutput.scrollTop = logOutput.scrollHeight;
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

function updateTotalProgress() {
  if (totalTracks <= 0) return;
  const pct = (completedTracks / totalTracks) * 100;
  totalProgress.style.width = `${pct}%`;
  $("total-percent").textContent = `${pct.toFixed(0)}%`;

  const elapsed = (Date.now() - startTime) / 1000;
  const avgPerTrack = completedTracks > 0 ? elapsed / completedTracks : 0;
  const remaining = (totalTracks - completedTracks) * avgPerTrack;
  const eta = formatTime(remaining);

  $("total-progress-label").textContent = _("total_progress_detail", {
    percent: pct.toFixed(0),
    completed: String(completedTracks),
    total: String(totalTracks),
    eta,
  });

  // Update track cards
  trackList.querySelectorAll(".track-card").forEach((card, idx) => {
    card.classList.toggle("active", idx === completedTracks);
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

async function saveConfig() {
  try {
    await invoke("save_config", { config });
  } catch (e) {
    console.error("Failed to save config:", e);
  }
}

// ─── Boot ────────────────────────────────────────────────────────────────────
document.addEventListener("DOMContentLoaded", init);
