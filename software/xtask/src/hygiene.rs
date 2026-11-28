//! 协作卫生检查：docs/archive/agent-guardrails.md 的 H2 / H4 / H5 / H6。
//!
//! 扫描随固件发布的三个源码树，`xtask` 自身不扫（检查器里的标记字符串会被自己命中）。
//! 注释识别是行级的：`//` 之后到行尾算注释文本，字符串字面量里出现 `//` 会误判；
//! 三个源码树当前没有这种写法。任何命中一律 FAIL，不降级成警告。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::fs::{collect_rs, rel};

const SCAN_ROOTS: [&str; 4] = [
    "software/contracts/src",
    "software/bsp/src",
    "software/algorithms/src",
    "software/firmware/src",
];
const ARCH_DOC: &str = "docs/software-architecture.md";
const REGISTRY_START: &str = "<!-- module-registry:start -->";
const REGISTRY_END: &str = "<!-- module-registry:end -->";
const LAYERS: [&str; 4] = ["app", "dev", "bsp", "lib"];
const DOMAINS: [&str; 4] = ["joint", "motor", "rc", "imu"];
const ROLES: [&str; 6] = ["task", "service", "protocol", "bus", "config", "chain"];
/// 必须带 owner 与触发条件的标记。
const OWNED_MARKERS: [&str; 2] = ["TODO", "FIXME"];
/// 无条件禁止的捷径标记：它们本身就是"先这样，以后再说"。
const FORBIDDEN_MARKERS: [&str; 2] = ["XXX", "HACK"];
/// 注释里禁止出现的历史/流程词（docs/archive/comment-style.md §7）。
const FORBIDDEN_WORDS: [&str; 5] = ["旧实现", "曾用", "已作废", "已废弃", "评审"];

type CheckResult = Result<String, (String, Vec<String>)>;

/// 跑 H2 / H5 / H4 / H6 并打印报告；有 FAIL 则返回错误（main 映射成退出码 1）。
pub fn run() -> Result<(), String> {
    let checks: [(&str, CheckResult); 4] = [
        ("H2", check_todo_ownership()),
        ("H5", check_comment_markers()),
        ("H4", check_module_docs()),
        ("H6", check_module_registry()),
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
        println!("hygiene: PASS —— H2/H4/H5/H6 全部通过");
        Ok(())
    }
}

