//! 源码树遍历与路径显示的唯一实现。
//!
//! `arch` 与 `hygiene` 扫的是不同的根，但"递归收集 `.rs`"和"打印成正斜杠路径"只能
//! 有一份：复制第二份的那天，两边的忽略规则就会分叉，命中报告里的路径也会两种写法。

use std::path::{Path, PathBuf};

/// 递归收集 `dir` 下的 `.rs` 文件；顺序由文件系统决定，调用方负责排序。
pub fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
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

/// 路径 → 正斜杠写法。命中行与注册表都用这一种，便于直接 grep。
pub fn rel(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 去掉一行的双引号字面量与 `//` 注释，返回"代码骨架"。
///
/// 两道门禁都靠它避免把文本当成代码：I4 数 ISR 体的花括号，H6 找裸 `todo!()`。
/// 已知边界：块注释（本仓由 H5 禁止）与 `'{'` 字符字面量不在覆盖内。
pub fn code_only(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_str = false;
    let mut escaped = false;
    let mut prev = '\0';
    for ch in line.chars() {
        if in_str {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
        } else if ch == '/' && prev == '/' {
            // `//` 之后到行尾都是注释；上一轮已把第一个 `/` 推入 out，这里也去掉它。
            out.pop();
            break;
        } else if ch == '"' {
            in_str = true;
        } else {
            out.push(ch);
        }
        prev = ch;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::code_only;

    #[test]
    fn code_only_drops_literals_and_line_comments() {
        assert_eq!(
            code_only("let s = \"{\"; let t = 1;"),
            "let s = ; let t = 1;"
        );
        assert_eq!(code_only("    if x { // } 注释"), "    if x { ");
        assert_eq!(code_only("// todo!() mention"), "");
        assert_eq!(code_only("let u = a / b; // 除法"), "let u = a / b; ");
    }
}
