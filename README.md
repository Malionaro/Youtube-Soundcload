# SoundSync Downloader

Tauri v2 desktop application (TypeScript + Rust) for downloading YouTube / SoundCloud / Spotify / Apple Music links via `yt-dlp` + `ffmpeg`. Includes a queue, browser extension (`soundsync-extension/`), and mobile remote control (`http://<pc-ip>:3030`).

## Start (Development)

```powershell
cd soundsync-tauri
npm install
npm run tauri dev
```

Requirements: Node 20+, Rust stable, `ffmpeg` + `yt-dlp` (the app checks for these during the system check on startup; installation via `winget` is supported).

## Build

```powershell
cd soundsync-tauri
npm run build
npm run build:tauri
```

## Remote / Extension

* Remote page: `http://<pc-ip>:3030` (QR code available in Settings). LAN access is protected by a token, which is injected server-side into the page; `localhost` access from the browser extension does not require a token.
* Load the extension: `chrome://extensions` → enable Developer mode → select **Load unpacked** and choose `soundsync-extension/`.

## Config

Never commit real `config.json` or `cookies.txt` files. Template: `config.example.json`.

On Windows, the app stores its configuration at:

`%APPDATA%\com.malo.soundsync.downloader.v2\config.json`

---

## Disclaimer & Legal Notice

This project may reference or demonstrate the use of tools such as YouTube audio downloaders or services like Soundload and tone downloader utilities. These tools can be used to extract audio from online video platforms for various personal applications.

**⚠️ Important Legal Notice:**

Downloading or distributing copyrighted content without permission from the original creator or rights holder may violate copyright laws in your country. The existence or mention of such tools does **not** imply that their use is legal in all contexts.

### ⚠️ Risks and Responsibilities

* **Copyright Infringement:** Many online videos and audio tracks are protected by copyright. Unauthorized downloading or sharing may lead to legal action.
* **Security Risks:** Some third-party tools or websites may contain malware, spyware, or violate user privacy.
* **No Commercial Use:** Using downloaded material in commercial or public projects may carry serious legal consequences.

### ❗ Liability Disclaimer

I do **not** encourage or condone any illegal activity. This repository and its contents are provided for **educational and informational purposes only**.

> **I am not responsible for how others use the tools or information referenced in this project.**
> Use at your own risk, and always consult local laws and platform terms of service before downloading or using any content.

---

## ⚠️ Antivirus Notice

Some antivirus software may flag this program during updates or execution.
This is often a **false positive** caused by the way compiled executables or auto-updating features behave.

> **This program does not contain any malicious code.**

To avoid issues:

* Temporarily **disable your antivirus.** If the program is flagged, verify the file hash published on the GitHub release page.
* Add this program as an exception in your antivirus settings only after verifying that the download came from the official source.

Always download the program from this official GitHub repository only.

---

✅ Stay informed. Stay legal.
