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
