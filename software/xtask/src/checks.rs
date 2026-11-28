//! 架构不变量检查（docs/software-architecture.md §9）。
//!
//! I1 / I2 / I3 全部在本进程内实现，不经过 shell：命令的可用性不取决于 PATH
//! 里哪个 bash 先命中。检查没法执行时（命令失败、产物缺失）一律 FAIL，不降级
//! 成 N/A —— N/A 不是通过。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{elf_path, host_triple, rustc_output, tool};

const BANNED: [&str; 4] = ["bsp", "embassy-stm32", "embassy-executor", "rtic"];
const SCAN_ROOTS: [&str; 2] = ["software/contracts/src", "software/algorithms/src"];

enum Outcome {
    Pass(String),
    Fail {
        summary: String,
        details: Vec<String>,
    },
}

struct Report {
    id: &'static str,
    outcome: Outcome,
}

/// 跑全部检查并打印报告；有 FAIL 则返回错误（main 映射成退出码 1）。
pub fn run() -> Result<(), String> {
    let reports = [
        Report {
            id: "I1",
            outcome: check_deps(),
        },
        Report {
            id: "I2",
            outcome: check_unsafe(),
        },
        Report {
            id: "I3",
            outcome: check_no_heap(),
        },
        Report {
            id: "I4",
            outcome: check_isr_purity(),
        },
        Report {
            id: "I7",
            outcome: check_fsm_single_owner(),
        },
    ];

    let mut failed = false;
    for report in &reports {
        match &report.outcome {
            Outcome::Pass(msg) => println!("PASS  {:<3} {msg}", report.id),
            Outcome::Fail { summary, details } => {
                failed = true;
                println!("FAIL  {:<3} {summary}", report.id);
                for line in details {
                    println!("          {line}");
                }
            }
        }
    }

    println!();
    if failed {
        println!("arch_check: FAIL —— 见上面 FAIL 行");
        // 报告已经打印过，这里不再让 main 追加一条没信息量的 `xtask: `。
        Err(String::new())
    } else {
        println!("arch_check: PASS —— I1/I2/I3/I4/I7 全部通过");
        Ok(())
    }
}

