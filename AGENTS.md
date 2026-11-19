# mit-motor 仓库规范（工程红线 + 门禁）

> 本文件是《开发规范 v2.0》`RULES.md` 的**寄生位**（§6.1：存量仓库不另建 `RULES.md`，避免多源漂移）。
> 上位法：`C:\Users\33696\.agents\AGENTS.md`。规格事实表 → [`docs/INDEX.md`](docs/INDEX.md)。
> 活跃 Thread 与交接 → [`.scratch/handoff.md`](.scratch/handoff.md)。
>
> 新增条款必须过三问：**没它会失效吗？能合并进现有条款吗？6 个月后还用吗？**

## 0. 开工先定模式（意图分级）

| 模式 | 适用 | 门禁 |
|---|---|---|
| **A 探针** | 摸底 embassy/PAC 行为、验证寄存器映射、实测外设怪癖 | 免门禁，限时 10–30 min，代码即用即抛，只把实测事实写进 `docs/INDEX.md` |
| **B 生产** | 算法、状态机、跨模块契约、中断/共享写 | 五步闭环：对齐 → 空壳脚手架冻结 → 派发 → 分级审核 → 归档 |

**本仓库当前阶段的事实**：无 G474RBT6 实板（PCB14 上是 G491RET6，且未焊 G474）。
因此 `1_bsp` 与一切"上板行为"结论只能是模式 A 或 `PENDING_HIL`，**不得报"已验证"**。

## 1. 底层物理铁律（PRE）

| # | 铁律 | 检查手段 |
|---|---|---|
| PRE-01 | **无堆**：链接产物零 `__rust_alloc`；`3_user`/`4_app` 不得引入 `alloc` | `llvm-nm <elf> \| grep __rust_alloc` 为空 |
| PRE-02 | **20 kHz 环纯净**：只由硬件事件驱动，禁 `await` / `defmt` / `.lock()` / 堆；预算 ≤ 25 µs，DWT 实测值进遥测 | `grep -n "await\|defmt\|\.lock()" 4_app/src/tasks/current.rs` 为空 |
| PRE-03 | **unsafe 白名单**：仅 `1_bsp/src/**` 与 `4_app/src/{fault,sections}.rs`；其余 crate 根 `#![forbid(unsafe_code)]` | `grep -rn unsafe` 白名单比对 |
| PRE-04 | **跨优先级只有两种通道**：seqlock `Snapshot<T>` 与有界队列；**单写者**；满即丢并计数；禁重试循环 | 每个队列有 `dropped` 计数器且进状态字 |
| PRE-05 | **无静默继续**：时钟/CRC/magic 校验失败一律进 fault dump；禁 `unwrap/expect/indexing_slicing`（clippy deny）；关键错误不得 `let _ =` 吞掉 | `cargo xtask clippy -D warnings` |
| PRE-06 | **量纲进标识符**：`_us/_rad/_rad_s/_amps/_v/_duty/_raw/_lsb`；同一参数不得跨模式换量纲 | review |
| PRE-07 | **数值唯一来源**：器件事实与电气硬约束在 `0_contracts/src/limits/hw.rs`，时序与容量在 `limits/budget.rs`，策略参数在 `3_user/src/policy.rs`；文档只引编号不复写数值（现状：三处已分离，`limits/mod.rs` 汇总） | grep 数值重复 |
| PRE-08 | **安全默认态**：上电六路输入全低、`nSLEEP` 低、`MOE=0`；S0–S5 全 PASS 才允许使能；故障**一律锁存**，清除需显式命令 + 无故障持续 T | 冷上电 LA 抓时序（E6） |
| PRE-09 | **数字只由工具产生**：Flash/RAM/时序数字一律用 `cargo xtask size` 或实测输出回填，**禁止手写** | 文档里的数字必须能追到一条命令 |
| PRE-10 | **契约唯一真源**：引脚只在 [`docs/hardware-requirements.md`](docs/hardware-requirements.md) §3；文件架构只在 [`docs/software-architecture.md`](docs/software-architecture.md)；硬件事实只在 [`docs/architecture.md`](docs/architecture.md) §2。其它文档只许指针，不许复制 | 改引脚时 grep 全仓只应命中一处 |

## 1.1 机制铁律（MECH）

> 管的是"对象放在哪、怎么分发、失败怎么表达、panic 分支从哪来"。
> 与 PRE-01…10 并列，违反同属红线。`1_bsp`/`2_system`/`4_app` **未建**时，
> 只对已存在的 crate 生效；未建 crate 的检查不得报成 PASS（同 §2 口径）。

