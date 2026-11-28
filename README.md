# VIctory — AMD Edition (`amd-enhanced-v2`)

Native Windows compatibility layer for Xbox Series build v1.0.2.1, adapted for **AMD Radeon** GPUs (targeted on Intel i5-13400F, 32 GB RAM, AMD Radeon RX 7900 XT 20 GB) with **AMD FidelityFX Super Resolution 4 / 3.1 upscaling**, **AMD FSR 3 Frame Generation (2x)**, an **in-game F1 settings menu**, **F3 performance telemetry panel**, **optional GTA-style keyboard + mouse controls (`F2` / `~` camera lock)**, and benchmark tooling.

> **This repository contains no game files, proprietary code, assets, or vendor SDK binaries.** You must supply your own legally obtained dump of the Xbox Series title (`game_scarlett_master.exe` v1.0.2.1). AMD FidelityFX SDK DLLs and Microsoft's D3D12 Agility SDK are fetched on demand from their official upstream releases under their own licences. See [docs/asset-policy.md](docs/asset-policy.md).

---

## Key Features & Changes

- **Native AMD FSR 4 / 3.1 Upscaling (`amd_fidelityfx_upscaler_dx12.dll` v2.4.0):**
  Replaces the title's console FidelityFX path on the host GPU (automatically selects FSR `4.1.2` when supported by the adapter, e.g., Radeon RX 7900 XT / RDNA4, or falls back to newest FSR `3.1.x`). Controlled via `--fsr quality|balanced|performance|dlaa|off` or the F1 menu (`FSR 4 / 3.1`).
- **AMD FSR 3 Frame Generation (`amd_fidelityfx_framegeneration_dx12.dll` v2.4.0):**
  Optical-flow 2x frame interpolation using the FidelityFX swapchain alongside PC FSR upscaling — no NVIDIA GPU or Streamline required. Enabled via `--framegen 2`, `--framegen-ready` (for live toggling), or the F1 menu (`FSR 3 FRAME GEN`).
- **Standalone Launcher Executable (`Play-Victory-AMD.exe`):**
  Drop `Play-Victory-AMD.exe` straight into the game folder next to `game_scarlett_master.exe` and double-click to play. Automatically locates `%LOCALAPPDATA%\victory\amd-sdk`, offers one-click downloads of AMD FidelityFX SDK v2.4.0 (`victory get-fsr`) and Windows 10 D3D12 Agility SDK (`victory get-d3d12`) when missing, and keeps logs/settings/caches outside the game directory (`%LOCALAPPDATA%\victory`).
- **In-Game F1 Settings Menu & F3 Stats Panel:**
  - **F1** opens the overlay settings menu (Up/Down/Left/Right/Enter, `Esc` closes): switch **FSR 4 / 3.1** mode, **FSR 3 FRAME GEN (2x)**, **FRAME RATE CAP**, **FPS AT THE TOP**, **STATS PANEL (F3)**, **FULLSCREEN**, **MOUSE CAPTURE**, **GTA KBM CONTROLS (F2 LOCK)**, **CONSOLE (Series X / Series S)**, **SERIES X / SERIES S PICTURE (`1440`, `1080`, `1080P TO 1440P`, `4K`)**, lighting/performance toggles, **PRELOAD SHADERS**, and **EXPORT / IMPORT SAVE** zip sharing.
  - **F3** shows rendered FPS, frame generation output FPS (`2x`), frame time, draws, GPU/CPU wait percentages, copies, RAM, and VRAM usage.
- **Resolution Scaling (1080p, 1440p, 4K, Series S):**
  - Series X supports native `2560x1440` (`--res 1440`, default), `1920x1080` (`--res 1080` via runtime output patching), `1080to1440`, and stock `3840x2160` (`--res 4k` / `VICTORY_FSR_4K`), all compatible with FSR 4 / 3.1.
  - Series S pairs internal render sizes (`1080`, `720`, `626`, `540`) directly with FSR presets (`dlaa`, `quality`, `balanced`, `performance`).
- **GTA PC-Style Keyboard & Mouse Controls (`VICTORY_KBM_GTA=1` / `--kbm-gta`):**
  Maps mouse movement to right-stick camera look with exponential velocity smoothing, `LMB`/`RMB`/`MMB` to shoot/aim/look-behind, `F2` or `~` to toggle cursor lock (auto-hides cursor via `WM_SETCURSOR`, automatically unlocks on `Esc`, `P`, F1 menu, or physical controller takeover), and PlayStation HID DualSense/DualShock 4 support.
- **AMD / RDNA Shader, Memory & Stability Fixes:**
  - RDNA4/AMD `dx.op.getDimensions` `REUSE` bitcode safety fix (`crates/victory-shader`).
  - Write-combined title memory optimization (`VICTORY_WC_NO_BACK`, `VICTORY_WC_NO_BACK_MIN_KB`), thread-priority scheduling (`VICTORY_HELPER_PRIORITY`), `VICTORY_SLOT_EVICT`, `VICTORY_ALLOCATOR_TRIM`, `VICTORY_BUDGET_PACK`, lighting estimation (`VICTORY_AMBIENT`), water lighting toggle (`VICTORY_WATER_LIGHT`), and Present interval micro-telemetry (`present_times.rs`).
