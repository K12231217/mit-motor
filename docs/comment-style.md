# 注释规范

依据（一手）：
rustdoc book《How to write documentation》建议摘要句独占一行、组件按「摘要 → 细节 → 示例」
组织，并在会 panic 时写 `# Panics`
<https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html>；
Rust API Guidelines `C-CRATE-DOC` / `C-EXAMPLE` / `C-FAILURE` / `C-LINK`
<https://rust-lang.github.io/api-guidelines/documentation.html#c-failure>（官方编号没有
`C-DOC-*`）；clippy `doc_markdown` / `too_long_first_doc_paragraph` / `doc_lazy_continuation`
<https://rust-lang.github.io/rust-clippy/master/index.html#doc_markdown>；rustfmt 默认
`max_width = 100`、`wrap_comments = false`（`rustfmt --print-config default`），注释不折行。

1. 摘要句：公开项第一行是一句陈述句，以「。」结尾，独占一行，细节从第二段开始。
   正：`/// 电流环周期预算，单位 µs。`
   反：`/// 电流环周期预算：25 µs（含快照发布 < 0.5 µs）。DWT 实测值进遥测。`
2. 段落：摘要与细节之间空一行；每段只谈一个主题（为什么 / 约束 / 算法），不复述签名
   已表达的类型和单位。
   正：`/// 采样窗口下限，单位 ns。` 空行 `/// MP6540HA 低边建立 2.0 µs + ADC 2.47 µs。`
   反：`/// 返回 Result<Self, DecodeError>，失败时是 Err(DecodeError::Rejected)。`
3. `# Errors`：公开的 `Result` 函数必须写，逐条列出每个错误变体的触发条件（C-FAILURE）。
   正：「# Errors」小节，空行后写 `ver` 与 [`ICD_VERSION`] 不符，或任一 `rsvd` 非 0。
   反：只写「解码失败时返回错误」，不说哪种输入会失败。
4. `# Panics` / `# Safety`：只在真会 panic、真 unsafe 时写；本仓 `forbid(unsafe_code)`
   且 `deny` 了 `unwrap` / `expect` / `indexing_slicing`，因此两者一律不写，也不写
   「不会 panic」这类空段落。正：（没有该小节）反：`# Panics` 加 `本函数不会 panic。`
5. 三种注释分工：`//!` 写 crate 与模块；`///` 写公开项（含公开字段与枚举变体）；
   `//` 写私有实现的「为什么」。
   正：`// 单写者 ⇒ 进入时 seq 必为偶，+1 奇、+2 偶，无需 CAS。`
   反：私有 `fn sqrt_f32` 用 `///` 写成「牛顿迭代 4 次，初值折半指数位模式」。
6. 反引号与混排：标识符、类型、常量、字段、表达式一律加反引号；中文与 ASCII、数字
   之间空一格。
   正：「/// 判据是 `pred > KAPPA · target_pp`，不降额会放过 p99 超差。」
   反：「/// `pred > KAPPA·target_pp`：精度不够」。
7. 禁止内容：历史（「旧值 3 µs…已作废」）、评审与流程状态（FROZEN / OPEN / 评审记录）、
   任务指令（「写进实现时…不要改形状」）、粗体当标题、ASCII 分隔线。
   正：`/// 相对增益的共模分量不可观测，因此只上报 B/C 两路。`
   反：`// ---------- 标题 ----------` 与 `/// **参数可辨识性的前提**`。
8. 小节标题：模块级用 `//! # 小节名`；项内只允许 `# Errors` / `# Panics` /
   `# Safety` / `# Examples` 四个标准小节，不自造标题，不用「小节名 + 空行」伪装。
   正：`//! # 协议` 下一行 `//! seq 为奇数表示正在写。`
   反：`//! **协议**`。
9. 行宽：≤ 100 显示列（CJK 记 2 列），手工折行；rustfmt 不检查注释宽度，超宽只能靠
   评审拦住。正：一行约 40 个汉字后换行。反：一行 140 列的 doc comment。

仓库约定：常量文档写「值 + 物理依据」（为什么是这个值），验证状态与历史归
`docs/INDEX.md`；测试注释只写断言结论；机器门禁见两个 crate 根的 `#![warn]` 与
`cargo xtask doc`（`RUSTDOCFLAGS=-D warnings`，零 warning）。
