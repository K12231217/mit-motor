# STATE — handoff

> 活跃 Thread 上限 3 个；关闭的 Thread 只进 commit message，不留在本文件。
> 上位：`AGENTS.md`（RULES）、`docs/INDEX.md`（SPEC）。

## 1. 活跃 Thread

| ID | 模式 | 目标 | 冻结/基线 | 状态 |
|---|---|---|---|---|
| T-012 | B | `bsp` clock + pwm：HSE 16M/PLL 170M 读回；TIM1 20 kHz/500 ns；按 2026-11-27 网表 nSLEEP PC7、nFAULT PC8、无 BKIN | `4f67b32`（T-011 脚手架） | **IMPLEMENTED / PENDING_HIL**：`rel/size/arch/clippy/hygiene` 已过；`main` 只读回、不 `enable`；硬件过压 break 缺口待补 |

## 2. 当前工作区

- 代码已按用途分组：`software/{contracts,algorithms,bsp,firmware,xtask}`；硬件资料 `hardware/pcb/`。
- 最新网表：`hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel`（以此为准）。
- T-012 的代码/文档改动与目录重排同批提交；`main` 仍只读回、不 `enable`。
- T-010（远端）已并入：载波保持 20 kHz，`SinCos`/`PiOutput`/`1/vbus` 缓存等 P0/P1 优化保留。
- 引脚/测试点只查 `docs/hardware-requirements.md` §3.1 / §5；不复制到别处。

## 3. HIL

**无 HIL 数据。** 板为 G474RBT6；尚未烧写、未跑 S0–S5、无示波器/LA 记录。

下一次台架（T-012）：
1. 烧 `software/firmware/src/bin/main.rs`；RTT 出现 `clock=ok`、`pwm=ok arr=4250 dtg=85 moe=false aoe=false bke=false`。
2. 安全关断态：HS PA8/9/10、LS PB13/14/15 全低；nSLEEP PC7 低；nFAULT PC8 高。
3. 不接电机、KA3005P 限流；临时 `enable` 后测 50 µs 周期、约 500 ns 死区，测完 `disable`。
4. 台架结论只在本节新增 `HIL: PASS` 行；没有记录就是 `PENDING_HIL`。

## 4. 待裁决（人）

- 硬件过压 break 缺口：补 TIM1_BKIN 还是只做软件 OVP 停机。
- 500 ns 死区 LA 实测；nSLEEP 1 ms 唤醒是否足够。
- T-013 的 A/C 相 OPAMP/ADC 交叉映射台架确认。
- `CalibRecord` 字段集与 A/B 新旧判定（决定 `bsp` flash 接口）。
