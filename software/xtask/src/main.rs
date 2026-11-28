//! Repo entry point: `cargo xtask <cmd>`.
//!
//! Every build / check / size / flash action lives here so CLion, VS Code and
//! CI execute the same code path. No extra tools (just, make) are required.
//!
//! Commands: check build rel fmt fmt-check clippy test doc size flash rtt hil dac arch
//!           hygiene tier help

mod checks;
mod fs;
mod hygiene;
mod tier;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};

pub(crate) const FW: &str = "software/firmware";

/// 门禁工具自己的 workspace 根；`fmt-check`/`clippy` 也必须覆盖它。
pub(crate) const XTASK: &str = "software/xtask";

/// `bsp` 是独立 thumb crate，自带 `.cargo/config.toml`；`fmt`/`clippy` 也要覆盖它。
pub(crate) const BSP: &str = "software/bsp";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".to_string());
    let rest: Vec<String> = args.collect();

    let result = match cmd.as_str() {
        "check" => cargo(&["check"]),
        "build" => cargo(&["build"]),
        "rel" | "release" => cargo(&["build", "--release"]),
        "fmt" => cargo(&["fmt", "--all"])
            .and_then(|()| root_cargo(&["fmt", "--all"]))
            .and_then(|()| xtask_cargo(&["fmt", "--all"]))
            .and_then(|()| bsp_cargo(&["fmt", "--all"])),
        "fmt-check" => cargo(&["fmt", "--all", "--", "--check"])
            .and_then(|()| root_cargo(&["fmt", "--all", "--", "--check"]))
            .and_then(|()| xtask_cargo(&["fmt", "--all", "--", "--check"]))
            .and_then(|()| bsp_cargo(&["fmt", "--all", "--", "--check"])),
        // `--all-targets` pulls in the test harness, which needs std on a
        // no_std thumb target; the firmware has one bin and no lib.
        // 根 workspace（算法层）与 xtask 也要过：`-D warnings` 把文档与 clippy
        // 提示一律升级为错误，测试目标同样拦。
        "clippy" => cargo(&["clippy", "--bins", "--", "-D", "warnings"])
            .and_then(|()| {
                root_cargo(&[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ])
            })
            .and_then(|()| xtask_cargo(&["clippy", "--all-targets", "--", "-D", "warnings"]))
            // bsp 是 no_std lib：`--all-targets` 会拉 host 测试装置而链接失败，只查 lib。
            .and_then(|()| bsp_cargo(&["clippy", "--", "-D", "warnings"])),
        // 根 workspace（host crate）的回归测试，不是 firmware。
        "test" => root_cargo(&["test", "--workspace"]),
        // 文档门禁：根 workspace 全量 rustdoc，任何警告即失败。
        "doc" => doc_check(),
        "size" => size(),
        // `probe-rs run` comes from software/firmware/.cargo/config.toml (single source).
        "flash" => cargo(&["run", "--release"]),
        "rtt" => rtt(&rest),
        // 台架主入口：编译 + 烧录 + 从复位开始抓 RTT，同时落一份日志。
        "hil" => hil(&rest),
        "dac" => dac(&rest),
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
  fmt         cargo fmt --all                (firmware + root + xtask + bsp)
  fmt-check   cargo fmt --all -- --check      (CI; same four workspaces)
  clippy      cargo clippy --bins -- -D warnings   (firmware)
              then cargo clippy --workspace --all-targets -- -D warnings (root)
              then cargo clippy --all-targets -- -D warnings (xtask)
              then cargo clippy -- -D warnings (bsp; no_std lib has no host tests)
  test        cargo test --workspace         (root workspace, host)
  doc         cargo doc --no-deps --workspace (RUSTDOCFLAGS=-D warnings)
  size        build --release, then print ELF sections and flash image
              against the FLASH length in software/firmware/memory.x
  flash       cargo run --release (probe-rs runner from .cargo/config.toml)
  rtt         probe-rs attach --chip <chip from .cargo/config.toml> [args]
  hil [--boot-freeze] [--enable-zero]
              build + flash + RTT from reset; log to .scratch/hil/run.log
              --boot-freeze holds TP2/PB7 high for 1 s at reset for probe capture
              --enable-zero sets MOE with zero vector (no motor; current-limited)
  dac <1|2> <code>
              hold DAC1 channel on the target (e.g. `dac 2 4095` for TP2)
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

/// Runs cargo in `software/xtask`（独立 host workspace）。门禁工具自己也要过门禁：
/// 否则 `fmt-check`/`clippy` 永远检查不到写检查器的那份代码。
fn xtask_cargo(args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(XTASK).args(args);
    run(&mut cmd)
}

/// Runs cargo in `software/bsp`（独立 thumb workspace，目标由它自己的 `.cargo/config.toml` 决定）。
fn bsp_cargo(args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(BSP).args(args);
    run(&mut cmd)
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
    cmd.current_dir(FW).arg("attach").args(["--chip", &chip]);
    add_configured_probe(&mut cmd);
    cmd.arg(&elf).args(rest);
    run_attached(&mut cmd)
}

/// 读 `.embeddedskills/config.json` 的 `probe-rs.probe`；多探针时不允许交互猜测。
fn configured_probe() -> Option<String> {
    let text = std::fs::read_to_string(".embeddedskills/config.json").ok()?;
    let start = text.find("\"probe\"")?;
    let after_key = &text[start + "\"probe\"".len()..];
    let colon = after_key.find(':')?;
    let value = after_key[colon + 1..].trim_start();
    let value = value.strip_prefix('"')?;
    let end = value.find('"')?;
    let probe = &value[..end];
    (!probe.is_empty()).then(|| probe.to_string())
}

fn add_configured_probe(cmd: &mut Command) {
    if let Some(probe) = configured_probe() {
        cmd.arg("--probe").arg(probe);
    }
}

/// Hold a DAC1 channel for bench measurement; no firmware rebuild/reset.
///
/// DAC1 DHR12R1 = 0x5000_0808, DHR12R2 = 0x5000_0814 (G474).
fn dac(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err("usage: cargo xtask dac <1|2> <0..4095>".to_string());
    }
    let channel = match args[0].as_str() {
        "1" => 1usize,
        "2" => 2usize,
        _ => return Err("DAC channel must be 1 or 2".to_string()),
    };
    let code: u16 = args[1]
        .parse()
        .map_err(|_| "DAC code must be an integer 0..4095".to_string())?;
    if code > 4095 {
        return Err("DAC code must be 0..4095".to_string());
    }
    let address = 0x5000_0800u32 + 8 + (channel - 1) as u32 * 12;
    let chip = chip_name()?;
    let mut cmd = Command::new("probe-rs");
    cmd.current_dir(FW)
        .args(["write", "b32"])
        .arg(format!("0x{address:08x}"))
        .arg(code.to_string())
        .args(["--chip", &chip]);
    add_configured_probe(&mut cmd);
    run(&mut cmd)
}