/// H2 —— `TODO` / `FIXME` 只能是 `TODO(owner, trigger): 说明`；`XXX` / `HACK` 一律禁止。
fn check_todo_ownership() -> CheckResult {
    let files = rust_files()?;
    let mut hits = Vec::new();
    let mut scanned = 0usize;

    for file in &files {
        let path = rel(file);
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
        let path = rel(file);
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
            if text.contains("user::") {
                hits.push(locate(
                    &path,
                    idx,
                    raw,
                    "旧 crate 名 `user::`：已改名 contracts / algorithms",
                ));
            }
        }
    }

    if hits.is_empty() {
        Ok(format!(
            "{scanned} 个文件：注释无旧 crate 名、无历史/流程词、无块注释、无分隔线"
        ))
    } else {
        Err((
            "以下注释违反 docs/archive/comment-style.md §7/§11：".to_string(),
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
            rel(file)
                .split_once("/src/")
                .map(|(dir, _)| dir.to_string())
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    let sections = crate_sections(&doc, &dirs);

    let mut hits = Vec::new();
    let mut scanned = 0usize;
    for file in &files {
        let path = rel(file);
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

struct ModuleRow {
    id: String,
    path: String,
    owner: String,
    contract: String,
}

/// H6 —— 模块注册表：每个源码文件恰好归一个模块，模块 ID 唯一且四轴合法。
///
/// 模块名不是文件名，而是 `<layer>.<domain>.<role>[.<qualifier>]`；path 只是存储位置。
/// 这条检查把"名字没有第二种选择"变成机器判据：重复 ID、非法角色、两个模块抢同一文件
/// 都直接 FAIL。
fn check_module_registry() -> CheckResult {
    let doc = std::fs::read_to_string(ARCH_DOC)
        .map_err(|e| (format!("无法读取 {ARCH_DOC}"), vec![e.to_string()]))?;
    let rows = parse_module_registry(&doc)?;
    let files = rust_files()?;

    let mut hits = Vec::new();
    let mut ids = BTreeSet::new();
    for row in &rows {
        if !valid_module_id(&row.id) {
            hits.push(format!(
                "{}：模块 ID 必须是 <layer>.<domain>.<role>[_<qualifier>]（四轴闭集）",
                row.id
            ));
        }
        if !ids.insert(row.id.clone()) {
            hits.push(format!("{}：模块 ID 重复", row.id));
        }
        if !valid_owner(&row.owner) {
            hits.push(format!(
                "{}：owner 必须是一个写者（不要逗号、加号、and）",
                row.id
            ));
        }
        if row.contract.trim().is_empty() {
            hits.push(format!("{}：contract 为空", row.id));
        }
        if !Path::new(&row.path).exists() {
            hits.push(format!("{}：path 不存在：{}", row.id, row.path));
        }
        if !path_matches_id(&row.path, &row.id) {
            hits.push(format!(
                "{}：path 必须按 ID 放在 <crate>/src/<domain>/<role>[_<qualifier>]：{}",
                row.id, row.path
            ));
        }
    }

    let mut scanned = 0usize;
    for file in &files {
        let path = rel(file);
        if is_module_tree_file(&path) {
            continue;
        }
        scanned += 1;
        let mut matches = rows.iter().filter(|row| path_covered(&path, &row.path));
        let first = matches.next();
        let second = matches.next();
        match (first, second) {
            (Some(_), None) => {}
            (None, _) => hits.push(format!(
                "{path}：没有模块注册项（{ARCH_DOC} 的 module-registry 表）"
            )),
            (Some(a), Some(b)) => hits.push(format!(
                "{path}：被 {} 与 {} 同时覆盖（一个文件只能属于一个模块）",
                a.id, b.id
            )),
        }
    }

    if hits.is_empty() {
        Ok(format!(
            "{} 个模块 / {scanned} 个文件，ID 与 owner 唯一",
            rows.len()
        ))
    } else {
        Err(("模块注册表不成立：".to_string(), hits))
    }
}

fn parse_module_registry(doc: &str) -> Result<Vec<ModuleRow>, (String, Vec<String>)> {
    let Some(start) = doc.find(REGISTRY_START) else {
        return Err((
            format!("{ARCH_DOC} 缺 {REGISTRY_START}"),
            vec!["模块注册表必须带 start/end 标记".to_string()],
        ));
    };
    let rest = &doc[start + REGISTRY_START.len()..];
    let Some(end) = rest.find(REGISTRY_END) else {
        return Err((
            format!("{ARCH_DOC} 缺 {REGISTRY_END}"),
            vec!["模块注册表必须带 start/end 标记".to_string()],
        ));
    };

    let mut rows = Vec::new();
    let mut hits = Vec::new();
    for (idx, raw) in rest[..end].lines().enumerate() {
        let line = raw.trim();
        if !line.starts_with('|')
            || line.starts_with("|---")
            || line.contains("module id")
            || line.is_empty()
        {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() != 4 {
            hits.push(format!("注册表第 {} 行：需要 4 列", idx + 1));
            continue;
        }
        rows.push(ModuleRow {
            id: strip_ticks(cells[0]).to_string(),
            path: strip_ticks(cells[1]).to_string(),
            owner: strip_ticks(cells[2]).to_string(),
            contract: strip_ticks(cells[3]).to_string(),
        });
    }

    if !hits.is_empty() {
        return Err(("模块注册表格式错误：".to_string(), hits));
    }
    if rows.is_empty() {
        return Err((
            "模块注册表为空".to_string(),
            vec!["在 start/end 之间至少登记一个模块".to_string()],
        ));
    }
    Ok(rows)
}

fn strip_ticks(s: &str) -> &str {
    s.trim().trim_matches('`').trim()
}

fn valid_owner(owner: &str) -> bool {
    let owner = owner.trim();
    !owner.is_empty()
        && !owner.contains(" and ")
        && !owner
            .chars()
            .any(|c| matches!(c, ',' | '+' | '&' | '/' | '、' | '，'))
}

fn valid_module_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    LAYERS.contains(&parts[0]) && DOMAINS.contains(&parts[1]) && valid_role_qualifier(parts[2])
}

fn valid_role_qualifier(part: &str) -> bool {
    ROLES.iter().any(|role| {
        part == *role
            || part
                .strip_prefix(role)
                .and_then(|rest| rest.strip_prefix('_'))
                .is_some_and(valid_qualifier)
    })
}

fn valid_qualifier(q: &str) -> bool {
    !q.is_empty()
        && q.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// path 是存储位置：`foo` 覆盖 `foo.rs` 与 `foo/**`，也覆盖精确的 `foo` 文件。
fn path_covered(file: &str, prefix: &str) -> bool {
    match file.strip_prefix(prefix) {
        Some("") | Some(".rs") => true,
        Some(rest) => rest.starts_with('/'),
        None => false,
    }
}

/// `src/bin/` 下不参与 `<crate>/src/<domain>/<role>` 投影的文件：bin 入口与 bin 内子模块
/// 的路径形状由 cargo 决定，不是模块 ID 的机械投影。逐条列出，新增必须显式加行并登记
/// 模块 ID；不许再按前缀整片豁免。
const BIN_FILES: [&str; 2] = [
    "software/firmware/src/bin/main.rs",
    "software/firmware/src/bin/trace/mod.rs",
];

/// crate 根、模块树 `mod.rs` 与 `BIN_FILES` 是公共头，不是模块目录。
fn is_module_tree_file(path: &str) -> bool {
    if path.contains("/src/bin/") {
        return BIN_FILES.contains(&path);
    }
    path.ends_with("/src/lib.rs") || path.ends_with("/mod.rs")
}

fn path_matches_id(path: &str, id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() != 3 || !layer_matches(path, parts[0]) {
        return false;
    }
    if is_module_tree_file(path) {
        return true;
    }
    let Some((_, after_src)) = path.split_once("/src/") else {
        return false;
    };
    let suffix = format!("{}/{}", parts[1], parts[2]);
    after_src == suffix
        || after_src == format!("{suffix}.rs")
        || after_src.starts_with(&format!("{suffix}/"))
}

fn layer_matches(path: &str, layer: &str) -> bool {
    match layer {
        "lib" => {
            path.starts_with("software/contracts/src/")
                || path.starts_with("software/algorithms/src/")
        }
        "bsp" => path.starts_with("software/bsp/src/"),
        "app" => path.starts_with("software/firmware/src/"),
        "dev" => true,
        _ => false,
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

fn read(file: &Path) -> Result<String, (String, Vec<String>)> {
    std::fs::read_to_string(file)
        .map_err(|e| (format!("无法读取 {}", rel(file)), vec![e.to_string()]))
}

fn locate(path: &str, idx: usize, line: &str, why: &str) -> String {
    format!("{path}:{}: {why}\n            {}", idx + 1, line.trim())
}
