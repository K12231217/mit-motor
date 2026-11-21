# docs/INDEX.md — 客观事实表 & 文档地图

> 本文件是《开发规范 v2.0》`SPEC.md` 的**寄生位**（§6.1：`SPEC.md` 寄生于 `docs/INDEX.md`）。
> 只记**代码签名表达不了**的宏观事实，以及 2 个以上候选方案的决策记录。
> 工程红线 → [`../AGENTS.md`](../AGENTS.md)。活跃 Thread → [`../.scratch/handoff.md`](../.scratch/handoff.md)。
> **增量冷启动**：改哪个模块就冻结哪个模块的局部事实，不要求全量补齐。

## 1. 一页事实表

| 项 | 值 | 证据 |
|---|---|---|
| 主控 | **STM32G474RBT6**，LQFP64，128 KB Flash / 128 KB SRAM | 2026-11-15 定稿；RC/RE 同脚位仅差 Flash。2026-11-16 料号变更获批：任务书题目 `STM32G491` → `STM32G474RBT6`（同封装 LQFP64，逐脚核对兼容），教务文字由老师/教务处理；**代码与文档以 G474RBT6 为准** |
| SRAM 划分 | 链接器只给 **RAM 96 K** @0x20000000（SRAM1 80 K + SRAM2 16 K）+ **CCMRAM 32 K** @0x10000000；CCM 在 `0x1000_0000` 只有 CPU 可达，在 `0x2001_8000` 有 SBUS 别名、DMA1/2 可达；DBANK=1 ⇒ Flash 只链接 bank 1 的 64 K，bank 2 留给标定档案双槽（地址不连续） | DS12288 §3.5（分块、零等待、别名）；`firmware/memory.x`（手写，勿开 `memory-x`）；bank 几何见 ADR-09 |
| 驱动 | **MP6540HA**（`MP6540HGU-A-Z`），HS/LS 六电平输入，**无内部死区、无 EN 脚** | `docs/architecture.md` §2.2 |
| 电流采样 | SOx 比例 1/9200；4.99k/4.99k 偏置 → 0.2712 V/A，2.97 mA/LSB；三路独立 ADC | `docs/hardware-requirements.md` HR-203/HR-201 |
| 时钟 | HSE 16 M → PLL M=4/N=85/R=2 ⇒ **170 MHz**（boost）；ADC 42.5 MHz；**无 48 MHz/USB 支路**（板载 USB 已砍，见 §1 已拍定口径） | HR-301/303（HR-304 的 USB 时钟已不实现，见 §4 落差 9） |
| PWM | TIM1 中心对齐 **20 kHz**，ARR=4250，死区由 BDTR 产生 | HR-302 |
| 工具链 | `nightly-2025-11-05-x86_64-pc-windows-gnu`（本机无 MSVC）；`thumbv7em-none-eabihf` | `rust-toolchain.toml` |
| 实测 rustc | 1.93.0-nightly (f15a7f385 2025-11-04) | `rustc --version`，2026-11-15 |
| 构建入口 | `cargo xtask <cmd>`（`check/build/rel/fmt/fmt-check/clippy/test/size/flash/rtt/arch/hygiene/tier/help`），唯一实现 `xtask/src/main.rs`；IDE 与 CI 只调它 | `.cargo/config.toml`、`xtask/src/main.rs` 的 `help()` |
| workspace 结构 | 根 `Cargo.toml` = workspace，成员只有 `0_contracts` + `3_user`，`exclude = ["firmware", "xtask"]`；`firmware/` 仍是独立 workspace（单 bin `blinky`）；`1_bsp`/`2_system`/`4_app` **未建** | 根 `Cargo.toml`、`firmware/Cargo.toml` 自带 `[workspace]` |
| 烧写目标 | `probe-rs --chip STM32G474RB --connect-under-reset` | `firmware/.cargo/config.toml` |
| 运行时 | RTIC 2.4 + `embassy-stm32` 0.1.0（`stm32g474rb`）混合；**单 bin `blinky` 模板，无电机代码** | `firmware/Cargo.toml` |
| 镜像基线 | release 镜像 **17,768 B / 27.1 %**（Flash 区 65,536 B = 64 KiB）；`.text` 14,632 / `.rodata` 2,552 / `.data` 112 / `.bss` 480 | `cargo xtask size`，2026-11-16 实测 |
| 门禁基线 | `test` / `check` / `fmt-check` / `clippy`（firmware + 根 workspace）/ `rel` / `size` / `arch`（= I1/I2/I3/I7）/ `hygiene`（H2/H4/H5）全 PASS；`tier`（H3）按基线分支判定，Tier B 无 HIL 即 FAIL（均为 `cargo xtask <cmd>`；数字以各命令输出为准） | 2026-11-21 实测 |
| 门控定版 | 四维结构门 `R ≥ 0.995`、`d ≥ 0.01`、`z ≤ 0.02`、`pred ≤ 0.9·0.5 pp`；`χ²(2) ≤ 9.21` **只在结构门之后**算；`z=\|A0\|·√3/√(G·A1)`、`G=200`、`λ=1−1/2000`、`N_eq ≤ 3999`；**组窗隔离**：200 拍独立累加、组内不写主窗口，结构门 + 组内前后半窗 χ² + 跨组 χ² 全过才并入并刷新生效增益，`pred` 只决定能否发布 | 实现 `3_user/src/gate.rs`（`GateParams`/`GainGate`）与 `gate/accum.rs`（`GainAccum`、私有 `GroupWindow`/`HalfWindow`、两级 χ²）、`gate/math.rs`（无 libm 数值）；数值唯一来源：器件/电气在 `0_contracts/src/limits/hw.rs`、时序容量在 `limits/budget.rs`、策略阈值在 `3_user/src/policy.rs`（三者已分离，`limits/mod.rs` 汇总；两处不许互抄数值）。已数值否定、禁止复活（离线实验，脚本未入仓）：`d` 当跨工况阈值（实测反序，见 AGENTS §4-8）、`λ_min ≥ 135` 这类常数（应现算）、TLS（低幅区本应被门控拒） |
| 算法层内容 | `3_user/src/`：`gate.rs` + `gate/accum.rs` + `gate/math.rs`（状态机 / 正规方程与 χ² / 无 libm 数学；阈值与公式见「门控定版」）、`encoder.rs` + `encoder/{frame,monitor}.rs`（MT6701 帧布局与 CRC / 四方接受判据、丢失拍数放宽与两帧去抖）、`fsm.rs`（状态 × 事件迁移矩阵，纯 `const fn`，按 `state × event × record_valid` 全表穷举；类型在 `contracts/src/fsm.rs`）、`policy.rs`（策略参数：门限/降额/盲推窗口/SPI 分频/清故障窗口，只引用 `contracts::limits` 派生）、`foc.rs`（Clarke/Park/逆 Park + 无 libm 正余弦）、`svpwm.rs`（几何扇区 SVPWM → 比较值）、`sense.rs`（原始码 → 安培）、`pid.rs`（条件积分抗饱和；这四个模块移植自 Apache-2.0 上游，见 ADR-12）；两个 crate 无 `todo!()`，测试条数以 `cargo xtask test` 为准 | `ls 3_user/src/gate 3_user/src/encoder`；`grep -rn 'todo!' 0_contracts/src 3_user/src` 为空 |
| 已拍定口径 | `D_MAX=0.80`；编码器 SPI **10.625 MHz**；故障动作 **coast**（台架不带重力负载）；**标定档案落 bank 2 双槽**（布局见 ADR-09；写入路径随 `1_bsp` 建）；**砍板载 USB**（上位机走 CAN FD） | `0_contracts/src/limits/hw.rs` 的 `D_MAX` 与 `CALIB_SLOT_*`、`3_user/src/policy.rs` 的 `ENC_SPI_HZ`；其余为 2026-11-16 用户裁定，代码落点未建（未验证） |
| 冻结脚手架 commit | `ab1634798e56034f332ec33b1359ea1b23dc4b4e`（T-001 冻结点；仅 xtask/CI/CLion 配置，无业务代码）。T-002 已改动工作区，当前 HEAD **未复核** | `git rev-parse HEAD`（未跑） |
| 任务书字段 | 专业 = **自动化**；任务书原文"轨道交通信号与控制"是笔误（2026-11-16 老师确认） | 2026-11-16 老师定案。**不得**据此推断答辩委员会的关注点 |
| 硬件现状 | **无 G474RBT6 实板**；PCB14 载 G491RET6，缺 HR §1.3 的 12 项实验设施 | `docs/agent-readiness.md` §1 |

