# Changelog - SoundSync Downloader

Alle wichtigen Änderungen an diesem Projekt werden in dieser Datei festgehalten.

----------------------------**V2.2.0**----------------------------

**Stability, Progress & ETA Update**

**Improvements:**
- **Rock-Solid Cancellation:** Fixed issues where UI would continue updating after a download was cancelled.
- **Real-Time Total Progress:** The main progress bar now smoothly includes the percentage of the current track, providing a more accurate overview.
- **Improved ETA Calculation:** Optimized the "Time Remaining" logic for playlists to prevent jumping and improve accuracy.
- **UI State Protection:** Implemented strict guards to ensure the UI stays synchronized with the actual download state.
- **Code Optimization:** Removed redundant logic and cleaned up event listeners for better performance.

**Bug fixes:**
- Fix: Progress bar no longer gets stuck at 0% or jumps unexpectedly.
- Fix: "Cancel" button now immediately hides progress overlays and stops all background processing.

----------------------------**V2.2.0**----------------------------

----------------------------**V2.1.0**----------------------------

**Major Update: Remote Control, UI Overhaul & Dynamic Versioning**

**New features:**
- **Remote-Control (Web-Interface):** Sende YouTube/SoundCloud Links direkt von deinem Handy an den PC.
- **QR-Code Integration:** Scanne den Code in den Einstellungen, um sofort loszulegen.
- **Copy-to-Clipboard (Remote):** Ein neuer Button zum Kopieren der Remote-URL mit visuellem Feedback (✅).
- **Intelligente Update-Prüfung:** Die App nutzt nun einen Semver-Check, um Updates präziser zu erkennen.

**Improvements (UI/UX):**
- **Premium Glassmorphism:** Komplett neues Design der Einstellungen mit modernen Karten-Layouts und Unschärfe-Effekten.
- **Unified Buttons:** Auswahl-Buttons (Durchsuchen/Auswählen) sind jetzt mit Icons kombiniert und optisch harmonisiert.
- **Improved "Open Folder" Button:** Der kleine Ordner-Button wurde mit Glassmorphism-Design und Hover-Animationen aufgewertet.
- **Dynamic Versioning:** Die Versionsnummer passt sich nun überall (Header, Footer, Logs, Update-Info) automatisch an.
- **Native Opener:** Umstieg auf das offizielle Tauri-Opener-Plugin für sichereres Öffnen von Links und Ordnern.

**Bug fixes:**
- Fix: SVG-Icons in Buttons werden bei Sprachumstellungen nicht mehr durch Text überschrieben.
- Fix: QR-Code Container Layout-Fehler (Textüberlappung) behoben.
- Fix: Die "Clear URL" (X) Taste wurde fest und sauber im Eingabefeld positioniert.

----------------------------**V2.1.0**----------------------------

----------------------------**V2.0.1**----------------------------

**Patch Update: Tray, Clipboard & Stability Fixes**

**New features:**
- **Tray Context Menu:** Rechtsklick auf das Tray-Icon öffnet nun ein Menü mit `UI öffnen` und `Beenden`.
- **Clipboard URL Prompt:** YouTube/SoundCloud Links im Clipboard lösen einen dezenten In-App Prompt aus.
- **Auto-Erkennung Toggle:** Die Clipboard-Überwachung kann jetzt in der UI deaktiviert werden.

**Improvements:**
- **Cleaner Shutdown:** WebView wird vor dem Beenden zerstört, um Warnmeldungen zu reduzieren.
- **Hidden Installers:** FFmpeg/yt-dlp Installationen laufen nun ohne störendes Terminal-Fenster im Hintergrund.
- **Reliable Startup:** Fehlende Elemente führen nicht mehr zu einem kompletten JavaScript-Absturz.

**Bug fixes:**
- Fix: Buttons reagierten teilweise nicht, wenn Drag-and-Drop Elemente fehlten.
- Fix: Cancel-Button beendet nun zuverlässig alle laufenden yt-dlp Prozesse.
- Fix: Der Status bei Abbruch wurde fälschlicherweise für spätere Downloads übernommen.

----------------------------**V2.0.1**----------------------------

---

## ⚠️ Antivirus Notice

Some antivirus software may flag this program during updates or execution.  
This is often a **false positive** caused by how compiled executables or auto-updating features behave.

> **This program does not contain any malicious code.**

To avoid issues:
- Temporarily **disable your antivirus** during the update,  
- or **add this program as an exception** in your antivirus settings.

Always download the program from this official GitHub repository only.

---
