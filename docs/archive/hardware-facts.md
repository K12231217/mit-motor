# 硬件事实（PCB14 / G474RBT6 引脚与模拟链）

> 本文件**只**记录硬件事实：引脚、器件行为、采样链与安全路径。固件文件架构见
> [software-architecture.md](software-architecture.md)；需求编号与验收方法见
> [hardware-requirements.md](hardware-requirements.md) §2/§3。
>
> **2026-11-27 更新**：当前实物以 `hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel` 与
> `hardware-requirements.md` §3.1 为准；本文 §2.1 的 PCB14 引脚表只作旧板参考。
> nSLEEP/nFAULT 为 PC7/PC8，无 TIM1_BKIN；SOA/SOB/SOC 经 OPA3/OPA2/OPA1（PB0/PA7/PA3）。
>
> 2026-11-15 主控变更：G491RET6 → **G474RBT6**（同封装 LQFP64、128 K Flash，引脚功能逐脚核对
> 兼容；RC/RE 是同脚位仅差 Flash 的升级路径）。本节引脚分配在 G474RBT6 上仍然成立，但有两处与
> PCB14 实物不符：**PC6/PC7/PC8 在网表里没有网络**（原"预留 strobe"实际未连接）、PC1 只有孤立
> 网络标签 `ADC12_IN7`。V2 板需求见 [hardware-requirements.md](hardware-requirements.md) §1/§3。

## 1. 范围

做：单关节驱动器的硬件——三相 PWM、低边电流采样、MT6701 角度、CAN FD、母线/温度采样与保护。

不做：无感控制、弱磁/MTPA、参数辨识、双编码器故障检测、CANopen、bootloader/OTA、热模型。

## 2. 引脚与器件事实

### 2.1 引脚（PCB14 网表 + G491RET6 LQFP64 手册核对）

| 功能 | 引脚 | 网名 | 外设 |
|---|---|---|---|
| PWM A/B/C | PA8(42) / PA9(43) / PA10(44) | PWM_AH / PWM_BH / PWM_CH | TIM1_CH1/CH2/CH3 |
| LS A/B/C | PA7(21) / PB14(36) / PB15(37) | PWM_AL / PWM_BL / PWM_CL | TIM1_CH1N/CH2N/CH3N（互补 + BDTR 死区）|
| SOA / SOB / SOC | PA2(14) / PC4(22) / PB1(25) | SOA_F / SOB_F / SOC_F | ADC1_IN3 / ADC2_IN5 / ADC3_IN1 |
| TIM1_BKIN | PA6(20) | BKIN | 比较器 TLV7021（过压）|
| nFAULT | PB0(24) | NFAULT | EXTI，开漏 + 10 kΩ 上拉 |
| nSLEEP | PC5(23) | NSLEEP | GPIO（内部 1 MΩ 下拉）|
| SPI1 | PB3(56) SCK / PB4(57) MISO | — | 两片 MT6701 共用 |
| 编码器片选 | PB5(58) / PB6(59) | ENC1_CS / ENC2_CS | GPIO |
| FDCAN2 | PB13(35) TX / PB12(34) RX | — | TCAN1044（S 脚 10 kΩ 下拉，常开）|
| USART3 | PB10(30) TX / PB11(33) RX | — | CN3（GH1.25-3P）|
| USB FS | PA11(45) DM / PA12(46) DP | — | Type-C，CC 5.1 kΩ 下拉（仅 device）|
| 母线电压 | PA3(17) | VBUS_ADC | ADC1_IN4，100 k/8.06 k → 13.4:1 |
| NTC | PC0(8) | ADC12_IN6 | ADC1_IN6 |
| HSE | PF0(5) / PF1(6) | OSC_IN / OSC_OUT | 16 MHz |
| 调试探针 | PC6(38) / PC7(39) / PC8(40) | — | 预留 strobe（PCB14 未连接）|

空闲可用：PC9(41)、PA15(51)、PC10–PC12(52–54)、PD2(55)、PB7(60)、PB9(62)、PA4(18)、PA5(19)、PB2(26)。

### 2.2 MP6540HGU-A-Z 的行为