## 2. 文档地图（权威等级）

改任何一条事实，**只改"唯一真源"那一处**，其余文档只留指针。

| 文档 | 定位 | 权威范围 |
|---|---|---|
| [`hardware-requirements.md`](hardware-requirements.md) | V2 板需求规格 v0.4（HR-1xx…8xx） | **引脚分配 §3 全仓唯一版本**；需求编号与验收方法 |
| [`software-architecture.md`](software-architecture.md) | 固件文件架构 v0.1（五 crate、不变量 I1–I6、段布局、三个数字契约） | **文件/模块边界唯一版本**（取代 `architecture.md` §4–§6） |
| [`architecture.md`](architecture.md) | 固件架构草稿 v0.1 | **硬件事实 §2**（引脚/驱动行为/采样链/安全路径）与决策历史 |
| [`agent-readiness.md`](agent-readiness.md) | 交 agent 开发前的七类缺口（A–G）+ S0–S8 自检阶梯 | bring-up 阶梯与**验收口径** |
| [`agent-guardrails.md`](agent-guardrails.md) | Agent 协作协议落地版（H1–H5 硬门禁 / S1–S3 软协议 / Skill 回归） | **门禁落点与限制**的唯一说明；命令实现仍在 `xtask/src/{checks,hygiene,tier}.rs` |
| [`hardware-taskbook.md`](hardware-taskbook.md) | 外包硬件任务书 v2.3 | 对外唯一版本（不复制引脚表） |
| [`taskbook-review.md`](taskbook-review.md) | 任务书对抗审核 V2.0 | 审核结论与错误清单 |
| [`research-plan-v1.1-2026-11-04.md`](research-plan-v1.1-2026-11-04.md) | 研究方案 V1.1 | 毕设主线与边界（正文题目仍写 G491；变更已获批，见 §1 主控） |
| [`research-findings.md`](research-findings.md) / [`reference-implementations.md`](reference-implementations.md) | 调研结论 v0.1 / 参考实现 v0.1 | 选型依据，**非契约** |
| `pcb/` | PCB14 网表、BOM、`.epro2` | 已在 `hw/pcb14` 分支，**本分支不含** |

