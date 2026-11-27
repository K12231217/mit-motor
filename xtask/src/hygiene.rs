//! 协作卫生检查：docs/agent-guardrails.md 的 H2 / H4 / H5。
//!
//! 扫描随固件发布的三个源码树，`xtask` 自身不扫（检查器里的标记字符串会被自己命中）。
//! 注释识别是行级的：`//` 之后到行尾算注释文本，字符串字面量里出现 `//` 会误判；
//! 三个源码树当前没有这种写法。任何命中一律 FAIL，不降级成警告。

use std::path::{Path, PathBuf};

const SCAN_ROOTS: [&str; 4] = ["0_contracts/src", "1_bsp/src", "3_user/src", "firmware/src"];
const ARCH_DOC: &str = "docs/software-architecture.md";
/// 必须带 owner 与触发条件的标记。
const OWNED_MARKERS: [&str; 2] = ["TODO", "FIXME"];
/// 无条件禁止的捷径标记：它们本身就是"先这样，以后再说"。
const FORBIDDEN_MARKERS: [&str; 2] = ["XXX", "HACK"];
/// 注释里禁止出现的历史/流程词（docs/comment-style.md §7）。
const FORBIDDEN_WORDS: [&str; 5] = ["旧实现", "曾用", "已作废", "已废弃", "评审"];

type CheckResult = Result<String, (String, Vec<String>)>;

/// 跑 H2 / H5 / H4 并打印报告；有 FAIL 则返回错误（main 映射成退出码 1）。
pub fn run() -> Result<(), String> {
    let checks: [(&str, CheckResult); 3] = [
        ("H2", check_todo_ownership()),
        ("H5", check_comment_markers()),
        ("H4", check_module_docs()),
    ];

    let mut failed = false;
    for (id, outcome) in checks {
        match outcome {
            Ok(summary) => println!("PASS  {id}  {summary}"),
            Err((summary, hits)) => {
                failed = true;
                println!("FAIL  {id}  {summary}");
                for hit in hits {
                    println!("          {hit}");
                }
            }
        }
    }

    println!();
    if failed {
        println!("hygiene: FAIL —— 见上面 FAIL 行");
        Err(String::new())
    } else {
        println!("hygiene: PASS —— H2/H4/H5 全部通过");
        Ok(())
    }
}

/// H2 —— `TODO` / `FIXME` 只能是 `TODO(owner, trigger): 说明`；`XXX` / `HACK` 一律禁止。
fn check_todo_ownership() -> CheckResult {
    let files = rust_files()?;
    let mut hits = Vec::new();
    let mut scanned = 0usize;

    for file in &files {
        let path = display(file);
        let src = read(file)?;
        scanned += 1;
        for (idx, raw) in src.lines().enumerate() {
            let Some(comment) = comment_text(raw) else {
                continue;
            };
            for marker in FORBIDDEN_MARKERS {
                if comment.contains(marker) {
                    hits.push(locate(&path, idx, raw, &format!("{marker} 无条件禁止")));
                }
            }
            for marker in OWNED_MARKERS {
                for (offset, _) in comment.match_indices(marker) {
                    if !owned_form(&comment[offset + marker.len()..]) {
                        hits.push(locate(
                            &path,
                            idx,
                            raw,
                            &format!("{marker} 必须写成 {marker}(owner, trigger): 说明"),
                        ));
                    }
                }
            }
        }
    }

    if hits.is_empty() {
        Ok(format!(
            "{scanned} 个文件：TODO/FIXME 均带 owner 与触发条件"
        ))
    } else {
        Err(("以下标记没有责任人、触发条件或本身被禁：".to_string(), hits))
    }
}

/// H5 —— 注释只允许 // / /// / //!，且不得承载历史、流程状态或 ASCII 分隔线。
fn check_comment_markers() -> CheckResult {
    let files = rust_files()?;
    let mut hits = Vec::new();
    let mut scanned = 0usize;

    for file in &files {
        let path = display(file);
        let src = read(file)?;
        scanned += 1;
        for (idx, raw) in src.lines().enumerate() {
            if raw.trim_start().starts_with("/*") {
                hits.push(locate(&path, idx, raw, "块注释：只允许 // / /// / //!"));
            }
            let Some(comment) = comment_text(raw) else {
                continue;
            };
            let text = comment.trim();
            if text.starts_with("SAFETY") {
                hits.push(locate(
                    &path,
                    idx,
                    raw,
                    "// SAFETY: 不适用：本仓靠 forbid(unsafe_code) 拦截",
                ));
            }
            if is_separator(text) {
                hits.push(locate(&path, idx, raw, "ASCII 分隔线（comment-style §7）"));
            }
            for word in FORBIDDEN_WORDS {
                if text.contains(word) {
                    hits.push(locate(
                        &path,
                        idx,
                        raw,
                        &format!("禁止的历史/流程词：{word}"),
                    ));
                }
            }
        }
    }

    if hits.is_empty() {
        Ok(format!(
            "{scanned} 个文件：注释无历史/流程词、无块注释、无分隔线"
        ))
    } else {
        Err((
            "以下注释违反 docs/comment-style.md §7/§11：".to_string(),
            hits,
        ))
    }
}

