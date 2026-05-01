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