## 3. 决策记录（ADR）

### ADR-01 构建入口 = `cargo xtask`（已定，2026-11-08）
候选：`just`/`make`（要装工具，Windows 上易翻车）、IDE 任务（CLion/VS Code 各写一套）、
**Rust xtask（唯一实现，跨 IDE/CI 同一代码路径）** → 选 xtask。
代价：`xtask` 是独立 workspace，改它不影响固件构建；但**固件路径被硬编码**（见 §4 落差 2）。
- 失效条件：`cargo metadata --no-deps` 出现第二个二进制目标（`firmware/` 之外新增 `[[bin]]`），
  `xtask/src/main.rs` 的 `ELF_REL` 硬编码路径即失效；或 CI / IDE / `.cargo/config.toml` 出现绕过
  xtask 直接调 `cargo build` / `rustc` 的入口（`grep -rn 'cargo build' .cargo .github` 非空）。

### ADR-02 运行时 = RTIC + embassy 混合（已定方向，细节未定）
候选：纯 RTIC + 裸 PAC（`gulitsky/reactance` 实证）、纯 embassy async（`okhsunur/oxifoc` 实证）、
**混合：TIM1/ADC/DMA 裸寄存器 + CAN/UART 用 embassy async 跑最低优先级 executor** → 选混合。
理由：固定优先级抢占 + 编译期静态资源检查满足 PRE-02/PRE-04；embassy async 只服务 CAN/UART 等冷路径
（板载 USB 已砍，见 §1 已拍定口径）。代价：两套中断/单例体系并存，衔接方式尚未验证（见 §5 未决 2）。
- 失效条件：板载 USB 恢复（HR-304 重新生效）或任一冷路径外设必须以 `async` 阻塞等待，使 executor
  进入 20 kHz 环；或 `rtic` 主版本变更（`firmware/Cargo.toml` 版本号首段变化）后
  `embassy_stm32::init()` 返回的 `Peripherals` 不能再交给 RTIC `init`（`cargo xtask check` 失败）。

### ADR-03 文件布局 = 五 crate（已定，**部分落地**：`0_contracts` + `3_user`，其余待建）
候选：单 crate 内模块分层、`{0_contracts,1_bsp,2_system,3_user,4_app}` 五 crate（依赖单向 + `unsafe` 白名单 +
host 可测）→ 选五 crate。检查手段见 `software-architecture.md` §1/§9。
现状：根 workspace 只有 `0_contracts` + `3_user`（§1 workspace 结构）；I1/I2/I3 已机器化（`cargo xtask arch`），
I4–I6 因缺 `4_app` 暂无检查实现。
代价：`3_user` 必须 `no_std` 且能在 x86 上 `cargo test`（`#![cfg_attr(not(test), no_std)]`，已落地）。
- 失效条件：根 workspace 出现第二个 `[[bin]]`（`cargo metadata --no-deps` 的 bin 目标数 > 1）；或
  `3_user/Cargo.toml` 的 `[dependencies]` 出现 `2_system`（`cargo xtask arch` 的 I1 立即失败）。

