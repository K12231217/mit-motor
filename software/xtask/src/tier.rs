//! H3 分级门禁：耦合硬件的改动没有台架证据就不得进 main。
//!
//! 分级按路径自动做（`TIER_B_PREFIXES`），不按人自觉；证据按 `.scratch/handoff.md`
//! §3 里的 `HIL: PASS` 行自动做。用法：`cargo xtask tier [base]`，base 缺省读
//! 环境变量 `TIER_BASE`，再退到 `origin/main`。基线是空提交（新分支）时无可比较，
//! 直接 PASS。

use std::process::Command;

const TIER_B_PREFIXES: [&str; 4] = [
    "software/firmware/",
    "software/bsp/",
    "software/system/",
    "software/app/",
];
const HANDOFF: &str = ".scratch/handoff.md";
const HIL_MARKER: &str = "HIL: PASS";

/// 比较 `base...HEAD`，Tier B 改动且无 HIL 证据则 FAIL。
pub fn run(rest: &[String]) -> Result<(), String> {
    let base = match rest {
        [] => std::env::var("TIER_BASE").unwrap_or_else(|_| "origin/main".to_string()),
        [base] => base.clone(),
        _ => return Err("`tier` 至多接受一个参数：[base]".to_string()),
    };

    if base.bytes().all(|b| b == b'0') {
        println!("PASS  H3  基线是空提交（新分支），没有可比较的历史");
        return Ok(());
    }

    let range = format!("{base}...HEAD");
    let output = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=ACMR", &range])
        .output()
        .map_err(|e| format!("无法运行 git: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut details: Vec<String> = stderr
            .lines()
            .filter(|line| line.starts_with("fatal:") || line.starts_with("error:"))
            .map(str::to_string)
            .collect();
        if details.is_empty() {
            details = stderr.lines().map(str::to_string).collect();
        }
        details.push(format!(
            "基线 `{base}` 不存在？PR 里先 `git fetch origin <base>:refs/remotes/origin/<base>`"
        ));
        return Err(fail("无法与基线比较", details));
    }

    let changed: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .filter(|line| !line.is_empty())
        .collect();
    let tier_b: Vec<&String> = changed
        .iter()
        .filter(|path| {
            TIER_B_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix))
        })
        .collect();

    if tier_b.is_empty() {
        println!(
            "PASS  H3  {} 个文件相对 `{base}` 有改动，全部是 Tier A（不碰硬件路径）",
            changed.len()
        );
        return Ok(());
    }

    let handoff = std::fs::read_to_string(HANDOFF)
        .map_err(|e| format!("无法读取 {HANDOFF}: {e}（HIL 证据只认这一处）"))?;
    // 只认独占一行的记录（可带列表符号）：正文里提到标记名不算证据。
    if handoff.lines().any(|line| {
        line.trim_start_matches(['-', '*', ' ', '\t'])
            .starts_with(HIL_MARKER)
    }) {
        println!(
            "PASS  H3  Tier B 改动 {} 个，{HANDOFF} 已有 `{HIL_MARKER}` 记录",
            tier_b.len()
        );
        return Ok(());
    }

    let mut details: Vec<String> = tier_b.iter().map(|path| (*path).clone()).collect();
    details.push(String::new());
    details.push("Tier B 改动需要台架证据才能进 main。二选一：".to_string());
    details.push("  1) 把改动留在 hil-pending 分支 / Draft PR，先不合 main；".to_string());
    details.push(format!(
        "  2) 跑实测（RTT、示波器、电流采样），在 {HANDOFF} §3 写一行以 `{HIL_MARKER}` 开头的记录。"
    ));
    Err(fail("Tier B 改动没有 HIL 证据", details))
}

fn fail(summary: &str, details: Vec<String>) -> String {
    println!("FAIL  H3  {summary}");
    for line in &details {
        if line.is_empty() {
            println!();
        } else {
            println!("          {line}");
        }
    }
    println!();
    println!("tier: FAIL —— 见上面 FAIL 行");
    // 报告已经打印过，不再让 main 追加一条没信息量的 `xtask: `。
    String::new()
}