/// H4 —— 每个模块文件都必须出现在 docs/software-architecture.md 对应 crate 的文件树里。
///
/// 不做"自动生成 FEATURE_MAP"：那会造出第二份架构真源（PRE-10）。这里只查
/// "新文件是否写进了唯一真源"，文档腐烂在新增文件那一刻就报错。
fn check_module_docs() -> CheckResult {
    let doc = std::fs::read_to_string(ARCH_DOC)
        .map_err(|e| (format!("无法读取 {ARCH_DOC}"), vec![e.to_string()]))?;
    let files = rust_files()?;

    let mut dirs: Vec<String> = files
        .iter()
        .filter_map(|file| {
            file.components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    let sections = crate_sections(&doc, &dirs);

    let mut hits = Vec::new();
    let mut scanned = 0usize;
    for file in &files {
        let path = display(file);
        let Some((dir, rest)) = path.split_once("/src/") else {
            hits.push(format!("{path}：不在 <crate>/src/ 下，无法归属文件树"));
            continue;
        };
        let needle = format!("src/{rest}");
        scanned += 1;
        match sections.iter().find(|(name, _)| name == dir) {
            Some((_, body)) if body.contains(&needle) => {}
            Some(_) => hits.push(format!(
                "{path}：{ARCH_DOC} 的 `{dir}/` 块缺 `{needle}`（新文件先写进文件架构）"
            )),
            None => hits.push(format!("{path}：{ARCH_DOC} 没有 `{dir}/` 文件树块")),
        }
    }

    if hits.is_empty() {
        Ok(format!("{scanned} 个模块文件都能在 {ARCH_DOC} 找到"))
    } else {
        Err(("以下模块没有写进文件架构：".to_string(), hits))
    }
}

/// 按 crate 目录头把文档切成块：以 `<dir>/` 起首的行开新块，直到下一个这样的行。
fn crate_sections(doc: &str, dirs: &[String]) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    for line in doc.lines() {
        if let Some(dir) = dirs.iter().find(|dir| line.starts_with(&format!("{dir}/"))) {
            sections.push((dir.clone(), String::new()));
            continue;
        }
        if let Some((_, body)) = sections.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    sections
}

/// `TODO(owner, trigger):` 的形态判据：两个非空字段，`(` 前紧贴标记，`:` 紧贴 `)`。
fn owned_form(rest: &str) -> bool {
    let Some(rest) = rest.strip_prefix('(') else {
        return false;
    };
    let Some(end) = rest.find(')') else {
        return false;
    };
    if !rest[end + 1..].starts_with(':') {
        return false;
    }
    let mut parts = rest[..end].split(',');
    let owner = parts.next().unwrap_or("").trim();
    let trigger = parts.next().unwrap_or("").trim();
    let extra = parts.next().is_some();
    !owner.is_empty() && !trigger.is_empty() && !extra
}

/// 行内注释文本：`//` 之后到行尾；没有注释返回 `None`。
fn comment_text(line: &str) -> Option<&str> {
    let idx = line.find("//")?;
    Some(&line[idx + 2..])
}

fn is_separator(text: &str) -> bool {
    text.len() >= 8
        && text
            .chars()
            .all(|c| matches!(c, '-' | '=' | '*' | '#' | '+' | '~'))
}

fn rust_files() -> Result<Vec<PathBuf>, (String, Vec<String>)> {
    let mut files = Vec::new();
    for root in SCAN_ROOTS {
        collect_rs(Path::new(root), &mut files)
            .map_err(|e| (format!("无法扫描 {root}"), vec![e.to_string()]))?;
    }
    files.sort();
    Ok(files)
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

fn read(file: &Path) -> Result<String, (String, Vec<String>)> {
    std::fs::read_to_string(file)
        .map_err(|e| (format!("无法读取 {}", display(file)), vec![e.to_string()]))
}

fn display(file: &Path) -> String {
    file.to_string_lossy().replace('\\', "/")
}

fn locate(path: &str, idx: usize, line: &str, why: &str) -> String {
    format!("{path}:{}: {why}\n            {}", idx + 1, line.trim())
}
