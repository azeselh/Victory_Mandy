//! `victory play`, `Play-Victory-AMD.exe`, and `victory` started with no command
//! at all (a double click): the title with the AMD settings this fork plays
//! with, so that the program alone, put beside the title's
//! `game_scarlett_master.exe`, is all there is to start.
//!
//! Nothing is written beside the program or into the title's folder: the
//! settings the F1 menu saves (`victory-settings.txt`), the logs and AMD's
//! FidelityFX files are kept under `%LOCALAPPDATA%\victory`, where the caches
//! and the saves already are.
//!
//! AMD's FidelityFX SDK DLLs (FSR 4 / 3.1 upscaling and FSR 3 frame
//! generation) are AMD's and are not part of this program. Where they are
//! missing, the first start asks once whether to fetch them from AMD's own
//! GitHub (`victory get-fsr` does it later); without them the title runs with
//! its own upscaling and without frame generation.
//!
//! The one thing that goes beside the program: on a PC whose Direct3D 12 runs
//! no shader model 6.6 (Windows 10's own stops at 6.5), Microsoft's newer
//! runtime, `D3D12\D3D12Core.dll` from Microsoft's NuGet package, after asking
//! (`victory get-d3d12`). Windows loads it only from there (lib.rs).

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The title's executable, looked for beside this program.
const GAME: &str = "game_scarlett_master.exe";

/// `scripts\get-amd-fsr.ps1`, run by Windows PowerShell.
const FETCH_FSR: &str = include_str!("../../../scripts/get-amd-fsr.ps1");

/// `scripts\get-d3d12.ps1`, run by Windows PowerShell.
const FETCH_D3D12: &str = include_str!("../../../scripts/get-d3d12.ps1");

/// The three AMD files FSR 4 / 3.1 upscaling and FSR 3 frame generation load.
const FSR_FILES: [&str; 3] = [
    "amd_fidelityfx_loader_dx12.dll",
    "amd_fidelityfx_upscaler_dx12.dll",
    "amd_fidelityfx_framegeneration_dx12.dll",
];

/// The settings the title is started with.
const SETTINGS: &[(&str, &str)] = &[
    // Submission and copy machinery
    ("VICTORY_THIN", "1"),
    ("VICTORY_JOIN_PARTS", "8"),
    ("VICTORY_RELAY", "2"),
    ("VICTORY_RELAY_COPY", "1"),
    ("VICTORY_WIDE_BACK", "1"),
    ("VICTORY_CARD_REHASH_RUNS", "1"),
    ("VICTORY_JOIN_COPIES", "1"),
    ("VICTORY_CARD_STEPS", "2"),
    ("VICTORY_CARD_AWAKE", "1"),
    ("VICTORY_CARD_BUFFERS_MIN_KB", "64"),
    ("VICTORY_INDIRECT_WINDOW", "1"),
    ("VICTORY_INDIRECT_BATCH", "2"),
    ("VICTORY_UPLOAD_AHEAD", "1"),
    ("VICTORY_SHOW_AFTER", "1"),
    ("VICTORY_MAP_THREADS", "1"),
    // A frame's texture passes within 8 ms together; helpers woken at each submission's start
    ("VICTORY_TEXSPK_PASS_MS", "8"),
    ("VICTORY_PASSES_HOT", "1"),
    // The layer's threads a frame waits for, above normal priority
    ("VICTORY_HELPER_PRIORITY", "1"),
    // Shader writes into big write-combined copies stay on the card
    ("VICTORY_WC_NO_BACK", "1"),
    ("VICTORY_WC_NO_BACK_MIN_KB", "1024"),
    // The title's page-mapping copies kept without Windows views
    ("VICTORY_SOFT_VIEWS", "copies"),
    // Textures
    ("VICTORY_TEXTURE_POOL", "1"),
    ("VICTORY_RIPE_BLOCKS", "0"),
    ("VICTORY_TEXTURE_NO_WAIT", "1"),
    ("VICTORY_TEXTURE_CATCHUP", "0"),
    ("VICTORY_TEXTURE_STAGE_MS", "16"),
    ("VICTORY_LEVELS_ON_DEMAND", "0"),
    ("VICTORY_PRELUDE_UPLOADS", "1"),
    ("VICTORY_SHOW_NEW_LATER", "1"),
    ("VICTORY_POOL_SMALL", "0"),
    ("VICTORY_TABLED_REREAD", "1"),
    ("VICTORY_QUEUE_HOST_TEXTURES", "0"),
    // Descriptor heaps' records
    ("VICTORY_HEAP_SYNC_FIRST", "1"),
    ("VICTORY_SYNC_IN_PLACE", "1"),
    ("VICTORY_SPANS_QUICK", "1"),
    ("VICTORY_SYNC_FINE", "1"),
    ("VICTORY_HEAP_SYNC_AT_SUBMIT", "1"),
    // Light without the title's ray tracing
    ("VICTORY_AMBIENT", "0.8"),
    ("VICTORY_AMBIENT_AIR", "1.5"),
    ("VICTORY_AMBIENT_SPECULAR", "1"),
    // Pixel programs keep the console's early depth test
    ("VICTORY_EARLY_DEPTH", "1"),
    // Left out: the decal heap's defragmentation and the heat-haze pass
    ("VICTORY_LEAVE_OUT", "02384a60fda6f2d0,731a39171b568298"),
    // Pipelines the title makes are written down, for `victory warm`
    ("VICTORY_PIPELINE_BOOK", "1"),
    // Memory stack: texture slots left unused are given back, old command allocators
    // are taken anew, small buffers share card memory
    ("VICTORY_SLOT_EVICT", "1"),
    ("VICTORY_ALLOCATOR_TRIM", "1"),
    ("VICTORY_BUDGET_PACK", "1"),
];

