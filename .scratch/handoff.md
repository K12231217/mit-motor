# STATE — handoff

> 活跃 Thread 上限 3 个；关闭的 Thread 只进 commit message，不留在本文件。
> 上位：`AGENTS.md`（RULES）、`docs/INDEX.md`（SPEC）。

## 1. 活跃 Thread

| ID | 模式 | 目标 | 冻结/基线 | 状态 |
|---|---|---|---|---|
| T-015 | B | 20 kHz current task：TIM1_UP → OPAMP/ADC → GainGate 残差 → DAC/TP_SYNC/录波 | T-014 HIL 后 | **HIL PASS（ADC/rate/DAC）**：`adc0≈2038`、`r_lsb≈-1`、`isr_count=20013/s`；TP1=1.2623 V、TP2 静态保持=2.496 V；`enable` 波形未测 |
| T-014 | B | trace 基础设施：PB6/PB7 TP_SYNC、DAC1_OUT1 残差/OUT2 冻结、TIM5 1 MHz、DWT、512/1536 预/后触发环与 `stride` | T-012 HIL 后 | **HIL: PASS（RTT/寄存器）**：`recorder pre=512 post=1536 len=2048`；stride 1/10 均锁存；物理 DAC/TP_SYNC 未测 |
| T-012 | B | `bsp` clock + pwm：HSE 16M/PLL 170M 读回；TIM1 20 kHz/500 ns；按 2026-11-27 网表 nSLEEP PC7、nFAULT PC8、无 BKIN | `4f67b32`（T-011 脚手架） | **HIL: PASS（safe-off）**：RTT `clock=ok`、`pwm=ok arr=4250 dtg=85 moe/aoe/bke=false`；`enable` 波形与 LA 未做；硬件过压 break 缺口待补 |

## 2. 当前工作区

- 代码已按用途分组：`software/{contracts,algorithms,bsp,firmware,xtask}`；硬件资料 `hardware/pcb/`。
- 最新网表：`hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel`（以此为准）。
- T-012 的代码/文档改动与目录重排同批提交；`main` 仍只读回、不 `enable`。
- T-010（远端）已并入：载波保持 20 kHz，`SinCos`/`PiOutput`/`1/vbus` 缓存等 P0/P1 优化保留。
- 引脚/测试点只查 `docs/hardware-requirements.md` §3.1 / §4.3 / §5；不复制到别处。
- T-014 环：2048×16 B = 32 KiB `.bss`；512 预 + 1536 后触发；stride=1 → 102.4 ms，stride=10 → 1.024 s（见 `docs/instrumentation.md` §4.2）。

## 3. HIL

HIL: PASS 2026-11-27 T-012 clock/PWM 安全关断态（无电机、电源 OVP 开、未 `enable`）

- 探针：J-Link V9.8 PLUS；`probe-rs download --verify` 成功；reset 后 RTT：
  - `clock=ok (HSE 16M / PLL 170M)`；RCC 日志 `sys=170 MHz`、`adc=42.5 MHz`。
  - `owner=ok (Board moved into Local)`。
  - `pwm=ok arr=4250 dtg=85 moe=false aoe=false bke=false nfault=false`。
- 未做：`enable` 后的 20 kHz/500 ns 波形、死区 LA、T-013 ADC/OPAMP。
- 已知坑：`embassy-stm32 0.1.0` 的 `ADC1::frequency()` 在 `adc12sel=PLL1_P` 时 `unwrap` 了
  `pll1_p`（RCC 日志为 `None`）；T-012 已移除 ADC 采样，T-013 必须绕过/修正。

HIL: PASS 2026-11-27 T-014 pre/post ring + stride（物理波形未测）

- RTT：`dwt=ok cycles=73`；`trace=freeze dac1=2048 dac2=4095`；`trace=ok dac1=2048 dac2=0`。
- `recorder pre=512 post=1536 len=2048 total=3584 trigger=true locked=true stride=1 rearm=true`；
  `stride=10` 同样 `pre=512 post=1536 len=2048`。自检先写满 2048 帧再触发，导出首帧 `k=1536`，证明覆盖最旧帧。
- 时间戳：TIM5 1 MHz；DWT 修前 `cycles=0`，补 `DCB::enable_trace()` 后 73。
- 未测：PB6/PB7 边沿、PA4/PA5 实际电压（示波器 192.168.1.77:5025 不可达）；真实 20 kHz 写者/2 kHz 运行时序。
- 已裁决：保留 2048×16 B；stride=1 抓瞬态，stride=10 抓慢速收敛（1.024 s）。
- T-015：ADC 根因是 PAC `adc_v4` 的 `RES` 编码与 G474 不一致（`TWELVEBIT` 实际 10 bit）；`wake` 等待从 1 ms 放宽到 10 ms 后 nFAULT 稳定。

HIL: PASS 2026-11-27 T-015 20 kHz current task（ADC/rate）

- `current errors=0 isr_cycles=1952 isr_count=20013 recorder pre=512 post=1536 len=2048 total=3536 locked=true`。
- `adc0 n=32 a=2037.7 b=2038.3 c=2038.8`；`r_lsb≈-1`；`nFAULT=false`、`n_sleep=true`、`MOE=false`。
- ADC 根因：PAC `adc_v4` 的 `RES` 编码与 G474 不一致；`Res::TWELVEBIT` 写成了 10 bit，
  raw 509 是 1.247 V 的 10-bit 码。按 `CFGR[4:3]=00` 后恢复 12 bit。
- `Pwm::wake()` nSLEEP 等待改为 10 ms，`nFAULT` 不再偶发低。
- 已测：TP1=1.2623 V；`cargo xtask dac 2 4095` 静态保持时 TP2=2.496 V（DAC2/DOR2=4095）。
- 未做：真实 76.8 ms 冻结脉冲的示波器单次捕获、`enable` 20 kHz/500 ns 波形与死区 LA；注入组/TIM1_TRGO2 仍待实现。

## 4. 待裁决（人）

- 硬件过压 break 缺口：补 TIM1_BKIN 还是只做软件 OVP 停机。
- 500 ns 死区 LA 实测；nSLEEP 1 ms 唤醒是否足够。
- T-013 的 A/C 相 OPAMP/ADC 交叉映射台架确认。
- `CalibRecord` 字段集与 A/B 新旧判定（决定 `bsp` flash 接口）。