### ADR-04 `memory.x` 手写、禁用 `memory-x`（已定）
候选：embassy-stm32 `memory-x`（metapac 生成）、手写。
理由：stm32-metapac 15.0.0 对 G4 SRAM 数字错（`g474rb/re` 给 96 K）。
代价：换料号（RC/RE）必须同步改 `memory.x` + `stm32g474rb` 特性。
- 失效条件：§1 主控不再是 RBT6（RC/RE 的 Flash 128 → 256/512 K，`memory.x` 的 `LENGTH` 与
  `stm32g474rb` 特性必须同步）；或 `stm32-metapac` 把 `g474rb` 的 SRAM 声明修正为 128 K
  （比对其生成的内存表与 §1 SRAM 行，一旦相等手写纠正即无必要）。

### ADR-05 µs 时基用 TIM5（已定）
理由：TIM2 被 embassy-stm32 `time-driver-any` 的 `any→TIM2` 映射占用，且其更新中断被 `init` 保留。
- 失效条件：embassy `build.rs` 的 any→TIM 映射改选 TIM5；或 RTC/TAMP 域被用作时基，与 TIM5 的
  时钟树冲突；或 `1_bsp`/`4_app` 出现第二处 `TIM5` 占用（`grep -rn 'TIM5' 1_bsp 4_app`
  命中非时基用途；两个 crate 未建时该条不适用）。

### ADR-06 估计器放 `3_user`，不放 `contracts`（已定，2026-11-16）
候选：把 `GateParams`/`GainAccum`/`GainGate` 连同 `sqrt_f32` 放进契约层（"反正数值都在 `limits.rs`"）、
**放 `3_user`（算法层），`contracts` 只留 `Trust` 与常量（策略阈值另迁 `3_user/src/policy.rs`）** → 选后者。
理由：`contracts` 是零依赖、`forbid(unsafe_code)`、无堆的接口事实表，凡 `1_bsp`/`4_app` 都要依赖它；
把有状态、带 f32 实现与回归测试的估计器塞进去，等于让所有下游 crate 一起编译门控实现，
且契约层会从"只读事实"退化成"业务逻辑仓库"。结果类型 `Trust`（`0_contracts/src/trust.rs`）留在契约层；
门限数值只由名字引用——器件与电气硬约束在 `0_contracts/src/limits/hw.rs`、时序容量在
`limits/budget.rs`、策略阈值在 `3_user/src/policy.rs`（2026-11-16 分层，见 AGENTS §4-10），
正是"跨 crate 只传事实、不传算法"。
否决"放契约层"的理由：契约层一旦有可执行算法，PRE-07 的"数值唯一来源"就退化为"实现在哪数值就在哪"，
阈值与公式开始各存一份；`contracts` 里的骨架测试也会被实现细节淹没。
代价：`3_user` 必须 host 可测（`#![cfg_attr(not(test), no_std)]`），`sin/cos/sqrt` 不外调（自带 `sqrt_f32`）；
`Trust` 变体变动会同时打穿契约层与算法层。
- 失效条件：`grep -rn '&mut self' 0_contracts/src` 非空（契约层开始持有可变状态，算法跟进）；
  或 `0_contracts`、`3_user` 的 `[dependencies]` 出现非 dev 依赖（零依赖前提被打破；I1 只查依赖方向）。

### ADR-07 论文定位 = 基线复现 + 可信性门控 + 可辨识性边界（已定，2026-11-16，导师）
形态：以 **Retianza 2023**（IEEE TIE，DOI `10.1109/TIE.2022.3222697`）为基线做**复现**，叠加**可信性门控**
与**可辨识性边界 / 误差界**；交付以实验数据说明"精度、收敛性、安全性"三项指标（任务书原文）。
否决的候选：以**信息几何耦合**为主线；"**门控即贡献**"。
依据：导师明确"**不需要提出新方法**"；Retianza 2023 已完整覆盖"零和约束下在线相对增益"，
其空白在可信性维度。
- 失效条件：§6 任务书四问或三项指标（精度 / 收敛性 / 安全性）被修订（表头日期与题目变更即触发，
  改题会同时改 §1 任务书字段）；或导师书面撤回"不需要提出新方法"；或 Retianza 2023 出现
  erratum / 撤稿，使基线结论不可复现。

### ADR-08 SPI1 单一所有者：aux 编码器并入 `current` 任务（已定，2026-11-16）
候选：两片 MT6701 各成任务（`current` 以 20 kHz 读主编码器、`aux_encoder` 以 1 kHz 读第二片，共享 SPI1 + 各自 CS）、
**`current` 独占 SPI1：只有 20 kHz 的 `current` 任务碰 SPI1，第二片由 `current` 每 20 拍顺带读一次（1 kHz）** → 选后者。
理由：两片共用 SCK/MISO，若 aux（prio 2）在帧中间被 prio 8 的 `current` 抢占并启动另一片，
会同时有两个驱动源争用 MISO；用互斥锁保护总线又违反 PRE-02（20 kHz 路径禁锁）。
两片 CS 均为软件 GPIO，插在环内串行切换即可；编码器读**不用 DMA**。
代价：`aux_encoder` 不再是独立任务，第二片的读取时刻被 20 拍栅格化（1 kHz，抖动 ≤ 50 µs）；
单拍内要完成两次读，环预算更紧（判据仍是 HR-801 的 < 25 µs）。
- 失效条件：出现第二个 SPI 主设备需求（例如再加一片传感器）；或 SPI1 被改为 DMA 搬运；
  或 `current` 无法在单拍内完成两次读（实测预算不足，须降 PWM 频率或另立任务）。

