# SoundSync Tauri Architecture

## System Overview

SoundSync is a modern desktop application for downloading high-fidelity audio and video from YouTube and SoundCloud. It is built using the **Tauri 2.0** framework, leveraging Rust for the backend and React for the frontend.

## Components

### [Backend] Rust (src-tauri)
- **lib.rs**: The main library crate that handles the application lifecycle, registers commands, and manages IPC.
- **Commands**:
    - `get_metadata`: Uses `yt-dlp --json-extract` to fetch video metadata (title, uploader, duration, thumbnail).
    - `download_track`: Executes `yt-dlp` as a subprocess with real-time progress parsing.
- **Subprocesses**:
    - `yt-dlp`: Used for metadata extraction and downloading.
    - `ffmpeg`: Used for audio/video post-processing and conversion.

### [Frontend] React (src)
- **Main App**: Handles the UI state (URL, format, loading, results).
- **Communication**: Uses `@tauri-apps/api/core` (`invoke`) to call Rust commands and `@tauri-apps/api/event` (`listen`) to receive progress updates.
- **Styles**: Modern CSS with Glassmorphism and Backdrop Blur for a premium aesthetic.

## Data Flow

1.  **User Input**: The user pastes a URL and selects a format.
2.  **Metadata Fetching**: The URL is sent to Rust via `get_metadata`. Rust calls `yt-dlp` and returns JSON.
3.  **Download Initiation**: The user clicks "Start Sync". Rust initiates `download_track`.
4.  **Progress Updates**: As `yt-dlp` downloads, Rust parses the output and emits `download-progress` events.
5.  **Completion**: When the download is complete, Rust returns a success message to the frontend.
