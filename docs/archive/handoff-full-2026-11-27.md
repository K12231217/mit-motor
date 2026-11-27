# handoff — 活跃 Thread 路由表

> 本文件是《开发规范 v2.0》`STATE.md` 的**寄生位**（§6.1）。窗口是缓存，这里是状态。
> 上限 3–5 个活跃 Thread；关闭的 Thread 沉淀进 commit message 后从这里删除。
> 上位：`AGENTS.md`（红线）、`docs/INDEX.md`（事实表）。

## 1. 活跃 Thread

| ID | 模式 | 目标 | 冻结脚手架 | 状态 |
|---|---|---|---|---|
| T-004 | A | 探针：`embassy_stm32::init()` 返回的 `Peripherals` 与裸 PAC `steal()` 的衔接 | 无（探针免脚手架） | **DONE（编译级）**：板已确认 G474RBT6；`software/firmware/src/bin/main.rs` 证明 `init(Board)` → RTIC Local；HIL 待 T-012 台架 |
| T-011 | B | `1_bsp` 脚手架：`Board`/`Pwm`/`Adc`/`Encoder`/`Dwt`/`Sample` 类型与签名；第一个切片的 API 与任务所有权图；不写驱动实现 | `4f67b32`（`wip(T-011)`） | **DONE**（脚手架；T-012 接续实现） |
| T-012 | B | clock + pwm 实现：HSE 16M/PLL 170M；TIM1 20 kHz/500 ns；按 2026-11-27 网表 nSLEEP PC7 / nFAULT PC8 / 无 BKIN | 无（沿用 T-011） | **IMPLEMENTED（PENDING_HIL）**：`rel/size/arch/clippy/hygiene` 全 PASS；`main` 只读回，`bke=false`；硬件过压 break 缺口待补 |

## 2. 历史验收门记录

2026-11-16 实测，命令原文输出摘要：

- [x] `cargo xtask test`：全绿（`contracts` 32 + `user` 41，0 ignored；条数以命令输出为准），4 条 `test result` 均 0 failed，exit 0
- [x] `cargo xtask clippy`（`-D warnings`）：`Finished` 无 warning，exit 0
- [x] `cargo xtask size`：`flash region 65,536 B (64 KiB)`；`flash image 17,768 B (27.1 % used, 47,768 left)`；
      `.text 14,632 / .rodata 2,552 / .data 112 / .bss 480`
- [x] `cargo xtask arch`：`PASS I1/I2/I3/I7` + `arch_check: PASS —— I1/I2/I3/I7 全部通过`，exit 0（I7 已做负向验证：往 `policy.rs` 塞一个 `MotorState` 注释即 FAIL）
- [x] 三个控制文件的数字均可追到上面四条命令或 `software/firmware/memory.x` / `0_software/contracts/src/limits/hw.rs`

T-006（2026-11-19，纯文档，命令原文输出摘要）：
- [x] `cargo xtask test`：`41 passed; 0 failed`（user），software/contracts/doc-tests 均 0 failed，exit 0
- [x] `cargo xtask check`：`Finished dev profile`，exit 0
- [x] `cargo xtask clippy`：`Finished dev profile`，无 warning，exit 0
- [x] `cargo xtask arch`：`PASS I1/I2/I3/I7` + `arch_check: PASS`，exit 0（MECH 检查未接入，见 `software-architecture.md` §9-8）

T-007（2026-11-21，xtask 工具 + 文档，命令原文输出摘要）：
- [x] `cargo xtask hygiene`：`PASS H2`（24 个文件）、`PASS H5`（24 个文件）、`PASS H4`（24 个模块），exit 0
- [x] `cargo xtask clippy`：firmware 与根 workspace（`--workspace --all-targets`）两遍均 `Finished`，exit 0
- [x] `cargo xtask tier`：**FAIL（按设计）**——`software/firmware/Cargo.toml`、`software/firmware/memory.x` 属 Tier B 且无台架证据
- [x] `cargo xtask test`：`33 passed`（contracts）+ `43 passed`（user），4 条 `test result` 均 0 failed，exit 0
- [x] `cargo xtask fmt-check` / `check` / `doc` / `arch` / `size`：全 PASS；`flash image 17,768 B (27.1 %)`，exit 0

T-008（2026-11-21，信号链移植，命令原文输出摘要）：
- [x] `cargo xtask test`：`33 passed`（contracts）+ `67 passed`（user，含新增 24 条），4 条 `test result` 均 0 failed，exit 0
- [x] `cargo xtask clippy`：firmware 与根 workspace（`--workspace --all-targets`）两遍均 `Finished`，exit 0
- [x] `cargo xtask hygiene`：`PASS H2/H5/H4`（32 个文件），exit 0
- [x] `cargo xtask fmt-check` / `check` / `doc` / `arch` / `size`：全 PASS；`flash image 17,768 B (27.1 %)`，exit 0