### ADR-09 状态迁移矩阵放 `3_user`，标定档案落 bank 2 双槽（已定，2026-11-16）
候选（迁移表位置）：`2_system/src/fsm.rs`（`software-architecture.md` 原方案）、
**`3_user/src/fsm.rs`（纯 `const fn`，类型在 `0_contracts/src/fsm.rs`）** → 选后者。
理由：`2_system` 不在 host workspace（根 `Cargo.toml` 成员只有 `0_contracts` + `3_user`），
放那里就等于放弃"`state × event × record_valid` 在宿主机上全表穷举"这一唯一收益；迁移规则是策略，
算法层本就承载策略（ADR-06 同一逻辑）。类型进契约层是因为它们跨 crate 传递并进日志与遥测。
候选（拒绝回执）：不加字段、靠主机轮询模式未变、**占用状态字 bit 15 `cmd_reject`** → 选 bit 15。
理由：模式（0–3）、标定（4–6）、采样/窗口/激励/一致（7–10）、编码器（11–14）、故障码（16–23）、
序号（24–31）都已分配，bit 15 是唯一空位；"没变"与"丢帧"无法区分，拒绝必须有可观测位。
候选（故障表示）：位掩码 `FaultFlags`、**单码 `FaultCode`（首个故障胜出）** → 选单码。
理由：线上故障字段只有 8 bit 且语义是"一个码"，掩码无处安放；单码还保住了"最先出事的那个"。
候选（制动）：新增 `Duty::Brake`（下三管常通）、**暂不提供，故障与停机一律 coast** → 选后者。
理由：`Duty` 的 `Pwm` 调制量由 `ClampedModulation` 在类型层锁在 `|m| ≤ 0.6`（`D_MAX = 0.80` 为保 ≥5 µs 低边采样窗），
制动是 `m = -1`，必须新增第三变体并决定"待机时制动还是自由停车"的硬件策略；台架不带重力负载，
现在没有需要制动的场景，先不做。`LoopAction` 因此只有 `Coast`/`OpenLoop`/`Current`。
候选（标定档案）：不存（每次上电自校准）、**bank 2 内 A/B 双槽，各 2 页（4 KB）** → 选后者。
理由：DBANK=1 时 bank 2 与 bank 1 不连续、且 `Read-while-write`，擦 bank 2 不停顿 bank 1 的取指，
这是"边转边写档案"在硬件上成立的唯一前提（依据见 `firmware/memory.x` 头注释：DS12288 §3.4 +
RM0440 Table 7，由 ST 员工对 G474RB 逐字确认）。槽起点 `0x0804_0000` / `0x0804_1000`，
页大小 2 KB（`hw::CALIB_SLOT_*` 与编译期断言）。
代价：标定档案需要版本与 `board_id` 字段才能判断"哪个槽更新、是否本芯片"，`CalibPayload`（12 B）
尚不含这两项；记录格式与写入路径随 `1_bsp` 落地时一并定，本轮只冻结槽布局。
- 失效条件：`3_user/src/fsm.rs` 不再是迁移的唯一实现（`cargo xtask arch` 的 I7 命中别处
  `MotorState`）；或状态字位图重新分配导致 bit 15 被占用；或 `Duty` 新增制动变体
  （ADR 需重开"制动策略"分支）；或 `1_bsp` 的 flash 驱动无法在 bank 2 内按页擦写
  （例如页大小不是 2 KB，`hw::FLASH_PAGE_BYTES` 与槽断言会先编译失败）。

### ADR-10 驱动所有权 = move 移交；设备族 = enum 静态分发（已定，2026-11-19）

候选（初始化与存储）：全局 `static mut` / `static Option<Periph>` + 运行期 `take()`、
**`init` 构造具体类型后 move 给任务** → 选后者。
理由：`take()` 把"是否初始化"从编译期推到每个热路径分支，初始化顺序错误只能到运行时才炸；
`static mut` 还要额外 `unsafe` 白名单，和 PRE-02 的零锁/零分支相冲。move 之后类型系统直接保证
"拿到就能用"，热路径只剩字段访问。

