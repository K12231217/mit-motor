# INDEX — SPEC（项目事实与决策）

> 全仓只有三个控制文件：`AGENTS.md` = RULES，本文件 = SPEC，`.scratch/handoff.md` = STATE。
> 其余文档是冷数据或 `docs/archive/` 历史；冷启动只读这三个，按指针单跳。

## 1. 一页事实

| 项 | 值 |
|---|---|
| 主控 | STM32G474RBT6，LQFP64，128 KB Flash / 128 KB SRAM |
| 板卡 | 2026-11-27 最新网表 `hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel`；PCB14 是 G491RET6 旧板 |
| 驱动 | MP6540HA（`MP6540HGU-A-Z`），HS/LS 六电平输入，无 EN、无内部死区 |
| 电流采样 | 片内 OPAMP1/2/3 follower 接 SOx；标度唯一源 `contracts::motor::config_hw::SOX_AMPS_PER_LSB` |
| 相序 | 唯一源 HR §3.1（最新网表）；代码按该表写，本表不复写 |
| 时钟 | `config_hw::HSE_HZ` → `config_budget::SYS_CLK_HZ`；PLL 参数与读回在 `bsp::motor::service_clock`（编译期断言钉死） |
| PWM | TIM1 中心对齐 `config_budget::CURRENT_LOOP_HZ`；`config_hw::PWM_ARR`、`PWM_DEADTIME_NS`；`AOE=0`；**本板无 BKIN** |
| 安全脚 | 脚位唯一源 HR §3.1；`BKE=0`，硬件过压 break 缺口 |
| 调试 | RTT only；TP_SYNC/DAC 脚位见 HR §5；不启用 USART3 |
| 工具链 | `nightly-2025-11-05-x86_64-pc-windows-gnu` + `thumbv7em-none-eabihf` |
| 构建 | `cargo xtask <cmd>`，唯一实现 `software/xtask/src/main.rs` |
| workspace | 根 workspace = `software/contracts` + `software/algorithms`；`software/firmware`/`bsp`/`xtask` 独立 |
| 当前状态 | 活跃 Thread 与 HIL 证据唯一在 [`.scratch/handoff.md`](../.scratch/handoff.md) §1/§3；本表不复写 |

> 测试数、镜像大小、门禁结果不写死：跑 `cargo xtask test` / `size` / `arch` / `hygiene`，以命令输出为准。

## 2. 冷数据指针

| 文档 | 内容 |
|---|---|
| [`hardware-requirements.md`](hardware-requirements.md) | 引脚 §3.1、采样/安全、测试点 §5、实验 E1–E6、投板检查 |
| [`software-architecture.md`](software-architecture.md) | 文件/模块边界、模块注册表、不变量 I1–I7、门禁落点 |
| [`instrumentation.md`](instrumentation.md) | TP_SYNC/DAC、TIM5/DWT、defmt 过滤、DATA 行与录波容量 |
| [`motor-wk4310h.md`](motor-wk4310h.md) | WK4310H 铭牌、极对、每相/磁链换算 |
| [`architecture/governance.md`](architecture/governance.md) | 架构降熵治理：六张清单、12 域体检、轮次日志与待确认；**冷数据，不冷启动** |
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
| 16 | 诊断通道：只读录波环与原子计数不走 Snapshot/队列，见架构 §3.4 | 已定 |
| 17 | seqlock 协议收在私有 `Snapshot<V, N>`，三个视图只保留"谁能重试"的差异 | 已定（轮次 6） |

## 4. 待裁决

- **硬件过压 break**：最新网表没有把比较器接到 TIM1_BKIN；补硬件还是只做软件 OVP 停机。
- **死区**：500 ns 是 HR-302 上限，待 LA 实测后定 DTG。
- **nSLEEP 唤醒**：`enable` 目前等 1 ms，待 MP6540HA 手册/实测确认。
- **标定档案**：`CalibRecord` 字段（board_id/版本/零偏/OVP）+ A/B 新旧判定，决定 `bsp` flash 接口。
- **相序交叉**：以 HR §3.1 为唯一事实；代码按该表写，**不列为 HIL 项**。
- **NTC**：网表 `ADC12_IN6` 只连 U1.11，脚 11 是 PC3（附加功能 `ADC12_IN9`），既无分压也无匹配通道；需原理图/实物确认（HR §3.1「待确认」）。
