use std::process::Command;

// Derive a CalVer version from the build commit and expose it as SCRY_VERSION.
// No dirty check, so a cargo git checkout never reads as modified.
fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/logs/HEAD");

    let version = git_version().unwrap_or_else(|| "v0.0.0-unknown".to_string());
    println!("cargo:rustc-env=SCRY_VERSION={version}");
}

// Build v{YY}.{M}.{D}-{secondsSinceMidnight}.r{rev8} from the HEAD commit time
// and revision. Returns None when git is unavailable.
fn git_version() -> Option<String> {
    let rev = out(Command::new("git").args(["rev-parse", "HEAD"]))?;
    let time = out(Command::new("git").env("TZ", "UTC0").args([
        "show",
        "-s",
        "--format=%cd",
        "--date=format-local:%Y-%m-%dT%H:%M:%S",
        "HEAD",
    ]))?;

    let year: u32 = time.get(0..4)?.parse().ok()?;
    let month: u32 = time.get(5..7)?.parse().ok()?;
    let day: u32 = time.get(8..10)?.parse().ok()?;
    let hour: u32 = time.get(11..13)?.parse().ok()?;
    let minute: u32 = time.get(14..16)?.parse().ok()?;
    let second: u32 = time.get(17..19)?.parse().ok()?;
    let secs = hour * 3600 + minute * 60 + second;
    let rev8 = rev.get(..8).unwrap_or(&rev);

    Some(format!(
        "v{}.{}.{}-{}.r{}",
        year % 100,
        month,
        day,
        secs,
        rev8
    ))
}

// Run a git command and return its trimmed stdout, or None when it fails.
fn out(cmd: &mut Command) -> Option<String> {
    let o = cmd.output().ok()?;
    if !o.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
