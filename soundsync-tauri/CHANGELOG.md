----------------------------**V2.3.13**----------------------------

**New Features:**
- **Multi-Link Clipboard Detection:** Auto-URL detection now finds multiple supported links in the clipboard and can add all detected links to the queue at once.
- **Clickable Notifications:** Clicking a SoundSync desktop notification now restores, shows, and focuses the app window.
- **Remote Language Sync:** SoundSync Remote now follows the main app language and supports German, English, and Polish text in its mobile UI.

**Bug Fixes & UX Improvements:**
- **Notification Translation Fix:** Fixed Windows notifications showing raw translation keys instead of readable text.
- **Remote Player Layout Fix:** Improved the Remote player controls so volume and playback buttons no longer overlap on narrow screens.
- **Better Download Error Details:** Download and conversion failures now show a readable cause, a suggested fix, and the relevant raw details in the activity log.

**Internal:**
- **Remote Config Endpoint:** Added a `/remote-config` endpoint so the Remote UI can read the active language from the app config.
- **yt-dlp Error Detail Capture:** Download failures now include the last stderr lines from `yt-dlp` instead of only returning an exit code.
- **Translation Catalog Expansion:** Added new German, English, and Polish strings for notifications, Remote UI, and download error diagnosis.
- **Unified Version Bump:** Synchronized all package version tags to `2.3.13` across `package.json`, `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, and the visible app UI.

---

----------------------------**V2.3.12**----------------------------

**New Features:**
- **Multi-File Conversion Path Handling:** The conversion dialog now keeps the real selected file paths internally, even when the UI displays a localized “files selected” summary.

**Bug Fixes & UX Improvements:**
- **SoundCloud Set Track Names:** Improved playlist parsing so SoundCloud playlist/set entries keep useful track names instead of falling back to generic queue labels.
- **SoundCloud Set URLs:** Playlist parsing now prefers `webpage_url`/`original_url` before raw extractor URLs, improving queue imports and downloads from SoundCloud sets.
- **Conversion Start Fix:** Fixed conversion starting with the visible input label instead of the selected file path when multiple files were chosen.

**Internal:**
- **Conversion Translation Keys:** Added localized `files_selected` text for German, English, and Polish.
- **Unified Version Bump:** Synchronized all package version tags to `2.3.12` across `package.json`, `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, and the visible app UI.

---

-------------------**V2.3.12**----------------------------

----------------------------**V2.3.11**----------------------------

**New Features:**

- **Localized Activity Log Messages:** Most SoundSync-generated log entries now use the active app language instead of hardcoded German or English text.
- **Reusable Log Translation Helper:** Added a shared `logKey(...)` helper so new log messages can consistently use i18n keys and placeholder variables.

**Bug Fixes & UX Improvements:**

- **Language-Synced Queue Logs:** Queue actions like adding links, importing playlists, clearing the queue, and duplicate-link warnings now follow the selected language.
- **Language-Synced Settings Logs:** Settings, theme, background, folder, cookies, TV mode, and auto-detection messages now follow the selected language.
- **Language-Synced Update/System Logs:** Update checks, restart hints, system-check failures, copy errors, and config-save errors now use localized text.

**Internal:**

- **Translation Catalog Expansion:** Added the new log-message keys to the German and English translation files.
- **Unified Version Bump:** Synchronized all package version tags to `2.3.11` across `package.json`, `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, and the visible app UI.

-------------------------**V2.3.11**----------------------------

----------------------------**V2.3.10**----------------------------

**New Features:**

- **Stable Translated Navigation Labels:** Added dedicated IDs for the top navigation labels so `Downloader`, `Search`, `Trending`, and `Clipboard Queue` can be updated reliably by the runtime translation system.

**Bug Fixes & UX Improvements:**

- **Release Translation Fix:** Fixed a release-build issue where key UI labels stayed in German even when English or another language was selected.
- **Bundled Translation Loading:** Replaced runtime `/src/i18n/...` fetches with the already bundled JSON imports, making translations work consistently in both dev and built Tauri apps.
- **Single Language Change Flow:** Removed the duplicate language selector handler so language changes update config, local storage, imported translations, and UI text through one path.
- **Tab Indicator Refresh:** The active tab indicator now recalculates after language updates so it stays aligned with translated labels.

**Internal:**

- **Unified Version Bump:** Synchronized all package version tags to `2.3.10` across `package.json`, `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, and the visible app UI.

----------------------------**V2.3.10**----------------------------

----------------------------**V2.3.7**----------------------------

**New Features:**

- **Transparent Tool Installation:** FFmpeg and yt-dlp install actions now clearly explain that Windows Package Manager (`winget`) is used, show the exact package IDs, and ask for confirmation before starting.
- **Automatic PO-Token Provider Setup:** Added a one-click setup flow for `bgutil-ytdlp-pot-provider`, including plugin download, provider preparation, Node.js LTS installation when needed, and system status detection.
- **Admin-Aware Provider Installer:** The PO-Token Provider setup now requests Windows administrator rights through UAC and writes an installation log to `%TEMP%\soundsync-pot-provider-install.log`.

