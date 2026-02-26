# Teams Audio Recorder - Educational PoC

A Tauri 2.0-based Windows application that captures system audio (including Teams meetings) using WASAPI loopback capture.

> **Disclaimer**: This application is for educational purposes only. Recording meetings without consent may violate privacy laws and terms of service.

## Features

- **WASAPI Loopback Capture**: Captures system audio output (what you hear through speakers/headphones)
- **Audio Recording**: Records all system audio including Teams, browser, and other applications
- **Ogg/Opus Export**: Saves recordings in compressed Ogg/Opus format (~30 MB/hr vs ~660 MB/hr for WAV)
- **WAV Export**: Optionally save as uncompressed WAV (32-bit float)
- **Pause/Resume**: Ability to pause and resume recordings
- **Real-time Status**: Shows recording duration, sample rate, and buffer size

## Requirements

- **Windows 10/11** (required for WASAPI loopback)
- **Rust** (1.70 or later)
- **Node.js** (18 or later)
- **npm**

## Prerequisites Installation

### 1. Install Rust
```powershell
# Download and run rustup-init.exe from https://rustup.rs/
# Or use winget:
winget install Rustlang.Rustup
```

### 2. Install Node.js
```powershell
# Using winget:
winget install OpenJS.NodeJS.LTS

# Or download from https://nodejs.org/
```

### 3. Install Visual Studio Build Tools (for Rust compilation)
```powershell
# Using winget:
winget install Microsoft.VisualStudio.2022.BuildTools

# Or download from https://visualstudio.microsoft.com/downloads/
# Select "Desktop development with C++" workload
```

## Build Instructions

```powershell
# Navigate to project directory
cd tauri-recorder

# Install npm dependencies
npm install

# Development mode (with hot reload)
npm run tauri dev

# Build for production
npm run tauri build
```

The built executable will be in `src-tauri/target/release/`.

## Building Release Binaries for Distribution

### Option 1: Build MSI Installer (Recommended)

```powershell
# Build MSI installer
npm run tauri build -- --bundles msi

# Output location:
# src-tauri/target/release/bundle/msi/Teams Audio Recorder_0.1.0_x64.msi
```

The MSI installer can be distributed and installed on any Windows 10/11 machine. It includes:
- The application executable
- Required DLLs
- Start menu shortcut
- Uninstaller

### Option 2: Build NSIS Installer

```powershell
# Build NSIS installer (alternative to MSI)
npm run tauri build -- --bundles nsis

# Output location:
# src-tauri/target/release/bundle/nsis/Teams Audio Recorder_0.1.0_x64-setup.exe
```

### Option 3: Build Portable Executable

```powershell
# Build standalone executable
npm run tauri build -- --bundles none

# Output location:
# src-tauri/target/release/teams-audio-recorder.exe
```

**Note**: The portable executable requires the WebView2 runtime to be installed on the target machine (included by default on Windows 11 and most Windows 10 installations).

### Option 4: Build All Formats

```powershell
# Build all bundle types (MSI, NSIS, etc.)
npm run tauri build

# Output location:
# src-tauri/target/release/bundle/
```

### Distribution Checklist

Before distributing the release binary:

1. **Test the build** on a clean Windows machine
2. **Verify WebView2** runtime is available (or include it)
3. **Check code signing** (optional but recommended for production)

### Including WebView2 Runtime

To ensure the app works on machines without WebView2:

```powershell
# Build with WebView2 runtime included
npm run tauri build -- --bundles msi --webview2 fixed-runtime

# This increases the installer size but ensures compatibility
```

### GitHub Actions CI/CD (Optional)

Create `.github/workflows/build.yml` for automated builds:

```yaml
name: Build Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: '20'
      
      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Install dependencies
        run: npm install
      
      - name: Build release
        run: npm run tauri build -- --bundles msi
      
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: teams-audio-recorder-msi
          path: src-tauri/target/release/bundle/msi/*.msi
```

## Cross-Compilation from Linux (mingw-w64)

You can build Windows binaries entirely from Linux — no GitHub Actions required.

### Why not just `opusenc`?

The previous `opusenc` crate required three pre-installed C system libraries
(`libopusenc`, `libopus`, `libogg`) which were a nightmare to cross-compile.

We replaced them with **two crates** that have a much simpler dependency story:

| Dependency | How it builds | Cross-compilation story |
|---|---|---|
| **audiopus** (libopus) | `audiopus_sys` compiles opus 1.3 from vendored C source | Need one manual step — see below |
| **ogg** | Pure Rust — no C at all | Just works™ |
| **windows** crate | Pure Rust FFI definitions | Just works™ |

> **Caveat:** `audiopus_sys`'s build script calls autotools `configure` without
> `--host`, so it uses the host compiler instead of the cross-compiler.
> We work around this by pre-building `libopus` with the correct cross-compiler
> and pointing `audiopus_sys` to it via `OPUS_LIB_DIR`.

### Prerequisites (Linux — Arch/Manjaro)

```bash
# Rust cross-compilation target
rustup target add x86_64-pc-windows-gnu

# mingw-w64 toolchain
sudo pacman -S mingw-w64-gcc

# Build tools for libopus
sudo pacman -S base-devel autoconf automake libtool
```