BOM 实发 **MP6540HGU-A-Z**，而 MPS 的订货号规则是：**`MP6540HGU-A-Z` 就是 MP6540HA**（PWM+EN 的 MP6540H 是 `MP6540HGU-P`）。
依据：数据手册引脚表把 6/7/8 排成 `PWMA/B/C`（H）与 **`HSA/HSB/HSC`（HA）** 两列；
MPS 论坛 topic 5570 有用户踩坑原话；厂商摘要写"MP6540HA has separate high-side and low-side inputs"。
**本节早前把它读成 H 版（pin 3/4/5 = ENA/B/C）是错的**，PCB14 网表里的 `PWM_AL/BL/CL` 命名本来就是对的（= LSA/LSB/LSC）。

- pin 6/7/8 = **HSA/HSB/HSC**，pin 3/4/5 = **LSA/LSB/LSC**。六路都是电平输入，**没有 EN 引脚**。
  代码里叫 `HS_x` / `LS_x`；把 pin 3/4/5 当 EN 拉高 = 该相低边常通，上管一开通即直通。
- **死区必须由 MCU 产生**（TIM1 BDTR），芯片内部不再代劳。
- 故障语义：六路全部无效 = 上下管全关 = **自由停车**。"BKIN 是低边常通（刹车）"只对 PWM+EN 的 H 版成立，这里不成立。
- `nSLEEP` 仍是真正的关断（10 kΩ 下拉，上电默认关断）。
- SOx 是**双向电流输出**：典型比例 1/9200（范围 1/10500…1/7800），1 A 相电流对应约 114 µA。
- 板上偏置网络是 **4.99 kΩ / 4.99 kΩ**（HR-203），静态 1.65 V，戴维南阻抗 2.495 kΩ。于是

  $$V_{SO} = 1.65 + \frac{I_{ph}}{9200}\times 2.495\,\text{k}\Omega \approx 1.65 + 0.2712\,I_{ph}$$

- 12 bit @ 3.3 V → **2.97 mA/LSB**（0.2712 V/A）。工作区按 ±5 A 设计，±6.08 A 只是电气满量程。
- 待实测：低边输入拉低是否使该相彻底高阻（手册只说 "enable the low-side MOSFET"，推定为关断）。

### 2.3 采样链

- 三个 SOx 落在 **三个独立 ADC** 上；TIM1_TRGO2 单次触发三路同时转换。**跨 dual 对（ADC1/2 ↔ ADC3/4）
  无硬件同步通路**，固定偏斜按 ≤ 50 ns 补偿（`software/contracts/src/motor/config_hw.rs` 的 `ADC_PAIR_SKEW_MAX_NS`）；
  "跨组零偏斜"已证伪，见 `docs/INDEX.md` §4-10。
- 前端 100 Ω + 100 pF：$\tau \approx 110\,\text{ns}$；ADC 采样 92.5 cyc @ 42.5 MHz = 2.176 µs，
  加转换 12.5 cyc = 0.294 µs，再留 0.5 µs 余量 ⇒ 窗口下限 5.0 µs（HR-202/HR-303）。
- 有效采样窗口 = **三相低边同时导通的零矢量时刻**（center-aligned、计数器回零）。
- VREF+ 接 3V3（TL431 的 2.5 V 只喂比较器）→ 绝对精度受 LDO 支配；相对增益标定不受影响。

### 2.4 硬件安全路径

- 过压：TLV7021 比较 `VBUS` 分压（100 k/9.76 k）与 2.5 V 基准，翻转点**按阻值计算约 28.1 V**（待实测），
  输出经 1 kΩ 直连 **TIM1_BKIN**，硬件关断 PWM。
- 驱动芯片故障：nFAULT 开漏 → PB0，需要软件响应。
- 注意：**HA 版六路全无效 = 上下管全关 = 自由停车**，不存在"BKIN 变成低边常通"的问题；
  反过来，若要刹车得主动把三相低边拉高。
- SOx 只在**低边导通**时有意义（见 §2.3），而 HA 版低边由 `LSx` 单独控制，采样窗口判据因此要看
  `LSx` 而不是 `PWMx=0`。

## 3. 未决（仅硬件事实）

未决清单的唯一来源是 [`docs/INDEX.md`](INDEX.md) §5；本文件不复制。与本文件直接相关的两条：
CCM 别名是否启用、OVP 翻转点与 EN 高阻行为待台架实测。
