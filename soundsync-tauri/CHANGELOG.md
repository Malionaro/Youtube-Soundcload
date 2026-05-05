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