/// Path settings a start keeps from its surroundings; every other
/// `VICTORY_...` variable is cleared before the settings are put in.
const KEPT: &[&str] = &[
    "VICTORY_HOME",
    "VICTORY_ADAPTER",
    "VICTORY_CONFIG",
    "VICTORY_GAME_DIR",
    "VICTORY_CACHE_DIR",
    "VICTORY_DATA_DIR",
    "VICTORY_STREAMLINE_DIR",
    "VICTORY_FIDELITYFX_DIR",
    "VICTORY_LOG",
];

const RESOLUTIONS: [&str; 4] = ["1440", "1080", "1080to1440", "4k"];
const FSR_MODES: [&str; 5] = ["off", "dlaa", "quality", "balanced", "performance"];
/// The Series S scene heights the F1 menu offers and the FSR mode of each: the picture
/// is 1920x1080, the scene that mode's share of it (720: the game's own size).
const S_SIZES: [(&str, &str); 4] = [
    ("1080", "dlaa"),
    ("720", "quality"),
    ("626", "balanced"),
    ("540", "performance"),
];

#[derive(clap::Args, Clone, Debug)]
pub struct Play {
    /// The title's executable. Without it: `game_scarlett_master.exe` beside
    /// this program, else in the current folder.
    exe: Option<PathBuf>,
    /// The Series S profile (1280x720 inside: faster, softer).
    #[arg(long)]
    series_s: bool,
    /// 1440: output and scene 2560x1440; 1080: output 1920x1080;
    /// 1080to1440: scene 1920x1080 upscaled to 1440p; 4k: the stock 3840x2160 output.
    #[arg(long, default_value = "1440", value_parser = RESOLUTIONS)]
    res: String,
    /// AMD FidelityFX FSR 4 / 3.1 in place of the title's upscaling.
    #[arg(long, visible_alias = "dlss", default_value = "quality", value_parser = FSR_MODES)]
    fsr: String,
    /// AMD FSR 3 frame generation on from the start (2x frames shown for each rendered).
    /// Values 2..=6 are accepted and run as 2x under FSR 3; omit to keep off (see `--framegen-ready`).
    #[arg(long, value_parser = clap::value_parser!(u32).range(2..=6))]
    framegen: Option<u32>,
    /// FSR 3 frame generation made ready but off at the start, so F1 can toggle it live.
    #[arg(long)]
    framegen_ready: bool,
    /// (The default now; still taken.)
    #[arg(long)]
    no_framegen_ready: bool,
    /// Frames a second at most (30: the title's own rate; 1000: no cap).
    #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u32).range(1..=1000))]
    fps: u32,
    /// Enable GTA V PC-style keyboard and mouse camera controls (F2 / `~` lock).
    #[arg(long)]
    kbm_gta: bool,
    /// Add or override a switch, as often as needed: `--set VICTORY_X=1`.
    #[arg(long = "set", value_name = "NAME=VALUE")]
    more: Vec<String>,
    /// Make every shader before the title starts (`victory preload`, as the
    /// F1 menu's "PRELOAD SHADERS" does): a while, maybe long.
    #[arg(long)]
    pub preload: bool,
}

/// What a preload is, said before it begins.
pub fn explain_preload() {
    println!();
    println!("Preloading shaders: every shader of the title is translated, then the pipelines the game");
    println!("made in earlier runs are built, and those it needs while loading are stored where it loads");
    println!("them from at the start, so it does not stop to make them while you play.");
    println!("This takes a while, and possibly a long time, depending on your hardware (CPU above all).");
    println!("What is done is kept: stopped (close this window), the next preload goes on from there.");
    println!("A later update of this build, or a new graphics driver, may need a new preload.");
    println!();
}

/// Runs `victory preload` over the title's folder with this start's settings
/// (the translations depend on them), then lets the start go on, whatever came of it.
pub fn preload(exe: &Path) {
    let folder = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let began = std::time::Instant::now();
    let done = std::env::current_exe().and_then(|program| {
        std::process::Command::new(program)
            .arg("preload")
            .arg(&folder)
            // Its own lines in this window; the start's log is the title's.
            .env_remove("VICTORY_LOG_FILE")
            .status()
    });
    let minutes = began.elapsed().as_secs_f64() / 60.0;
    match done {
        Ok(s) if s.success() => println!("Shaders preloaded in {minutes:.1} min. Starting the game."),
        Ok(s) => println!("The preload stopped ({s}) after {minutes:.1} min; what it did is kept. Starting the game."),
        Err(e) => println!("The preload did not start ({e}). Starting the game."),
    }
}

