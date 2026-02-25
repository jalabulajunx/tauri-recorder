# Teams Audio Recorder - Educational PoC

A Tauri 2.0-based Windows application that captures system audio (including Teams meetings) using WASAPI loopback capture.

> **Disclaimer**: This application is for educational purposes only. Recording meetings without consent may violate privacy laws and terms of service.

## Features

- **WASAPI Loopback Capture**: Captures system audio output (what you hear through speakers/headphones)
- **Audio Recording**: Records all system audio including Teams, browser, and other applications
- **WAV Export**: Saves recordings in high-quality WAV format (32-bit float)
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

## Cross-Compilation from Linux

You can build Windows binaries from Linux using cross-compilation. This is useful if you don't have access to a Windows machine.

### Option 1: Using Cross (Recommended)

[Cross](https://github.com/cross-rs/cross) is a tool for cross-compiling Rust applications using Docker containers.

```bash
# Install cross
cargo install cross

# Install Docker (if not already installed)
# Ubuntu/Debian:
sudo apt-get update && sudo apt-get install docker.io
sudo usermod -aG docker $USER
# Log out and back in for group changes to take effect

# Add Windows target
rustup target add x86_64-pc-windows-gnu

# Build for Windows
cross build --target x86_64-pc-windows-gnu --release
```

**Limitations**:
- Produces a `.exe` file but **cannot create MSI/NSIS installers**
- The Windows API bindings (`windows-rs` crate) may have issues with cross-compilation
- WebView2 runtime must be present on the target machine

### Option 2: Using Cargo Directly with MinGW

```bash
# Install MinGW-w64 cross-compiler
sudo apt-get install mingw-w64

# Add Windows target
rustup target add x86_64-pc-windows-gnu

# Configure cargo for cross-compilation
mkdir -p ~/.cargo
cat >> ~/.cargo/config.toml << 'EOF'
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
ar = "x86_64-w64-mingw32-gcc-ar"
EOF

# Build
cargo build --target x86_64-pc-windows-gnu --release
```

### Option 3: Using GitHub Actions (Best for Production)

The most reliable way to build Windows binaries from Linux is to use GitHub Actions with a Windows runner. This is already configured in [`.github/workflows/build.yml`](.github/workflows/build.yml).

```bash
# Push a tag to trigger the build
git tag v0.1.0
git push origin v0.1.0
```

The workflow will:
1. Run on a Windows machine in the cloud
2. Build MSI, NSIS, and portable executables
3. Create a GitHub Release with all artifacts

### Cross-Compilation Caveats for This Project

This project uses the `windows` crate for WASAPI audio capture. Cross-compilation has specific challenges:

1. **COM/Windows API**: The `windows-rs` crate is designed for native Windows compilation. Cross-compilation may fail due to Windows-specific API bindings.

2. **Recommended Approach**: Use GitHub Actions (Option 3) for building Windows binaries. It's free for public repositories and provides a genuine Windows environment.

3. **Alternative**: Use a Windows VM (VirtualBox, VMware) or Windows dual-boot setup.

### Quick Test with GitHub Actions

If you have this project on GitHub, you can manually trigger the build:

1. Go to **Actions** tab in your repository
2. Select **Build Release** workflow
3. Click **Run workflow**
4. Download the artifacts when complete

## Usage

1. **Start Recording**: Click the "Start Recording" button to begin capturing system audio
2. **Pause/Resume**: Use the pause button to temporarily stop recording
3. **Stop Recording**: Click "Stop" to end the recording session
4. **Save Recording**: Click "Save Recording" to export as WAV file
5. **Clear**: Discard the current recording buffer

## How It Works

### WASAPI Loopback Capture

The application uses Windows Audio Session API (WASAPI) with loopback mode to capture system audio:

1. **Initialize COM**: Sets up the COM library for Windows API calls
2. **Get Default Endpoint**: Retrieves the default audio render device (speakers/headphones)
3. **Loopback Stream**: Creates an audio stream with `AUDCLNT_STREAMFLAGS_LOOPBACK` flag
4. **Capture Audio**: Reads audio buffers from the capture client
5. **Store Samples**: Accumulates audio samples in a thread-safe buffer
6. **Export WAV**: Writes samples to a WAV file using the hound library

### Technical Details

- **Sample Format**: 32-bit float (matches Windows audio engine)
- **Sample Rate**: Matches system audio format (typically 48kHz)
- **Channels**: Matches system configuration (typically stereo)
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
- `tauri` - Cross-platform desktop framework
- `windows` - Windows API bindings (WASAPI)
- `hound` - WAV file encoding
- `parking_lot` - High-performance synchronization primitives
- `tokio` - Async runtime

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
