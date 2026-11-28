//! Shared command-line front end for `victory` and `Play-Victory-AMD`.

pub mod census;
pub mod inspect;
#[cfg(windows)]
pub mod play;
#[cfg(all(windows, feature = "diagnostics"))]
pub mod replay;
pub mod run;
pub mod shader;
pub mod translate;
#[cfg(windows)]
pub mod warm;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

#[cfg(windows)]
#[unsafe(no_mangle)]
#[used]
#[allow(non_upper_case_globals)]
pub static D3D12SDKVersion: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(614);

fn agility_version(dll: &[u8]) -> Option<u32> {
    let signature = 0xFEEF_04BD_u32.to_le_bytes();
    let at = dll.windows(4).position(|w| w == signature)?;
    let most = u32::from_le_bytes(dll.get(at + 8..at + 12)?.try_into().ok()?);
    let (major, minor) = (most >> 16, most & 0xffff);
    (major == 1 && minor >= 600).then_some(minor)
}

fn exported_sdk_version(dll: &[u8]) -> Option<u32> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(dll.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(dll.get(at..at + 4)?.try_into().ok()?));
    let pe = u32_at(0x3c)? as usize;
    (dll.get(pe..pe + 4)? == b"PE\0\0").then_some(())?;
    let sections = usize::from(u16_at(pe + 6)?);
    let optional = pe + 24;
    let table = optional + usize::from(u16_at(pe + 20)?);
    (u16_at(optional)? == 0x20b).then_some(())?;
    let offset = |rva: u32| -> Option<usize> {
        (0..sections).find_map(|i| {
            let s = table + i * 40;
            let (size, va, raw_size, raw) = (
                u32_at(s + 8)?,
                u32_at(s + 12)?,
                u32_at(s + 16)?,
                u32_at(s + 20)?,
            );
            (va <= rva && rva < va + size.max(raw_size)).then(|| (rva - va + raw) as usize)
        })
    };
    let exports = offset(u32_at(optional + 112)?)?;
    let count = u32_at(exports + 24)? as usize;
    let functions_at = offset(u32_at(exports + 28)?)?;
    let names_at = offset(u32_at(exports + 32)?)?;
    let ordinals_at = offset(u32_at(exports + 36)?)?;
    (0..count).find_map(|i| {
        let name = offset(u32_at(names_at + 4 * i)?)?;
        if dll.get(name..name + 16)? != b"D3D12SDKVersion\0" {
            return None;
        }
        let ordinal = usize::from(u16_at(ordinals_at + 2 * i)?);
        u32_at(offset(u32_at(functions_at + 4 * ordinal)?)?)
    })
}

#[cfg(windows)]
fn os_runtime() -> u32 {
    let system = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"C:\Windows".into());
    std::fs::read(system.join("System32").join("D3D12Core.dll"))
        .ok()
        .and_then(|dll| exported_sdk_version(&dll))
        .map_or(4, |v| v.min(614))
}

#[cfg(windows)]
pub(crate) fn windows_build() -> Option<u32> {
    if let Some(build) = std::env::var("VICTORY_WINDOWS_BUILD")
        .ok()
        .and_then(|v| v.trim().parse().ok())
    {
        return Some(build);
    }
    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfo) -> i32;
    }
    let mut info = OsVersionInfo {
        size: size_of::<OsVersionInfo>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    // SAFETY: `info` is an OSVERSIONINFOW with its size set.
    (unsafe { RtlGetVersion(&mut info) } == 0).then_some(info.build)
}

#[cfg(windows)]
pub fn match_d3d12_core() {
    use std::sync::atomic::Ordering::Relaxed;
    let Some(core) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("D3D12").join("D3D12Core.dll")))
    else {
        return;
    };
    let Ok(bytes) = std::fs::read(&core) else {
        D3D12SDKVersion.store(os_runtime(), Relaxed);
        return;
    };
    match agility_version(&bytes) {
        Some(v) => {
            if v != 614 {
                println!("{} is the D3D12 Agility SDK 1.{v}: asking Windows for that version.", core.display());
            }
            D3D12SDKVersion.store(v, Relaxed);
        }
        None => {
            D3D12SDKVersion.store(os_runtime(), Relaxed);
            println!(
                "{} is not a D3D12 Agility SDK runtime Windows can use: Windows' own is used.",
                core.display()
            );
        }
    }
}