/// `%LOCALAPPDATA%\victory`, or the folder `VICTORY_HOME` names.
fn home() -> Result<PathBuf> {
    if let Some(home) = victory_core::config::home_override() {
        return Ok(home);
    }
    let base = std::env::var_os("LOCALAPPDATA").context("no %LOCALAPPDATA% to keep settings and logs in")?;
    Ok(PathBuf::from(base).join("victory"))
}

/// Where `victory get-fsr` puts AMD's FidelityFX SDK files.
fn amd_home() -> Result<PathBuf> {
    Ok(home()?.join("amd-sdk"))
}

fn has_fsr(dir: &Path) -> bool {
    FSR_FILES.iter().all(|f| dir.join(f).is_file())
}

/// The folder with AMD's FidelityFX SDK DLLs (`amd_fidelityfx_loader_dx12.dll`,
/// `amd_fidelityfx_upscaler_dx12.dll`, `amd_fidelityfx_framegeneration_dx12.dll`):
/// `VICTORY_FIDELITYFX_DIR`, where `get-fsr` puts them, or beside this program.
fn fidelityfx_dir(here: &Path) -> Option<PathBuf> {
    let named = std::env::var_os("VICTORY_FIDELITYFX_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let ours = amd_home().ok();
    let legacy = home().ok().map(|d| d.join("amd-fidelityfx"));
    let beside = [
        here.join("fidelityfx"),
        here.join("amd-sdk").join("FidelityFX").join("bin"),
        here.join("amd-sdk"),
        here.to_path_buf(),
    ];
    named
        .into_iter()
        .chain(ours)
        .chain(legacy)
        .chain(beside)
        .find(|d| has_fsr(d))
}

/// The lines `NAME=value` of the menu's file.
fn parse_menu(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .filter(|(k, _)| {
            k.starts_with("VICTORY_")
                && k.bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        })
        .collect()
}

/// The settings of a start and whether it is the Series S profile: the
/// options, what the menu saved over them (`menu`), `--set` over both.
/// `fidelityfx`: the folder with AMD's FidelityFX SDK DLLs, if present.
fn settings(
    o: &Play,
    menu: &[(String, String)],
    fidelityfx: Option<&Path>,
) -> (BTreeMap<String, String>, bool) {
    let saved = |k: &str| menu.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let series_s = saved("VICTORY_MENU_CONSOLE").map_or(o.series_s, |c| c == "series-s");
    let res = saved("VICTORY_MENU_RES")
        .filter(|r| RESOLUTIONS.contains(r))
        .unwrap_or(&o.res);
    let mut fsr = saved("VICTORY_MENU_FSR")
        .or_else(|| saved("VICTORY_MENU_DLSS"))
        .filter(|d| FSR_MODES.contains(d))
        .unwrap_or(&o.fsr);
    // Series S picture: the scene's size, which with FSR on is its mode (the menu keeps the
    // two matched); not saved yet, it is the FSR mode's.
    let s_res = saved("VICTORY_MENU_S_RES")
        .and_then(|r| S_SIZES.iter().find(|(h, _)| *h == r))
        .or_else(|| S_SIZES.iter().find(|(_, m)| *m == fsr))
        .map_or("720", |(h, _)| *h);
    if series_s
        && fsr != "off"
        && let Some((_, mode)) = S_SIZES.iter().find(|(h, _)| *h == s_res)
    {
        fsr = *mode;
    }
    // 0: off; 2..=6: enabled (FSR 3 outputs 2x).
    let (mut framegen, mut times) = (o.framegen.is_some(), o.framegen.unwrap_or(2));
    if let Some(saved) = saved("VICTORY_MENU_FRAMEGEN") {
        let n = saved.parse::<u32>().ok().filter(|n| (1..=6).contains(n));
        framegen = n.is_some();
        if let Some(n) = n.filter(|n| *n >= 2) {
            times = n;
        }
    }
    let fps = saved("VICTORY_MENU_FPS")
        .and_then(|f| f.parse::<u32>().ok())
        .filter(|f| (1..=1000).contains(f))
        .unwrap_or(o.fps);

    let mut s: BTreeMap<String, String> = SETTINGS
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    let mut set = |k: &str, v: &str| drop(s.insert(k.to_owned(), v.to_owned()));
    if o.kbm_gta {
        set("VICTORY_KBM_GTA", "1");
    }
    if !series_s {
        match res {
            "1440" => set("VICTORY_NATIVE_1440P", "1"),
            "1080to1440" => set("VICTORY_RENDER_1080P", "1"),
            "1080" => {
                set("VICTORY_OUTPUT", "1920x1080");
                set("VICTORY_DISPLAY_MODE", "1920x1080");
            }
            _ => {}
        }
    }
    let (mut fsr_on, mut framegen_x) = (false, None);
    if fsr != "off"
        && let Some(dir) = fidelityfx
    {
        fsr_on = true;
        set("VICTORY_FSR", "1");
        set("VICTORY_FSR_MODE", fsr);
        set("VICTORY_DLSS_MODE", fsr);
        set("VICTORY_FIDELITYFX_DIR", &dir.to_string_lossy());
        if framegen || (o.framegen_ready && !o.no_framegen_ready) {
            let x = if framegen { times.min(2) } else { 0 };
            framegen_x = Some(x);
            set("VICTORY_FSR_FRAMEGEN", "1");
            set("VICTORY_FRAMEGEN_X", &x.to_string());
            // Frame generation reserves VRAM; keep a margin in the texture pool.
            set("VICTORY_TEXTURE_ROOM_PERCENT", if framegen { "80" } else { "86" });
        }
    }
    set(
        "VICTORY_FRAME_INTERVAL_US",
        &(1_000_000.0 / f64::from(fps)).round().to_string(),
    );
    for (k, v) in menu.iter().filter(|(k, _)| !k.starts_with("VICTORY_MENU_")) {
        if v.is_empty() {
            s.remove(k);
        } else {
            s.insert(k.clone(), v.clone());
        }
    }
    for (k, v) in o.more.iter().filter_map(|m| m.split_once('=')) {
        s.insert(k.trim().to_owned(), v.trim().to_owned());
    }
    // For the menu: what it shows as the current choices.
    let mut set = |k: &str, v: &str| drop(s.insert(k.to_owned(), v.to_owned()));
    set("VICTORY_MENU_CONSOLE", if series_s { "series-s" } else { "series-x" });
    set("VICTORY_MENU_RES", res);
    set("VICTORY_MENU_S_RES", if fsr_on { s_res } else { "720" });
    set("VICTORY_MENU_FSR", if fsr_on { fsr } else { "off" });
    set("VICTORY_MENU_FRAMEGEN", &framegen_x.unwrap_or(0).to_string());
    set("VICTORY_MENU_FPS", &fps.to_string());
    (s, series_s)
}

/// `year-month-day-hour-minute-second` (UTC) of `seconds` since 1970.
fn stamp(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (day, month) = (
        doy - (153 * mp + 2) / 5 + 1,
        if mp < 10 { mp + 3 } else { mp - 9 },
    );
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// A new log file's name under `logs`, the ten newest of ours kept.
fn log_file(logs: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(logs).with_context(|| format!("creating {}", logs.display()))?;
    let mut old: Vec<PathBuf> = std::fs::read_dir(logs)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let s = n.to_string_lossy();
                s.starts_with(LOG_PREFIX) || s.starts_with("play-")
            })
        })
        .collect();
    old.sort();
    for gone in old.iter().rev().skip(9) {
        let _ = std::fs::remove_file(gone);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Ok(logs.join(format!("{LOG_PREFIX}{}-{}.log", stamp(now), std::process::id())))
}