**Bug Fixes & UX Improvements:**

- **PowerShell UAC Fix:** Fixed the elevated PO-Token Provider installer by removing unsupported redirected output parameters from `Start-Process -Verb RunAs`.
- **System Check Upgrade:** The system check now reports whether the PO-Token Provider is ready or which setup pieces are missing.
- **Defender-Safe Transparency:** The setup flow does not disable or bypass Windows Defender; users are told when Windows may ask for explicit approval.
- **Unified Version Bump:** Synchronized all package version tags to `2.3.7` across the app metadata and UI.

----------------------------**V2.3.7**----------------------------

----------------------------**V2.3.6**----------------------------

**New Features:**

- **Sammelkorb (Clipboard Queue):** Introduced a brand new clipboard queue tab to collect multiple music links (YouTube, SoundCloud, etc.) and download them all in a single batch with one click.
- **Companion Browser-Extension:** Built and integrated a companion browser extension (`soundsync-extension`) that lets you send links directly from your web browser to the Downloader's Sammelkorb.
- **Apple Music Playlist Support:** Added native support for resolving and importing playlists and albums from Apple Music (`music.apple.com`).
- **Interactive Queue Editing & Manual Imports:** Allows editing track titles in real-time within the queue, plus a dynamic manual input field that imports entire Spotify/Apple Music playlists automatically when detected.

**Bug Fixes & UX Improvements:**

- **Unified Version Bump:** Synchronized all package version tags to `2.3.6` across the entire codebase (`tauri.conf.json`, `Cargo.toml`, `package.json`, `index.html`, and `CHANGELOG.md`).

----------------------------**V2.3.6**----------------------------

----------------------------**V2.3.5**----------------------------

**New Features:**

- **Auto-Updater on Startup:** Added an automated update check during application startup. If a newer version is available, it gracefully alerts the user with a clean localized dialog to trigger an automatic update, keeping the downloader seamlessly up-to-date.
- **Backend Modularization & Refactoring:** Restructured the massive Rust backend by splitting the single giant `lib.rs` file into cleanly separated logical modules (`commands.rs`, `models.rs`, `server.rs`, `spotify.rs`, `utils.rs`), improving maintainability and code readability without changing core behaviors.
- **Premium Overhauled Remote UI:** Redesigned the local network remote interface (`remote.html`) from the ground up, giving it a premium dark glassmorphism aesthetic with Outfit typography, modern gradient highlights, sliding navigation bars, and glowing status indicators.

**Bug Fixes & UX Improvements:**

