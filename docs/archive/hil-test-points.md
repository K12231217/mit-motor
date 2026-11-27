# HIL 测试点与探针（G474RBT6，2026-11-27 网表）

> 权威网表：`hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel`（Schematic14_1，2026-11-27）。
> 引脚分配以此为准；`hardware-requirements.md` §3.1 是它的文字覆盖表。
> `hardware/pcb/Netlist_PCB14_2026-11-11.tel` 是 G491RET6 旧板，`hardware/pcb/Desktop.ioc` 是更早的
> CubeMX 工程；两者与最新网表冲突时**一律不用**。

## 1. 最新网表（U1 = G474RBT6 LQFP64）

| U1 脚 | MCU | 网络 | 功能 / 备注 |
|---|---|---|---|
| 42 / 43 / 44 | PA8 / PA9 / PA10 | PWM_AH / PWM_BH / PWM_CH | TIM1_CH1/2/3，高边 |
| 35 / 36 / 37 | PB13 / PB14 / PB15 | PWM_AL / PWM_BL / PWM_CL | TIM1_CH1N/2N/3N，低边 |
| 39 | **PC7** | **NSLEEP** | 驱动使能输出，高有效 |
| 40 | **PC8** | **NFAULT** | 驱动故障输入，开漏低有效，上拉 |
| — | — | **无 BKIN 网络** | 硬件过压 break 未引出；`BKE` 必须为 0 |
| 18 | PA4 | DAC1_OUT1 | TP1；外部脚，未接内部采样链 |
| 19 | PA5 | DAC1_OUT2 | TP2；外部脚，未接内部采样链 |
| 17 | PA3 | OPA1 | OPAMP1 输入；经 R24 接 **SOC**（C 相） |
| 21 | PA7 | OPA2 | OPAMP2 输入；经 R25 接 **SOB**（B 相） |
| 24 | PB0 | OPA3 | OPAMP3 输入；经 R26 接 **SOA**（A 相） |
| 15/16/17（U9） | — | SOA / SOB / SOC | MP6540 驱动侧输出；经 4.99 kΩ 网络到 OPA |
| 33 | PB11 | VBUS_ADC | 母线分压 ADC |
| 5 / 6 | PF0 / PF1 | OSC_IN / OSC_OUT | 16 MHz HSE |
| 56 / 57 | PB3 / PB4 | SPI1_SCK / SPI1_MISO | MT6701 只读 |
| 52 / 54 | PC10 / PC12 | ENC2_CS / ENC1_CS | 编码器片选 |
| 59 / 60 | PB6 / PB7 | USART3_TX / USART3_RX | 网表网络名；实际 AF 需核对（.ioc 为 USART1） |
| 45 / 46 | PA11 / PA12 | CAN_RX / CAN_TX | FDCAN1；USB 已不在本板 |
| 28 | — | VREF+ | 3V3 基准/偏置源 |

**相序注意**：A 相在 OPA3/PB0，C 相在 OPA1/PA3，B 相在 OPA2/PA7。T-013 的 ADC/OPAMP
通道映射必须按这个交叉写，不能假设 A→ADC1、B→ADC2、C→ADC3。

## 2. 测试点

最新网表只导出 TP1/TP2/TP3：

| TP | 网络 | 用途 |
|---|---|---|
| TP1 | DAC1_OUT1（PA4） | 外部模拟注入/基准；不是内部电流采样链 |
| TP2 | DAC1_OUT2（PA5） | 同上 |
| TP3 | 未分配 | 网表无网络，按实物确认 |

实物的 `SOA/SOB/SOC` 测试脚：先通断确认是 U9.15/16/17（驱动侧）还是 OPA1/2/3（运放输入侧）。
两侧只差 4.99 kΩ 网络，但 HIL 结论不能混。PCB14 的 TP5/6/7 是滤波后 ADC 侧，和本板不是同一套。

## 3. T-012：clock + PWM 探针

先测安全关断态（当前 `main.rs` 不调用 `enable`）：

| 探针 | 位置 | 期望 |
|---|---|---|
| +3V3 / GND | 板级 3V3/GND | 3.30 V |
| HSE（可选） | PF0 / PF1（5/6） | 16 MHz |
| HS A/B/C | PA8/9/10（42/43/44）→ U9.6/7/8 | 低 |
| LS A/B/C | PB13/14/15（35/36/37）→ U9.3/4/5 | 低 |
| nSLEEP | PC7（39）→ U9.2 | 低 |
| nFAULT | PC8（40）→ U9.1 | 高 |
| BKIN | 无 | 不存在 |

RTT 必须出现：`clock=ok (HSE 16M / PLL 170M)`、
`pwm=ok arr=4250 dtg=85 moe=false aoe=false bke=false`。

安全关断态通过后，才允许在不接电机、KA3005P 限流下临时 `pwm.enable()` 测：HS/LS 互补、
周期 50 µs、死区约 500 ns、`AOE=false`、`BKE=false`；测完立即 `disable()`。

**硬件过压缺口**：最新网表没有把比较器输出接到 TIM1_BKIN，`BKE=0`。在补上硬件 break
或明确的软件 OVP 停机路径之前，不允许把 `enable` 用于带母线/带电机的长时间运行。

## 4. T-013：电流采样 / DAC / 片内 OPAMP

- `SOA/SOB/SOC`（U9.15/16/17）→ 4.99 kΩ 网络 → `OPA3/OPA2/OPA1`（PB0/PA7/PA3）。
  静态期望偏置约 1.65 V（VREF+/2）；HR-203 标度 0.2712 V/A、2.97 mA/LSB。
- `DAC1_OUT1/2`（PA4/PA5，TP1/TP2）只到外部脚；只能作为外部注入/基准，经外部电阻送到
  SOA/SOB/SOC 或 OPA，不能假设写 DAC 就改内部采样链。
- 片内 OPAMP1/2/3 作 follower；输出内部到 ADC1/2/3。没有 OPAMP4/COMP2_OUT 网络。
- 相序交叉（见 §1）必须在 `sense`/ADC 配置里显式处理；否则 A/C 相会互换。

## 5. 安全红线

- 确认 nSLEEP=PC7、nFAULT=PC8、无 BKIN 后再上电；当前代码已按此改。
- 先限流、先看 3V3/5V、nSLEEP 低，再开 PWM；不接电机。
- 台架结论只写进 `.scratch/handoff.md` §3 的 `HIL: PASS` 行；没有记录就是 `PENDING_HIL`。
