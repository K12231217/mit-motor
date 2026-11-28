# V2 引脚需求表（历史，已作废）

> 冷数据：仅作 PCB14 → V2 差异的历史记录，**不是引脚真源**。
> 现行唯一真源：[`../hardware-requirements.md`](../hardware-requirements.md) §3.1（2026-11-27 网表 + DS Table 12）。
> 本表与本表之后任何冲突行一律作废，不再维护。

`沿用` = 与 PCB14 同脚同功能；`改` = 与 PCB14 不同（原因写在第 5 列）；`新增` = PCB14 上不存在。
V2 与 PCB14 的差别集中在模拟前端（为片内 OPAMP 让路）、6 路 PWM、CAN/UART/BKIN 的挪位——
**"同封装可直接换料"只对"能不能跑起来"成立，不代表 V2 能沿用 PCB14 的走线。**

| 功能 | 引脚 | Pin | PCB14 | G474RBT6 上确认 | V2 动作 |
|---|---|---|---|---|---|
| PWM_HS A/B/C | PA8 / PA9 / PA10 | 42 / 43 / 44 | 已接（同名）| TIM1_CH1/2/3 | 沿用 |
| PWM_LS A/B/C | **PB13 / PB14 / PB15** | 35 / 36 / 37 | PCB14: PA7/PB14/PB15（当时当 EN）| TIM1_CH1N/2N/3N | **改**：PA7 让给 SOB，PB13 让出 CAN |
| SOA / SOB / SOC | **PA1 / PA7 / PB0** | 13 / 21 / 24 | PCB14: PA2/PC4/PB1 | ADC1_IN2 / ADC2_IN4 / ADC3_IN12 + OPAMP1/2/3_VINP + COMP1/2/4_INP | **改**：为片内 OPAMP 跟随器让路 |
| EXT_I_REF（ADC4）| **PB12** | 34 | PCB14: FDCAN2_RX | ADC4_IN3 | 新增（HR-211）；LQFP64 的 ADC4 只有 PB12/14/15 |
| VBUS_ADC | PA3 | 17 | 已接 | ADC1_IN4 + COMP2_INP | 沿用 |
| NTC | PC0 | 8 | 已接 | ADC1_IN6 | 沿用 |
| TIM1_BKIN | **PB10** | 30 | PCB14: PA6 | TIM1_BKIN | **改**：PA6 留给 OPAMP2_VOUT；PA11 是 BKIN2 且是 USB_DM |
| nFAULT | **PB10 + PC9**（同网络）| 30 / 41 | PCB14: PB0 | BKIN + GPIO 读 | **改**：必须同时可读，否则故障源不可知 |
| nSLEEP | PC5 | 23 | 已接 | GPIO，10 kΩ 下拉 | 沿用 |
| SPI1_SCK / MISO | PB3 / PB4 | 56 / 57 | 已接 | SPI1 | 沿用（**不用 MOSI**，MT6701 只读 3 线）|
| ENC1_CS / ENC2_CS | **PB7 / PB9** | 60 / 62 | PCB14: PB5/PB6 | GPIO | **改**：PB5/PB6 让给 CAN |
| FDCAN2_RX / TX | **PB5 / PB6** | 58 / 59 | PCB14: PB12/PB13 | FDCAN2 | **改** |
| USART3_TX / RX | **PC10 / PC11** | 52 / 53 | PCB14: PB10/PB11 | USART3 | **改** |
| USB_DM / DP | PA11 / PA12 | 45 / 46 | 已接 | USB_FS | 沿用 + ESD |
| SWD | PA13 / PA14 | 49 / 50 | 已接 | SWD | 沿用 + NRST(7) |
| BOOT0 | PB8 | 61 | 已接 | BOOT0 | 沿用 + 10 kΩ 下拉 + 跳线 |
| HSE | PF0 / PF1 | 5 / 6 | 已接 | OSC_IN/OUT | 沿用（16 MHz 或 24 MHz 均可，不影响 USB）|
| STROBE1/2/3 | PC6 / PC7 / PC8 | 38 / 39 / 40 | **网表里无网络** | TIM8_CH1/2/3、HRTIM1_CHF1/CHF2/CHE1 | 新增引出（HR-501）|
| DAC 遥测 | PA4 / PA5 | 18 / 19 | 空闲 | DAC1_OUT1/2 | V2 历史；最新 T-014：OUT1 残差 / OUT2 冻结（HR-803） |
| COMP2_OUT | PA2 | 14 | 空闲 | COMP2_OUT | 新增 TP（HR-808）|
| 状态 LED | PB2 | 26 | 空闲 | GPIO | V2 历史；最新网表无 MCU LED（HR-505）|
| 母线分流（选装）| PA0 | 12 | 空闲 | ADC1_IN1/ADC2_IN1 | 预留（HR-210）|
| 保持空置 | PA6、PB1 | 20 / 25 | — | OPAMP2_VOUT / OPAMP3_VOUT | 便于日后关掉 OPAMP 直接接 ADC |
| 其他空闲 | PA15, PB11, PC12, PC13, PC14, PC15, PC1, PC2, PC3, PD2 | 51,33,54,2,3,4,9,10,11,55 | 空闲 | — | 留焊盘（HR-606）|

> **§3 表核对基准**：本表已按 `docs/DS_stm32g474rb/Pinouts_and_pin_description.md` Table 12 逐脚核对
> （AF 与 additional functions 均查），2026-11-16。**任何引脚改动必须重核该表；散文与表冲突时以本表为准。**

**核对结论**：PCB14 在用的 29 个 MCU 信号（不含 NRST 与只有网络标签的 PC1），在 `STM32G474R(B-C-E)Tx.xml` 上逐脚比对无功能丢失，
所以"换料能跑"成立；但 V2 的功能分配另有 6 处改动（模拟前端、6 路 PWM、CAN、UART、BKIN、ADC4），
**PCB14 的走线不能直接沿用**。上表是交给外包的唯一版本。

**引脚层面的两个附带发现**（来自网表，不是原理图）：
- PC6/PC7/PC8 在网表里没有任何网络——architecture §9 写的"预留 strobe"在 PCB14 上并不存在。
- PC1 只有一个孤立网络名 `ADC12_IN7`，没有其它连接，属未完成设计。

时钟树：HSE 16 M → PLL M=4/N=85/P=2 ⇒ 170 MHz（boost）；ADC 42.5 MHz；
USB 48 MHz 由 HSI48+CRS；CAN FD 位定时由 HSE 派生。
