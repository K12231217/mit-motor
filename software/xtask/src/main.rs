//! Repo entry point: `cargo xtask <cmd>`.
//!
//! Every build / check / size / flash action lives here so CLion, VS Code and
//! CI execute the same code path. No extra tools (just, make) are required.
//!
//! Commands: check build rel fmt fmt-check clippy test doc size flash rtt arch
//!           hygiene tier help

mod checks;
mod hygiene;
mod tier;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub(crate) const FW: &str = "software/firmware";

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
        // 根 workspace（算法层）也要过：`-D warnings` 把文档与 clippy 提示
        // 一律升级为错误，测试目标同样拦。
        "clippy" => cargo(&["clippy", "--bins", "--", "-D", "warnings"]).and_then(|()| {
            root_cargo(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ])
        }),
        // 根 workspace（host crate）的回归测试，不是 firmware。
        "test" => root_cargo(&["test", "--workspace"]),
        // 文档门禁：根 workspace 全量 rustdoc，任何警告即失败。
        "doc" => doc_check(),
        "size" => size(),
        // `probe-rs run` comes from software/firmware/.cargo/config.toml (single source).
        "flash" => cargo(&["run", "--release"]),
        "rtt" => rtt(&rest),
        // 架构不变量（docs/software-architecture.md §9）的唯一实现在 checks.rs。
        "arch" if rest.is_empty() => checks::run(),
        "arch" => Err(format!(
            "`arch` takes no arguments (got {})",
            rest.join(" ")
        )),
        // 协作卫生（docs/archive/agent-guardrails.md H2/H4/H5/H6）的唯一实现在 hygiene.rs。
        "hygiene" if rest.is_empty() => hygiene::run(),
        "hygiene" => Err(format!(
            "`hygiene` takes no arguments (got {})",
            rest.join(" ")
        )),
        // 分级门禁（H3）：Tier B 改动无 HIL 证据不得进 main。
        "tier" => tier::run(&rest),
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
  clippy      cargo clippy --bins -- -D warnings   (firmware)
              then cargo clippy --workspace --all-targets -- -D warnings (root)
  test        cargo test --workspace         (root workspace, host)
  doc         cargo doc --no-deps --workspace (RUSTDOCFLAGS=-D warnings)
  size        build --release, then print ELF sections and flash image
              against the FLASH length in software/firmware/memory.x
  flash       cargo run --release (probe-rs runner from .cargo/config.toml)
  rtt         probe-rs attach --chip <chip from .cargo/config.toml> [args]
  arch        architecture invariants, pure Rust (docs/software-
              software-architecture.md §9): I1 dependency direction, I2 unsafe,
              I3 no heap; FAILs if the release ELF is missing
  hygiene     collaboration checks (docs/archive/agent-guardrails.md):
              H2 TODO(owner, trigger), H5 comment markers,
              H4 every module file listed in the architecture doc,
              H6 module registry: unique four-axis ID and owner per file
  tier [base] H3 tier gate: Tier B paths (software/firmware/, software/bsp/, system/,
              app/) changed vs `base` (default origin/main or $TIER_BASE)
              need a `HIL: PASS` line in .scratch/handoff.md"
    );
}

fn cargo(args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(FW).args(args);
    run(&mut cmd)
}

/// Runs cargo in the repo root workspace (contracts + algorithms), not firmware.
fn root_cargo(args: &[&str]) -> Result<(), String> {
    run(Command::new("cargo").args(args))
}

/// `cargo doc` 零警告：rustdoc 警告一律升级为错误。
fn doc_check() -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.args(["doc", "--no-deps", "--workspace"])
        .env("RUSTDOCFLAGS", "-D warnings");
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

/// Release ELF 路径：从 `cargo metadata` 读目标目录、bin 名，从
/// `software/firmware/.cargo/config.toml` 读目标三元组。改包名/文件名不再改 xtask。
pub(crate) fn elf_path() -> Result<PathBuf, String> {
    let manifest = Path::new(FW).join("Cargo.toml");
    let out = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let target_dir = json_string_field(&text, "target_directory")
        .ok_or_else(|| "cargo metadata: no target_directory".to_string())?;
    let bin =
        first_bin_name(&text).ok_or_else(|| "cargo metadata: no [[bin]] target".to_string())?;
    let triple = build_target()?;
    Ok(PathBuf::from(target_dir)
        .join(triple)
        .join("release")
        .join(bin))
}

fn json_string_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":");
    let start = text.find(&needle)? + needle.len();
    let rest = &text[start..];
    let quote = rest.find('"')?;
    let mut out = String::new();
    let mut chars = rest[quote + 1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            _ => out.push(c),
        }
    }
    None
}

/// 找 `"kind":["bin"]` 后面的 `"name"`；`custom-build` 的 `crate_types` 也含
/// `"bin"`，所以先检查 `kind` 与 `"bin"` 之间只有 JSON 标点。
fn first_bin_name(text: &str) -> Option<String> {
    let mut rest = text;
    while let Some(i) = rest.find("\"kind\"") {
        let after = &rest[i + "\"kind\"".len()..];
        let Some(bin) = after.find("\"bin\"") else {
            rest = after;
            continue;
        };
        let between = &after[..bin];
        if between
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '"' | '[' | ':' | ',' | ']'))
        {
            let tail = &after[bin + "\"bin\"".len()..];
            if let Some(name) = json_string_field(tail, "name") {
                return Some(name);
            }
        }
        rest = after;
    }
    None
}

fn build_target() -> Result<String, String> {
    let path = Path::new(FW).join(".cargo").join("config.toml");
    let src = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    for line in src.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("target =") {
            return Ok(value.trim().trim_matches('"').to_string());
        }
    }
    Err(format!("no `target = ...` in {}", path.display()))
}

fn size() -> Result<(), String> {
    cargo(&["build", "--release"])?;

    let elf = elf_path()?;
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
    Err("no FLASH ... LENGTH = <n>K line in software/firmware/memory.x".to_string())
}

fn rtt(rest: &[String]) -> Result<(), String> {
    cargo(&["build", "--release"])?;
    let chip = chip_name()?;
    // probe-rs attach wants the ELF (for defmt symbol decoding), and paths are
    // resolved from the firmware dir, same as the `flash` runner.
    let elf = elf_path()?;
    let mut cmd = Command::new("probe-rs");
    cmd.current_dir(FW)
        .arg("attach")
        .args(["--chip", &chip])
        .arg(&elf)
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

pub(crate) fn rustc_output(args: &[&str]) -> Result<String, String> {
    let out = Command::new("rustc")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run rustc: {e}"))?;
    if !out.status.success() {
        return Err("rustc failed".to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(crate) fn host_triple() -> Result<String, String> {
    let verbose = rustc_output(&["-vV"])?;
    verbose
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_string)
        .ok_or_else(|| "rustc -vV has no host line".to_string())
}

pub(crate) fn tool(dir: &Path, name: &str) -> Result<PathBuf, String> {
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
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