| # | 铁律 | 检查手段 |
|---|---|---|
| MECH-01 | **驱动对象不设全局 `static`**。外设在 `init` 里构造成确定已初始化的具体类型，用 move 交给任务/结构体；RTIC 的 `Local`/`Shared` 资源由框架持有，源码里不得出现 `static mut`、`static Option<驱动>`、`OnceCell`/`OnceLock` + 运行期 `take()`。跨任务只共享**最新值**（本仓 `Snapshot<T>`；若改用 `embassy_sync::watch::Watch`，须先写 ADR），绝不共享驱动对象 | `rg -n "static\s+mut|static\s+.*Option<|OnceCell|OnceLock" 1_bsp 4_app`（未建时 N/A）；除 `Snapshot`/`Watch` 外共享驱动 = review 打回 |
| MECH-02 | **异构设备用 enum 静态分发**。设备族在编译期封闭，`enum` + `match`；禁 `dyn Trait`、禁 `Box<dyn …>`/`&dyn …` 字段。trait 只能作编译期契约，且单板至多一个实现；真需要泛型时最多一个实现参数，单板只单态化一次。不为"通用库"预留实现位 | `rg -n "dyn [A-Z]"` 在 `1_bsp`/`2_system`/`4_app` 为空；`cargo xtask size` 的 `.rodata` 不因 vtable 增长 |
| MECH-03 | **热路径返回 `()`，校验收口在构造点**。`start`/`step`/`set_duty`/`take_sample` 等每拍入口不返回 `Option`/`Result`；合法性由类型保证（`Duty` 已夹限、`RawSample3` 已满 3 路）。`try_new() -> Result<_, _>` 只在 `init`/命令层调一次。算法层的"可解性/查询"（如 `GainGate::pred_pp() -> Option<f32>`）不在此限——那是值的语义，不是外设状态 | `rg -n "fn (start|step|set_duty|take_sample)\([^)]*\)\s*->\s*(Option|Result)"`（未建时 N/A）；构造点之外的 `is_within_limits`/范围校验 = review 打回 |
| MECH-04 | **热路径零隐式 panic**。禁 `arr[i]`、`unwrap`/`expect`、切片与 `assert!`（`debug_assert!` 也不得进热路径）。用 `iter().zip()`、定长数组、`get()` + 计数。索引/解包的 panic 分支会拉入 `core::panicking`，带消息格式化时连 `core::fmt`；是否真的变大由 `cargo xtask size` 和 `llvm-nm` 判定，不靠估计 | crate 根 `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]`；`cargo xtask clippy`；release ELF 里 `core::fmt` 相关符号必须能解释 |

## 2. 门禁命令（唯一入口 `cargo xtask`）

**现在可用（2026-11-16 实测全 PASS）**：

```sh
cargo xtask test        # = cargo test --workspace：host 单测（状态见 docs/INDEX.md §4 落差 4）
cargo xtask check       # cargo check
cargo xtask fmt-check   # rustfmt --check
cargo xtask clippy      # clippy -D warnings
cargo xtask rel         # release 构建（验收构型）
cargo xtask size        # .text/.rodata/.data/.bss + 镜像 vs memory.x
cargo xtask arch        # 架构不变量 I1/I2/I3（纯 Rust：xtask/src/checks.rs，不经 bash）
cargo xtask doc         # cargo doc --no-deps + RUSTDOCFLAGS=-D warnings（注释规范见 docs/comment-style.md）
```

**未落地 —— 禁止在任何文档里当作"已有"引用**：`tools/replay`、`board.yaml`、`self_test`、
`doc_check.py`、任何 HIL 结果；`1_bsp`/`2_system`/`4_app` 三个 crate 未建，`arch` 只实现 I1/I2/I3，
I4–I6 无检查实现。新建它们属于 Thread 的一部分，落地后才进本节。

**Worker 交付封顶线（Level 2.5）**：上述八条全绿 + 回执附命令原文输出 + 触及文件清单。
涉及**电机出力 / CAN-485 时序 / 保护动作 / 标定参数生效**的改动 → Thread 置 `PENDING_HIL`，由人在台架签收（Level 4）。

## 3. 读写边界（Router / Worker）

- **Router**（主对话）：只写 `docs/INDEX.md`、`.scratch/handoff.md`、本文件，以及**空壳脚手架**
  （目录 + 类型/结构体 + 函数签名 + 顶部 `PRE` 注释 + `todo!()`）。**不写业务实现**。
- **Worker**：只填函数体与测试；**不许改签名、不许改 `PRE` 注释、不许顺手优化**。
- 本仓单人开发、无 CI 分支保护，用**两次提交**替代 worktree 隔离：
  `wip(<thread>): 冻结脚手架` → 实现提交。发现腐化就回到第一个 commit 重来，不打补丁超过 2 轮。
- 遇规格冲突或缺项**立即停工上报**，零猜测。

## 4. 不二过台账

