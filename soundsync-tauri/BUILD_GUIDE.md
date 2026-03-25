# SoundSync Tauri Build Guide

This guide describes how to build and run the SoundSync Tauri application.

## Prerequisites

- **Rust 1.70+**: [rustup.rs](https://rustup.rs)
- **Node.js 18+**: [nodejs.org](https://nodejs.org)
- **yt-dlp**: Installed and in your PATH.
- **FFmpeg**: The application expects FFmpeg in `e:\win11 stuff\PYTHON\Youtube-Soundcload\ffmpeg\bin\ffmpeg.exe` or in your system PATH.

## Setup

1.  **Clone/Open the project**:
    ```bash
    cd soundsync-tauri
    ```

2.  **Install dependencies**:
    ```bash
    npm install
    ```

## Development

To start the app in development mode with hot-reloading:

```bash
npm run tauri dev
```

## Production Build

To create a standalone executable:

```bash
npm run tauri build
```

The output will be located in `src-tauri/target/release/soundsync-tauri.exe`.

## Troubleshooting

### Windows Build Errors
If you encounter errors related to `windows-sys` or `msvc` during compilation:
1. Ensure "C++ development with MSVC" is installed in Visual Studio Installer.
2. Try running `cargo clean` and then `npm run tauri dev`.
3. Check that your Rust targets are up to date: `rustup update`.