#[cfg(windows)]
#[repr(transparent)]
pub struct SdkPath(*const std::ffi::c_char);
// SAFETY: points at a static NUL-terminated string literal.
#[cfg(windows)]
unsafe impl Sync for SdkPath {}

#[cfg(windows)]
#[unsafe(no_mangle)]
#[used]
#[allow(non_upper_case_globals)]
pub static D3D12SDKPath: SdkPath = SdkPath(c".\\D3D12\\".as_ptr());

#[derive(Parser)]
#[command(name = "victory", version, about = "Run an Xbox Series title natively on Windows (AMD FSR 4 / 3.1 + FSR 3 FrameGen)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Command {
    Inspect {
        file: PathBuf,
        #[arg(long)]
        imports: bool,
        #[arg(long)]
        exports: bool,
    },
    Census {
        paths: Vec<PathBuf>,
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        missing: bool,
    },
    LibExports {
        lib: PathBuf,
        #[arg(long)]
        list: bool,
    },
    ShaderInfo {
        paths: Vec<PathBuf>,
        #[arg(long)]
        verbose: bool,
        #[arg(long)]
        disasm: bool,
    },
    Translate {
        paths: Vec<PathBuf>,
        #[arg(long)]
        validate: bool,
        #[arg(long)]
        pipelines: bool,
        #[arg(long)]
        list: bool,
        #[arg(long, value_parser = parse_jobs)]
        jobs: Option<usize>,
        #[arg(long)]
        warm: Option<PathBuf>,
    },
    Shaders {
        paths: Vec<PathBuf>,
        #[arg(long, value_parser = parse_jobs)]
        jobs: Option<usize>,
    },
    #[cfg(windows)]
    Warm {
        #[arg(long, value_parser = parse_jobs)]
        jobs: Option<usize>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        again: bool,
        #[arg(long)]
        used: bool,
        #[arg(long, hide = true)]
        these: Option<String>,
    },
    #[cfg(windows)]
    Preload {
        paths: Vec<PathBuf>,
        #[arg(long, value_parser = parse_jobs)]
        jobs: Option<usize>,
    },
    #[cfg(windows)]
    Play(play::Play),
    #[cfg(windows)]
    GetFsr,
    #[cfg(windows)]
    GetD3d12,
    #[cfg(windows)]
    #[command(hide = true)]
    ShaderModel,
    Run {
        exe: PathBuf,
        #[arg(long, value_enum, default_value_t = Unimplemented::Trap)]
        unimplemented: Unimplemented,
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        execute: bool,
        #[arg(long, value_enum, default_value_t = Console::SeriesX)]
        console: Console,
        #[arg(last = true)]
        title_args: Vec<String>,
    },
    #[cfg(all(windows, feature = "diagnostics"))]
    Replay(replay::Replay),
    Config,
    #[cfg(windows)]
    UpscalerCheck {
        #[arg(long, default_value = "2560x1440")]
        output: String,
        #[arg(long)]
        device: bool,
        #[arg(long)]
        present: bool,
    },
}