/// The start of the built-in launcher's log names.
const LOG_PREFIX: &str = "victory-amd-play-";

/// Runs the AMD fetcher: AMD's FidelityFX SDK DLLs from AMD's GitHub into `to`.
fn fetch_fsr(to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("creating {}", to.display()))?;
    let script = to.join("get-amd-fsr.ps1");
    std::fs::write(&script, FETCH_FSR).with_context(|| format!("writing {}", script.display()))?;
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-To")
        .arg(to)
        .status()
        .context("starting Windows PowerShell")?;
    let _ = std::fs::remove_file(&script);
    anyhow::ensure!(
        status.success() && has_fsr(to),
        "AMD FidelityFX files did not arrive (see above); the title runs with its own upscaling and without frame generation"
    );
    Ok(())
}

/// `victory get-fsr`.
pub fn get_fsr() -> Result<()> {
    let to = amd_home()?;
    let _ = std::fs::remove_file(to.join("declined"));
    fetch_fsr(&to)
}

/// Asks once, at a start without AMD's FidelityFX files, whether to fetch them.
fn offer_fsr(picked: bool) {
    let Ok(to) = amd_home() else { return };
    if picked {
        println!("FSR was picked in the F1 menu: fetching AMD FidelityFX SDK files from");
        println!("github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK (about 87 MB, once).");
        let _ = std::fs::remove_file(to.join("declined"));
        if let Err(e) = fetch_fsr(&to) {
            println!("{e:#}");
        }
        return;
    }
    let declined = to.join("declined");
    if declined.exists() || !std::io::stdin().is_terminal() {
        println!(
            "No AMD FidelityFX files: running with the title's own upscaling (`victory get-fsr` fetches them)."
        );
        return;
    }
    println!("AMD FidelityFX files (FSR 4 / 3.1 upscaling + FSR 3 frame generation) are not on this PC yet.");
    println!("They are AMD's own, under AMD's licence, and come from github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK");
    println!("(about 87 MB, once).");
    print!("Download them now? [Y/n] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    if answer.trim().to_ascii_lowercase().starts_with('n') {
        let _ = std::fs::create_dir_all(&to);
        let _ = std::fs::write(&declined, "asked once; `victory get-fsr` fetches them\r\n");
        println!("Not downloaded. `victory get-fsr` does it later; the title runs with its own upscaling.");
    } else if let Err(e) = fetch_fsr(&to) {
        println!("{e:#}");
    }
}