/// I1 —— 依赖单向：`algorithms` 只能依赖 `contracts`。
fn check_deps() -> Outcome {
    let output = Command::new("cargo")
        .args(["tree", "-p", "algorithms", "--prefix", "none"])
        .output();

    let output = match output {
        Ok(output) => output,
        Err(e) => {
            return Outcome::Fail {
                summary: "cargo tree -p algorithms 无法启动".to_string(),
                details: vec![e.to_string()],
            }
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let mut details: Vec<String> = stderr.lines().map(str::to_string).collect();
        details.push(format!("exit status: {}", output.status));
        return Outcome::Fail {
            summary: "cargo tree -p algorithms --prefix none 失败".to_string(),
            details,
        };
    }

    let hits: Vec<String> = stdout
        .lines()
        .filter(|line| is_banned_line(line))
        .map(str::to_string)
        .collect();
    if hits.is_empty() {
        Outcome::Pass(
            "cargo tree -p algorithms --prefix none：无 bsp/embassy-stm32/embassy-executor/rtic"
                .to_string(),
        )
    } else {
        Outcome::Fail {
            summary: "cargo tree -p algorithms 出现禁用依赖：".to_string(),
            details: hits,
        }
    }
}

/// `--prefix none` 的每行形如 `<name> v<version>`，可带 `(path)` 或 `(*)`。
/// 只认"包名 + 版本"两列，避免命中路径里出现的同名字符串。
fn is_banned_line(line: &str) -> bool {
    let mut it = line.split_whitespace();
    let (Some(name), Some(version)) = (it.next(), it.next()) else {
        return false;
    };
    BANNED.contains(&name)
        && version
            .strip_prefix('v')
            .is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
}

/// I2 —— 禁止 `unsafe`，唯一豁免是 lint 名 `unsafe_code`。
fn check_unsafe() -> Outcome {
    let mut files = Vec::new();
    for root in SCAN_ROOTS {
        if let Err(e) = collect_rs(Path::new(root), &mut files) {
            return Outcome::Fail {
                summary: format!("无法扫描 {root}"),
                details: vec![e.to_string()],
            };
        }
    }
    files.sort();

    let mut hits = Vec::new();
    for file in &files {
        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(e) => {
                return Outcome::Fail {
                    summary: format!("无法读取 {}", file.display()),
                    details: vec![e.to_string()],
                }
            }
        };
        let path = file.to_string_lossy().replace('\\', "/");
        for (idx, raw) in bytes.split(|b| *b == b'\n').enumerate() {
            let line = String::from_utf8_lossy(raw);
            let line = line.trim_end_matches('\r');
            if line.contains("unsafe") && !line.contains("unsafe_code") {
                hits.push(format!("{path}:{}:{}", idx + 1, line.trim_end()));
            }
        }
    }

    if hits.is_empty() {
        Outcome::Pass(format!(
            "{} 中 unsafe 零命中（已豁免 lint 名 unsafe_code）",
            SCAN_ROOTS.join(", ")
        ))
    } else {
        Outcome::Fail {
            summary: format!("以下位置出现 unsafe（扫描：{}）：", SCAN_ROOTS.join(", ")),
            details: hits,
        }
    }
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rs(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// I3 —— 无堆：链接产物里没有分配器符号。
///
/// ELF 路径由 `cargo metadata` 推出（`cargo xtask rel` 的产物）。不存在就直接
/// FAIL 并给出下一步命令，不自动构建、不静默通过。
fn check_no_heap() -> Outcome {
    let elf = match elf_path() {
        Ok(path) => path,
        Err(e) => {
            return Outcome::Fail {
                summary: "无法从 cargo metadata 推出 ELF 路径".to_string(),
                details: vec![e],
            }
        }
    };
    if !elf.is_file() {
        return Outcome::Fail {
            summary: format!("release ELF 不存在：{}", elf.display()),
            details: vec!["先跑 `cargo xtask rel` 生成 ELF（本检查不自动构建）".to_string()],
        };
    }

    let nm = match llvm_nm() {
        Ok(nm) => nm,
        Err(e) => {
            return Outcome::Fail {
                summary: "找不到 llvm-nm".to_string(),
                details: vec![e],
            }
        }
    };

    let output = match Command::new(&nm).arg(&elf).output() {
        Ok(output) => output,
        Err(e) => {
            return Outcome::Fail {
                summary: format!("无法运行 {}", nm.display()),
                details: vec![e.to_string()],
            }
        }
    };
    if !output.status.success() {
        let mut details: Vec<String> = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(str::to_string)
            .collect();
        details.push(format!("exit status: {}", output.status));
        return Outcome::Fail {
            summary: format!("llvm-nm {} 失败", elf.display()),
            details,
        };
    }

    let hits: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("__rust_alloc") || line.contains("__rg_alloc"))
        .map(str::to_string)
        .collect();
    if hits.is_empty() {
        Outcome::Pass(format!(
            "llvm-nm {}：无 __rust_alloc/__rg_alloc",
            elf.display()
        ))
    } else {
        Outcome::Fail {
            summary: format!("{} 含分配器符号：", elf.display()),
            details: hits,
        }
    }
}

/// I4 —— 20 kHz 任务只由硬件事件驱动：ISR 函数体内无 `defmt`/`await`/锁/解包。
///
/// 只看 `#[task(binds = TIM1_UP…)]` 的函数体，不看整个文件：`init`/`monitor` 本来
/// 就用 `defmt`。锚点找不到即 FAIL —— 任务改名或挪走不能让这条不变量悄悄失效。
fn check_isr_purity() -> Outcome {
    const SOURCE: &str = "software/firmware/src/bin/main.rs";
    const NEEDLES: [&str; 5] = ["defmt", ".await", ".lock(", "unwrap(", "expect("];

    let text = match std::fs::read_to_string(SOURCE) {
        Ok(text) => text,
        Err(e) => {
            return Outcome::Fail {
                summary: format!("无法读取 {SOURCE}"),
                details: vec![e.to_string()],
            }
        }
    };
    let lines: Vec<&str> = text.lines().collect();
    let Some((start, end)) = isr_body(&lines) else {
        return Outcome::Fail {
            summary: format!("{SOURCE} 里找不到 `#[task(binds = TIM1_UP…)] fn current`"),
            details: vec!["锚点改名后必须同步改本检查，不能降级成 N/A".to_string()],
        };
    };

    let mut hits = Vec::new();
    for (offset, line) in lines[start..=end].iter().enumerate() {
        for needle in NEEDLES {
            if line.contains(needle) {
                hits.push(format!(
                    "{SOURCE}:{}:{}",
                    start + offset + 1,
                    line.trim_end()
                ));
                break;
            }
        }
    }

    if hits.is_empty() {
        Outcome::Pass(format!(
            "{SOURCE}:{}–{end} 的 TIM1 更新 ISR 无 defmt/await/锁/解包",
            start + 1
        ))
    } else {
        Outcome::Fail {
            summary: "20 kHz ISR 体内出现禁用构造：".to_string(),
            details: hits,
        }
    }
}

/// 从 `#[task(binds = TIM1_UP…)]` 之后的 `fn current(` 起，按花括号配对截出函数体。
fn isr_body(lines: &[&str]) -> Option<(usize, usize)> {
    let anchor = lines
        .iter()
        .position(|line| line.contains("#[task(binds = TIM1_UP"))?;
    let start = anchor + lines[anchor..].iter().position(|l| l.contains("fn current("))?;

    let mut depth = 0i32;
    let mut opened = false;
    for (offset, line) in lines[start..].iter().enumerate() {
        for ch in strip_strings(line).chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth == 0 {
            return Some((start, start + offset));
        }
    }
    None
}

/// 去掉字符串字面量，避免 `"{"`/`"}"` 干扰花括号配对。
fn strip_strings(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_str = false;
    let mut escaped = false;
    for ch in line.chars() {
        if in_str {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
        } else if ch == '"' {
            in_str = true;
        } else {
            out.push(ch);
        }
    }
    out
}

/// I7 —— 状态迁移只有一处：`MotorState` 只允许在 `fsm.rs`、其测试模块与 crate 根出现。
///
/// 判据是"命名"而不是"语义"：只要别处拿到 `MotorState`，就能写出第二条迁移路径。
/// 一条 grep 挡不住逻辑错误，但能挡住"在 CAN 任务里顺手改一下状态"这种最贵的退化。
fn check_fsm_single_owner() -> Outcome {
    const ALLOWED: [&str; 5] = [
        "software/contracts/src/joint/protocol_fsm.rs",
        "software/contracts/src/lib.rs",
        "software/algorithms/src/joint/service_fsm.rs",
        "software/algorithms/src/joint/service_fsm/tests.rs",
        "software/algorithms/src/lib.rs",
    ];
    const NEEDLE: &str = "MotorState";

    let mut files = Vec::new();
    for root in SCAN_ROOTS {
        if let Err(e) = collect_rs(Path::new(root), &mut files) {
            return Outcome::Fail {
                summary: format!("无法扫描 {root}"),
                details: vec![e.to_string()],
            };
        }
    }
    files.sort();

    let mut hits = Vec::new();
    let mut scanned = 0usize;
    for file in &files {
        let path = file.to_string_lossy().replace('\\', "/");
        if ALLOWED.contains(&path.as_str()) {
            continue;
        }
        scanned += 1;
        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(e) => {
                return Outcome::Fail {
                    summary: format!("无法读取 {}", file.display()),
                    details: vec![e.to_string()],
                }
            }
        };
        for (idx, raw) in bytes.split(|b| *b == b'\n').enumerate() {
            let line = String::from_utf8_lossy(raw);
            let line = line.trim_end_matches('\r');
            if line.contains(NEEDLE) {
                hits.push(format!("{path}:{}:{}", idx + 1, line.trim_end()));
            }
        }
    }

    if hits.is_empty() {
        Outcome::Pass(format!(
            "{NEEDLE} 仅在 fsm.rs、其测试模块与 crate 根出现（扫描 {} 个文件）",
            scanned
        ))
    } else {
        Outcome::Fail {
            summary: format!("{NEEDLE} 出现在迁移矩阵之外："),
            details: hits,
        }
    }
}

fn llvm_nm() -> Result<PathBuf, String> {
    let host = host_triple()?;
    let sysroot = rustc_output(&["--print", "sysroot"])?;
    let bin = PathBuf::from(sysroot.trim())
        .join("lib")
        .join("rustlib")
        .join(host.trim())
        .join("bin");
    tool(&bin, "llvm-nm")
}