fn parse_jobs(value: &str) -> std::result::Result<usize, &'static str> {
    value
        .parse::<std::num::NonZeroUsize>()
        .map(std::num::NonZeroUsize::get)
        .map_err(|_| "expected a worker count of 1 or more")
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Unimplemented {
    Trap,
    Zero,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Console {
    SeriesX,
    SeriesS,
}

const KNOWN_SUBCOMMANDS: &[&str] = &[
    "inspect",
    "census",
    "lib-exports",
    "shader-info",
    "translate",
    "shaders",
    "warm",
    "preload",
    "play",
    "get-fsr",
    "get-d3d12",
    "shader-model",
    "run",
    "replay",
    "config",
    "upscaler-check",
    "help",
    "-h",
    "--help",
    "-V",
    "--version",
];

pub fn entry(always_play_default: bool) -> Result<()> {
    #[cfg(windows)]
    {
        let _ = std::hint::black_box(&D3D12SDKPath);
        match_d3d12_core();
        victory_hle::reserve_upload_space();
    }
    #[cfg(all(windows, feature = "diagnostics"))]
    if std::env::var_os("VICTORY_AUDIO_CAPTURE").is_some() {
        victory_core::logging::init("info");
        if victory_hle::modules::audio::capture::child() {
            return Ok(());
        }
    }
    let raw: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let bare = raw.len() == 1;
    let first = raw.get(1).and_then(|s| s.to_str());
    let use_play = cfg!(windows)
        && (bare
            || (always_play_default
                && first.is_some_and(|s| !KNOWN_SUBCOMMANDS.contains(&s))));
    let cli = if use_play {
        let mut args = vec![std::ffi::OsString::from("victory"), std::ffi::OsString::from("play")];
        args.extend(raw.into_iter().skip(1));
        Cli::parse_from(args)
    } else {
        Cli::parse()
    };

    #[cfg(windows)]
    if let Command::Play(options) = &cli.command {
        let preload =
            options.preload || std::env::var_os("VICTORY_PRELOAD").is_some_and(|v| v == "1");
        let done = play::prepare(options).and_then(|ready| {
            if preload {
                play::preload(&ready.exe);
            }
            victory_core::logging::init("info");
            let console = if ready.series_s {
                Console::SeriesS
            } else {
                Console::SeriesX
            };
            run::run(&ready.exe, Unimplemented::Trap, false, true, console)
        });
        if let Err(e) = &done
            && bare
        {
            eprintln!("Error: {e:#}");
            play::hold_window();
        }
        return done;
    }

    victory_core::logging::init("info");
    match cli.command {
        Command::Inspect {
            file,
            imports,
            exports,
        } => inspect::run(&file, imports, exports),
        Command::Census {
            paths,
            strict,
            missing,
        } => census::run(paths, strict, missing),
        Command::LibExports { lib, list } => inspect::lib_exports(&lib, list),
        Command::ShaderInfo {
            paths,
            verbose,
            disasm,
        } => shader::run(paths, verbose || disasm, disasm),
        Command::Translate {
            paths,
            validate,
            pipelines,
            list,
            jobs,
            warm,
        } => {
            if let Some(n) = std::env::var("VICTORY_VIEW_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
            {
                victory_shader::emit::set_view_heap_limit(n);
            }
            if let Some(n) = std::env::var("VICTORY_HOST_WAVE")
                .ok()
                .and_then(|v| v.parse().ok())
            {
                victory_shader::emit::set_host_max_wave(n);
            }
            if let Some(file) = warm {
                return translate::warm(paths, &file, jobs);
            }
            translate::run(paths, validate, pipelines, list, jobs)
        }
        Command::Shaders { paths, jobs } => translate::precompile(paths, jobs),
        #[cfg(windows)]
        Command::Warm {
            jobs,
            limit,
            again,
            used,
            these,
        } => warm::run(jobs, limit, again, used, these),
        #[cfg(windows)]
        Command::Preload { paths, jobs } => {
            let jobs = jobs
                .or_else(|| std::env::var("VICTORY_PRELOAD_JOBS").ok()?.trim().parse().ok())
                .filter(|&n| n > 0);
            if let Some(n) = jobs {
                println!("Preloading on {n} threads at a time (VICTORY_PRELOAD_JOBS / --jobs).");
            }
            play::explain_preload();
            translate::precompile(paths, jobs)?;
            warm::run(jobs, None, false, false, None)?;
            warm::into_library(jobs)
        }
        #[cfg(windows)]
        Command::Play(_) => unreachable!("handled above"),
        #[cfg(windows)]
        Command::GetFsr => play::get_fsr(),
        #[cfg(windows)]
        Command::GetD3d12 => play::get_d3d12(),
        #[cfg(windows)]
        Command::ShaderModel => {
            let model = victory_hle::streamline::highest_shader_model();
            if let Some(core) = victory_hle::streamline::d3d12_core_path() {
                println!("runtime {}", core.display());
            }
            match model {
                Some((major, minor)) => println!("{major}.{minor}"),
                None => println!("none"),
            }
            Ok(())
        }
        Command::Run {
            exe,
            unimplemented,
            strict,
            execute,
            console,
            title_args,
        } => {
            victory_hle::runtime::set_title_args(title_args);
            run::run(&exe, unimplemented, strict, execute, console)
        }
        #[cfg(all(windows, feature = "diagnostics"))]
        Command::Replay(options) => replay::run(options),
        Command::Config => show_config(),
        #[cfg(windows)]
        Command::UpscalerCheck {
            output,
            device,
            present,
        } => {
            let size = output
                .split_once(['x', 'X'])
                .and_then(|(w, h)| Some([w.trim().parse().ok()?, h.trim().parse().ok()?]))
                .ok_or_else(|| anyhow::anyhow!("--output {output}: not <width>x<height>"))?;
            for line in victory_hle::streamline::check(size, device || present, present) {
                println!("{line}");
            }
            Ok(())
        }
    }
}

fn show_config() -> Result<()> {
    let cfg = victory_core::Config::load()?;
    let status = |p: &Option<PathBuf>| match p {
        Some(p) if p.exists() => format!("{}", p.display()),
        Some(p) => format!("{}  (does not exist)", p.display()),
        None => "(not set)".to_owned(),
    };
    println!(
        "config file : {}",
        cfg.source
            .as_ref()
            .map_or("(none)".into(), |p| p.display().to_string())
    );
    println!("game_dir    : {}", status(&cfg.game_dir));
    println!("cache_dir   : {}", cfg.cache_dir.display());
    println!("data_dir    : {}", cfg.data_dir.display());
    for t in &cfg.test_titles {
        println!("test_title  : {}", status(&Some(t.clone())));
    }
    for p in cfg
        .game_dir
        .iter()
        .chain(&cfg.test_titles)
        .chain([&cfg.cache_dir, &cfg.data_dir])
    {
        if let Err(e) = victory_core::ensure_outside_repo(p) {
            println!("POLICY VIOLATION: {e}");
        }
    }
    Ok(())
}

/// Paths given on the command line, or the configured game + test titles.
pub(crate) fn default_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    if !paths.is_empty() {
        return Ok(paths);
    }
    let cfg = victory_core::Config::load()?;
    let all: Vec<PathBuf> = cfg.game_dir.into_iter().chain(cfg.test_titles).collect();
    anyhow::ensure!(
        !all.is_empty(),
        "no paths given and none configured (see victory.example.toml)"
    );
    Ok(all)
}

#[cfg(test)]
mod tests {
    fn core_bytes(major: u32, minor: u32) -> Vec<u8> {
        let mut b = vec![0u8; 40];
        for w in [0xFEEF_04BD_u32, 0x0001_0000, (major << 16) | minor, 2 << 16] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        b.extend_from_slice(&[0u8; 16]);
        b
    }

    #[test]
    fn agility_versions_are_read_from_the_file() {
        assert_eq!(super::agility_version(&core_bytes(1, 620)), Some(620));
        assert_eq!(super::agility_version(&core_bytes(1, 614)), Some(614));
        assert_eq!(super::agility_version(&core_bytes(10, 0)), None);
        assert_eq!(super::agility_version(&[0u8; 64]), None);
        assert_eq!(super::agility_version(&core_bytes(1, 620)[..48]), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_own_runtime_tells_its_version() {
        let system = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap();
        if let Ok(dll) = std::fs::read(system.join("System32").join("D3D12Core.dll")) {
            let v = super::exported_sdk_version(&dll)
                .expect("System32's D3D12Core.dll exports D3D12SDKVersion");
            assert_eq!(super::os_runtime(), v.min(614));
        }
        assert_eq!(super::exported_sdk_version(&[0u8; 64]), None);
        assert_eq!(super::exported_sdk_version(b"MZ"), None);
    }
}