T-009（2026-11-22，电流环 + 独立监督 + C0 零位闸门，命令原文输出摘要）：
- [x] `cargo xtask test`：`33 passed`（contracts）+ `85 passed`（user，含新增 13 条），4 条 `test result` 均 0 failed，exit 0
- [x] `cargo xtask check` / `fmt-check` / `doc`：全 PASS，exit 0
- [x] `cargo xtask clippy`：firmware 与根 workspace 两遍均 `Finished`，exit 0
- [x] `cargo xtask arch`：`PASS I1/I2/I3/I7` + `arch_check: PASS —— I1/I2/I3/I7 全部通过`，exit 0

T-010（2026-11-27，host 侧契约/文档收口，命令原文输出摘要）：
- [x] `cargo xtask test`：`44 passed`（contracts）+ `89 passed`（user，含 WK4310H 2 条），4 条 `test result` 均 0 failed，exit 0
- [x] `cargo xtask check` / `fmt-check` / `doc`：全 PASS，exit 0
- [x] `cargo xtask clippy`：firmware 与根 workspace 两遍均 `Finished`，exit 0
- [x] `cargo xtask arch`：`PASS I1/I2/I3/I7`，exit 0
- [x] `cargo xtask hygiene`：`PASS H2/H4/H5/H6`（49 个文件 / 31 个模块），exit 0
- [x] H6 模块注册表：`<layer>.<domain>.<role>[_<qualifier>]`，每个源码文件恰好归一个模块，owner/contract 非空
- [x] `cargo xtask size`：`flash image 20,224 B (30.9 % used)`，exit 0（bin 名改为 `main`，路径由 metadata 推出）
- [x] `software/firmware/src/bin/main.rs`：S0 bring-up（170 MHz 时钟读回、Board 移入 RTIC Local、PWM safe-off、PA3 单次 ADC 采样）；编译/`rel`/`size` 通过，**未上板，PENDING_HIL**
- [ ] `cargo xtask tier`：Tier B 仍按设计 FAIL；本会话无 HIL，不补台架证据

## 3. 最近一次台架实测（HIL）

**无 HIL 数据**。2026-11-27 用户确认板为 **G474RBT6**；尚未烧写、未跑 S0–S5、无示波器/LA 记录。`hardware-requirements.md` 里所有"待实测"条目（OVP 翻转点、EN 低是否高阻、
ADC 跨组偏斜）全部仍为 `OPEN`。本仓现有数字全部来自合成数据（host 数值实验），不是台架证据——
口径见 `docs/INDEX.md` §6。

Tier B 门禁（`cargo xtask tier`）在 main 上**按设计 FAIL**：`software/firmware/Cargo.toml`、`software/firmware/memory.x`
与 `software/firmware/src/bin/main.rs` 属硬件耦合路径且无台架证据（与 `feat/contracts-and-gate` 同一提交）。
2026-11-21 裁定：**接受这颗红**，等 G474RBT6 台架证据再补记录，不为变绿改门禁。补证据只有一种形态：
在 §3 新增独占一行、以固定标记开头的记录（格式见 `docs/agent-guardrails.md` §1 的 H3）。
在这条记录出现前，本节的"无"就是事实，任何"已验证"表述都不成立。

## 4. 下一个可验收单元

**台架（T-012）**：烧 `software/firmware/src/bin/main.rs`，RTT 必须出现 `clock=ok` 与 `pwm=ok arr=4250 dtg=85 moe=false aoe=false bke=false`；
LA 抓 PA8/PB13 确认 20 kHz、死区约 500 ns、复位后六路低。通过后再做 T-013 的 ADC 注入组与 S0–S5 →
按 HR §7 采 **E1**、**E4**、**E6**。host 侧无阻塞项。

## 5. 待裁决（人）

未决清单的唯一来源是 [`docs/INDEX.md`](../docs/INDEX.md) §5（`take_sample` 语义、embassy 衔接、
`INVALID_SAMPLE_LIMIT`、CCM 别名是否启用），本文件不复制。板型已定 G474RBT6，引脚以 2026-11-27 网表为准；T-012 的 HIL 待裁项：
500 ns 死区 LA 实测、nSLEEP 唤醒 1 ms 是否足够、硬件过压 break 缺口如何补（当前 BKE=0）。
下一块待裁：`CalibRecord` 的字段集（`board_id` 96 bit UID + 版本 + 编码器零偏 + OVP 实测点）与 A/B 新旧判定，
它决定 `1_bsp` 的 flash 驱动接口（ADR-09 只冻结了槽布局）。

## 6. 归档规则

Thread 关闭时：结论写进 `docs/INDEX.md`（事实/ADR）与本文件 §3（若有 HIL 数据），
commit message 写清 `wip`/`feat` + Thread ID，然后从 §1 删除该行。
