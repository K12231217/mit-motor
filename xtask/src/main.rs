//! Repo entry point: `cargo xtask <cmd>`.
//!
//! Every build / check / size / flash action lives here so CLion, VS Code and
//! CI execute the same code path. No extra tools (just, make) are required.
//!
//! Commands: check build rel fmt fmt-check clippy size flash rtt help

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const FW: &str = "firmware";
const ELF_REL: &str = "target/thumbv7em-none-eabihf/release/blinky";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".to_string());
    let rest: Vec<String> = args.collect();

    let result = match cmd.as_str() {
        "check" => cargo(&["check"]),
        "build" => cargo(&["build"]),
        "rel" | "release" => cargo(&["build", "--release"]),
        "fmt" => cargo(&["fmt", "--all"]),
        "fmt-check" => cargo(&["fmt", "--all", "--", "--check"]),
        // `--all-targets` pulls in the test harness, which needs std on a
        // no_std thumb target; the firmware has one bin and no lib.
        "clippy" => cargo(&["clippy", "--bins", "--", "-D", "warnings"]),
        "size" => size(),
        // `probe-rs run` comes from firmware/.cargo/config.toml (single source).
        "flash" => cargo(&["run", "--release"]),
        "rtt" => rtt(&rest),
        "help" | "-h" | "--help" => {
            help();
            Ok(())
        }
        other => {
            eprintln!("xtask: unknown command `{other}`");
            help();
            Err(String::new())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("xtask: {msg}");
            }
            ExitCode::FAILURE
        }
    }
}

fn help() {
    println!(
        "\
usage: cargo xtask <command> [args]

  check       cargo check (dev profile)
  build       cargo build (dev profile)
  rel         cargo build --release           (the acceptance profile)
  fmt         cargo fmt --all
  fmt-check   cargo fmt --all -- --check      (CI)
  clippy      cargo clippy --all-targets -- -D warnings
  size        build --release, then print ELF sections and flash image
              against the FLASH length in firmware/memory.x
  flash       cargo run --release (probe-rs runner from .cargo/config.toml)
  rtt         probe-rs attach --chip <chip from .cargo/config.toml> [args]"
    );
}

fn cargo(args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(FW).args(args);
    run(&mut cmd)
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let status = cmd
        .status()
        .map_err(|e| format!("cannot run `{program}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{program}` failed ({status})"))
    }
}

fn size() -> Result<(), String> {
    cargo(&["build", "--release"])?;

    let elf = Path::new(FW).join(ELF_REL);
    let elf = elf
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 path: {}", elf.display()))?;

    let host = host_triple()?;
    let sysroot = rustc_output(&["--print", "sysroot"])?;
    let bin = PathBuf::from(sysroot.trim())
        .join("lib")
        .join("rustlib")
        .join(host.trim())
        .join("bin");

    let llvm_size = tool(&bin, "llvm-size")?;
    let objcopy = tool(&bin, "llvm-objcopy")?;

    let out = Command::new(&llvm_size)
        .args(["-A", elf])
        .output()
        .map_err(|e| format!("cannot run {}: {e}", llvm_size.display()))?;
    if !out.status.success() {
        return Err("llvm-size failed".to_string());
    }

    let mut sections: BTreeMap<String, u64> = BTreeMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut it = line.split_whitespace();
        let (Some(name), Some(size)) = (it.next(), it.next()) else {
            continue;
        };
        if matches!(name, ".text" | ".rodata" | ".data" | ".bss") {
            if let Ok(n) = size.parse::<u64>() {
                sections.insert(name.to_string(), n);
            }
        }
    }

    let tmp: PathBuf = std::env::temp_dir().join("mit-motor-flash.bin");
    run(Command::new(&objcopy)
        .args(["-O", "binary"])
        .arg(elf)
        .arg(&tmp))?;
    let image = std::fs::metadata(&tmp)
        .map_err(|e| format!("cannot stat {}: {e}", tmp.display()))?
        .len();
    let _ = std::fs::remove_file(&tmp);

    let flash = flash_len()?;
    let pct = |n: u64| 100.0 * n as f64 / flash as f64;

    println!("flash region   {:>12} B  ({})", group(flash), kib(flash));
    println!(
        "flash image    {:>12} B  ({:.1} % used, {} left)",
        group(image),
        pct(image),
        group(flash.saturating_sub(image))
    );
    for (name, n) in &sections {
        println!("  {name:<8}   {:>12} B", group(*n));
    }
    if image > flash {
        return Err(format!(
            "image exceeds the {} region by {} B",
            kib(flash),
            group(image - flash)
        ));
    }
    Ok(())
}

fn flash_len() -> Result<u64, String> {
    let path = Path::new(FW).join("memory.x");
    let src = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    for line in src.lines() {
        if !line.trim_start().starts_with("FLASH") {
            continue;
        }
        let Some(idx) = line.find("LENGTH") else {
            continue;
        };
        let digits: String = line[idx..]
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(k) = digits.parse::<u64>() {
            return Ok(k * 1024);
        }
    }
    Err("no FLASH ... LENGTH = <n>K line in firmware/memory.x".to_string())
}

fn rtt(rest: &[String]) -> Result<(), String> {
    cargo(&["build", "--release"])?;
    let chip = chip_name()?;
    // probe-rs attach wants the ELF (for defmt symbol decoding), and paths are
    // resolved from the firmware dir, same as the `flash` runner.
    let mut cmd = Command::new("probe-rs");
    cmd.current_dir(FW)
        .arg("attach")
        .args(["--chip", &chip])
        .arg("target/thumbv7em-none-eabihf/release/blinky")
        .args(rest);
    run(&mut cmd)
}

/// Reads the probe-rs chip name out of the firmware runner, so the chip name
/// exists in exactly one place.
fn chip_name() -> Result<String, String> {
    let path = Path::new(FW).join(".cargo").join("config.toml");
    let src = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    for line in src.lines() {
        if !line.trim_start().starts_with("runner") {
            continue;
        }
        if let Some(i) = line.find("--chip ") {
            if let Some(name) = line[i + 7..].split_whitespace().next() {
                return Ok(name.to_string());
            }
        }
    }
    Err(format!("no `--chip <name>` in {}", path.display()))
}

fn rustc_output(args: &[&str]) -> Result<String, String> {
    let out = Command::new("rustc")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run rustc: {e}"))?;
    if !out.status.success() {
        return Err("rustc failed".to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn host_triple() -> Result<String, String> {
    let verbose = rustc_output(&["-vV"])?;
    verbose
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_string)
        .ok_or_else(|| "rustc -vV has no host line".to_string())
}

fn tool(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let plain = dir.join(name);
    if plain.is_file() {
        return Ok(plain);
    }
    let exe = dir.join(format!("{name}.exe"));
    if exe.is_file() {
        return Ok(exe);
    }
    Err(format!(
        "{} not found (needs the `llvm-tools` component: rustup component add llvm-tools)",
        plain.display()
    ))
}

fn kib(n: u64) -> String {
    format!("{} KiB", n / 1024)
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
