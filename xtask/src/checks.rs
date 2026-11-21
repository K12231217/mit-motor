//! 架构不变量检查（docs/software-architecture.md §9）。
//!
//! I1 / I2 / I3 全部在本进程内实现，不经过 shell：命令的可用性不取决于 PATH
//! 里哪个 bash 先命中。检查没法执行时（命令失败、产物缺失）一律 FAIL，不降级
//! 成 N/A —— N/A 不是通过。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{host_triple, rustc_output, tool, ELF_REL, FW};

const BANNED: [&str; 4] = ["bsp", "embassy-stm32", "embassy-executor", "rtic"];
const SCAN_ROOTS: [&str; 2] = ["0_contracts/src", "3_user/src"];

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

/// 跑三条检查并打印报告；有 FAIL 则返回错误（main 映射成退出码 1）。
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
        println!("arch_check: PASS —— I1/I2/I3/I7 全部通过");
        Ok(())
    }
}

/// I1 —— 依赖单向：`user` 只能依赖 `contracts`。
fn check_deps() -> Outcome {
    let output = Command::new("cargo")
        .args(["tree", "-p", "user", "--prefix", "none"])
        .output();

    let output = match output {
        Ok(output) => output,
        Err(e) => {
            return Outcome::Fail {
                summary: "cargo tree -p user 无法启动".to_string(),
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
            summary: "cargo tree -p user --prefix none 失败".to_string(),
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
            "cargo tree -p user --prefix none：无 bsp/embassy-stm32/embassy-executor/rtic"
                .to_string(),
        )
    } else {
        Outcome::Fail {
            summary: "cargo tree -p user 出现禁用依赖：".to_string(),
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
/// ELF 路径复用 `crate::ELF_REL`（`cargo xtask rel` 的产物）。不存在就直接
/// FAIL 并给出下一步命令，不自动构建、不静默通过。
fn check_no_heap() -> Outcome {
    let elf = Path::new(FW).join(ELF_REL);
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

/// I7 —— 状态迁移只有一处：`MotorState` 只允许在 `fsm.rs`、其测试模块与 crate 根出现。
///
/// 判据是"命名"而不是"语义"：只要别处拿到 `MotorState`，就能写出第二条迁移路径。
/// 一条 grep 挡不住逻辑错误，但能挡住"在 CAN 任务里顺手改一下状态"这种最贵的退化。
fn check_fsm_single_owner() -> Outcome {
    const ALLOWED: [&str; 5] = [
        "0_contracts/src/fsm.rs",
        "0_contracts/src/lib.rs",
        "3_user/src/fsm.rs",
        "3_user/src/fsm/tests.rs",
        "3_user/src/lib.rs",
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
