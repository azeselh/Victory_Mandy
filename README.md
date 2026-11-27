# Victory_Mandy (`amd-enhanced-v2`) — AMD RX 7900 / 32 GB RAM Optimization Branch

A fork of [VIctory](https://github.com/Marvisak/VIctory), a compatibility layer that runs an Xbox Series title natively on Windows from user-supplied files.

## The goal

**Reach a stable 30 fps or better at 1920×1080 without ray tracing**, then push frame rates higher and frame times lower on Windows PCs — targeting an Intel i5-13400F, 32 GB RAM, and AMD Radeon RX 7900 XT (20 GB) first (and 16 GB RAM later).

The approach is **porting, not emulating**: patching the title's renderer so it behaves like a PC renderer instead of watching and copying console shared memory every frame. Shaders are translated ahead of time into a versioned cache (`TRANSLATOR_VERSION = 2`) kept across builds. Ray tracing remains optional and off by default. Visual polish fixes (texture pop-in, indoor exposure settling) follow frame-rate stability.

**This repository contains no game files, code, or assets.** You need your own dump of the title. It is not affiliated with or endorsed by the game's developer or publisher.

---

## Where it stands (2026-11-27)

Built on top of **[VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)** (`PORT_CONSTANTS`, `PORT_DESCRIPTORS`, `PORT_CBV`, `PORT_GUARD`, `PORT_JOURNAL`, `VICTORY_THIN=1`, `VICTORY_JOIN_PARTS=8`), **[cool_fox](https://github.com/DoubleGhosty/cool_fox)** 32 GB RAM residency controls (`VICTORY_RESIDENCY=1`, `VICTORY_SWEEP_PER_HEAP=1`, `VICTORY_CARD_BUFFERS_MIN_KB=64`, `TablePlan`, pool growth backoff), and synced with **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)** (`a-publier` fixes through `4eb6cfdf`), plus dedicated CPU/GPU hot-path optimizations and repeatable benchmark tooling on `amd-enhanced-v2`.

**Current status:** **Stable 30 fps at 1080p in dense gameplay has NOT yet been reached.** CPU submission, draw-state, and descriptor overheads have been reduced, while synchronous `host::resident` / `host::mapped` waits during streaming and dense `1.5M–2.1M`-draw scenes keep gameplay in the `16–23 fps` range depending on scene load.

### Recent measured runs (Windows 11, Intel i5-13400F, 32 GB RAM, Radeon RX 7900 XT 20 GB, 1920×1080, RT off)

All runs below used `--release`, seeded save data outside the repository, `VICTORY_THIN=1`, `VICTORY_RELAY=2`, `VICTORY_RESIDENCY=1`, `VICTORY_WAIT_RESIDENT=1`, `VICTORY_BVH_BUILDER=0`, `VICTORY_FAST_CORES=1`, `VICTORY_PANEL=0`, and `VICTORY_FRAME_INTERVAL_MS=16.667` (`-UnlockFps`):

| Run | `VICTORY_THIN_THREADS` | Observed 256-frame block load (`draws` / `dispatches`) | Observed throughput (`frame presented` logs) | Min free RAM / Peak private commit | `mapped()` waits (cumulative) |
|---|---:|---|---|---:|---:|
| `20261127-resume` (`baseline-8` / `repeat-8`) | 8 | Gameplay (`1.54M–2.07M` draws) | `14.51–14.68 FPS` (`frames.csv` steady+0..1000) / `12.37–13.80 FPS` (`180–240s`) | — | `31.2k–33.4k` (`11.7s–19.6s`) |
| `20261127-resume` (`candidate-4`) | 4 | Gameplay (`1.41M–1.85M` draws) | `15.03 FPS` (`180–240s`, med `50.57 ms`, p95 `85.36 ms`) / `15.19 FPS` (`steady`) | — | `27.4k` (`9.61s`) |
| `opt3_clean` | 8 | `1.49M–1.53M` draws (`f=4864..5120`) / `1.78M–1.94M` (`f=5376..6400`) | `17.46–17.99 FPS` / `15.47–16.42 FPS` | `546 MB` / `31,192 MB` | `28.8k` (`11.93s`, max `34.5 ms`) |
| `opt4_shadow` | 8 | `2.03M–2.05M` draws, `~1.10M` disp (`f=4608..4864`) | `16.43–16.79 FPS` (`59.6–60.9 ms`) | `1,028 MB` / `29,905 MB` | `21.2k` (`7.51s`, `resident` `28.0k` / `19.11s`) |
| `opt4_thin4` | 4 | `2.04M–2.17M` draws, `~1.11M` disp (`f=5376..6400`) | `15.39–16.46 FPS` (`60.8–65.0 ms`) | `1,247 MB` / `30,052 MB` | `27.6k` (`9.44s`) |
| `opt5_draw` *(prior to removing unused `completes_rects`)* | 8 | `1.54M–1.62M` (`f=4864, 5376`) / `831k–846k` (`f=5632..6400`) | `17.22–17.62 FPS` / `22.03–22.63 FPS` (`44.2–45.4 ms`) | `3,082 MB` / `29,421 MB` | `27.6k` (`9.15s`, max `12.6 ms`) |

> **Measurement caveat:** In the `opt3`–`opt5` runs above, `Present` returned `DXGI_STATUS_OCCLUDED` (`0x087A0001`), so `frames.csv` was not emitted and per-block `frame presented` throughput is shown instead. `frame presented` block averages are not equivalent to `frames.csv` Present intervals or pure GPU execution time. Keep `VICTORY_THIN_THREADS=8` as the default (4 workers underperformed at `2.04M+` draws in `opt4_thin4`).

---

## Changelog & implemented improvements

### GPU, CPU, and submission-path optimizations (`amd-enhanced-v2`)
- **Descriptor heap pass & view decoding (`crates/victory-gpu/src/descriptor/host.rs`, `draw.rs`):**
  - Unified `buffer_copy` decoding via `srv_range_info` and `unordered_range_info`, removing duplicate buffer-extent and descriptor-type queries.
  - Deduplicated `heap_buffer_pass` slots (`sort_unstable` + `dedup`), compared against read-only `prev_entries` (`LookResult::{Same, Set(...)}`) to skip redundant `set_ranges` (`apply_range`) calls, and offloaded `Arc<Shadow>` drops to parallel `victory-views` workers.
- **Per-draw / per-dispatch hot paths (`crates/victory-gpu/src/draw.rs`):**
  - Returned and reused `&HostPipeline` across `prepare` / `prepare_for` / `prepare_with`, eliminating duplicate `host_pipeline` lookups in `draw`, `draw_indexed`, `dispatch_mesh`, and `execute_indirect`.
  - Replaced heap-allocated `Vec`s in `set_targets` (`Few<TargetKey, 8>` + stack `[Option<...>; 8]`), `bind` (`[Option<ID3D12DescriptorHeap>; 2]`), and `tables` (`Few<u32, 8>`).
- **Submit & write-watch fast paths (`crates/victory-gpu/src/host.rs`, `watch.rs`, `host/send.rs`):**
  - Fused `ask_once`, `stale_under_writes`, and `declared_writes` into a single pass (`gather_submission_steps`, `filter_stale_spans`, `push_declared`) inside `submit`.
  - Added `watch::kept()` fast early-outs before building/sorting page piece vectors in `ask_ahead_held`, `refresh_spans`, and `flush_all`, plus pre-sized `pages` capacity in `kernel_written`.
  - Partitioned dirty card runs into long `runs` and `short_groups` in a single pass (`host/send.rs`), eliminated duplicate range clones in `card_before`, and guarded `Batch::add` against empty `ahead` maps.
- **Shadow synchronization & parallel copies (`crates/victory-gpu/src/shadow.rs`):**
  - Held `shadow` locks while recording pending ranges in-flight and cleaned expired weak references, closing a concurrency race that could drop writes.
  - Replaced `Card.before` mutex wrapping with `OnceLock<ID3D12Resource>` + `before_lock` for lock-free `card_sides()` reads without COM `AddRef`/`Release` churn.
  - Merged `copied` and `ranged` under a single `Mutex<CopiedPages>` and added a lock-free `pending_any` (`Acquire`/`Release`) hint to skip `pending.lock()` in `nothing_new`, `update`, `stale_under_writes`, `merge_under_writes`, `writes_pending`, and `writes_pending_in` when no shader writes are pending.
  - Added a direct-copy fast path in `Shadow::take` for `whole && !upcheck::on()` (avoiding temporary `TITLE` page copies and per-page allocations), batched `changed_runs` under a single `kept` lock, and exposed `VICTORY_WIDE_THREADS` for parallel worker tuning.
- **Diagnostics timestamp calibration (`crates/victory-gpu/src/host/stamps.rs`, `timeline.rs`, `timeline/record.rs`):**
  - When the GPU clock calibration interval `card - card0 < frequency / 20` (< 50 ms), falls back to the nominal QPC/GPU clock ratio (`timeline::frequency() / frequency`) and averages both calibration offsets instead of amplifying `GetClockCalibration` per-sample jitter over short windows (keeping test bounds `near = 2_000` unchanged).

### Ported fixes from `Belobug/VIctory-Belobug-AMD` (`a-publier`)
- **PR #354:** Keep port-2 descriptor sync runs and fallback pages strictly disjoint and fix missed-slot fallback counters (`descriptor.rs`, `port_descriptors.rs`).
- **PR #324:** Prevent `REUSE` optimization from merging `dx.op.getDimensions` calls, avoiding AMD shader compiler crashes on RDNA4 compute pipelines (`TRANSLATOR_VERSION = 2`).
- **PR #336:** Fall back to the newest available FSR 3.1 provider under `-Fsr4K` when FSR 4.1.2 is not offered by the FidelityFX SDK for the adapter.
- **PR #343:** Validate stack memory before unwinding in `VICTORY_PROFILE` crash/stack-walk paths (`crash.rs`, `stacks.rs`).
- **PR #351, #348, #327, #325, #290, #328, #288:** Improved directly indexed heap diagnostics, HUD local VRAM vs OS budget vs dedicated capacity telemetry, DRED `light` default only on diagnostic runs, CPU counter step-back guard in `panel.rs`, and AMD/WARP test compatibility.
- **PR #295:** Opt-in GTA V PC-style keyboard & mouse mapping (`VICTORY_KBM_GTA=1`) with cursor auto-hide under mouse-look (`F2` / `` ` ``).
- **PR #294, #332, #326, #303, #340, #316:** Fingerprint-scoped `warm-bad.txt` (`VICTORY_WARM_BAD=0` / `--again`), `--warm` using all logical cores by default, CLI zero-worker/host-wave/policy-check validations, and accurate late profiling window reporting.

### Launcher & benchmarking tools
- **`tools/Start-Victory-AMD.ps1`:** Enhanced RX 7900 / 32 GB RAM PowerShell launcher with `-Safe`, `-Output`, `-UnlockFps`, `-RayTracing`, `-SkipWarmup`, `-WarmOnly`, `-Diagnostics`, `-RunDirectory`, `-SeedDataDirectory` (clones save data into the run folder, hashes initial files, preserves the original), `-RunSeconds`, `-More`, and automatic environment cleanup on exit/failure.
- **`crates/victory-gpu/src/present_times.rs`:** Bounded non-blocking writer recording `frames.csv` intervals between accepted host DXGI `Present` calls.
- **`tools/Measure-VictoryFrames.ps1` & `tools/Measure-VictoryLog.ps1`:** Summarize weighted FPS, median/p95/p99 frame times, hitches, gaps, and dropped samples by time (`-AfterSeconds`, `-UntilSeconds`) or frame range (`-AfterFrame`, `-UntilFrame`), plus log window summaries (`-MinDrawsPerFrame`, `-AfterFrame`).

---

## How to run it

### Prerequisites
- Windows 10/11 x64
- CPU with AVX2
- Direct3D 12 GPU supporting Shader Model 6.6 and Mesh Shaders (e.g., Radeon RX 7900 XT / XTX, RX 9070 XT)
- Rust toolchain (pinned via `rust-toolchain.toml`) and MSVC C++ build tools
- Your own retail dump of the title (`game_scarlett_master.exe` + shader/data files)

### 1. Clone, configure, and build

```powershell
git clone -b amd-enhanced-v2 https://github.com/azeselh/Victory_Mandy.git
cd Victory_Mandy
copy victory.example.toml victory.toml   # Edit paths outside this repository
cargo build --release -p victory --locked
```

Optional ahead-of-time shader translation and pipeline warming (recommended before first launch; both can be stopped and resumed later):

```powershell
.\target\release\victory.exe shaders <path-to-shader-archives>
.\target\release\victory.exe warm --used
```

### 2. Recommended launch (`tools\Start-Victory-AMD.ps1`)

Sets the AMD RX 7900 / 32 GB RAM profile (`VICTORY_RELAY=2`, `VICTORY_RESIDENCY=1`, `VICTORY_THIN=1`, `VICTORY_SWEEP_PER_HEAP=1`, `VICTORY_CARD_BUFFERS_MIN_KB=64`, `VICTORY_FAST_CORES=1`, `VICTORY_WAIT_RESIDENT=1`, ports enabled):

```powershell
# Default launch
.\tools\Start-Victory-AMD.ps1 -TitleExecutable "C:\path\to\game_scarlett_master.exe"

# Recommended 1080p uncapped FPS profile without RT (keeps gameplay uncapped up to 60+ fps ceiling)
.\tools\Start-Victory-AMD.ps1 `
    -TitleExecutable "C:\path\to\game_scarlett_master.exe" `
    -Output 1920x1080 `
    -UnlockFps `
    -SkipWarmup

# With opt-in GTA V PC keyboard & mouse controls (press F2 or ~ to toggle mouse-look)
.\tools\Start-Victory-AMD.ps1 `
    -TitleExecutable "C:\path\to\game_scarlett_master.exe" `
    -Output 1920x1080 `
    -UnlockFps `
    -SkipWarmup `
    -More @{ VICTORY_KBM_GTA = '1' }

# Safe fallback mode (disables aggressive enhanced optimizations if troubleshooting)
.\tools\Start-Victory-AMD.ps1 -TitleExecutable "C:\path\to\game_scarlett_master.exe" -Safe
```

Key `Start-Victory-AMD.ps1` parameters:
- `-Output <WxH>` (e.g. `1920x1080`, `2560x1440`, `3840x2160`)
- `-UnlockFps` (sets `VICTORY_FRAME_INTERVAL_MS=16.667`)
- `-RayTracing` (enables title ray-traced indirect lighting; off by default)
- `-Diagnostics` (builds/uses `diag` profile with `--features diagnostics`)
- `-Safe` (disables thin/port switches for comparison/debugging)
- `-SkipWarmup` / `-WarmOnly` / `-Jobs <N>`
- `-More @{ KEY = 'VAL' }` (extra environment overrides, automatically restored on exit)

### 3. Alternative launcher (`scripts\play.ps1`) & FidelityFX FSR

If you prefer upstream's `scripts\play.ps1` (supports `-Fsr4K` directly and uses `diag` builds):

```powershell
cargo build --locked --profile diag -p victory --features diagnostics
.\scripts\play.ps1 -Exe "C:\path\to\game_scarlett_master.exe"
.\scripts\play.ps1 -Exe "C:\path\to\game_scarlett_master.exe" -Fsr4K quality
.\scripts\play.ps1 -Exe "C:\path\to\game_scarlett_master.exe" -Safe
```

For PC FSR 4.1.2 (or automatic FSR 3.1 fallback when FSR 4.1.2 is unavailable on the adapter/SDK), set `[paths].fidelityfx_dir` in `victory.toml` or `VICTORY_FIDELITYFX_DIR` to the folder containing AMD's signed `amd_fidelityfx_upscaler_dx12.dll` (`Kits/FidelityFX/signedbin` from [AMD FSR SDK 2.4.0](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/tree/v2.4.0/Kits/FidelityFX/signedbin)). `-Fsr4K quality` renders 2560×1440 -> 3840×2160, `balanced` renders 2258×1270 -> 3840×2160, and `performance` renders 1920×1080 -> 3840×2160 (or pass `VICTORY_FSR_4K` via `-More` / `victory run`).

### 4. Reproducible benchmarks & frame analysis (outside the repository)

Store all run artifacts, logs, and save copies outside the git repository (for example in `$env:LOCALAPPDATA\victory\benchmarks\...`):

```powershell
$runDir = "$env:LOCALAPPDATA\victory\benchmarks\my-run"
$seedDir = "$env:LOCALAPPDATA\victory\benchmarks\seed-data"

.\tools\Start-Victory-AMD.ps1 `
    -TitleExecutable "C:\path\to\game_scarlett_master.exe" `
    -Output 1920x1080 -UnlockFps -SkipWarmup `
    -RunDirectory $runDir `
    -SeedDataDirectory $seedDir `
    -RunSeconds 270 `
    -More @{ VICTORY_BVH_BUILDER = '0'; VICTORY_PANEL = '0'; VICTORY_THIN_THREADS = '8' }

# Analyze Present intervals (by seconds or by frame range)
.\tools\Measure-VictoryFrames.ps1 -CsvPath "$runDir\frames.csv" -AfterSeconds 180 -UntilSeconds 240
.\tools\Measure-VictoryFrames.ps1 -CsvPath "$runDir\frames.csv" -AfterSeconds 0 -AfterFrame 4800 -UntilFrame 5800

# Summarize frame presented log blocks by minimum draw/dispatch load
.\tools\Measure-VictoryLog.ps1 -LogPath "$runDir\stderr.log" -AfterFrame 4096 -MinDrawsPerFrame 500000
```

> **Note:** Compare runs only across matching camera/scene draw counts (warmup/menus vs open street vs dense interiors vary significantly), verify `frames.csv` when unoccluded (`S_OK`), and never run builds or tests during benchmark windows.

---

## Verification & repository checks

Run all verification checks after edits:

```powershell
git diff --check
cargo xtask policy
.\tools\tests\Measure-VictoryFrames.Tests.ps1
.\tools\tests\Start-Victory-AMD.Tests.ps1
.\tools\tests\Recorded-Run.Tests.ps1
.\tools\tests\Measure-VictoryLog.Tests.ps1
cargo test --workspace --locked -- --test-threads=1
cargo test -p victory-gpu --features diagnostics --lib --locked -- --test-threads=1
cargo build --release -p victory --locked
```

Latest verified test run on commit `7913bae6`:
- `git diff --check`, `cargo xtask policy` (`asset policy: 279 files ok`), and all 4 PowerShell test scripts passed.
- `cargo test --workspace --locked -- --test-threads=1`: **0 failures** across all workspace crates (`victory-gpu`: 396 passed / 34 ignored; `victory-shader`: 303 passed / 4 ignored; `victory-hle`: 162 passed / 2 ignored + integration tests; `victory-loader`: 59 passed; `victory-formats`: 41 passed / 5 ignored; `victory-pe`: 36 passed; `victory`: 32 passed; `victory-core`: 13 passed; `xtask`: 9 passed; `victory-pipeline`: 7 passed; `victory-game`: 3 passed).
- `cargo test -p victory-gpu --features diagnostics --lib --locked -- --test-threads=1`: **445 passed, 0 failed, 34 ignored**, including `host::stamps::tests::a_submissions_times_are_the_pcs`.
- `cargo build --release -p victory --locked`: passed cleanly (`2m 01s`).

---

## Switches reference

### Renderer patches (enabled in `Start-Victory-AMD.ps1`)
| Switch | What it does |
|---|---|
| `VICTORY_PORT_CONSTANTS=1` | Card reads the title's 174 MB constants heap directly from title memory (no uploads or fills) |
| `VICTORY_PORT_DESCRIPTORS=1` | Descriptor-table stores report written slots instead of full-heap scanning |
| `VICTORY_PORT_CBV=1` | Translated shaders read constant buffers in place as raw bytes |
| `VICTORY_PORT_GUARD=1` | Shared write-guard lock with per-64 KiB block claims and parallel refresh planning beside submit |
| `VICTORY_PORT_JOURNAL=1` | Heap records queried at submit with 64-part partitioned write-watch journal locks |

### Other useful switches
| Switch | What it does |
|---|---|
| `--console series-s` | Uses Series S profile (1080p internal output, softer image, faster GPU pass) |
| `VICTORY_OUTPUT=1920x1080` | Overrides title render output resolution (faster card execution on Series X profile) |
| `VICTORY_FRAME_INTERVAL_MS=16.667` | Uncaps 30 fps frame limiter while keeping game simulation clock accurate |
| `VICTORY_THIN_THREADS=8` | Worker count for parallel command-list translation (default `8`; benchmarked ahead of `4` on i5-13400F) |
| `VICTORY_WIDE_THREADS` | Worker count override for parallel shadow memory refreshes/compares (default `cores / 4`, clamped `2..=6`) |
| `VICTORY_D3D12_ADAPTER` | Explicit zero-based DXGI adapter index or adapter name substring on hybrid-graphics systems |
| `VICTORY_AMBIENT_SETTLE` | Controls indoor lighting settle speed (default `250,3`; `0` disables) |
| `VICTORY_TEXTURE_STORE=1` | Persists converted textures across runs (off by default) |
| `VICTORY_KBM_GTA=1` | Optional GTA V PC-style keyboard & mouse pad mapping (`VICTORY_MOUSE_SENS`, `VICTORY_MOUSE_INVERT_Y=1`) |

### Known issues
- Trees, signs, and far objects flicker in the distance in some places.
- Indoor lighting can look too bright or too dark until it settles (dark rooms without `-RayTracing`).
- Newly streamed-in textures may pop in late during rapid movement.
- First run after a build or translator bump stutters while driver PSOs are compiled unless pre-warmed (`victory warm`).
- Triggering an in-world frame capture hits the title's watchdog timeout.

---

## Further reading & architecture notes

See [docs/findings.md](docs/findings.md), [docs/README-fxceless.md](docs/README-fxceless.md), and [docs/replay.md](docs/replay.md) for deep technical notes on:
- Parallel thin command-list translation (`VICTORY_THIN`)
- Asynchronous submission preparation (`VICTORY_ASYNC_SUBMIT`, `VICTORY_JOIN_PARTS`) and AVX2 memory diffing
- Relay read-back beside write guards (`VICTORY_RELAY`, `VICTORY_LAND`) and on-card indirect clamping (`VICTORY_CARD_STEPS`)
- Copy-queue texture uploads and GPU-written texture readback

---

## Credits

This repository builds upon community work across several forks (preserving original commit authors where cherry-picked):

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and contributors: loader, runtime services, D3D12 backend, and shader recompiler.
- **[VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)** & **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**: renderer ports 1–7, parallel list recording/submission, and replay tooling.
- **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)**: ambient light sky following, pipeline workers, RDNA/AMD shader & FidelityFX fixes, and community PRs.
- **[DoubleGhosty/cool_fox](https://github.com/DoubleGhosty/cool_fox)**: 32 GB RAM residency & sweep controls.
- **[DexNom/VIctory](https://github.com/DexNom/VIctory)**: FidelityFX FSR 4.1.2 4K bridge, ray-tracing fixes, cube gradients, and sparse textures.
- **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**, **[myokex/victory-amd](https://github.com/myokex/victory-amd)**, **Spexivus**, **joroyt**, **tightfall**, **Joshua Odom**, **Geckomacabre**, **cxy0072019-sketch**, and **romainhedouin**.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was studied for reference on title behavior; no XCL code is included.

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE) and [docs/asset-policy.md](docs/asset-policy.md).
