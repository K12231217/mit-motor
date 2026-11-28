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
因此 `bsp` 与一切"上板行为"结论只能是模式 A 或 `PENDING_HIL`，**不得报"已验证"**。

## 1. 底层物理铁律（PRE）

| # | 铁律 | 检查手段 |
|---|---|---|
| PRE-01 | **无堆**：链接产物零 `__rust_alloc`；`algorithms`/`app` 不得引入 `alloc` | `llvm-nm <elf> \| grep __rust_alloc` 为空 |
| PRE-02 | **20 kHz 环纯净**：只由硬件事件驱动，禁 `await` / `defmt` / `.lock()` / 堆；预算 ≤ 25 µs，DWT 实测值进遥测 | `cargo xtask arch` I4：`software/firmware/src/bin/main.rs` 的 TIM1 ISR 体零命中（`app` 落地后改指向 `app.motor.task_current`） |
| PRE-03 | **unsafe 白名单**：仅 `software/bsp/src/**` 与 `software/app/src/{fault,sections}.rs`；其余 crate 根 `#![forbid(unsafe_code)]` | `grep -rn unsafe` 白名单比对 |
| PRE-04 | **跨优先级只有两种控制通道**：seqlock `Snapshot<T>` 与有界队列；**单写者**；满即丢并计数；禁重试循环。**诊断例外（ADR-16）**：只读、单向、有界的诊断数据（录波环、`AtomicU32` 计数）可绕过，条件是单写者、读者不回灌控制、写者内无锁/分配/重试、并在模块注册表登记 | 每个队列有 `dropped` 计数器且进状态字；诊断通道在注册表有 owner/contract |
| PRE-05 | **无静默继续**：时钟/CRC/magic 校验失败一律进 fault dump；禁 `unwrap/expect/indexing_slicing`（clippy deny）；关键错误不得 `let _ =` 吞掉 | `cargo xtask clippy -D warnings` |
| PRE-06 | **量纲进标识符**：`_us/_rad/_rad_s/_amps/_v/_duty/_raw/_lsb`；同一参数不得跨模式换量纲 | review |
| PRE-07 | **数值唯一来源**：器件事实与电气硬约束在 `software/contracts/src/motor/config_hw.rs`，时序与容量在 `software/contracts/src/joint/config_budget.rs`，策略参数在 `software/algorithms/src/motor/config_policy.rs`；文档只引编号不复写数值 | grep 数值重复 |
| PRE-08 | **安全默认态**：上电六路输入全低、`nSLEEP` 低、`MOE=0`；S0–S5 全 PASS 才允许使能；故障**一律锁存**，清除需显式命令 + 无故障持续 T | 冷上电 LA 抓时序（E6） |
| PRE-09 | **数字只由工具产生**：Flash/RAM/时序数字一律用 `cargo xtask size` 或实测输出回填，**禁止手写** | 文档里的数字必须能追到一条命令 |
| PRE-10 | **契约唯一真源**：引脚只在 [`docs/hardware-requirements.md`](docs/hardware-requirements.md) §3.1（2026-11-27 网表）；文件架构只在 [`docs/software-architecture.md`](docs/software-architecture.md)；硬件事实只在 HR §3.1/§5。其它文档只许指针，不许复制 | 改引脚时 grep 全仓只应命中一处 |

**PRE-05 补充（配置期断言）**：器件事实、帧布局、队列深度、位域用 `const _: () = assert!(...)` 钉死；运行期初始化失败仍进 fault dump，不用 `ASSERT`/`panic!` 代替。

**PRE-08 补充（唯一输出边界）**：`f32 → PWM/CCR` 的最后一次转换必须完成 `NaN → 0`、`±∞ → 饱和`、`D_MAX` 夹紧；只允许这一处做，上游热路径不得重复 `isfinite`/限幅。现行实现：`Q15::from_ratio`（`software/contracts/src/motor/protocol_types.rs`）与 `ClampedModulation`（同文件）。

## 1.1 机制铁律（MECH）

> 管的是"对象放在哪、怎么分发、失败怎么表达、panic 分支从哪来"。
> 与 PRE-01…10 并列，违反同属红线。`system`/`app` **未建**、`bsp` 是 T-011
> 脚手架（函数体 `todo!()`）时，只对已存在的代码生效；未建/未实现的检查不得报成 PASS（同 §2 口径）。

