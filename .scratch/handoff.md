# handoff — 活跃 Thread 路由表

> 本文件是《开发规范 v2.0》`STATE.md` 的**寄生位**（§6.1）。窗口是缓存，这里是状态。
> 上限 3–5 个活跃 Thread；关闭的 Thread 沉淀进 commit message 后从这里删除。
> 上位：`AGENTS.md`（红线）、`docs/INDEX.md`（事实表）。

## 1. 活跃 Thread

| ID | 模式 | 目标 | 冻结脚手架 | 状态 |
|---|---|---|---|---|
| T-001 | B | 建立仓库规范：`AGENTS.md` / `docs/INDEX.md` / 本文件，校正文档漂移 | `ab1634798e56034f332ec33b1359ea1b23dc4b4e` | **DONE**（仅文档，无需 HIL） |
| T-002 | B | workspace 落地（根 `Cargo.toml` = `0_contracts` + `3_user`）、门控估计器搬进 `3_user`、`memory.x` 保守值、`arch` 机器校验 | 事实与证据见 `docs/INDEX.md` §1/§3（ADR-06）、§4 落差 1/3/4 | **DONE**（host 侧，无 HIL） |
| T-003 | B | `arch_check` 去 bash 依赖：I1/I2/I3 改成 `xtask/src/checks.rs` 内的 Rust 实现，`ci/arch_check.sh` 删除 | 事实与证据见 `docs/INDEX.md` §4 落差 3 | **DONE**（`cargo xtask arch` → 3 PASS） |
| T-004 | A | 探针：`embassy_stm32::init()` 返回的 `Peripherals` 与裸 PAC `steal()` 的衔接 | 无（探针免脚手架） | **BLOCKED**：无 G474 实板；可先在 host 读 embassy 源码给结论，寄存器行为属 `UNVERIFIED` |
| T-005 | B | 状态 × 事件迁移矩阵（`3_user/src/fsm.rs` + 类型 `0_contracts/src/fsm.rs`）、状态字 bit 15 `cmd_reject`、清故障窗口、bank 2 标定档案双槽布局、`arch` 增 I7 | 事实与证据见 `docs/INDEX.md` §3 ADR-09、§1 SRAM/算法层行 | **DONE**（host 侧；档案写入路径与制动策略未做，见 ADR-09 失效条件） |
| T-006 | B | 规范：驱动所有权 move 移交、enum 静态分发、热路径返回 `()`、热路径零隐式 panic；同步 `software-architecture.md` §3.3/§9 | 事实见 `docs/INDEX.md` §3 ADR-10、`AGENTS.md` §1.1 MECH-01…04 | **DONE**（纯文档，无代码可测；门禁见 §2） |
| T-007 | B | 协作协议落地：H1–H5 硬门禁（`hygiene` / `tier` / clippy 双 workspace）、S1–S3 软协议、`docs/agent-guardrails.md` | 无业务代码；决策见 `docs/INDEX.md` §3 ADR-11 | **DONE**（host 侧；`tier` 对 Tier B 按设计 FAIL） |

## 2. 本会话验收门（T-002 + T-003 + T-005 + T-006）

2026-11-16 实测，命令原文输出摘要：

- [x] `cargo xtask test`：全绿（`contracts` 32 + `user` 41，0 ignored；条数以命令输出为准），4 条 `test result` 均 0 failed，exit 0
- [x] `cargo xtask clippy`（`-D warnings`）：`Finished` 无 warning，exit 0
- [x] `cargo xtask size`：`flash region 65,536 B (64 KiB)`；`flash image 17,768 B (27.1 % used, 47,768 left)`；
      `.text 14,632 / .rodata 2,552 / .data 112 / .bss 480`
- [x] `cargo xtask arch`：`PASS I1/I2/I3/I7` + `arch_check: PASS —— I1/I2/I3/I7 全部通过`，exit 0（I7 已做负向验证：往 `policy.rs` 塞一个 `MotorState` 注释即 FAIL）
- [x] 三个控制文件的数字均可追到上面四条命令或 `firmware/memory.x` / `0_contracts/src/limits/hw.rs`

T-006（2026-11-19，纯文档，命令原文输出摘要）：
- [x] `cargo xtask test`：`41 passed; 0 failed`（user），contracts/doc-tests 均 0 failed，exit 0
- [x] `cargo xtask check`：`Finished dev profile`，exit 0
- [x] `cargo xtask clippy`：`Finished dev profile`，无 warning，exit 0
- [x] `cargo xtask arch`：`PASS I1/I2/I3/I7` + `arch_check: PASS`，exit 0（MECH 检查未接入，见 `software-architecture.md` §9-8）

T-007（2026-11-21，xtask 工具 + 文档，命令原文输出摘要）：
- [x] `cargo xtask hygiene`：`PASS H2`（24 个文件）、`PASS H5`（24 个文件）、`PASS H4`（24 个模块），exit 0
- [x] `cargo xtask clippy`：firmware 与根 workspace（`--workspace --all-targets`）两遍均 `Finished`，exit 0
- [x] `cargo xtask tier`：**FAIL（按设计）**——`firmware/Cargo.toml`、`firmware/memory.x` 属 Tier B 且无台架证据
- [x] `cargo xtask test`：`33 passed`（contracts）+ `43 passed`（user），4 条 `test result` 均 0 failed，exit 0
- [x] `cargo xtask fmt-check` / `check` / `doc` / `arch` / `size`：全 PASS；`flash image 17,768 B (27.1 %)`，exit 0

## 3. 最近一次台架实测（HIL）

**无**。至今没有任何 G474RBT6 实板数据：未烧写、未跑自检、无示波器/LA 记录。
`hardware-requirements.md` 里所有"待实测"条目（OVP 翻转点、EN 低是否高阻、ADC 跨组偏斜）全部仍为 `OPEN`。
本仓现有数字全部来自合成数据（host 数值实验），不是台架证据——口径见 `docs/INDEX.md` §6。

Tier B 门禁（`cargo xtask tier`）在 `feat/contracts-and-gate` 上**按设计 FAIL**：`firmware/Cargo.toml`
与 `firmware/memory.x` 相对 main 有改动，属硬件耦合路径且无台架证据。整改只有两条：改动留在
hil-pending 分支，或补一条独立成行的台架证据记录（格式见 `docs/agent-guardrails.md` §1 的 H3）。
不要为了让它变绿而改门禁代码。

## 4. 下一个可验收单元

**台架**：`self_test` 的 S0–S5（板子可用性的 PASS 位）→ 按 HR §7 采 **E1**（电流链静态标定）、
**E4**（相对增益校准：普通 RLS vs 可信闸门）、**E6**（故障时序 OVP / nFAULT）。
host 侧无阻塞项；T-004 仍卡在无 G474 实板。

## 5. 待裁决（人）

未决清单的唯一来源是 [`docs/INDEX.md`](../docs/INDEX.md) §5（`take_sample` 语义、embassy 衔接、
`INVALID_SAMPLE_LIMIT`、CCM 别名是否启用），本文件不复制。活跃 Thread 中只有 T-004 受外部条件（实板）阻塞。
下一块待裁：`CalibRecord` 的字段集（`board_id` 96 bit UID + 版本 + 编码器零偏 + OVP 实测点）与 A/B 新旧判定，
它决定 `1_bsp` 的 flash 驱动接口（ADR-09 只冻结了槽布局）。

## 6. 归档规则

Thread 关闭时：结论写进 `docs/INDEX.md`（事实/ADR）与本文件 §3（若有 HIL 数据），
commit message 写清 `wip`/`feat` + Thread ID，然后从 §1 删除该行。
