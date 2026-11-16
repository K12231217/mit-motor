# docs/INDEX.md — 客观事实表 & 文档地图

> 本文件是《开发规范 v2.0》`SPEC.md` 的**寄生位**（§6.1：`SPEC.md` 寄生于 `docs/INDEX.md`）。
> 只记**代码签名表达不了**的宏观事实，以及 2 个以上候选方案的决策记录。
> 工程红线 → [`../AGENTS.md`](../AGENTS.md)。活跃 Thread → [`../.scratch/handoff.md`](../.scratch/handoff.md)。
> **增量冷启动**：改哪个模块就冻结哪个模块的局部事实，不要求全量补齐。

## 1. 一页事实表

| 项 | 值 | 证据 |
|---|---|---|
| 主控 | **STM32G474RBT6**，LQFP64，128 KB Flash / 128 KB SRAM | 2026-11-15 定稿；RC/RE 同脚位仅差 Flash。2026-11-16 料号变更获批：任务书题目 `STM32G491` → `STM32G474RBT6`（同封装 LQFP64，逐脚核对兼容），教务文字由老师/教务处理；**代码与文档以 G474RBT6 为准** |
| SRAM 划分 | 链接器只给 **RAM 96 K** @0x20000000（SRAM1 80 K + SRAM2 按 16 K 计）+ **CCMRAM 32 K** @0x10000000（**DMA 不可达**）；DBANK=1 ⇒ Flash 只链接 bank 1 的 64 K，bank 2 声明不用 | `firmware/memory.x`（手写，勿开 `memory-x`）；分块与 DBANK 均待一手手册，见 §5-4 |
| 驱动 | **MP6540HA**（`MP6540HGU-A-Z`），HS/LS 六电平输入，**无内部死区、无 EN 脚** | `docs/architecture.md` §2.2 |
| 电流采样 | SOx 比例 1/9200；4.99k/4.99k 偏置 → 0.2712 V/A，2.97 mA/LSB；三路独立 ADC | `docs/hardware-requirements.md` HR-203/HR-201 |
| 时钟 | HSE 16 M → PLL M=4/N=85/R=2 ⇒ **170 MHz**（boost）；ADC 42.5 MHz；**无 48 MHz/USB 支路**（板载 USB 已砍，见 §1 已拍定口径） | HR-301/303（HR-304 的 USB 时钟已不实现，见 §4 落差 9） |
| PWM | TIM1 中心对齐 **20 kHz**，ARR=4250，死区由 BDTR 产生 | HR-302 |
| 工具链 | `nightly-2025-11-05-x86_64-pc-windows-gnu`（本机无 MSVC）；`thumbv7em-none-eabihf` | `rust-toolchain.toml` |
| 实测 rustc | 1.93.0-nightly (f15a7f385 2025-11-04) | `rustc --version`，2026-11-15 |
| 构建入口 | `cargo xtask <cmd>`（`check/build/rel/fmt/fmt-check/clippy/test/size/flash/rtt/arch/help`），唯一实现 `xtask/src/main.rs`；IDE 与 CI 只调它 | `.cargo/config.toml`、`xtask/src/main.rs` 的 `help()` |
| workspace 结构 | 根 `Cargo.toml` = workspace，成员只有 `0_contracts` + `3_user`，`exclude = ["firmware", "xtask"]`；`firmware/` 仍是独立 workspace（单 bin `blinky`）；`1_bsp`/`2_system`/`4_app` **未建** | 根 `Cargo.toml`、`firmware/Cargo.toml` 自带 `[workspace]` |
| 烧写目标 | `probe-rs --chip STM32G474RB --connect-under-reset` | `firmware/.cargo/config.toml` |
| 运行时 | RTIC 2.4 + `embassy-stm32` 0.1.0（`stm32g474rb`）混合；**单 bin `blinky` 模板，无电机代码** | `firmware/Cargo.toml` |
| 镜像基线 | release 镜像 **17,768 B / 27.1 %**（Flash 区 65,536 B = 64 KiB）；`.text` 14,632 / `.rodata` 2,552 / `.data` 112 / `.bss` 480 | `cargo xtask size`，2026-11-16 实测 |
| 门禁基线 | `test` / `check` / `fmt-check` / `clippy` / `rel` / `size` / `arch` 全 PASS（均为 `cargo xtask <cmd>`；数字以各命令输出为准） | 2026-11-16 实测 |
| 门控定版 | 四维结构门 `R ≥ 0.995`、`d ≥ 0.01`、`z ≤ 0.02`、`pred ≤ 0.9·0.5 pp`；`χ²(2) ≤ 9.21` **只在结构门之后**算；`z=\|A0\|·√3/√(G·A1)`、`G=200`、`λ=1−1/2000`、`N_eq ≤ 3999` | 实现 `3_user/src/gate.rs`（`GateParams`/`GainAccum`/`GainGate`）；数值唯一来源：器件/电气在 `0_contracts/src/limits/hw.rs`、时序容量在 `limits/budget.rs`、策略阈值在 `3_user/src/policy.rs`（三者已分离，`limits/mod.rs` 汇总；两处不许互抄数值）。已数值否定、禁止复活（离线实验，脚本未入仓）：`d` 当跨工况阈值（实测反序，见 AGENTS §4-8）、`λ_min ≥ 135` 这类常数（应现算）、TLS（低幅区本应被门控拒） |
| 算法层内容 | `3_user/src/`：`gate.rs`（四维门 + χ²，阈值/公式见「门控定版」）、`encoder.rs`（MT6701 帧解析、四方接受判据、丢失拍数放宽、两帧去抖）、`policy.rs`（策略参数：门限/降额/盲推窗口/SPI 分频，只引用 `contracts::limits` 派生）；两个 crate 无 `todo!()`，测试条数以 `cargo xtask test` 为准 | `ls 3_user/src`；`grep -rn 'todo!' 0_contracts/src 3_user/src` 为空 |
| 已拍定口径 | `D_MAX=0.80`；编码器 SPI **10.625 MHz**；故障动作 **coast**（台架不带重力负载）；**不存标定档案** ⇒ 无 flash 写入路径，标定走 HR-407 每次上电自校准；**砍板载 USB**（上位机走 CAN FD） | `0_contracts/src/limits/hw.rs` 的 `D_MAX`、`3_user/src/policy.rs` 的 `ENC_SPI_HZ`；其余为 2026-11-16 用户裁定，代码落点未建（未验证） |
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