| # | 铁律 | 检查手段 |
|---|---|---|
| MECH-01 | **驱动对象不设全局 `static`**。外设在 `init` 里构造成确定已初始化的具体类型，用 move 交给任务/结构体；RTIC 的 `Local`/`Shared` 资源由框架持有，源码里不得出现 `static mut`、`static Option<驱动>`、`OnceCell`/`OnceLock` + 运行期 `take()`。跨任务只共享**最新值**（本仓 `Snapshot<T>`；若改用 `embassy_sync::watch::Watch`，须先写 ADR），绝不共享驱动对象 | `rg -n "static\s+mut|static\s+.*Option<|OnceCell|OnceLock" bsp app`（未建时 N/A）；除 `Snapshot`/`Watch` 外共享驱动 = review 打回 |
| MECH-02 | **异构设备用 enum 静态分发**。设备族在编译期封闭，`enum` + `match`；禁 `dyn Trait`、禁 `Box<dyn …>`/`&dyn …` 字段。trait 只能作编译期契约，且单板至多一个实现；真需要泛型时最多一个实现参数，单板只单态化一次。不为"通用库"预留实现位 | `rg -n "dyn [A-Z]"` 在 `bsp`/`system`/`app` 为空；`cargo xtask size` 的 `.rodata` 不因 vtable 增长 |
| MECH-03 | **驱动就绪与固定配置由 `init` 一次建立。** 每个新值只在首次进入其责任边界时校验一次。热路径直接消费已验证的驱动和值。采样完成、时效和故障等运行期事实，由唯一责任模块判定并处理。 | 评审驱动构造路径、输入类型和热路径函数体。重复检查驱动是否就绪或重验上游已保证的范围，评审打回。返回类型按操作语义定，签名搜索仅作辅助。 |
| MECH-04 | **热路径零隐式 panic**。禁 `arr[i]`、`unwrap`/`expect`、切片与 `assert!`（`debug_assert!` 也不得进热路径）。用 `iter().zip()`、定长数组、`get()` + 计数。索引/解包的 panic 分支会拉入 `core::panicking`，带消息格式化时连 `core::fmt`；是否真的变大由 `cargo xtask size` 和 `llvm-nm` 判定，不靠估计 | crate 根 `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]`；`cargo xtask clippy`；release ELF 里 `core::fmt` 相关符号必须能解释 |
| MECH-05 | **模块名由四轴推导，owner 唯一**：`<layer>.<domain>.<role>[_<qualifier>]`，四轴取自 `docs/software-architecture.md` §1.1 的闭集；有可变状态的模块只有一个写者。名字不允许同义词，注册表外的模块不存在 | `cargo xtask hygiene` 的 H6：每个源码文件恰好归一个模块、ID 唯一且四轴合法、owner/contract 非空 |

## 2. 门禁命令（唯一入口 `cargo xtask`）

**现在可用（2026-11-21 复核）**：

```sh
cargo xtask test        # = cargo test --workspace：host 单测（条数以命令输出为准）
cargo xtask check       # cargo check
cargo xtask fmt-check   # rustfmt --check
cargo xtask clippy      # firmware + 根 workspace 各一遍 clippy -D warnings
cargo xtask rel         # release 构建（验收构型）
cargo xtask size        # .text/.rodata/.data/.bss + 镜像 vs memory.x
cargo xtask arch        # 架构不变量 I1/I2/I3/I4/I7（纯 Rust：software/xtask/src/checks.rs，不经 bash）
cargo xtask doc         # cargo doc --no-deps + RUSTDOCFLAGS=-D warnings（注释规范见 docs/archive/comment-style.md）
cargo xtask hygiene     # H2 无主 TODO / H5 注释标记 / H4 文件架构 / H6 模块 ID+owner+state 注册表
cargo xtask tier        # H3：Tier B 路径有改动且无 HIL 证据时 FAIL（基线缺省 origin/main 或 $TIER_BASE）
```

H1–H6 的判定边界与已知限制、S1–S3 软协议 → [`docs/archive/agent-guardrails.md`](docs/archive/agent-guardrails.md)。
本节的命令是唯一入口，本文不复写实现细节。

**未落地 —— 禁止在任何文档里当作"已有"引用**：`tools/replay`、`board.yaml`、`self_test`、
`doc_check.py`、任何 HIL 结果；`system`/`app` 未建，`bsp` 的 `bus_encoder`/`Board::init`
仍是 `todo!()`，`arch` 实现 I1/I2/I3/I4/I7，I5–I6 无检查实现。新建它们属于 Thread 的一部分，落地后才进本节。

**Worker 交付封顶线（Level 2.5）**：上述命令全绿 + 回执附命令原文输出 + 触及文件清单。
`cargo xtask tier` 报 Tier B 时，交付状态只能是 `PENDING_HIL`（不得写"已验证"）。
涉及**电机出力 / CAN-485 时序 / 保护动作 / 标定参数生效**的改动 → Thread 置 `PENDING_HIL`，由人在台架签收（Level 4）。

## 3. 读写边界（Router / Worker）

- **Router**（主对话）：只写 `docs/INDEX.md`、`.scratch/handoff.md`、本文件，以及**空壳脚手架**
  （目录 + 类型/结构体 + 函数签名 + 顶部 `PRE` 注释 + `todo!()`）。**不写业务实现**。
- **Worker**：只填函数体与测试；**不许改签名、不许改 `PRE` 注释、不许顺手优化**。
- 本仓单人开发、无 CI 分支保护，用**两次提交**替代 worktree 隔离：
  `wip(<thread>): 冻结脚手架` → 实现提交。发现腐化就回到第一个 commit 重来，不打补丁超过 2 轮。
- 遇规格冲突或缺项**立即停工上报**，零猜测。

## 4. 不二过台账

完整 14 条与历史在 [`docs/archive/lessons.md`](docs/archive/lessons.md)；现行红线只保留最常触发的 4 条：

| # | 拦截动作 |
|---|---|
| 1 | 料号/引脚/量程只来自手册原文行或最新网表，不能按型号规则猜。 |
| 2 | 线协议枚举必须有 `Unknown = 0`；全零快照不得被读成可信。 |
| 3 | 输出边界只能在 `bsp` 一处夹紧；`svpwm` 不得由调用方传满量程。 |
| 4 | “不确定度/协方差”字段必须两侧闭合：默认值 + 设置路径 + 非零时行为改变的测试。 |

新增条目先过三问，再写入 archive 的完整台账。

## 5. 上下文装载（换会话不断档）

新会话固定读：本文件 → `docs/INDEX.md` → `.scratch/handoff.md` → 跑 `cargo xtask test && cargo xtask check`。
输出 5 行启动简报（阶段 / 本会话验收门 / 不做的事 / 门禁结果 / 待裁决项）后再动手。
长日志外存文件，对话内只留路径指针。
