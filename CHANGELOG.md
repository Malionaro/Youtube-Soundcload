----------------------------**V2.0.1**----------------------------

**Patch Update: Tray, Clipboard & Stability Fixes**

**New features:**
- **Tray Context Menu:** Right-clicking the tray icon now opens a menu with `UI öffnen` and `Beenden`.
- **Clipboard URL Prompt:** YouTube and SoundCloud links copied to the clipboard now trigger a bottom-right in-app prompt asking whether the URL should be übernommen.
- **Auto-Erkennung Toggle:** Clipboard URL detection can now be turned on and off directly in the UI.

**Improvements:**
- **Cleaner Shutdown:** The WebView is destroyed before quitting, reducing Chromium/WebView2 shutdown warnings.
- **Hidden Dependency Installers:** FFmpeg and yt-dlp installation now run without showing a terminal window on Windows.
- **More Reliable Startup:** Optional UI elements no longer break the whole interface if they are missing.

**Bug fixes:**
- Fixed buttons becoming unresponsive because missing drag-and-drop elements caused JavaScript startup errors.
- Fixed the cancel button so running yt-dlp processes are actually terminated.
- Fixed cancellation state being reused by later downloads.
- Fixed existing config files disabling the new auto URL detection by default.
- Fixed tray right-click behavior so the app no longer exits immediately without a menu.

----------------------------**V2.0.1**----------------------------

---

----------------------------**V2.0.0**----------------------------

**Major Update: The Zero-Config & High-Speed Evolution**

**New features:**
- **Zero-Python Architecture:** No manual Python installation required anymore! The app is now a fully compiled, standalone executable.
- **Parallel Downloads:** Download up to 3 tracks simultaneously for maximum speed.
- **System Tray Integration:** App stays active in the background. Minimize to tray and manage it via the new right-click context menu.
- **Auto-Dependency Setup:** Missing tools like FFmpeg or yt-dlp are now automatically detected and installed via winget on the first start.
- **Smart Clipboard Detection:** Automatic detection and insertion of URLs directly from your clipboard.

**Improvements:**
- **Performance Boost:** Massive reduction in CPU and RAM usage thanks to the Rust-based Tauri 2.0 backend.
- **Enhanced UI Proportions:** Polished Glassmorphism design with perfectly aligned buttons and input fields.
- **Improved Stability:** Robust error handling for network interruptions and conversion processes.
- **Optimized Icon Management:** Fixed Windows icon caching issues with a completely new binary resource system.

**Bug fixes:**
- Fixed UI scaling and alignment issues on high-DPI displays.
- Resolved "Failed to unregister class" warnings during app initialization.
- Fixed several memory leaks during long playlist extractions.

----------------------------**V2.0.0**----------------------------

---

## Antivirus Notice

Some antivirus software may flag this program during updates or execution.
This is often a false positive caused by how compiled executables or auto-updating features behave.

> This program does not contain any malicious code.

To avoid issues:
- Temporarily disable your antivirus during the update,
- or add this program as an exception in your antivirus settings.

Always download the program from this official GitHub repository only.