- **Remote Icon Alignment Fix:** Resolved an issue where icons (like search magnifier and links) in the Remote interface were offset or out of alignment due to Lucide dynamically rendering SVG tags.
- **Unified Version Bump:** Synchronized all package version tags to `2.3.5` across the entire codebase (`tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, `package.json`, `index.html`, and `CHANGELOG.md`).

----------------------------**V2.3.5**----------------------------

----------------------------**V2.3.4**----------------------------

**New Features:**

- **Playlist Auto-Scroll Control:** Added a dedicated toggle in Settings to turn the playlist auto-scrolling on or off independently from the log.
- **Multilingual Settings:** Translated the new playlist auto-scroll settings fully into German, English, and Polish.

**Bug Fixes & UX Improvements:**

- **Instant Settings Saving:** Saving settings now bypasses the debounce delay, instantly writing the configuration to disk before the window closes.
- **Settings State Synchronization:** Fixed a bug where settings checkboxes (like AI-Tagging) were reset to HTML defaults when opening the settings modal.
- **Live Settings Application:** Toggles like Eco-Mode and Discord Rich Presence are now applied live in real-time without requiring an application restart.
- **Header Version Fix:** Corrected the header version badge ID so the application's actual dynamic version number is displayed correctly.
- **Settings Translation Fix:** Wrapped the settings menu version number in a separate container, protecting it from being deleted when changing languages.
- **UI Simplification:** Removed the redundant "Auto-URL Detection" checkbox from the settings panel as it is already fully controllable from the main screen.

---

-------------------------**V2.3.4**----------------------------

----------------------------**V2.3.3**----------------------------

**New Features:**

- **TV & Controller Mode:** Added a brand new 10-foot TV Mode that enlarges the UI for big screens and enables full keyboard/controller arrow navigation.
- **P2P Mesh Network:** The background server now acts as a local file-sharing hub, exposing downloaded files securely on your local network.
- **Full Localization:** Implemented a completely dynamic, 100% translated UI supporting English, German, and Polish across all menus and tooltips.

**Bug Fixes & UX Improvements:**

- **Tooltip Fix:** Resolved an issue where custom hover tooltips were transparent and overlapped with text behind them.
- **UI Consistency:** Enhanced info badges and hover effects for better usability and a more polished glassmorphism design.

----------------------------**V2.3.3**----------------------------

----------------------------**V2.3.1**----------------------------

**Bug Fixes & UX Improvements:**

- **Filename Fix:** Converted standalone files no longer have a mandatory `_converted` suffix unless necessary.
- **Filename Fix:** YouTube downloads now use safe Windows filenames to prevent issues with special characters.
- **UI Fix:** Track cards now have a strictly enforced height, preventing text and thumbnail overlapping when many items are present.

----------------------------**V2.3.0**----------------------------

**Eco-Mode & Performance Optimization**

**New Features:**

- **🌿 Eco-Mode (Standby):** Reduces CPU/GPU usage when the app is in the background or minimized by pausing animations and clipboard polling.
- **📜 Auto-Scroll Toggle:** Control whether the log and track list should automatically scroll to the latest entry.
- **📁 YouTube Cookies Labeling:** Clarified `cookies.txt` usage specifically for YouTube authentication with improved file filtering.

**UI/UX Improvements:**

- **💡 Light Mode Overhaul:** Complete redesign of the light theme with high-contrast elements, proper shadows, and readable text.
- **🌙 Dark Mode Overhaul:** Fully consistent glassmorphism with deeper contrast and refined element visibility.
- **📏 Fixed Track Sizing:** Track cards now maintain a consistent size even with 100+ items (no more shrinking).
- **🔄 Sidebar Auto-Scroll:** The download list now automatically follows the currently active track.

**Performance & Internal:**

- **🚀 Terminal Suppression:** The console window no longer flashes or stays open on startup.
- **⚡ Optimized Polling:** Clipboard watcher now respects the app state and stops when unnecessary.
- **🛠️ Config Persistence:** New settings (Eco-Mode, Auto-Scroll) are correctly saved and loaded.

----------------------------**V2.2.2**----------------------------

**Glassmorphism & Auto-Update Release**

**New Features:**

- **Auto-Updater:** Click "Auf Updates prüfen" → downloads and installs the update automatically (.msi/.exe). No more manual browser downloads.
- **Custom Background Image:** Choose your own wallpaper in Settings → it now persists across app restarts.
- **iOS-Style Toggle Switches:** New toggle design with I/O symbols for clear on/off indication.
- **Dark Mode Dropdowns:** All select menus now match the dark glass theme — no more white backgrounds.

**UI/UX Improvements:**

- **Glassmorphism Overhaul:** Frosted glass effects, subtle reflections, and refined backdrop-blur throughout the entire UI.
- **Dynamic Accent Colors:** The accent color picker now updates all related glow, hover, and shadow colors in real time.
- **Smoother Animations:** Cards, sidebar, and modals use spring-based cubic-bezier curves for a premium feel.
- **Custom SVG Dropdown Arrows:** Replaced native browser arrows with sleek, white SVG chevrons.
- **Version Badge Glow:** The version badge now pulses with a subtle glow animation.

**Performance:**

- **Debounced Config Saving:** Settings are now saved with a 500ms debounce, preventing IPC spam.
- **RAF-Based Log Scrolling:** Log output uses `requestAnimationFrame` to avoid layout thrashing during downloads.
- **Optimized Track Highlighting:** Active track cards are now highlighted via direct ID lookup instead of iterating all cards.
- **Reduced Clipboard Polling:** Interval increased from 1.5s to 2.5s, reducing CPU overhead by ~40%.
- **DOM Cleanup:** Log limit reduced from 500 to 300 nodes for smoother rendering.

**Bug Fixes:**

- Fix: Background image no longer disappears after restarting the app.
- Fix: Removed debug `alert()` that blocked the UI when receiving remote URLs.
- Fix: Toggle switches no longer show duplicate white circles in settings.
- Fix: Removed unused `relaunch` import and dead `fetchSpotifyMetadata` code.

----------------------------**V2.2.1**----------------------------

**New Features & Performance Update**

**New Features:**

- **Batch Conversion:** Multi-select files in the conversion dialog for faster processing.
- **Global Keyboard Shortcuts:**
  - `Ctrl + Enter`: Start Download.
  - `Ctrl + L`: Clear Logs.
  - `Ctrl + ,`: Open Settings.
  - `Esc`: Close Modals.

**Improvements:**

- **Performance:** Enabled Hardware Acceleration (`-hwaccel auto`) and Multithreading (`-threads 0`) for FFmpeg/yt-dlp for significantly faster processing.
- **UI Animations:** Added sleek entrance animations, progress bar pulsing, and smoother transitions.
- **Immediate Cancellation:** Improved the responsiveness of the "Cancel" button to ensure UI state updates instantly.
- **Robustness:** Fixed concurrency bugs causing system overload on large playlists.
- **Permission Fix:** Resolved folder access issues on Windows by updating capability scopes.

**Bug Fixes:**

- Fix: UI no longer gets stuck in "converting" state when files are already in the target format.
- Fix: Addressed potential TypeScript build errors related to task management.

----------------------------**V2.2.1**----------------------------
