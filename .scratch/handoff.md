# STATE — handoff

> 活跃 Thread 上限 3 个；关闭的 Thread 只进 commit message，不留在本文件。
> 上位：`AGENTS.md`（RULES）、`docs/INDEX.md`（SPEC）。

## 1. 活跃 Thread

| ID | 模式 | 目标 | 冻结/基线 | 状态 |
|---|---|---|---|---|
| T-015 | B | 20 kHz current task：TIM1_UP → OPAMP/ADC → GainGate 残差 → DAC/TP_SYNC/录波 | T-014 HIL 后 | **HIL PASS（ADC/rate/DAC/freeze；2026-11-28 无电机复跑）**：`adc0≈2039`、`r_lsb∈{-1,0,1}`、`isr_count=20004/s`、`isr_cycles=692`（MOE=0）/1789（MOE=1）；`--enable-zero` 时 `trigger=true locked=true`；TP1=1.2623 V、TP2=2.496 V；六路门极/死区波形未测 |
| T-014 | B | trace 基础设施：PB6/PB7 TP_SYNC、DAC1_OUT1 残差/OUT2 冻结、TIM5 1 MHz、DWT、512/1536 预/后触发环与 `stride` | T-012 HIL 后 | **HIL: PASS（RTT/寄存器）**：`recorder pre=512 post=1536 len=2048`；stride 1/10 均锁存；物理 DAC/TP_SYNC 未测 |
| T-016 | A | 编码器 bring-up：SPI1 软件片选 + 两片 MT6701 原始帧（无磁铁、无电机） | T-012 关闭后 | **HIL PASS（SPI/CRC）**：两片各 8/8 帧 CRC 通过；SPI Mode 1 实测（Mode 0/2/3 全 0）；T_PwrUp 后需等 ≥32 ms 再读；`mag=10`（弱场 + 失锁位），无磁铁角度无意义；角度/零位/跳变待装磁铁 |

## 2. 当前工作区

- 代码已按用途分组：`software/{contracts,algorithms,bsp,firmware,xtask}`；硬件资料 `hardware/pcb/`。
- 最新网表：`hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel`（以此为准）。
- T-012 的代码/文档改动与目录重排同批提交；`main` 仍只读回、不 `enable`。
- T-010（远端）已并入：载波保持 20 kHz，`SinCos`/`PiOutput`/`1/vbus` 缓存等 P0/P1 优化保留。
- 引脚/测试点只查 `docs/hardware-requirements.md` §3.1 / §4.3 / §5；不复制到别处。
- 架构降熵治理：`docs/architecture/governance.md` 记有轮次 1–13 的熵源、方案、反证与验证；门禁现覆盖 firmware / 根 / xtask / bsp 四个 workspace，`cargo xtask test` 含检查器自测。
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
- 已测：`hil --boot-freeze` 先给 1 s 高；boot 结束后约 100 ms 又出现 76.8 ms 自动触发高脉冲，二者均约 2.49 V。
- 已实现并 HIL 通过：ADC1/2/3 注入组 + TIM1_TRGO2 + 三路 JDR1。
- HIL：`adc0 a/b/c≈2039`、`isr_count=20004/s`、`isr_cycles≈2005`、`errors=0`；注入组必须 `JQDIS=1` 且 `JADSTART=1`（RM0440 §21.4.15/§21.4.18）。
- **HIL PASS（enable-zero）**：`moe=true`、`aoe=false`、`bke=false`、`nfault=false`、`isr_count=20004/s`、`errors=0`、`adc0≈2048`；
- **HIL PASS（低边窗口判据）**：`--enable-zero` 时 `DATA`/`trigger=true`，普通 `hil`（MOE=0）时 `recorder trigger=false`；判据由 TIM1 `MOE=1 + CC1NE/2NE/3NE=1` 实时产生。低边同时导通按互补 PWM 默认，不测门极波形；只用 TP1/TP2/PB6/PB7。
- 复位启动：最早期把 PB7 拉高作“已进 init”标记；`Trace` 提前初始化；`Pwm::wake` 的 nFAULT 等待从 10 ms 放宽到 100 ms；时钟校验重试 10×10 ms；wake/ADC/零位/门控失败时 TP2+PB7 常高作故障码。
- 手动复位“全无输出”已定位：`probe-rs`/GDB 留下的 `DEMCR.VC_CORERESET=1` 把核停在 `Reset`（PC 0x080001d8，DHCSR C_HALT=1）；NRST 仍为 3.3 V，但固件没执行。清除 DEMCR bit0 或断电重上电后恢复；不是芯片卡死。