<details><summary>Ubuntu/Debian prerequisites</summary>

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt install mingw-w64 build-essential autoconf automake libtool pkg-config
```

</details>

### Configure Cargo for cross-compilation

Create `.cargo/config.toml` (already included in this repo):

```toml
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
ar = "x86_64-w64-mingw32-gcc-ar"
```

### Step 1 — Build libopus for Windows

The included helper script downloads, cross-compiles, and installs libopus
into `./opus-win64/`:

```bash
./scripts/build-opus-win64.sh
```

This only needs to run once (or when you want a newer Opus version).

### Step 2 — Build the Rust binary

```bash
OPUS_LIB_DIR=$PWD/opus-win64/lib LIBOPUS_STATIC=1 \
  cargo build \
    --manifest-path src-tauri/Cargo.toml \
    --target x86_64-pc-windows-gnu \
    --release
```

The `.exe` lands at `src-tauri/target/x86_64-pc-windows-gnu/release/tauri-recorder.exe`.

### Full Tauri build (with bundler — MSI/NSIS)

For MSI/NSIS installers you still need the Tauri CLI and a WiX/NSIS toolchain.
The simplest path for installer builds is running on Windows natively or in a VM.
For just the `.exe` + WebView2, the mingw cross-build above is sufficient.

### GitHub Actions (optional, for production)

GitHub Actions with `windows-latest` runner remains the easiest way to get
full MSI/NSIS installers. See [`.github/workflows/build.yml`](.github/workflows/build.yml).

## Usage

1. **Start Recording**: Click the "Start Recording" button to begin capturing system audio
2. **Pause/Resume**: Use the pause button to temporarily stop recording
3. **Stop Recording**: Click "Stop" to end the recording session
4. **Save Recording**: Click "Save Recording" to export as Ogg/Opus (default) or WAV
5. **Clear**: Discard the current recording buffer

## How It Works

### WASAPI Loopback Capture

The application uses Windows Audio Session API (WASAPI) with loopback mode to capture system audio:

1. **Initialize COM**: Sets up the COM library for Windows API calls
2. **Get Default Endpoint**: Retrieves the default audio render device (speakers/headphones)
3. **Loopback Stream**: Creates an audio stream with `AUDCLNT_STREAMFLAGS_LOOPBACK` flag
4. **Capture Audio**: Reads audio buffers from the capture client
5. **Store Samples**: Accumulates audio samples in a thread-safe buffer
6. **Encode Ogg/Opus**: Encodes the buffer into Ogg/Opus using `audiopus` + `ogg`

### Ogg/Opus Encoding Pipeline

The save process constructs an RFC 7845-compliant Ogg/Opus stream:

1. **OpusHead** header — codec version, channels, pre-skip, original sample rate
2. **OpusTags** header — vendor string, comment list
3. **Audio packets** — 20 ms Opus frames wrapped in OGG pages with granule positions at 48 kHz

If the WASAPI device outputs at a non-Opus rate (e.g. 44.1 kHz), a linear
interpolation resampler converts to 48 kHz before encoding.

### Technical Details

- **Sample Format**: 32-bit float (matches Windows audio engine)
- **Sample Rate**: Matches system audio format (typically 48 kHz)
- **Channels**: Matches system configuration (typically stereo)
- **Opus Bitrate**: 64 kbps VBR (configurable in `lib.rs`)
- **Frame Duration**: 20 ms (960 samples/channel at 48 kHz)
- **Buffer Management**: Thread-safe with parking_lot Mutex

## Project Structure

```
tauri-recorder/
|-- src/                    # Frontend (HTML/CSS/JS)
|   |-- index.html          # Main UI
|   |-- main.js             # Frontend logic
|   |-- styles.css          # Styling
|-- src-tauri/              # Backend (Rust)
|   |-- src/
|   |   |-- lib.rs          # Main library with WASAPI code
|   |   |-- main.rs         # Entry point
|   |-- Cargo.toml          # Rust dependencies
|   |-- tauri.conf.json     # Tauri configuration
|   |-- capabilities/       # Permission configuration
|-- package.json            # npm configuration
|-- README.md               # This file
```

## Key Dependencies

### Rust (Cargo.toml)
- `tauri` — Cross-platform desktop framework
- `windows` — Windows API bindings (WASAPI loopback capture)
- `audiopus` — Safe Opus encoder (compiles libopus 1.3 from vendored C source)
- `ogg` — Pure-Rust OGG container writer (no C dependency)
- `hound` — WAV file encoding (fallback format)
- `parking_lot` — High-performance synchronization primitives
- `tokio` — Async runtime

### JavaScript
- Tauri API for IPC communication

## Troubleshooting

### "Failed to get default audio endpoint"
- Ensure you have audio output devices configured
- Check Windows audio settings

### "COM initialization failed"
- Run the application as a normal user (not administrator)
- Check Windows integrity with `sfc /scannow`

### No audio recorded
- Verify system audio is playing
- Check volume mixer for muted applications
- Ensure the correct audio device is set as default

## Legal Notice

This application is provided for educational purposes to demonstrate WASAPI loopback capture. Users are responsible for ensuring compliance with applicable laws and regulations regarding audio recording in their jurisdiction. Recording conversations without consent may be illegal in some jurisdictions.

## License

MIT License - See LICENSE file for details.
