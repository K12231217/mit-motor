# INDEX — SPEC（项目事实与决策）

> 全仓只有三个控制文件：`AGENTS.md` = RULES，本文件 = SPEC，`.scratch/handoff.md` = STATE。
> 其余文档是冷数据或 `docs/archive/` 历史；冷启动只读这三个，按指针单跳。

## 1. 一页事实

| 项 | 值 |
|---|---|
| 主控 | STM32G474RBT6，LQFP64，128 KB Flash / 128 KB SRAM |
| 板卡 | 2026-11-27 最新网表 `hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel`；PCB14 是 G491RET6 旧板 |
| 驱动 | MP6540HA（`MP6540HGU-A-Z`），HS/LS 六电平输入，无 EN、无内部死区 |
| 电流采样 | SOx 1/9200 + 4.99 kΩ/4.99 kΩ 偏置；0.2712 V/A、2.25 mA/LSB；片内 OPAMP1/2/3 follower |
| 相序 | `SOA→OPA3/PB0`、`SOB→OPA2/PA7`、`SOC→OPA1/PA3`（A/C 交叉；详见 HR §3.1） |
| 时钟 | HSE 16 MHz → PLL M4/N85/R2 = 170 MHz；ADC PLLP/8 = 42.5 MHz |
| PWM | TIM1 中心对齐 20 kHz，ARR=4250；死区 500 ns（待 LA 实测）；`AOE=0`；**本板无 BKIN** |
| 安全脚 | `nSLEEP=PC7`、`nFAULT=PC8`；硬件过压 break 缺口，`BKE=0` |
| 调试 | RTT only；`PB6/PB7=TP_SYNC_LOOP/FREEZE`（不启用 USART3）；`PA4/PA5=DAC1_OUT1/2`（残差/冻结） |
| 工具链 | `nightly-2025-11-05-x86_64-pc-windows-gnu` + `thumbv7em-none-eabihf` |
| 构建 | `cargo xtask <cmd>`，唯一实现 `software/xtask/src/main.rs` |
| workspace | 根 workspace = `software/contracts` + `software/algorithms`；`software/firmware`/`bsp`/`xtask` 独立 |
| 当前状态 | `software/bsp` clock/pwm/dwt/timebase/trace 已实现；T-012 safe-off **HIL PASS**；T-014 pre/post 环 + stride **HIL PASS**；T-015 20 kHz current task **HIL PASS（ADC/rate）**（`adc0≈2038`、`r_lsb≈-1`、`isr_count≈20013/s`）；encoder 仍 `todo!()`；`enable` 波形/LA 未做 |

> 测试数、镜像大小、门禁结果不写死：跑 `cargo xtask test` / `size` / `arch` / `hygiene`，以命令输出为准。

## 2. 冷数据指针

| 文档 | 内容 |
|---|---|
| [`hardware-requirements.md`](hardware-requirements.md) | 引脚 §3.1、采样/安全、测试点 §5、实验 E1–E6、投板检查 |
| [`software-architecture.md`](software-architecture.md) | 文件/模块边界、模块注册表、不变量 I1–I7、门禁落点 |
| [`instrumentation.md`](instrumentation.md) | TP_SYNC/DAC、TIM5/DWT、defmt 过滤、DATA 行与录波容量 |
| [`motor-wk4310h.md`](motor-wk4310h.md) | WK4310H 铭牌、极对、每相/磁链换算 |
| [`archive/`](archive/) | 历史 ADR 全文、研究/任务书、旧 HIL 表；**不冷启动** |

## 3. ADR 索引

| # | 决策 | 状态 |
|---|---|---|
| 01 | 构建入口 = `cargo xtask` | 已定 |
| 02 | 运行时 = RTIC + embassy 混合 | 已定方向 |
| 03 | 五 crate 分层，`contracts`/`algorithms` 先落地 | 部分落地 |
| 04 | `memory.x` 手写，禁用 `memory-x` | 已定 |
| 05 | µs 时基用 TIM5（TIM2 归 embassy） | 已定 |
| 06 | 估计器放 `algorithms`，`contracts` 只留事实 | 已定 |
| 07 | 论文 = Retianza 2023 复现 + 可信门控 + 可辨识性边界 | 已定 |
| 08 | SPI1 单一所有者：aux 编码器并入 `current` | 已定 |
| 09 | 迁移矩阵放 `algorithms`；标定档案 bank 2 双槽 | 已定 |
| 10 | 驱动 move 移交；设备族 enum 静态分发 | 已定 |
| 11 | 门禁 = 命令 + 唯一真源，不生成第二份地图 | 已定 |
| 12 | 信号链移植 Apache-2.0，不自己推公式 | 已定 |
| 13 | 电流环 + 独立监督 + C0 零位闸门 | 已实现 |
| 14 | 顶层按用途分组（`software/`/`hardware/`），组内不用数字前缀 | 已定 |
| 15 | 载波保持 20 kHz；P0/P1 热路径优化（SinCos/PiOutput/1/vbus 缓存） | 已实现（host） |

## 4. 待裁决

- **硬件过压 break**：最新网表没有把比较器接到 TIM1_BKIN；补硬件还是只做软件 OVP 停机。
- **死区**：500 ns 是 HR-302 上限，待 LA 实测后定 DTG。
- **nSLEEP 唤醒**：`enable` 目前等 1 ms，待 MP6540HA 手册/实测确认。
- **标定档案**：`CalibRecord` 字段（board_id/版本/零偏/OVP）+ A/B 新旧判定，决定 `bsp` flash 接口。
- **相序交叉**：T-013 的 ADC/OPAMP 通道映射必须按 §1 的 A/C 交叉写，待台架确认。
