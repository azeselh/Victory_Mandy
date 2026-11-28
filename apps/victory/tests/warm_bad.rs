#![cfg(windows)]

use std::fs;
use std::path::Path;
use std::process::Command;

fn run_warm(root: &Path, cache: &Path, args: &[&str]) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_victory"));
    command
        .current_dir(root)
        .env_remove("VICTORY_CONFIG")
        .env_remove("VICTORY_WAVE_WIDE")
        .env("VICTORY_CACHE_DIR", cache)
        .env("VICTORY_DATA_DIR", root.join("data"))
        .env("VICTORY_D3D12_ADAPTER", u32::MAX.to_string())
        .env("VICTORY_D3D12_WARP", "0")
        .args(["warm", "--jobs", "1"])
        .args(args);
    let output = command.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn parse_fingerprint(stdout: &str) -> String {
    let (_, after) = stdout
        .split_once("made before (")
        .unwrap_or_else(|| panic!("missing fingerprint prefix in:\n{stdout}"));
    let (fp, _) = after
        .split_once(',')
        .unwrap_or_else(|| panic!("missing fingerprint comma in:\n{stdout}"));
    assert_eq!(fp.len(), 16, "unexpected fingerprint {fp:?} in:\n{stdout}");
    assert!(
        fp.bytes().all(|b| b.is_ascii_hexdigit()),
        "non-hex fingerprint {fp:?}"
    );
    fp.to_owned()
}

#[test]
fn warm_skips_bad_entries_and_reports_fingerprint() {
    let help = Command::new(env!("CARGO_BIN_EXE_victory"))
        .args(["warm", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success(), "{help:?}");

    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    let pipelines = cache.join("pipelines");
    fs::create_dir_all(&pipelines).unwrap();

    let ids = [0x11u8, 0x22, 0x33, 0x44];
    let keys = ids.map(|id| format!("{id:02x}").repeat(16));
    let mut book = Vec::new();
    for id in ids {
        book.extend(16u32.to_le_bytes());
        book.extend([id; 16]);
    }
    fs::write(pipelines.join("known-pipelines.bin"), book).unwrap();
    fs::write(pipelines.join("known-shaders.bin"), b"").unwrap();
    fs::write(pipelines.join("early-pipelines.txt"), format!("{}\n{}\n", keys[0], keys[1])).unwrap();

    let probe = run_warm(root.path(), &cache, &["--limit", "0"]);
    let fp = parse_fingerprint(&probe);

    fs::write(
        pipelines.join("warm-bad.txt"),
        format!("{} exit code: 0xc0000005\n{} hung\n", keys[0], keys[2]),
    )
    .unwrap();

    let out = run_warm(root.path(), &cache, &[]);
    assert!(
        out.contains(&format!("4 pipelines known, 0 made before ({fp}")),
        "{out}"
    );
    assert!(out.contains("2 bad; 2 to make"), "{out}");
    assert!(out.contains("2 have no host pipeline"), "{out}");

    fs::write(
        pipelines.join("warm-bad.txt"),
        format!("{} exit code: 0xc0000005\n", keys[0]),
    )
    .unwrap();
    let updated = run_warm(root.path(), &cache, &[]);
    assert!(updated.contains("1 bad; 3 to make"), "{updated}");
}