- **Benchmarking & Verification Tooling (`tools/`):**
  - `tools/Start-Victory-AMD.ps1` (`-Safe`, `-RunDirectory`, `-RunSeconds`, `-SeedDataDirectory`, `-Fsr4K`) and `tools/Measure-VictoryFrames.ps1` with Pester unit tests in `tools/tests/*.Tests.ps1`.

---

## Quick Start (Prebuilt Executable)

1. Build (see below) or copy `Play-Victory-AMD.exe` into your title folder next to `game_scarlett_master.exe`.
2. Double-click `Play-Victory-AMD.exe`.
   - On first launch, if `%LOCALAPPDATA%\victory\amd-sdk` does not yet hold the signed FidelityFX SDK v2.4.0 DLLs, confirm `[Y/n]` to download them (~87 MB total) from `GPUOpen-LibrariesAndSDKs/FidelityFX-SDK` (SHA-256 and Authenticode signature verified).
   - On Windows 10, confirm `[Y/n]` to fetch Microsoft's D3D12 Agility SDK (`D3D12\D3D12Core.dll`, ~22 MB) if shader model 6.6 is missing.
3. In-game shortcuts:
   - **`F1`**: Settings menu (Up/Down/Left/Right, Enter to apply/save/restart)
   - **`F2`** or **`` ` `` / `~`**: Toggle cursor lock when `GTA KBM CONTROLS` is enabled (`Esc` / `P` unlocks)
   - **`F3`**: Toggle telemetry stats panel

### Useful Launch Commands

Run from PowerShell inside the game folder (or anywhere while pointing to `game_scarlett_master.exe`):

```powershell
# Recommended AMD 1080p + FSR Quality + FSR 3 FrameGen (2x) + GTA KBM controls:
.\Play-Victory-AMD.exe --res 1080 --fsr quality --framegen 2 --kbm-gta

# 1440p + FSR Quality + FSR 3 FrameGen ready to toggle live in F1:
.\Play-Victory-AMD.exe --res 1440 --fsr quality --framegen-ready

# 1440p + FSR 4 Native AA (no upscaling scale factor) + 60 FPS cap:
.\Play-Victory-AMD.exe --res 1440 --fsr dlaa --fps 60

# Preload shaders and build driver pipeline cache before starting:
.\Play-Victory-AMD.exe --preload

# Manually download / refresh AMD FidelityFX SDK v2.4.0 DLLs:
.\victory.exe get-fsr
```

---

## Building from Source

Prerequisites on Windows 10/11 (x64):
- [Rust toolchain](https://rustup.rs) (edition / toolchain specified in `rust-toolchain.toml`)
- Visual Studio Build Tools with **Desktop development with C++**
- PowerShell 5.1+

```powershell
git clone https://github.com/azeselh/victory-fork -b amd-enhanced-v2
cd victory-fork
copy victory.example.toml victory.toml   # set game_dir outside the repository

# Download signed AMD FidelityFX SDK v2.4.0 binaries (optional if Play-Victory-AMD downloads on first run)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\get-amd-fsr.ps1 -To "$env:LOCALAPPDATA\victory\amd-sdk"

# Build release binaries (victory.exe and Play-Victory-AMD.exe)
cargo build --release -p victory --locked

# Copy standalone launcher into your game directory (example path):
copy target\release\Play-Victory-AMD.exe 'C:\Path\To\YourGameDump\Play-Victory-AMD.exe'
```

### Benchmark Launcher (`tools/Start-Victory-AMD.ps1`)

For repeatable frame-time measurements outside the repository:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools\Start-Victory-AMD.ps1 `
  -RunDirectory "$env:LOCALAPPDATA\victory\runs\bench-01" `
  -RunSeconds 180
```

See [docs/windows-memory.md](docs/windows-memory.md), [docs/findings.md](docs/findings.md), and [docs/architecture.md](docs/architecture.md) for architecture details and troubleshooting.

---

## Credits & Upstream Projects

Built by combining work from:
- **[Marvisak/VIctory](https://github.com/Marvisak/VIctory)** — original loader, HLE, D3D12 translation, and shader recompiler
- **[readmet3xt-lab/foxhound](https://github.com/readmet3xt-lab/foxhound)** — rendering, synchronization, descriptor/upload pipeline, and performance base
- **[trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA)** — F1 settings menu, F3 panel, shader preload, FSR 3 Frame Generation integration, `victory play` standalone launcher architecture, and save zip import/export
- **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)**, **[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)**, **[DexNom/VIctory](https://github.com/DexNom/VIctory)**, **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**, **[codec-d/cool_fox](https://github.com/codec-d/cool_fox)** — AMD FidelityFX PC upscaling, 1440p/resolution work, and platform fixes

## License

GPL-3.0-or-later. See [LICENSE](LICENSE) and [docs/asset-policy.md](docs/asset-policy.md).
