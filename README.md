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

## Team-Mode (P2P, free)

Two PCs can download one combined playlist together. Both apps already run a small local server on port **3030**; the Team-Mode adds a token-protected `/team/*` protocol on top. No central server, no accounts, no cost.

1. PC A: **Settings → Team-Download (P2P) → Create team**. Copy the generated invite link `soundsync-team://IP:PORT/TEAM_ID?token=…`.
2. PC B: paste the link into **Join**. Both PCs are now in one team (same LAN; for internet use you need your own reachability, e.g. a VPN like Tailscale/ZeroTier or port-forwarding — no third-party service is bundled).
3. Each PC can **add its own playlist**; everything is merged into one de-duplicated list.
4. **Start team download** on both PCs. Tracks are split automatically ("work-stealing") and downloaded in parallel onto each PC.
5. **Failover:** if one PC is temporarily **blocked by YouTube** (HTTP 429 / rate-limit / bot-check / 403) or **drops offline**, its unfinished tracks are automatically re-queued and **taken over by the other PC**. A blocked PC pauses briefly and re-joins after a cooldown.
6. Finished files can be shared: the PC downloads its share, uploads it to the coordinator, and **Get missing files** syncs anything you don't have yet.

Output format/quality/folder are chosen per PC. By default the coordinator is one of the two PCs. Self-hosted API mode (`soundsync-relay`): run the standalone free `soundsync-relay` on a Raspberry Pi / always-on server as coordinator; both PCs connect only outward with `team_join` to `soundsync-team://<server>:<port>/<team_id>?token=...` - no port forwarding on the PCs. See `soundsync-tauri/relay/README.md`.

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