候选（异构设备）：`dyn Trait` 运行时分发、多层泛型/关联类型的通用库、**enum + `match` 静态分发** → 选后者。
理由：本板设备族封闭（MP6540HA + MT6701；整机 RM/DM/UT 三类），扩展点在编译期就已知；
enum 让新增设备型变成编译错误而不是漏实现，也不为 vtable 和单态化膨胀付费。

校验边界以 [AGENTS.md §1.1 MECH-03](../AGENTS.md) 为准。`Duty::Pwm` 直接消费构造时已夹限的
`ClampedModulation`。`RawSample3` 携带三路码和拍号，同拍归属与采样有效性由采集端在当拍确认。
写入操作可返回 `()`，采样结果与算法查询按数据语义选择返回类型。`software-architecture.md` §9 的
签名搜索用于定位评审入口，验收同时检查构造路径、输入类型和函数体。

代价：跨任务只能拿到 `Snapshot<T>` 的值，拿不到驱动句柄；需要"读改写外设"的需求必须回到所有者任务。
新增设备型必须改 enum（故意）。详见 `AGENTS.md` §1.1 MECH-01…04 与 `software-architecture.md` §3.3。
- 失效条件：固件需要运行时发现/插拔未知设备型（enum 无法穷举）；或 RTIC/embassy 的资源模型不允许
  把 `init` 产出的具体类型 move 进任务（`cargo xtask check` 失败）；或同一功能需要同一固件的第二块
  板不同实现（单态化出现第二份代码，泛型前提破裂）。

### ADR-11 协作门禁 = 命令 + 唯一真源，不生成第二份地图（已定，2026-11-21）

来源：`docs/agent-guardrails.md`（Cursor 分享的 H1–H5 / S1–S3 重构版）。
候选（H4 架构地图）：脚本自动生成 `FEATURE_MAP.md` + CI `--check`、
**只查"每个模块文件是否写进 `docs/software-architecture.md` §2 的文件树"** → 选后者。
理由：生成物立刻成为第二份架构真源，与 PRE-10 的"文件架构唯一真源"直接冲突；而 H4 的真实
目标是"新模块出现时文档同步"，查包含关系就够了。代价：不能从地图反查调用关系，也不能排序/统计。
候选（H3 分级）：人工在 PR 模板里勾选 Tier、**按路径自动判 Tier B + 按 `handoff.md` 的 `HIL: PASS`
行判证据** → 选后者。理由：勾选栏永远能勾"否"；路径表写在 `xtask/src/tier.rs`，改判定必须改代码。
代价：`firmware/memory.x`、`firmware/Cargo.toml` 这类"只改配置"的硬件耦合改动同样算 Tier B
（本分支即活例，`cargo xtask tier` 按设计 FAIL）；单仓无分支保护，直接 push main 的信任边界止于
`github.event.before`。
候选（H5 注释）：照搬"只留 `ERRATA:`/`REF:`"、**取交集：禁历史/流程词、块注释、`// SAFETY:`、
ASCII 分隔线，Why 注释保留** → 选后者。理由：`docs/comment-style.md` §12 要求写 Why（物理依据、
不变量），传感器/电机代码大多数没有外部编号可指，照搬会删掉唯一能读懂门控与编码器判据的注释。
- 失效条件：出现 `5_*` 之类不在 `TIER_B_PREFIXES` 的硬件耦合目录（门禁静默漏判）；或
  `docs/software-architecture.md` §2 不再按 `<crate>/` 分块（`hygiene` H4 整体报缺块）；或
  `cargo xtask clippy` 的根 workspace 一遍被删（H2 只剩 firmware 覆盖）。
- 现状（2026-11-21 裁定）：main 上 `cargo xtask tier` **持续红**，`firmware/` 三个文件等台架证据；
  这是门禁在工作，不是故障。解除条件只有一条：`.scratch/handoff.md` §3 出现独占一行的 HIL 记录。

### ADR-12 信号链 = 移植 Apache-2.0 实现，不自己推公式（已定，2026-11-21）