## 4. 文档 vs 代码树：已确认的落差

| # | 文档说 | 树里实际 | 后果 / 处理 |
|---|---|---|---|
| 1 | 根 `Cargo.toml` workspace = 5 个成员（`software-architecture.md` §2） | 根 workspace 只有 `0_contracts` + `3_user`；`1_bsp`/`2_system`/`4_app` 未建 | 已解决一半：成员与 `exclude` 按 ADR-03 落地（见 §1 workspace 结构）；余三 crate 无板不建空壳 |
| 2 | `cargo xtask` 面向整个固件 | `xtask/src/main.rs` 的 `ELF_REL`、`rtt()` 仍硬编码 `firmware/target/thumbv7em-none-eabihf/release/blinky` | 仍成立但未误测：根 workspace `exclude = ["firmware"]`，唯一固件 bin 就是 `blinky`。多 bin 前必须先改 xtask |
| 3 | `ci/arch_check.sh` 六条检查（§9） | 脚本已删除；`cargo xtask arch` 纯 Rust 实现 I1/I2/I3，CI 已接入；I4–I6 未实现 | 已解决 I1–I3；I4–I6 仍无机器校验 |
| 4 | `3_user/tests/` + `fixtures/`（§10 未决 3） | 无独立 `tests/`；测试内联在 crate 源文件里 | 已解决：`cargo xtask test` 全绿（`contracts` + `user`，0 ignored；条数以命令输出为准，不写进文档） |
| 5 | `board.yaml` / `self_test` / `tools/replay`（`agent-readiness.md` §2 A–D） | **均不存在** | agent 可判定性缺口未闭合 |
| 6 | `hardware-requirements.md` §9 的 `64,632 / 16,080 B` | 实测镜像 17,768 B（另一工具链测量） | 违反 PRE-09，待删或改指针 |
| 7 | `hardware-requirements.md` §1/§72 说 SRAM = SRAM1 80 K + SRAM2 32 K + CCM 16 K | `firmware/memory.x` 取保守子集：RAM 96 K（SRAM2 按 16 K）+ CCM 32 K | 两者总和都是 128 K；待一手手册确认后只改 `memory.x` 一处（见 §5-4） |
| 8 | HR-305 写占空比上限 `d ≤ 90 %` | `0_contracts/src/limits/hw.rs` 的 `D_MAX = 0.80` | 已裁：0.90 在 50 µs 周期下只剩 2.5 µs 共导窗口，低于 HR-305 自己要求的 3 µs；以 `hw.rs` 为准，HR-305 待改 |
| 9 | HR-304 要求 USB 时钟 HSI48+CRS | 板载 USB 已砍（§1 已拍定口径），固件无 USB 路径 | 此处无代码可查（`firmware/` 仍是 `blinky` 模板）；HR-304 待标废弃或删 |
| 10 | `architecture.md` §2.3 说三路 SOx"无采样时刻偏斜" | 跨 dual 对（ADC1/2 ↔ ADC3/4）无硬件同步通路；固定偏斜按 ≤ 50 ns 补偿（`limits/hw.rs` 的 `ADC_PAIR_SKEW_MAX_NS`） | "跨组零偏斜"已证伪（§6 映射 2）；`architecture.md` §2.3 待改 |

