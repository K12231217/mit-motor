# 注释规范

依据（一手）：rustdoc book《How to write documentation》建议摘要句独占一行、组件按「摘要 → 细节 → 示例」
组织，并在会 panic 时写 `# Panics`
<https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html>；
Rust API Guidelines `C-CRATE-DOC` / `C-EXAMPLE` / `C-FAILURE` / `C-LINK`
<https://rust-lang.github.io/api-guidelines/documentation.html#c-failure>（官方编号没有
`C-DOC-*`）；clippy `doc_markdown` / `too_long_first_doc_paragraph` / `doc_lazy_continuation`；
rustfmt 默认 `max_width = 100`、`wrap_comments = false`（`rustfmt --print-config default`），注释不折行。

1. 摘要句：公开项第一行是一句陈述句，以「。」结尾，独占一行，细节从第二段开始。
   正：`/// 电流环周期预算，单位 µs。`
   反：`/// 电流环周期预算：25 µs（含快照发布 < 0.5 µs）。DWT 实测值进遥测。`
2. 段落：摘要与细节之间空一行；细节不复述签名已表达的类型与单位。
   正：`/// 采样窗口下限，单位 ns。` 空行 `/// 推导（HR-202 + HR-303）：…4.47 µs，再留 0.5 µs 余量 ⇒ 5.0 µs。`
   反：`/// 返回 Result<Self, DecodeError>，失败时是 Err(DecodeError::Rejected)。`
3. `# Errors`：公开的 `Result` 函数必须写，逐条列出每个错误变体的触发条件（C-FAILURE）。
   正：「# Errors」小节，空行后写 `ver` 与 [`ICD_VERSION`] 不符，或任一 `rsvd` 非 0。
   反：只写「解码失败时返回错误」，不说哪种输入会失败。
4. `# Panics`：只在真会 panic 时写；本仓 `deny` 了 `unwrap` / `expect` / `indexing_slicing`，
   正常路径不写该小节，`unsafe` 相关见第 11 条。
   正：（没有该小节）反：`# Panics` 加 `本函数不会 panic。`
5. 三种注释分工：`//!` 写 crate 与模块；`///` 写公开项（含公开字段与枚举变体）；
   `//` 写私有实现的「为什么」。
   正：`` // 单写者 ⇒ 进入时 `seq` 必为偶，+1 即奇、+2 即偶，无需 CAS。 ``（`snapshot.rs`）
   反：私有 `fn sqrt_f32` 用 `///` 写成「牛顿迭代 4 次，初值折半指数位模式」。
6. 反引号与混排：标识符、类型、常量、字段、表达式一律加反引号；中文与 ASCII、数字之间空一格。
   正：「/// 判据是 `pred > KAPPA · target_pp`，不降额会放过 p99 超差。」
   反：「/// `pred > KAPPA·target_pp`：精度不够」。
7. 禁止内容：历史（旧值 / 已作废）、评审与流程状态（FROZEN / OPEN / 评审记录）、任务指令、
   粗体当标题、ASCII 分隔线——评审时 grep 到即删。
   正：`/// 相对增益的共模分量不可观测，因此只上报 B/C 两路。`
   反：`// ---------- 标题 ----------` 与 `/// **参数可辨识性的前提**`。
8. 小节标题：模块级用 `//! # 小节名`；项内只允许 `# Errors` / `# Panics` / `# Examples`
   三个标准小节（`# Safety` 见第 11 条，本仓不写），不自造标题。
   正：`//! # 协议` 下一行 `//! seq 为奇数表示正在写。`
   反：`//! **协议**`。
9. 行宽上限：单行 ≤ 100 显示列，CJK 记 2 列，超出即手工折行；rustfmt 与 `cargo xtask doc` 都不查宽度，
   只能按这条硬上限逐行数（`contracts` 与 `algorithms` 当前无超宽行）。
   正：`software/algorithms/src/motor/service_gate/accum.rs` 的 χ² 说明拆成两行（公式与 `Σi = Si⁻¹·σ_res²` 各占一行），每行 ≤ 100 列。
   反：把 `software/contracts/src/motor/config_hw.rs` 里 `D_MAX` 的三行推导并成一行（> 100 列）。
10. 物理常量必须有单位与来源：摘要句写「<含义>，单位 <单位>。」（无量纲写「无量纲」），
    细节给 datasheet 章节、RM 条款或实测方法；没有单位的物理量不得作为常量出现。
    正：`/// 采样窗口下限，单位 ns。` + `/// 低边建立 2.0 µs + 采样 92.5 cyc（2.176 µs）…`。
    反：`// 一圈的计数（u32 形态，便于移位与取模）。`（`encoder.rs`：无单位、无来源）。
11. `// SAFETY:` 规则：本仓现役两个 crate 均 `#![forbid(unsafe_code)]`（`software/contracts/src/lib.rs`、
    `software/algorithms/src/lib.rs`），`cargo xtask arch` 的 I2 扫这两个 crate 的 `src`，把任何 `unsafe`
    判为失败 ⇒ `// SAFETY:` 与 `# Safety` **不适用**；出现 `unsafe` 即为违规，由 I2 拦截，
    不用注释解释。未来 `bsp`/`app` 的白名单见 AGENTS PRE-03，本规范不为它留注释豁免。
    正：两处 `#![forbid(unsafe_code)]`，全仓 `grep -rn '// SAFETY'` 为空。
    反：给安全代码写 `/// # Safety`——没有 unsafe 调用者，该小节恒为空，也拦不住真实违规。
12. 禁止 What 注释：复述代码逻辑的注释一律删除，只留 Why（物理依据、不变量、安全边界、
    为什么是这个值）。
    正：`// 有符号 14 bit 角度差：16383 → 0 是 +1 而不是 -16383。`（`encoder.rs`）
    反：`// 一圈掩码：diff & TURN_MASK 得到 0..16383。`（`encoder.rs`：读代码即得）。

仓库约定：验证状态与历史归 `docs/INDEX.md`；测试注释只写断言结论；机器门禁见两个 crate 根的
`#![warn]` 与 `cargo xtask doc`（`RUSTDOCFLAGS=-D warnings`，零 warning）。