候选：按教科书自己写 Clarke/Park/SVPWM/PI；**移植 `calebfletcher/foc`（`park_clarke`/`pid`/`pwm`，
Apache-2.0）与 `okhsunrog/oxifoc`（`svpwm`/`current_sense`，Apache-2.0），正余弦移植 `micromath`
（MIT OR Apache-2.0）** → 选后者。
理由：这四项是标准件，手写只会引入符号与扇区错误（本次移植时就抓到一处 S6 通道顺序抄错，
由上游的"占空比还原矢量"测试当场打回）；上游已在 STM32 FOC 上跑通，许可证允许修改与再分发。
本地改动只有三类：定点 `I16F16` → `f32`、量纲后缀、PID 的输出限幅与条件积分（源码无抗饱和）。
候选（`calib`/`safety` 是否独立成模块）：**不建**——相对增益与可信性判据已在 `gate.rs`，
再建一层就是第二条判据路径（PRE-04/I7 的同类风险）。
代价：`3_user` 继承上游的算法约定（SVPWM 的 `1.5·v/vbus` 归一化、PI 的增益单位），换实现要同步改
`4_app` 的调用点；`micromath` 的 0.002 rad 误差进入 Park 变换。
- 失效条件：上游改为 copyleft 许可（当前 Apache-2.0/MIT）；或 `3_user` 为三角/开方引入
  `libm`/`cordic` 依赖（ADR-06 的零依赖前提）；或实板发现 SVPWM 输出不满足低边采样窗口
  （需回到 `D_MAX` 夹紧点，见 `software-architecture.md` §2 的 `pid.rs`/`svpwm.rs` 注释）。

## 4. 文档 vs 代码树：已确认的落差

| # | 文档说 | 树里实际 | 后果 / 处理 |
|---|---|---|---|
| 1 | 根 `Cargo.toml` workspace = 5 个成员（`software-architecture.md` §2） | 根 workspace 只有 `0_contracts` + `3_user`；`1_bsp`/`2_system`/`4_app` 未建 | 已解决一半：成员与 `exclude` 按 ADR-03 落地（见 §1 workspace 结构）；余三 crate 无板不建空壳 |
| 2 | `cargo xtask` 面向整个固件 | `xtask/src/main.rs` 的 `ELF_REL`、`rtt()` 仍硬编码 `firmware/target/thumbv7em-none-eabihf/release/blinky` | 仍成立但未误测：根 workspace `exclude = ["firmware"]`，唯一固件 bin 就是 `blinky`。多 bin 前必须先改 xtask |
| 3 | `ci/arch_check.sh` 六条检查（§9） | 脚本已删除；`cargo xtask arch` 纯 Rust 实现 I1/I2/I3，CI 已接入；I4–I6 未实现 | 已解决 I1–I3；I4–I6 仍无机器校验 |
| 4 | `3_user/tests/` + `fixtures/`（§10 未决 3） | 无独立 `tests/`；测试内联在 crate 源文件里 | 已解决：`cargo xtask test` 全绿（`contracts` + `user`，0 ignored；条数以命令输出为准，不写进文档） |
| 5 | `board.yaml` / `self_test` / `tools/replay`（`agent-readiness.md` §2 A–D） | **均不存在** | agent 可判定性缺口未闭合 |
| 6 | `hardware-requirements.md` §9 的 `64,632 / 16,080 B` | 实测镜像 17,768 B（另一工具链测量） | 违反 PRE-09，待删或改指针 |
| 7 | `hardware-requirements.md` §1 原写 SRAM = SRAM1 80 K + SRAM2 32 K + CCM 16 K | DS12288 §3.5：SRAM1 80 K + SRAM2 16 K + CCM 32 K；`memory.x` 链接 96 K（SRAM1+SRAM2），CCM 声明未链接 | 已按一手手册改正 HR 与 `memory.x` 注释；SRAM2/CCM 的旧写法是笔误，两者对调后总和仍 128 K。CCM 的 `0x2001_8000` SBUS 别名是否启用属独立改动（§5-4） |
| 8 | HR-305 写占空比上限 `d ≤ 90 %` | `0_contracts/src/limits/hw.rs` 的 `D_MAX = 0.80` | 已裁：0.90 在 50 µs 周期下只剩 2.5 µs 共导窗口，低于 HR-305 自己要求的 3 µs；以 `hw.rs` 为准，HR-305 待改 |
| 9 | HR-304 要求 USB 时钟 HSI48+CRS | 板载 USB 已砍（§1 已拍定口径），固件无 USB 路径 | 此处无代码可查（`firmware/` 仍是 `blinky` 模板）；HR-304 待标废弃或删 |
| 10 | `architecture.md` §2.3 说三路 SOx"无采样时刻偏斜" | 跨 dual 对（ADC1/2 ↔ ADC3/4）无硬件同步通路；固定偏斜按 ≤ 50 ns 补偿（`limits/hw.rs` 的 `ADC_PAIR_SKEW_MAX_NS`） | "跨组零偏斜"已证伪（§6 映射 2）；`architecture.md` §2.3 待改 |
| 11 | `architecture.md` §2.2 写 R7/R8 = 2 kΩ/2 kΩ → 7.4 mA/LSB | HR-203 与 `hw.rs` 用 4.99 kΩ/4.99 kΩ → 0.2712 V/A、2.97 mA/LSB；`gate` 测试也按 2.97 构造数据 | 以 HR-203 与 `hw.rs`（`SOX_AMPS_PER_LSB` 由此派生）为准；§2.2 待改，改前不得引用其标度 |