HIL: PASS 2026-11-28 T-015 复跑（无电机，`cargo xtask hil`，J-Link `1366:0105:000602712682`）

- `clock=ok (HSE 16M / PLL 170M)`；`adc=ok`；`opamp en/into/vm` 全 true。
- `adc0 n=32 a=2038.9 b=2039.6 c=2039.1`；`pwm=ok arr=4250 dtg=85 moe=false aoe=false bke=false nfault=false n_sleep=true`。
- 1 s 汇总：`current errors=0 isr_cycles=692 isr_count=20004 recorder … trigger=false locked=false`（MOE=0 不触发，符合窗口判据）。
- 未测：PB6/PB7 边沿、DAC 实际电压（无示波器读数）。

HIL: PASS 2026-11-28 T-015 零矢量使能复跑（无电机，`cargo xtask hil --enable-zero`）

- `hil=enable-zero moe=1`；`pwm=ok arr=4250 dtg=85 moe=true aoe=false bke=false nfault=false`。
- 1 s 汇总：`isr_cycles=1789 isr_count=20003`（< 25 µs 预算）；`recorder pre=512 post=1536 len=2048 total=3536 trigger=true locked=true stride=1`。
- DATA 帧：`ia/ib/ic≈2039`（零位 2039），`r_lsb ∈ {-1,0,1}`，`flags=0`。
- 未测：六路门极/死区物理波形、电机负载。

HIL: PASS 2026-11-28 T-016 编码器 SPI bring-up（两片 MT6701 已焊、无磁铁、无电机）

- `cargo xtask hil`：`enc id=1 spi_ok=8/8 crc_ok=8/8 last=0x8682b6 ang=8608 mag=10`；`enc id=2 … 0x88be9d ang=8751 mag=10`。
- 模式扫描（同一固件逐模式 8 帧）：Mode 0/2/3 均 `crc_ok=0/8`，**Mode 1 两片均 8/8**；已按实测锁定 `MODE_1`（手册 Figure-24 的"空闲 CLK 高"表述与本板不符）。
- 复位后 60 µs 读会全错（`crc_ok=0/8`），等 35 ms（T_PwrUp 上限 32 ms）后正常 ⇒ 上电等待是必要条件。
- `mag=10`：`Mg[1:0]=2`（磁场过弱）、`Mg[3]=1`（失锁/超速位，手册语义不确定）；无磁铁时角度不可用。
- 未做：磁铁装配后的角度线性/零位/跳变、双编码器切换时序、20 kHz 任务内读取（SPI1 目前 init 用完即释放）。

## 4. 待裁决（人）

- nSLEEP 唤醒时间缺 MP6540HA 手册确认（现实现：`wake` ≤100 ms 轮询 nFAULT，`enable` 另等 1 ms；多次 HIL 未见抖动）。
- T-013 的 A/C 映射由 HR §3.1 固定（本文件不复写引脚），不作为 HIL 项。
- `CalibRecord` 字段集与 A/B 新旧判定（决定 `bsp` flash 接口）。
- 已裁决 2026-11-28：硬件过压 break（只做软件 OVP）、死区（暂不 LA 实测，保持 500 ns）、NTC（不装）→ INDEX §3 ADR-18/19/20。
