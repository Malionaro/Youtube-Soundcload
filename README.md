# SoundSync Downloader

Tauri v2 Desktop-App (TypeScript + Rust) zum Laden von YouTube / SoundCloud / Spotify / Apple-Music-Links via `yt-dlp` + `ffmpeg`. Mit Sammelkorb-Queue, Browser-Extension (`soundsync-extension/`) und Handy-Remote (`http://<pc-ip>:3030`).

## Start (Dev)

```powershell
cd soundsync-tauri
npm install
npm run tauri dev
```

Voraussetzungen: Node 20+, Rust stable, `ffmpeg` + `yt-dlp` (prüft die App beim Start im System-Check, Install per winget möglich).

## Build

```powershell
cd soundsync-tauri
npm run build
npm run build:tauri
```

## Remote / Extension

- Remote-Seite: `http://<pc-ip>:3030` (QR in Einstellungen). LAN-Zugriff ist per Token geschützt (wird serverseitig in die Seite injiziert); `localhost` (Extension) braucht kein Token.
- Extension laden: `chrome://extensions` → Entwicklermodus → `soundsync-extension/` als entpackte Extension laden.

## Config

Echte `config.json` / `cookies.txt` nie committen. Vorlage: `config.example.json`. Die App speichert die Config unter `%APPDATA%\com.malo.soundsync.downloader.v2\config.json` (Windows).

---

## Disclaimer & Legal Notice

This project may reference or demonstrate the use of tools such as YouTube audio downloaders or services like Soundload and tone downloader utilities. These tools can be used to extract audio from online video platforms for various personal applications.

**⚠️ Important Legal Notice:**

Downloading or distributing copyrighted content without permission from the original creator or rights holder may violate copyright laws in your country. The existence or mention of such tools does **not** imply that their use is legal in all contexts.

### ⚠️ Risks and Responsibilities
- **Copyright Infringement:** Many online videos and audio tracks are protected by copyright. Unauthorized downloading or sharing may lead to legal action.
- **Security Risks:** Some third-party tools or websites may contain malware, spyware, or violate user privacy.
- **No Commercial Use:** Using downloaded material in commercial or public projects may carry serious legal consequences.

### ❗ Liability Disclaimer

I do **not** encourage or condone any illegal activity. This repository and its contents are provided for **educational and informational purposes only**.

> **I am not responsible for how others use the tools or information referenced in this project.**  
> Use at your own risk, and always consult local laws and platform terms of service before downloading or using any content.

---

## ⚠️ Antivirus Notice

Some antivirus software may flag this program during updates or execution.  
This is often a **false positive** caused by how compiled executables or auto-updating features behave.

> **This program does not contain any malicious code.**

To avoid issues:
- **Do NOT disable your antivirus.** If flagged, verify the file hash from the GitHub release page,
- add this program as an exception in your antivirus settings only after verifying the download source.

Always download the program from this official GitHub repository only.

---

✅ Stay informed. Stay legal.