/// `D3D12` beside this program: where Windows looks for Microsoft's newer
/// Direct3D 12 runtime (`D3D12SDKPath`).
fn d3d12_dir() -> Result<PathBuf> {
    let program = std::env::current_exe().context("this program's own path")?;
    Ok(program
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join("D3D12"))
}

/// The highest shader model the default GPU runs, asked by a process of our
/// own (`victory shader-model`).
fn shader_model() -> Option<(u32, u32)> {
    let program = std::env::current_exe().ok()?;
    let out = std::process::Command::new(program)
        .arg("shader-model")
        .env_remove("VICTORY_LOG_FILE")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (major, minor) = text.lines().rev().find_map(|l| l.trim().split_once('.'))?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Runs the fetcher: Microsoft's `D3D12Core.dll` from nuget.org into `to`.
fn fetch_d3d12(to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("creating {} (beside executable)", to.display()))?;
    let script = std::env::temp_dir().join(format!("victory-get-d3d12-{}.ps1", std::process::id()));
    std::fs::write(&script, FETCH_D3D12).with_context(|| format!("writing {}", script.display()))?;
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-To")
        .arg(to)
        .status()
        .context("starting Windows PowerShell")?;
    let _ = std::fs::remove_file(&script);
    anyhow::ensure!(
        status.success() && to.join("D3D12Core.dll").is_file(),
        "Microsoft's Direct3D 12 runtime did not arrive (see above)"
    );
    Ok(())
}

/// `victory get-d3d12`.
pub fn get_d3d12() -> Result<()> {
    fetch_d3d12(&d3d12_dir()?)
}

/// Makes sure the GPU runs shader model 6.6 before starting the title.
fn check_shader_model() -> Result<()> {
    let needed = victory_hle::streamline::SHADER_MODEL_NEEDED;
    let Some(found) = shader_model() else { return Ok(()) };
    let to = d3d12_dir()?;
    let had = to.join("D3D12Core.dll").is_file();
    let windows_10 = !had && crate::windows_build().is_some_and(|build| build < 22000);
    if found >= needed && !windows_10 {
        return Ok(());
    }
    if !had {
        if found < needed {
            println!(
                "This PC's Direct3D 12 runs shader model {}.{}, and the game's translated shaders need 6.6.",
                found.0, found.1
            );
            println!("Windows 10's own Direct3D 12 stops at 6.5; Microsoft's newer runtime (the D3D12 Agility SDK) runs 6.6.");
        } else {
            println!("This is Windows 10: Microsoft's newer D3D12 Agility SDK runtime is recommended for shader model 6.6 root signatures.");
        }
        println!("It is Microsoft's own, from nuget.org (about 22 MB, once), into {}.", to.display());
        let yes = if std::io::stdin().is_terminal() {
            print!("Download it now? [Y/n] ");
            let _ = std::io::stdout().flush();
            let mut answer = String::new();
            let _ = std::io::stdin().read_line(&mut answer);
            !answer.trim().to_ascii_lowercase().starts_with('n')
        } else {
            true
        };
        if yes {
            match fetch_d3d12(&to) {
                Ok(()) => {
                    crate::match_d3d12_core();
                    if let Some(now) = shader_model()
                        && now >= needed
                    {
                        println!(
                            "Shader model {}.{} with Microsoft's runtime. Starting the game.",
                            now.0, now.1
                        );
                        return Ok(());
                    }
                }
                Err(e) => println!("{e:#}"),
            }
        }
    }
    if found >= needed {
        return Ok(());
    }
    let now = shader_model().unwrap_or(found);
    anyhow::bail!(
        "this PC's Direct3D 12 runs shader model {}.{}, and the game needs 6.6: `victory get-d3d12` fetches Microsoft's newer runtime",
        now.0,
        now.1
    )
}

/// DLLs beside this program under Windows DLL names (for diagnostics/logging).
fn mod_dlls(here: &Path) -> Vec<String> {
    const NAMES: [&str; 7] = [
        "winmm.dll",
        "version.dll",
        "dxgi.dll",
        "d3d12.dll",
        "xinput1_4.dll",
        "dinput8.dll",
        "winhttp.dll",
    ];
    NAMES
        .iter()
        .filter(|n| here.join(n).is_file())
        .map(|n| (*n).to_owned())
        .collect()
}

/// What a start needs once its settings are in the environment.
pub struct Ready {
    pub exe: PathBuf,
    pub series_s: bool,
}