## 5. 未决（阻塞实现，或被刻意留白待人裁定）

1. **`take_sample()` 语义**：挂 ADC3 EOC 中断，还是读上一拍（确定性更好）→ 决定 `Hw` trait 与窗口判据。
2. **`embassy_stm32::init()` 与裸 PAC 单例的衔接**：`steal()` 还是从返回的 `Peripherals` 取。
   无实板 ⇒ 只能读 embassy 源码给结论，寄存器行为属 `UNVERIFIED`；阻塞 `1_bsp/src/pwm.rs`、`1_bsp/src/adc.rs`（均**未建**）。
3. **`INVALID_SAMPLE_LIMIT = 8`**（`3_user/src/policy.rs`，标 `[OPEN]`）：8 × 50 µs = 400 µs 无冻结依据，
   阻塞于 E2/E3 的无效样本比例统计。
4. **SRAM 分块与 DBANK 的一手确认**：DS12288 / RM0440 第 3 章。现取保守子集（§1 SRAM 行、§4 落差 7）。
   DBANK 可在写 option bytes 前翻成 0，但切换会 mass erase。

## 6. 任务书四条 ↔ 本仓产出 ↔ 证据（2026-11-16 老师定稿）

| 任务书重点问题 | 本仓产出 | 证据位置 |
|---|---|---|
| 1. 总体方案与硬件实现 | V2 板需求 HR-1xx…8xx、引脚表、MP6540HA 行为核对 | `docs/hardware-requirements.md` §2/§3；`docs/architecture.md` §2（均为文档，无代码断言） |
| 2. 与 PWM 同步的低边采样时序 | ADC 双对（ADC1/2、ADC3/4）方案；`D_MAX=0.80` 时窗口 **5 µs**（2 µs 建立 + 2.18 µs 采样 + 余量）；**"跨组零偏斜"已证伪**：跨 dual 对无硬件同步通路，固定偏斜按 ≤ 50 ns 补偿 | `0_contracts/src/limits/hw.rs`（`D_MAX`、`SAMPLE_WINDOW_MIN_NS`、`ADC_PAIR_SKEW_MAX_NS`，附编译期断言 `PWM_HALF_PERIOD_NS × (1000 − D_MAX_PERMILLE) ≥ 1000 × SAMPLE_WINDOW_MIN_NS`）；`docs/architecture.md` §2.3；补偿落点 `1_bsp/src/adc.rs`（**未建**，无代码可查） |
| 3. 零和约束下相对增益自校准 | λ-RLS 信息形式、2 个自由参数、可辨识性边界、`N_eq = 3999`、幅值参考 0.14 A / 0.55 A | `3_user/src/policy.rs`（`R_MAX`…`TARGET_PP`、`N_EQ_MAX`）、`3_user/src/gate.rs`；断言 `cargo xtask test`（gate 用例：`pred` 降额边际、`N_eq` 计算、三相等权 `s_rms`） |
| 4. 测量异常在线检测与故障安全 | 四维门 + χ²、锁存语义、MT6701 四方接受判据、coast 动作 | `3_user/src/gate.rs`、`3_user/src/encoder.rs`；断言 `cargo xtask test`（CRC 24 bit 逐位翻转、全 0 帧护栏、丢失窗口锁存） |

> 路径口径：本表按当前树写。`1_bsp`/`2_system`/`4_app` 三个 crate **尚未建**，凡标「未建」处没有可点开的文件；
> `0_contracts/src/limits/{hw,budget}.rs` 与 `3_user/src/policy.rs` 已落地（`limits/mod.rs` 汇总），可直接 `ls` 核对。

> **诚实边界**：本仓现有数字全部来自**合成数据**（host 数值实验），不是实验台架数据。任务书要求
> "以实验数据说明精度、收敛性与安全性" ⇒ **证据缺口在台架，不在代码**；采集顺序见 `.scratch/handoff.md` §4。