/// 台架主入口：release 构型 + 烧录 + 从复位开始抓 RTT，并落盘。
///
/// `probe-rs run` 同时完成 flash 和 RTT，避免“先 flash、再 attach，启动日志已丢”的
/// 常见操作失误。固定测试点只认 `docs/instrumentation.md`，本命令不引入新探针。
fn hil(args: &[String]) -> Result<(), String> {
    let mut boot_freeze = false;
    let mut enable_zero = false;
    for arg in args {
        match arg.as_str() {
            "--boot-freeze" => boot_freeze = true,
            "--enable-zero" => enable_zero = true,
            _ => return Err("usage: cargo xtask hil [--boot-freeze] [--enable-zero]".to_string()),
        }
    }
    println!("HIL bench card (fixed test points, docs/instrumentation.md):");
    println!("  CH1 PA4/TP1  DAC1_OUT1 residual     0 A = 1.65 V");
    println!("  CH2 PA5/TP2  DAC1_OUT2 freeze       0 V normal / 3.3 V frozen");
    println!("  CH3 PB6      TP_SYNC_LOOP           20 kHz ISR envelope");
    println!("  CH4 PB7      TP_SYNC_FREEZE         frozen window");
    println!("  trigger: CH2 rising, single; start 10 ms/div, then 20 us/div for PWM");
    println!();

    let mut features = Vec::new();
    if boot_freeze {
        features.push("hil-boot-freeze");
    }
    if enable_zero {
        features.push("hil-enable-zero");
    }
    if features.is_empty() {
        cargo(&["build", "--release"])?;
    } else {
        cargo(&["build", "--release", "--features", &features.join(",")])?;
    }
    let chip = chip_name()?;
    let elf = elf_path()?;
    let log_dir = std::env::current_dir()
        .map_err(|e| format!("cannot get cwd: {e}"))?
        .join(".scratch")
        .join("hil");
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("cannot create {}: {e}", log_dir.display()))?;
    let log = log_dir.join("run.log");
    println!("RTT log: {}", log.display());

    let mut cmd = Command::new("probe-rs");
    cmd.current_dir(FW).arg("run").args(["--chip", &chip]);
    add_configured_probe(&mut cmd);
    cmd.arg("--target-output-file").arg(&log).arg(&elf);
    run_attached(&mut cmd)
}

/// RTT 会话以 Ctrl+C 结束；Windows 返回 `STATUS_CONTROL_C_EXIT`，Unix 返回 130。
fn run_attached(cmd: &mut Command) -> Result<(), String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let status = cmd
        .status()
        .map_err(|e| format!("cannot run `{program}`: {e}"))?;
    if status.success() || is_ctrl_c(status) {
        Ok(())
    } else {
        Err(format!("`{program}` failed ({status})"))
    }
}

fn is_ctrl_c(status: ExitStatus) -> bool {
    matches!(status.code(), Some(130) | Some(-1_073_741_510))
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
                return Ok(name.trim_matches('"').to_string());
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