## 5. 未决（阻塞实现，或被刻意留白待人裁定）

1. **`take_sample()` 语义**：挂 ADC3 EOC 中断，还是读上一拍（确定性更好）→ 决定如何确认样本同拍、判定窗口有效性并表达缺样（见 ADR-10）。
2. **`embassy_stm32::init()` 与裸 PAC 单例的衔接**：`steal()` 还是从返回的 `Peripherals` 取。
   无实板 ⇒ 只能读 embassy 源码给结论，寄存器行为属 `UNVERIFIED`；阻塞 `1_bsp/src/pwm.rs`、`1_bsp/src/adc.rs`（均**未建**）。
3. **`INVALID_SAMPLE_LIMIT = 8`**（`3_user/src/policy.rs`，标 `[OPEN]`）：8 × 50 µs = 400 µs 无冻结依据，
   阻塞于 E2/E3 的无效样本比例统计。
4. **CCM 别名是否启用**：DS12288 §3.5 给出 `0x2001_8000` 的 SBUS 别名（DMA 可达），但
   `memory.x` 仍只链接 SRAM1+SRAM2 的 96 K。扩到 128 K 单一区域需要重排栈位置与 `link.x` 段，
   属独立改动，本轮不做。DBANK 与 SRAM 分块已按 DS12288 §3.5 + RM0440 Table 7 关闭（ADR-09）。

## 6. 任务书四条 ↔ 本仓产出 ↔ 证据（2026-11-16 老师定稿）

| 任务书重点问题 | 本仓产出 | 证据位置 |
|---|---|---|
| 1. 总体方案与硬件实现 | V2 板需求 HR-1xx…8xx、引脚表、MP6540HA 行为核对 | `docs/hardware-requirements.md` §2/§3；`docs/architecture.md` §2（均为文档，无代码断言） |
| 2. 与 PWM 同步的低边采样时序 | ADC 双对（ADC1/2、ADC3/4）方案；`D_MAX=0.80` 时窗口 **5 µs**（2 µs 建立 + 2.18 µs 采样 + 余量）；**"跨组零偏斜"已证伪**：跨 dual 对无硬件同步通路，固定偏斜按 ≤ 50 ns 补偿 | `0_contracts/src/limits/hw.rs`（`D_MAX`、`SAMPLE_WINDOW_MIN_NS`、`ADC_PAIR_SKEW_MAX_NS`，附编译期断言 `PWM_HALF_PERIOD_NS × (1000 − D_MAX_PERMILLE) ≥ 1000 × SAMPLE_WINDOW_MIN_NS`）；`docs/architecture.md` §2.3；补偿落点 `1_bsp/src/adc.rs`（**未建**，无代码可查） |
| 3. 零和约束下相对增益自校准 | λ-RLS 信息形式、2 个自由参数、可辨识性边界、`N_eq = 3999`、幅值参考 0.14 A / 0.55 A | `3_user/src/policy.rs`（`R_MAX`…`TARGET_PP`、`N_EQ_MAX`）、`3_user/src/gate.rs`；断言 `cargo xtask test`（gate 用例：`pred` 降额边际、`N_eq` 计算、三相等权 `s_rms`） |
| 4. 测量异常在线检测与故障安全 | 四维门 + χ²、锁存语义、MT6701 四方接受判据、coast 动作、状态 × 事件矩阵（未标定禁闭环、运行中禁切标定/禁提交档案、无窗口禁清故障） | `3_user/src/gate.rs`、`3_user/src/encoder.rs`、`3_user/src/fsm.rs`；断言 `cargo xtask test`（CRC 24 bit 逐位翻转、全 0 帧护栏、丢失窗口锁存、矩阵全表穷举）、`cargo xtask arch` 的 I7 |

> 路径口径：本表按当前树写。`1_bsp`/`2_system`/`4_app` 三个 crate **尚未建**，凡标「未建」处没有可点开的文件；
> `0_contracts/src/limits/{hw,budget}.rs` 与 `3_user/src/policy.rs` 已落地（`limits/mod.rs` 汇总），可直接 `ls` 核对。

> **诚实边界**：本仓现有数字全部来自**合成数据**（host 数值实验），不是实验台架数据。任务书要求
> "以实验数据说明精度、收敛性与安全性" ⇒ **证据缺口在台架**。但代码缺口不止台架：`1_bsp`/`2_system`/`4_app`
> 仍是空目录，ADR-12 只补齐了能脱离实板的信号链（`foc`/`svpwm`/`sense`/`pid`）。采集顺序见 `.scratch/handoff.md` §4。