| # | 现象 | 根因类别 | 拦截动作 |
|---|---|---|---|
| 1 | 把 `MP6540HGU-A-Z` 读成 PWM+EN 的 H 版 | 流程缺口：靠型号规则猜，未查一手手册 | 料号/引脚/量程只能来自 `board.yaml` 或手册**原文行**；文档结论必须带来源 |
| 2 | 手写 Flash 数字互相矛盾（`hardware-requirements.md` §9 的 64,632/16,080 与实测镜像 17,768 不同，来自另一次工具链） | 流程缺口 | PRE-09 |
| 3 | `stm32-metapac` 的 G4 SRAM 数字错（`g474rb/re` 给 96K） | 工具缺陷 | `firmware/memory.x` 手写，**禁止**开 `memory-x` |
| 4 | TIM2 被 embassy `time-driver-any` 占用 | 工具局限 | µs 时基用 TIM5；新增定时器前先查 embassy `build.rs` 的 any→TIM 映射 |
| 5 | debug 构型已占 52 % Flash，全量应用会超 128 K | 真违规：预算未前置 | HIL 出数一律 release；确需 RTT 时 `[profile.dev] opt-level = 2` |
| 6 | RTIC 模板的 25 MHz monotonic 与 G4 默认 HSI16 不符 | 工具局限 | 改时钟树必须同步 `Mono::start(...)` 频率 |
| 7 | `σ̂_e` 的 √3 被双计：预测式里已有显式 √3，实现又用残差 σ（=√3·σ_ch）再乘一次，`pred` 第一项虚高 3 倍 | 真违规：同量纲两种口径混用 | 噪声口径必须写明"单通道 vs 残差方差"：`σ̂_ch=√(sse/(3(weight−2)))` 是单通道，残差方差 = 3σ_ch²，公式里的 √3 只乘一次 |
| 8 | 把 `d = det S/tr²S` 当跨工况阈值（离线实测反序：arc 工况 `d=0.250` 误差 100 pp；单方向+变幅 `d=2.5e-7` 误差 1.40 pp；旋转 `d=4.1e-9` 误差 0.32 pp。实验脚本未入仓，数字来自 2026-11-16 数值实验记录） | 真违规：门限只在单一工况族内标定 | 任何门限必须给"跨工况单调性"证据；`d` 只作**同工况内**的无激励/秩亏门（`D_MIN=0.01`），不作跨工况精度判据 |
| 9 | `#[repr(C, packed)]` + `forbid(unsafe_code)` + `deny(indexing_slicing)` 三者叠加：取字段要 `read_unaligned`（unsafe），按字节索引又被 clippy 禁，实现无路可走 | 设计封死：约束组合自相矛盾 | 线协议优先**自然对齐布局**（字段顺序/填充自己排，`size_of`+`offset_of!` 断言守形状），`packed` 只留给真正省字节且无解包路径的帧 |
| 10 | 契约层混入策略参数：算法超参（`R_MAX`/`KAPPA`/`TARGET_PP`）与容错常数曾和 `D_MAX`、队列深度并列在一个 `limits.rs`，`0_contracts` 成了"调参表"（2026-11-16 已拆分） | 真违规：层次边界侵蚀 | `0_contracts` 只放器件事实与电气互锁断言（`limits/hw.rs`、`limits/budget.rs`）；策略参数进 `3_user/src/policy.rs`。review 时见 `0_contracts/src/limits/` 出现可调超参即打回 |
| 11 | 字段有初值、却没有设置者：`GainAccum::do2` 曾有默认值 `0.0` 且构造后无写入路径，`pred` 第二项 `(σ̂_e² + δo²)` 退化成 `σ̂_e²`——零位残余不确定度从未生效 | 真违规：不确定度字段两侧未闭合 | 任何"不确定度 / 协方差"类字段必须两侧闭合：有默认值 + 有设置路径 + 有测试证明它非零时行为改变（`do2 > 0` ⇒ `pred` 必须更大）。现行闭合点：`gate.rs` 构造取 `do2_from_half_width(DO_LSB_HALF_WIDTH)`、`set_zero_uncertainty_lsb` 重算、`pred` 第二项使用 |
| 12 | 容错常数写成裸整数却与物理量耦合：原 `ENC_MAX_MISS = 5` 折算 `(5+1) × 45 LSB × 14 极对 × 360 / 16384 ≈ 83°` 电角，"5 帧"无法解释、改转速时也不知道要不要改 | 真违规：时间窗被截成拍数 | 凡是"能容忍多久 / 多久算丢多久"的常数必须由时间窗或物理界限派生，并配编译期断言；现行做法见 `3_user/src/policy.rs`：`ENC_MAX_MISS=1` 由 `THETA_E_MAX_DEG=30` 与 `MAX_BLIND_US` 反解，文件末尾两条 `const _` 守混叠底线与电角度底线 |

## 5. 上下文装载（换会话不断档）

新会话固定读：本文件 → `docs/INDEX.md` → `.scratch/handoff.md` → 跑 `cargo xtask test && cargo xtask check`。
输出 5 行启动简报（阶段 / 本会话验收门 / 不做的事 / 门禁结果 / 待裁决项）后再动手。
长日志外存文件，对话内只留路径指针。