/// Finds the title, puts the settings into the environment and names the
/// log file (`VICTORY_LOG_FILE`). Called before logging is set up.
pub fn prepare(o: &Play) -> Result<Ready> {
    let program = std::env::current_exe().context("this program's own path")?;
    let here = program.parent().map(Path::to_path_buf).unwrap_or_default();
    let exe = match &o.exe {
        Some(exe) => exe.clone(),
        None => [here.join(GAME), PathBuf::from(GAME)]
            .into_iter()
            .find(|p| p.is_file())
            .with_context(|| {
                format!(
                    "no {GAME} beside this program ({}): put the program into your own dump's folder, \
                     or name the title's executable (`Play-Victory-AMD <path to {GAME}>`)",
                    here.display()
                )
            })?,
    };
    anyhow::ensure!(exe.is_file(), "no {}", exe.display());
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let exe = PathBuf::from(exe.to_string_lossy().trim_start_matches(r"\\?\").to_owned());

    check_shader_model()?;

    let home = home()?;
    std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;
    let menu_file = home.join("victory-settings.txt");
    let menu = parse_menu(&std::fs::read_to_string(&menu_file).unwrap_or_default());
    let picked = menu
        .iter()
        .rev()
        .find(|(k, _)| k == "VICTORY_MENU_FSR" || k == "VICTORY_MENU_DLSS")
        .map(|(_, v)| v.as_str());
    if (picked.unwrap_or(o.fsr.as_str()) != "off" || o.framegen.is_some() || o.framegen_ready)
        && fidelityfx_dir(&here).is_none()
    {
        offer_fsr(picked.is_some_and(|d| FSR_MODES.contains(&d)));
    }
    let fidelityfx = fidelityfx_dir(&here);
    let (mut s, series_s) = settings(o, &menu, fidelityfx.as_deref());
    let mods = mod_dlls(&here);

    s.insert(
        "VICTORY_MENU_FILE".to_owned(),
        menu_file.to_string_lossy().into_owned(),
    );
    s.insert(
        "VICTORY_MENU_PLAY".to_owned(),
        program.to_string_lossy().into_owned(),
    );
    s.insert("VICTORY_MENU_ARGS".to_owned(), restart_args(o, &exe));
    let log = log_file(&home.join("logs"))?;
    let console = if series_s { "series-s" } else { "series-x" };
    let line: Vec<String> = s.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mods_line = if mods.is_empty() {
        String::new()
    } else {
        println!(
            "Beside this program, loaded in its place by Windows: {}.",
            mods.join(", ")
        );
        format!("# beside executable: {}\n", mods.join(" "))
    };
    std::fs::write(
        &log,
        format!(
            "# console={console} build=release switches: {}\n{mods_line}",
            line.join(" ")
        ),
    )
    .with_context(|| format!("writing {}", log.display()))?;
    println!("Log: {}", log.display());
    s.insert("VICTORY_LOG_FILE".to_owned(), log.to_string_lossy().into_owned());

    let leftover: Vec<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| k.starts_with("VICTORY_") && !KEPT.contains(&k.as_str()))
        .collect();
    // SAFETY: called before spawning title or GPU worker threads.
    unsafe {
        for k in leftover {
            std::env::remove_var(k);
        }
        for (k, v) in &s {
            std::env::set_var(k, v);
        }
    }
    Ok(Ready { exe, series_s })
}

/// A PowerShell word holding `text` as it is.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The arguments the F1 menu's restarts start the title again with.
fn restart_args(o: &Play, exe: &Path) -> String {
    let mut args = vec!["play".to_owned(), quoted(&exe.to_string_lossy())];
    if o.series_s {
        args.push("--series-s".to_owned());
    }
    args.extend([
        "--res".to_owned(),
        quoted(&o.res),
        "--fsr".to_owned(),
        quoted(&o.fsr),
    ]);
    if let Some(x) = o.framegen {
        args.extend(["--framegen".to_owned(), x.to_string()]);
    }
    if o.framegen_ready && !o.no_framegen_ready {
        args.push("--framegen-ready".to_owned());
    }
    if o.kbm_gta {
        args.push("--kbm-gta".to_owned());
    }
    args.extend(["--fps".to_owned(), o.fps.to_string()]);
    for more in &o.more {
        args.extend(["--set".to_owned(), quoted(more)]);
    }
    args.join(" ")
}

/// After a start that went wrong in a window of its own (a double click):
/// the text stays until a key is pressed.
pub fn hold_window() {
    if std::io::stdin().is_terminal() {
        print!("Press Enter to close this window.");
        let _ = std::io::stdout().flush();
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Play {
        Play {
            exe: None,
            series_s: false,
            res: "1440".to_owned(),
            fsr: "quality".to_owned(),
            framegen: None,
            framegen_ready: false,
            no_framegen_ready: false,
            fps: 1000,
            kbm_gta: false,
            more: Vec::new(),
            preload: false,
        }
    }

    #[test]
    fn a_restart_keeps_the_options_of_the_command_line() {
        use clap::Parser;
        #[derive(clap::Parser)]
        struct Line {
            #[command(flatten)]
            play: Play,
        }
        let exe = Path::new(r"C:\Games\GTA VI's\game_scarlett_master.exe");
        let first = [
            "x",
            "--series-s",
            "--fsr",
            "performance",
            "--framegen",
            "2",
            "--kbm-gta",
            "--set",
            "VICTORY_X=a b",
            "--preload",
        ];
        let o = Line::try_parse_from(first).unwrap().play;
        let args = restart_args(&o, exe);
        assert_eq!(
            args,
            r"play 'C:\Games\GTA VI''s\game_scarlett_master.exe' --series-s --res '1440' --fsr 'performance' --framegen 2 --kbm-gta --fps 1000 --set 'VICTORY_X=a b'"
        );
        let mut words: Vec<String> = Vec::new();
        let (mut word, mut open, mut chars) = (String::new(), false, args.chars().peekable());
        while let Some(c) = chars.next() {
            match c {
                '\'' if open && chars.peek() == Some(&'\'') => {
                    word.push('\'');
                    chars.next();
                }
                '\'' => open = !open,
                ' ' if !open => words.push(std::mem::take(&mut word)),
                _ => word.push(c),
            }
        }
        words.push(word);
        assert_eq!(words[1], exe.to_string_lossy());
        let again = Line::try_parse_from(std::iter::once("x".to_owned()).chain(words.into_iter().skip(2)))
            .unwrap()
            .play;
        assert_eq!(restart_args(&again, exe), args);
        assert!(o.preload && !again.preload);
        let plain = Line::try_parse_from(["x"]).unwrap().play;
        assert_eq!(
            restart_args(&plain, exe),
            r"play 'C:\Games\GTA VI''s\game_scarlett_master.exe' --res '1440' --fsr 'quality' --fps 1000"
        );
    }

    #[test]
    fn a_plain_start_is_the_launchers() {
        let dir = Path::new(r"C:\amd-sdk");
        let (s, series_s) = settings(&options(), &[], Some(dir));
        assert!(!series_s);
        for (k, v) in SETTINGS {
            assert_eq!(s.get(*k).map(String::as_str), Some(*v), "{k}");
        }
        let get = |k: &str| s.get(k).map(String::as_str);
        assert_eq!(get("VICTORY_NATIVE_1440P"), Some("1"));
        assert_eq!(
            (get("VICTORY_FSR"), get("VICTORY_FSR_MODE"), get("VICTORY_DLSS_MODE")),
            (Some("1"), Some("quality"), Some("quality"))
        );
        assert_eq!(get("VICTORY_FIDELITYFX_DIR"), Some(r"C:\amd-sdk"));
        assert!(!s.contains_key("VICTORY_FSR_FRAMEGEN") && !s.contains_key("VICTORY_FRAMEGEN_X"));
        assert!(!s.contains_key("VICTORY_TEXTURE_ROOM_PERCENT"));

        // ready, off, with a little memory held back for it, when asked
        let mut o = options();
        o.framegen_ready = true;
        let (s, _) = settings(&o, &[], Some(dir));
        let get = |k: &str| s.get(k).map(String::as_str);
        assert_eq!(
            (get("VICTORY_FSR_FRAMEGEN"), get("VICTORY_FRAMEGEN_X")),
            (Some("1"), Some("0"))
        );
        assert_eq!(get("VICTORY_TEXTURE_ROOM_PERCENT"), Some("86"));
        assert_eq!(get("VICTORY_FRAME_INTERVAL_US"), Some("1000"));
        assert_eq!(
            (get("VICTORY_MENU_FSR"), get("VICTORY_MENU_FRAMEGEN")),
            (Some("quality"), Some("0"))
        );
        assert_eq!(
            (get("VICTORY_MENU_CONSOLE"), get("VICTORY_MENU_FPS")),
            (Some("series-x"), Some("1000"))
        );
    }

    #[test]
    fn without_amd_files_there_is_no_fsr_or_framegen() {
        let (s, _) = settings(&options(), &[], None);
        let get = |k: &str| s.get(k).map(String::as_str);
        assert!(!s.contains_key("VICTORY_FSR") && !s.contains_key("VICTORY_FSR_FRAMEGEN"));
        assert_eq!(get("VICTORY_MENU_FSR"), Some("off"));
        assert!(!s.contains_key("VICTORY_FRAMEGEN_X") && !s.contains_key("VICTORY_TEXTURE_ROOM_PERCENT"));
        let mut o = options();
        o.framegen_ready = true;
        let (s, _) = settings(&o, &[], None);
        assert!(!s.contains_key("VICTORY_FSR_FRAMEGEN") && !s.contains_key("VICTORY_FRAMEGEN_X"));
    }

    #[test]
    fn framegen_and_1080p_options_are_applied() {
        let mut o = options();
        o.res = "1080".to_owned();
        o.framegen = Some(2);
        o.kbm_gta = true;
        o.fps = 60;
        let (s, _) = settings(&o, &[], Some(Path::new("x")));
        let get = |k: &str| s.get(k).map(String::as_str);
        assert_eq!(
            (get("VICTORY_OUTPUT"), get("VICTORY_DISPLAY_MODE"), get("VICTORY_NATIVE_1440P")),
            (Some("1920x1080"), Some("1920x1080"), None)
        );
        assert_eq!(
            (get("VICTORY_FSR"), get("VICTORY_FSR_FRAMEGEN"), get("VICTORY_FRAMEGEN_X")),
            (Some("1"), Some("1"), Some("2"))
        );
        assert_eq!(get("VICTORY_TEXTURE_ROOM_PERCENT"), Some("80"));
        assert_eq!(get("VICTORY_KBM_GTA"), Some("1"));
        assert_eq!(get("VICTORY_FRAME_INTERVAL_US"), Some("16667"));
    }

    #[test]
    fn what_the_menu_saved_goes_over_the_options() {
        let menu = parse_menu(
            "# Saved by the F1 menu\r\nVICTORY_MENU_FRAMEGEN=2\r\nVICTORY_MENU_CONSOLE=series-s\r\n\
             VICTORY_MENU_FSR=performance\r\nVICTORY_MENU_FPS=60\r\nVICTORY_BVH_BUILDER=\r\n\
             VICTORY_AMBIENT=0.5\r\nVICTORY_EARLY_DEPTH=\r\nnot a setting=1\r\n",
        );
        let (s, series_s) = settings(&options(), &menu, Some(Path::new("x")));
        let get = |k: &str| s.get(k).map(String::as_str);
        assert!(series_s);
        assert_eq!(get("VICTORY_NATIVE_1440P"), None);
        assert_eq!(
            (get("VICTORY_FSR_MODE"), get("VICTORY_FRAMEGEN_X")),
            (Some("performance"), Some("2"))
        );
        assert_eq!(get("VICTORY_TEXTURE_ROOM_PERCENT"), Some("80"));
        assert_eq!(get("VICTORY_FRAME_INTERVAL_US"), Some("16667"));
        assert_eq!((get("VICTORY_AMBIENT"), get("VICTORY_EARLY_DEPTH")), (Some("0.5"), None));
        assert_eq!(get("VICTORY_BVH_BUILDER"), None);
        assert_eq!(get("VICTORY_MENU_FRAMEGEN"), Some("2"));
    }

    #[test]
    fn the_series_s_picture_sets_the_scene() {
        let get = |s: &BTreeMap<String, String>, k: &str| s.get(k).cloned();
        let s_menu = |more: &str| parse_menu(&format!("VICTORY_MENU_CONSOLE=series-s\r\n{more}"));
        let (s, _) = settings(&options(), &s_menu("VICTORY_MENU_FSR=balanced\r\n"), Some(Path::new("x")));
        assert_eq!(get(&s, "VICTORY_MENU_S_RES").as_deref(), Some("626"));
        assert_eq!(get(&s, "VICTORY_FSR_MODE").as_deref(), Some("balanced"));
        let (s, _) = settings(
            &options(),
            &s_menu("VICTORY_MENU_FSR=quality\r\nVICTORY_MENU_S_RES=540\r\n"),
            Some(Path::new("x")),
        );
        assert_eq!(get(&s, "VICTORY_FSR_MODE").as_deref(), Some("performance"));
        assert_eq!(get(&s, "VICTORY_MENU_FSR").as_deref(), Some("performance"));
        let (s, _) = settings(&options(), &s_menu("VICTORY_MENU_FSR=dlaa\r\n"), Some(Path::new("x")));
        assert_eq!(
            (get(&s, "VICTORY_FSR_MODE").as_deref(), get(&s, "VICTORY_MENU_S_RES").as_deref()),
            (Some("dlaa"), Some("1080"))
        );
        for (more, dir) in [
            ("VICTORY_MENU_FSR=off\r\nVICTORY_MENU_S_RES=540\r\n", Some(Path::new("x"))),
            ("VICTORY_MENU_S_RES=540\r\n", None),
        ] {
            let (s, _) = settings(&options(), &s_menu(more), dir);
            assert_eq!((get(&s, "VICTORY_FSR"), get(&s, "VICTORY_FSR_MODE")), (None, None));
            assert_eq!(get(&s, "VICTORY_MENU_S_RES").as_deref(), Some("720"));
        }
        let (s, series_s) = settings(&options(), &parse_menu("VICTORY_MENU_S_RES=540\r\n"), Some(Path::new("x")));
        assert!(!series_s);
        assert_eq!(get(&s, "VICTORY_FSR_MODE").as_deref(), Some("quality"));
    }

    #[test]
    fn paths_and_the_named_card_are_kept() {
        for k in ["VICTORY_HOME", "VICTORY_ADAPTER", "VICTORY_CACHE_DIR", "VICTORY_DATA_DIR", "VICTORY_FIDELITYFX_DIR"] {
            assert!(KEPT.contains(&k), "{k}");
        }
    }

    #[test]
    fn stamps_are_dates() {
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(stamp(1_795_753_200), "20261127-042000");
        assert_eq!(stamp(951_782_400), "20000229-000000");
    }
}
