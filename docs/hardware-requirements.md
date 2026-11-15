# 硬件需求规格（V2 板，STM32G474RET6 + MP6540HA）

> 状态：草稿 v0.3（2026-11-15）。主控 **STM32G474RET6**（LQFP64，512 K Flash / 128 K SRAM），
> 驱动 **MP6540HA**（订货号 `MP6540HGU-A-Z`，HS/LS 六输入，见 [architecture.md](architecture.md) §2.2）。
> 与 PCB14 的关系：同封装、引脚功能逐脚核对兼容（§1），**但毕设必需的三类设施在 PCB14 上不存在**（§1.3）。
> 相关：[architecture.md](architecture.md) 固件结构；[research-plan-v1.1-2026-11-04.md](research-plan-v1.1-2026-11-04.md) 研究方案；
> 外包任务书的对抗审核与错误清单见 [taskbook-review.md](taskbook-review.md)。

## 0. 这份文档怎么用

V2 板不是为了换 MCU。换 MCU 是换料级改动；V2 板要做的是**补齐三个实验的物理条件**：
物理增益注入没有位置、ADC 同步性无法实测、台架标定缺电流注入点。

每条需求都带验收方法。没有验收方法的需求不写——毕设的可信性来自可测，不是来自设计意图。

## 1. 主控更换的实际影响（已逐脚核对）

| 项 | PCB14 / G491RET6 | V2 / G474RET6 | 后果 |
|---|---|---|---|
| 封装 | LQFP64 | **LQFP64** | 同封装，PCB14 可**直接换料**，不必重排信号 |
| 引脚功能 | G491RET6 | G474RET6 | G491 在用的 29 个信号功能**全部存在**（功能集合无丢失）|
| Flash | 512 K | **512 K** | 不变；debug 构建也可容纳（§1.1）|
| SRAM | 112 K | **128 K**（SRAM1 80 + SRAM2 32 + CCM 16）| CCM 不可被 DMA 访问 |
| 外设 | 3 ADC | 5 ADC / 7 COMP / 4 DAC / 6 OP-AMP / HRTIM / CORDIC / FMAC | §2.H 的调试设施因此可以白拿 |

核对依据：CubeMX 器件库 `db/mcu/STM32G474R(B-C-E)Tx.xml`、`STM32G491R(C-E)Tx.xml` 两个器件文件
按脚位比对，以及 PCB14 网表 `pcb/Netlist_PCB14_2026-11-11.tel`（**在 `hw/pcb14` 分支上，本分支没有这个文件**）；`firmware/` 已切到 `stm32g474re` 并
`cargo build` / `cargo build --release` 双双通过。

**两条路径，建议都走**：
- 路径 A（1 天）：现有 PCB14 换焊 G474RET6，验证固件能否跑起来；
  代价是没有 §1.3 的任何设施，只能测"能不能跑"。
- 路径 B（1–2 周）：打 V2 板，把 §1.3 的缺口一次补齐。**只有路径 B 能产出论文的实验数据。**

### 1.1 Flash 预算（已实测，反转结论写在这里）

烧进 flash 的镜像大小（`llvm-objcopy -O binary`，不是 ELF 大小）：

| 构建 | flash 镜像 | 占 128 K | 占 512 K |
|---|---|---|---|
| debug（仅 blinky + defmt）| **65,216 B** | 50 % | 12 % |
| release（同一模板）| **16,664 B** | 13 % | 3 % |

**128 K 到底够不够，取决于构型，不取决于代码量：**

- 全量应用（FOC + 标定状态机 + FDCAN + USB CDC + 遥测 + 编码器）release 估算 40–50 KB，
  其中**自己写的算法只占 10–15 KB**，其余是 embassy 外设层与 USB 协议栈 ⇒ 128 K 在 release 下够用（约 35 %）。
- 但本模板的 **debug/release = 3.9×**（65,216 / 16,664，未优化 + 未内联 + defmt 格式化代码），
  同一套全量应用在 debug 构型就是 110–175 KB ⇒ **128 K 在 debug 构型下不够**。
- 而 bring-up 与硬件在环实验恰恰是"flash debug 构型、看 RTT"的阶段。

所以选 RET6 的理由不是"128 K 写不下代码"，而是**"128 K 只能在 release 构型下工作，而开发阶段需要 debug 构型"**。
代价：RET6 与 RBT6 同封装同脚位，只差 BOM 价差，零改板风险。

### 1.2 三个必须在投板前确认的技术风险

1. **ADC1/2（ADC12 组）与 ADC3（ADC345 组）跨组是否同拍启动**。毕设"三 ADC 无采样偏斜"的立论依赖它，
   而 G4 的两个组有独立的触发选择寄存器与时钟源（按 RM0440 ADC 触发表逐条核对）。
   本设计不假设为零：HR-201 要求**实测偏斜并写入文档**，HR-504/804 提供两种实测手段。
   若偏斜不可忽略（> 50 ns），论文表述改为"偏斜固定且可标定"，并给出补偿量与补偿后残差。
2. **USB FS 时钟**：170 MHz 下 VCO = 340 MHz，PLLQ 只能取 /2/4/6/8（42.5/56.7），得不到 48 MHz。
   V2 用 HSI48 + CRS 供 USB；HSE 16 MHz 只服务 CAN FD 位定时。这是时钟树的前提，不是优化项。
3. **RAM 的分块**：128 K 总量里 CCM 16 K 不可被 DMA 访问，ADC/SPI/USB 缓冲必须放 SRAM1/SRAM2，
   黑匣子（HR-814）放在 SRAM2；`stm32-metapac` 对 G4 SRAM 的数字是错的（`g474rb/re` 给 96 K），
   所以 `memory.x` 必须手写（已做）。

### 1.3 PCB14 缺口清单（V2 板的存在理由）

按网表核对，PCB14 上**真正空着的 MCU 脚**有 22 个（PC13/14/15、PC1/2/3、PA0/1/4/5、PB2、PC6/7/8/9、
PA15、PC10/11/12、PD2、PB7、PB9），缺口不在引脚余量，而在**电路与引出**：

| # | 缺口 | PCB14 现状 | 后果 |
|---|---|---|---|
| 1 | 增益注入 | 固定 2 k/2 k，无跳线 | 研究内容三的物理增益注入无法做 |
| 2 | 偏置注入 | 只有 +3V3 方向（18 k + TP11/TP18 等）| 只能注正偏置，负偏置缺 |
| 3 | 串联分流位置 | 相线直焊、无断点 | 台架无电流注入点，DMM 参考无处接 |
| 4 | J_SYNC | 无 | ADC 同步性只能靠论文假设 |
| 5 | Strobe | PC6/PC7/PC8 在网表里**没有网络**（architecture §9 写"预留"，实际未连接）| 时序只能猜 |
| 6 | DAC 遥测 | PA4 空 | 只能读帧，看不见波形 |
| 7 | 调试排针 | 无 | 逻辑分析仪无处接 |
| 8 | MCU 控制 LED | 无（LED2 是 3V3 电源指示）| 状态不可见 |
| 9 | USB ESD | 无 | 静电防护缺失 |
| 10 | CAN 连接器 | CAN_H/L 与 VBUS/GND 同一 XT30 | 共模噪声、误插 |
| 11 | 相线端接 | 直焊 | 换电机、插分流器都要动烙铁 |
| 12 | 叠层 | 未记录（疑似 2 层）| 模拟前端噪声无保证 |

## 2. 设计需求

### A. 电源与功率（HR-1xx）

| 编号 | 需求 | 指标 / 取值 | 验收 |
|---|---|---|---|
| HR-101 | 母线输入 | 12–24 V DC，额定 24 V，绝对上限 28 V（由 HR-401 决定）；XT30 输入/直通各一 | 12 V / 24 V 上电，空载 < 80 mA |
| HR-102 | 相电流能力 | 连续 5 A/相、峰值 10 A（≤2 s）；MP6540HA 额定与降额以手册为准 | 5 A 连续 10 min，壳温 < 100 °C |
| HR-103 | 逻辑供电 | 5 V ≥ 500 mA（buck）；**3.3 V LDO ≥ 200 mA，精度 ±1 %** | TP3/TP4 实测，负载阶跃 < 20 mV |
| HR-104 | 母线储能 | ≥220 µF / 35 V，距 MP6540HA 的 VBUS/PGND 引脚 < 5 mm | 目视 + 环路面积复核 |
| HR-105 | 反接与浪涌 | 反接保护 + TVS；反接 10 s 不损坏 | 反接试验 |
| HR-106 | CAN 物理层 | TCAN1044（非隔离）+ 4 pin 锁扣连接器，**与功率输入分开**（缺口 10）| 2 Mbit/s 数据段 24 h 无误帧 |
| HR-107 | USB | Type-C 16P，CC 各 5.1 kΩ，仅 device；DP/DM 22 Ω 串阻；**必须加 ESD 阵列**（缺口 9）| 上电枚举 + 插拔 200 次 |
| HR-108 | 编码器接口 | 2× MT6701 共用 SPI1（SCK/MISO）+ 2 个独立 CS，4 pin 排线 ×2，MISO 上拉 | 读角 1 h 无错帧 |
| HR-109 | 相线端接 | 三相各一个 2 pin 端子（缺口 11），配合 HR-206 | 插拔 50 次接触电阻 < 5 mΩ |

### B. 电流采样链（HR-2xx，毕设核心）

单通道拓扑（三相完全同构）：

```svg
<svg viewBox="0 0 760 300" xmlns="http://www.w3.org/2000/svg" font-family="ui-sans-serif,system-ui,sans-serif" font-size="12">
  <line x1="180" y1="48" x2="400" y2="48" stroke="var(--muted)" stroke-width="1.2"/>
  <text x="404" y="52" fill="var(--muted)">+3V3（= VREF+）</text>
  <line x1="180" y1="270" x2="560" y2="270" stroke="var(--muted)" stroke-width="1.2"/>
  <text x="564" y="274" fill="var(--muted)">GND</text>

  <rect x="16" y="126" width="112" height="48" rx="6" fill="none" stroke="var(--accent)" stroke-width="1.4"/>
  <text x="72" y="146" text-anchor="middle" fill="var(--fg)">MP6540HA</text>
  <text x="72" y="162" text-anchor="middle" fill="var(--fg)">SOx（1/9200）</text>
  <line x1="128" y1="150" x2="440" y2="150" stroke="var(--fg)" stroke-width="1.4"/>

  <rect x="232" y="76" width="16" height="44" fill="none" stroke="var(--fg)" stroke-width="1.3"/>
  <line x1="240" y1="76" x2="240" y2="48" stroke="var(--fg)" stroke-width="1.3"/>
  <text x="256" y="102" fill="var(--fg)">R 4.99k</text>
  <rect x="232" y="180" width="16" height="44" fill="none" stroke="var(--fg)" stroke-width="1.3"/>
  <line x1="240" y1="224" x2="240" y2="270" stroke="var(--fg)" stroke-width="1.3"/>
  <text x="256" y="208" fill="var(--fg)">R 4.99k</text>

  <rect x="204" y="60" width="128" height="180" rx="4" fill="none" stroke="var(--warning)" stroke-width="1" stroke-dasharray="4 3"/>
  <text x="204" y="54" fill="var(--warning)">增益注入：整对等值替换，零位不变</text>

  <line x1="330" y1="150" x2="330" y2="126" stroke="var(--fg)" stroke-width="1.3"/>
  <circle cx="330" cy="124" r="3" fill="var(--fg)"/><circle cx="330" cy="116" r="3" fill="var(--fg)"/>
  <text x="340" y="126" fill="var(--success)">JP</text>
  <line x1="330" y1="113" x2="330" y2="92" stroke="var(--fg)" stroke-width="1.3"/>
  <rect x="322" y="52" width="16" height="40" fill="none" stroke="var(--fg)" stroke-width="1.3"/>
  <line x1="330" y1="52" x2="330" y2="48" stroke="var(--fg)" stroke-width="1.3"/>
  <text x="346" y="76" fill="var(--fg)">18k → +3V3</text>
  <text x="346" y="92" fill="var(--muted)">偏置 ≈ +0.74 A，增益 −12 %</text>

  <rect x="440" y="142" width="44" height="16" fill="none" stroke="var(--fg)" stroke-width="1.3"/>
  <text x="462" y="134" text-anchor="middle" fill="var(--fg)">100Ω</text>
  <line x1="484" y1="150" x2="620" y2="150" stroke="var(--fg)" stroke-width="1.4"/>

  <line x1="540" y1="150" x2="540" y2="196" stroke="var(--fg)" stroke-width="1.3"/>
  <line x1="526" y1="196" x2="554" y2="196" stroke="var(--fg)" stroke-width="1.6"/>
  <line x1="526" y1="205" x2="554" y2="205" stroke="var(--fg)" stroke-width="1.6"/>
  <line x1="540" y1="205" x2="540" y2="270" stroke="var(--fg)" stroke-width="1.3"/>
  <text x="562" y="205" fill="var(--fg)">100 pF</text>

  <rect x="620" y="126" width="126" height="48" rx="6" fill="none" stroke="var(--accent)" stroke-width="1.4"/>
  <text x="683" y="146" text-anchor="middle" fill="var(--fg)">ADCx_INy</text>
  <text x="683" y="162" text-anchor="middle" fill="var(--fg)">三路独立 ADC</text>

  <circle cx="196" cy="150" r="4" fill="var(--c1)"/>
  <text x="196" y="172" text-anchor="middle" fill="var(--c1)">TP</text>
  <circle cx="590" cy="150" r="4" fill="var(--c2)"/>
  <text x="590" y="172" text-anchor="middle" fill="var(--c2)">TP</text>

  <line x1="560" y1="150" x2="560" y2="240" stroke="var(--c3)" stroke-width="1" stroke-dasharray="4 3"/>
  <line x1="560" y1="240" x2="700" y2="240" stroke="var(--c3)" stroke-width="1" stroke-dasharray="4 3"/>
  <circle cx="700" cy="240" r="4" fill="var(--c3)"/>
  <text x="560" y="262" fill="var(--c3)">J_SYNC：三路 ADC 输入共点（同步性实测）</text>
</svg>
```

| 编号 | 需求 | 指标 / 取值 | 验收 |
|---|---|---|---|
| HR-201 | 三路采样落在三个不同 ADC 单元（ADC1/ADC2/ADC3），由**同一 TIM1 事件**触发 | 目标偏斜 ≤ 50 ns；实测值写入测试记录，不做零假设 | HR-504 或 HR-804 |
| HR-202 | 输入网络 | 100 Ω 串阻 + 100 pF 到 GND；三通道拓扑/取值/封装**完全一致**；采样时间 ≥ 68.5 cyc（@42.5 MHz ≈ 1.6 µs），推荐 92.5 cyc（2.2 µs）| 三通道互换电阻后零点与增益一致性 < 1 LSB |
| HR-203 | 偏置与标度 | **4.99 kΩ/4.99 kΩ（±0.1 %，25 ppm）** → 静态 VREF/2、戴维南 2.495 kΩ、0.2712 V/A；12 bit → **2.97 mA/LSB**；±5 A 摆幅 ±1.356 V，电气满量程 ±6.08 A | 台架 ±7 点 × 双向（E1）|
| HR-204 | **增益注入网络**（每相）| 默认 4.99k/4.99k；**上下各并联**一颗（串联会把 Rth 抬高 10 倍，见 [taskbook-review](taskbook-review.md) §2）：并 47 k ⇒ **−9.5 %**，并 1.5 k ⇒ −25 %，换 6.8 k ⇒ +35 %；等值成对 ⇒ 零位保持 VREF/2 | 切换后零位漂移 < 5 mV（E4）|
| HR-205 | 偏置注入网络（每相）| 18 kΩ→+3V3 ⇒ +0.74 A，**但增益同时 −12 %**（18 k 拉低戴维南）；100 kΩ ⇒ +0.15 A / −2.4 %；"纯偏置"应改单个分压电阻的贴装值而非并联注入 | 静态读数变化与预期差 < 5 %，增益耦合量实测记录（E5）|
| HR-206 | 串联分流位置（每相）| 2512 **10 mΩ / 1 W** + 四线开尔文焊盘 + 0 Ω 旁路跳线；短接时相线直通 | 5 A 下 DMM 200 mV 档，误差 < 0.5 % |
| HR-207 | 基准同源 | VREF+ 与 VDDA 接 +3V3（LDO），SOx 偏置也取自 +3V3；不装外部基准芯片（相对标定不需要；绝对刻度用 HR-810 反算 VDDA）| TP4 实测 3.3 V ±1 % |
| HR-208 | 通道对称性 | 三路之间不允许任何单侧滤波、TVS 或不同封装；如需滤波必须三路同值 | 目视 + BOM 比对 |
| HR-209 | SOx 节点可注入 | 保留裸 SOx 焊盘 + 100 Ω 隔离，允许外部源经串阻注入而不拆电阻 | 注入 ±1 mA 时读数线性 |
| HR-210 | **（决策项）母线分流** | 1 mΩ + 差分放大 → PA0(ADC1_IN1)，默认不装；这是架构 §10 未决"是否需要第二观测通道"唯一的硬件入口 | 装机后与三路和电流对比 |
| HR-211 | 外部电流真值通道 | **PB12(ADC4_IN3)** + 3 pin 接口（只出 `EXT_I_REF` 与 GND，**不要把 VREF+ 引到连接器**，见 [taskbook-review](taskbook-review.md) P1-5）；100 Ω 串阻 + 钳位；与三路同帧采样，用来消掉跨仪器对时误差 | 与 DMM/INA240 读数在同一帧内对比，偏差 < 1 % |

> HR-203 的边界：±6.08 A 时 SOx 节点贴到 0 V/3.3 V 轨，**工作区按 ±5 A 设计**，±6 A 只作为满量程定义。
> 这一条必须写进论文，否则"2.97 mA/LSB"会被误读成全量程可用。

### C. 时钟与时序（HR-3xx）

| 编号 | 需求 | 指标 | 验收 |
|---|---|---|---|
| HR-301 | HSE | 16 MHz 晶振（CAN FD 位定时；HSI16 ±1 % 不够）| 频率实测 ±50 ppm |
| HR-302 | 主频与 PWM | SYSCLK 170 MHz（boost）；TIM1 中心对齐 20 kHz，ARR = 4250；**MP6540HA 是 HSx/LSx 六输入、无内部死区，死区必须由 TIM1 BDTR 产生（建议 200–500 ns，按手册 t_d 定）** | STROBE1 周期 50 µs ±0.1 %；死区用 LA 实测 |
| HR-303 | ADC 时钟 | 42.5 MHz（PLLP 170/4）；三个 ADC 同源同频（不同源则 HR-201 不成立）| 寄存器读回 + 时序实测 |
| HR-304 | USB 时钟 | HSI48 + CRS；**不**从 PLL 取 48 MHz（整数分频不可行）| 枚举成功 + SOF 计数 |
| HR-305 | 采样窗口 | 全低边共导窗口 ≥ 3 µs（ADC 2.5 µs + 余量）⇒ 占空比上限 d ≤ 90 %，由固件夹紧 | 示波器测窗口（E3）|

### D. 保护与故障（HR-4xx）

| 编号 | 需求 | 指标 | 验收 |
|---|---|---|---|
| HR-401 | 母线过压 | 比较器（TLV7021）+ 2.5 V 基准 → **TIM1_BKIN（PB10，与 nFAULT 共网）**；100k/9.76k ⇒ 翻转 **28.1 V ±2 %**；响应 ≤ 1 µs | 可调电源缓升，记录翻转点（E6）|
| HR-402 | 驱动故障 | nFAULT 开漏 + 10 kΩ 上拉，**同一网络接两个脚**：PB10(TIM1_BKIN，硬件封锁) + PC9(GPIO/EXTI，软件读故障)；MP6540 的 nFAULT 自清行为按手册确认 | 拉低即封锁 PWM，且软件读得到 |
| HR-403 | 低边通道 | LS_A/B/C = TIM1_CH1N/2N/3N（`PB13/PB14/PB15`，与 HS 互补）；**没有独立 EN 引脚**，相高阻 = 上下输入全低 | LA 测六路时序与死区 |
| HR-404 | 上电默认态 | nSLEEP（PC5）10 kΩ 下拉、六路输入全低、PWM 无输出；只有固件显式释放 | 冷上电用 LA 抓时序 |
| HR-405 | 温度 | NTC 10 kΩ 分压（PC0），贴在 MP6540HA 与母线电容之间 | 两点校准 |
| HR-406 | 母线电压检测 | PA3，100k/8.06k ⇒ 满量程 44.2 V，10.8 mV/LSB | 与 DMM 比对 5 点 |
| HR-407 | 参数保存 | 不需要掉电保存（每次上电自校准）；不设 EEPROM | — |
| HR-408 | 停车语义 | 六路全无效 = 上下管全关 = **自由停车**（不是刹车）；要刹车需主动拉高三相 LS。故障动作：硬件 break 先动作，固件只负责锁存 | 堵转下触发 OVP，确认电机自由滑行 |

### E. 外部调试设施（HR-5xx）

| 编号 | 需求 | 指标 | 验收 |
|---|---|---|---|
| HR-501 | 三个 strobe | **PC6 / PC7 / PC8**（PCB14 已预留但未引出）→ 3 pin 排针 + TP；由 **TIM8_CH1/2/3**（或 HRTIM1_CHF1/CHF2/CHE1）硬件驱动，TIM8 以 TIM1_TRGO 为触发源保持固定相位，边沿位置由 CCR 在周期内编程，不依赖中断 | 示波器/LA 抓取（E3）|
| HR-502 | 调试排针 J_DBG | 2×9：PWM_A/B/C、EN_A/B/C、nFAULT、BKIN、STROBE1/2/3、CAN_TX/RX、UART_TX、VBUS_DIV、GND×2 | 逐针通断 |
| HR-503 | 模拟测试点 | 见 §5 表；丝印标网络名 + TP 编号 | 逐点通断/电压 |
| HR-504 | J_SYNC | 三路 ADC 输入经 3 位跳线可共接到同一注入节点（外部信号源或 TP17 的 DAC 驱动）| 方波能同时出现在三路 |
| HR-505 | 状态指示 | **PB2** 单 LED（新增，PCB14 的 LED2 不可控），编码见 §6 | 逐状态点亮 |
| HR-506 | SWD | PA13/PA14 + NRST(7) + 3V3 + GND，5 pin 2.54 或 10 pin 1.27；PB3 = SPI1_SCK 故 **SWO 不可用**，调试输出走 RTT | probe-rs 烧写 + RTT |
| HR-507 | 串口 | USART3（PB10/PB11）引出 3 pin，115200–1 M | 环回测试 |

### F. 结构与工艺（HR-6xx）

| 编号 | 需求 | 指标 | 验收 |
|---|---|---|---|
| HR-601 | 叠层 | 4 层：信号 / 地 / 电源 / 信号；模拟前端下方完整地平面；功率地与模拟地单点连接 | 叠层图评审 |
| HR-602 | 可手换 | 关键电阻 0603；功率器件与分流器 2512 | BOM 核对 |
| HR-603 | 载流 | 5 A 连续下相线温升 < 20 K（1 oz 至少 3 mm 宽，或改 2 oz）| 5 A 10 min 红外/热电偶 |
| HR-604 | 机械 | 尺寸/孔位由 YHorizon-JM 关节结构决定（**待图纸**），暂定 ≤ Ø45 mm、M2.5 ×4 | 与结构件试装 |
| HR-605 | 标识 | 丝印：版本号、日期、TP/J 编号、LED 编码简表 | 目视 |
| HR-606 | 改板余量 | 22 个空闲脚中预留 4 个焊盘（PA0、PC1、PA15、PB7）+ 1 组备用 UART | 原理图评审 |

### G. 交付与记录（HR-7xx）

| 编号 | 需求 |
|---|---|
| HR-701 | 交付：原理图 PDF、网表、BOM（含替代料）、装配图、阻抗/叠层、需求-验收对照表 |
| HR-702 | 每块板实测记录：OVP 翻转点、3V3/5V、三路零位码、三路增益（台架）、ADC 偏斜、上电时序 |
| HR-703 | 变更记录：每次改板写明改了哪几条 HR 编号 |

### H. G474 片上资源用于调试与开发（HR-8xx）

单列一节的理由：CORDIC / FMAC / DAC / 内部 COMP / AWD / 硬件过采样 / VREFINT / SRAM2 保持
这些片上特性，正好补上毕设最缺的三样东西——**不依赖外部仪器的自检通道、可在固件里改的实验条件、
故障后的现场**。

| 编号 | 需求 | 指标 / 配置 | 验收 |
|---|---|---|---|
| HR-801 | CORDIC | Park / inverse Park 与 atan2 走 CORDIC，取代查表与多项式 | DWT 计时：单次电流环 < 25 µs 且余量可读 |
| HR-802 | FMAC | 标定估计器的点积累加（最小二乘）用 FMAC，不进 20 kHz 环 | 10 ms 标定任务 CPU 占用 < 20 % |
| HR-803 | DAC 模拟遥测 | **PA4（DAC1_OUT1）** 输出可选变量（i_a / i_q / 残差 / 标定状态），1 µs 更新 + RC 低通 → TP17；PA5(DAC1_OUT2) 预留 | 波形与 STROBE1 同拍对齐 |
| HR-804 | DAC → J_SYNC 自检 | PA4 经跳线驱动 J_SYNC 共点：三路 ADC 看同一 DAC 斜坡，**不需要外部信号源**即可测同步、线性与偏斜 | 三路读同一斜坡偏差 ≤ 2 LSB；扫相位得偏斜 |
| HR-805 | 注入组 / 规则组分工 | 三路电流走**注入组**（TIM1 触发、可抢占），VBUS/NTC/VREFINT 走**规则组 + DMA 慢扫** | 20 kHz 下电流点不丢，慢通道 1 ms 更新 |
| HR-806 | ADC 模拟看门狗 | AWD1/2/3 设过流与采样越限阈值，超限直接进硬件中断（不依赖软件残差）| 注入超限电流，中断命中且动作在下一周期内 |
| HR-807 | 硬件过采样 | 台架模式 4–16× 过采样（与 DMM 对齐）；控制环单次采样；模式可切 | 同一电流下读数稳定度 < 1 LSB |
| HR-808 | 内部比较器 + DAC 阈值 | COMP2_INP 与 VBUS_ADC **同脚 PA3**，不需额外分压；DAC 作可调阈值 ⇒ 过压/欠压实验直接改固件数值；COMP2_OUT 引到空闲脚 **PB9** 供 LA 抓取。外部 TLV7021 保留为不依赖固件的硬保护 | 改 DAC 值即改翻转点；COMP→TIM1_BKIN 内部互连按 RM0440 §21 核对，不可用则仅作可观测 |
| HR-809 | PVD 掉电检测 | 3V3 跌破阈值 → 立即 EN 拉低 + 锁存状态（**不**借此保存参数，见 HR-407）| 缓慢降压，LA 抓 EN 时序 |
| HR-810 | VREFINT 监测 | 每遥测帧带 VREFINT 原始码，固件反算 VDDA；绝对刻度按实测 VDDA 换算 | 与 DMM 比 3.3 V，误差 < 0.5 % |
| HR-811 | ADC 自校准 | 上电执行 ADCAL（含单端偏移校准），结果进测试记录 | 三路零位差 < 3 LSB |
| HR-812 | DWT 计时 | ISR 入口/出口 CYCCNT 周期数写入遥测帧 | 与 LA 脉宽互为校验，误差 < 5 % |
| HR-813 | 触发相位可编程 | ADC 触发时刻以 ≤6 ns 步进可调（TIM8 计数器 @170 MHz = 5.9 ns/计数，或 HRTIM 184 ps），用于扫窗口与偏斜 | 扫相位时采样点沿 SOx 波形平移 |
| HR-814 | 黑匣子放 SRAM2 | 故障前 512–4096 样本 + TIM1/ADC 关键寄存器快照放 SRAM2（Standby 保持），**复位**（非掉电）后仍可读出；启动不清该段，需 magic word | 触发故障 → 复位 → dump 完整 |
| HR-815 | DFU 恢复 | BOOT0（PB8）跳线 + 系统 bootloader（USBD-DFU）：时钟配错或固件跑飞时能恢复，不必飞线 | 擦除固件后 10 min 内恢复 |
| HR-816 | 双调试通道 | RTT（defmt，走 SWD，不占引脚）+ USB CDC 命令行；两者都不得进 20 kHz 环 | 打开 RTT 时 STROBE2 脉宽不变 |

> HR-801/802 的取舍：没有 CORDIC/FMAC 也能跑，但 170 MHz 下 25 µs 环预算与 10 ms 标定任务会同时吃紧。
> 先按"有它们"设计，再用 DWT 证明余量；证明不了就降 PWM 频率，而不是砍标定。

## 3. 引脚分配（LQFP64，最终版）

`沿用` = 与 PCB14 同脚同功能；`改` = 与 PCB14 不同（原因写在第 5 列）；`新增` = PCB14 上不存在。
V2 与 PCB14 的差别集中在模拟前端（为片内 OPAMP 让路）、6 路 PWM、CAN/UART/BKIN 的挪位——
**"同封装可直接换料"只对"能不能跑起来"成立，不代表 V2 能沿用 PCB14 的走线。**

| 功能 | 引脚 | Pin | PCB14 | G474RET6 上确认 | V2 动作 |
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
| DAC 遥测 | PA4 / PA5 | 18 / 19 | 空闲 | DAC1_OUT1/2 | 新增 TP17/TP18（HR-803）|
| COMP2_OUT | PA2 | 14 | 空闲 | COMP2_OUT | 新增 TP（HR-808）|
| 状态 LED | PB2 | 26 | 空闲 | GPIO | 新增（HR-505）|
| 母线分流（选装）| PA0 | 12 | 空闲 | ADC1_IN1/ADC2_IN1 | 预留（HR-210）|
| 保持空置 | PA6、PB1 | 20 / 25 | — | OPAMP2_VOUT / OPAMP3_VOUT | 便于日后关掉 OPAMP 直接接 ADC |
| 其他空闲 | PA15, PB11, PC12, PC13, PC14, PC15, PC1, PC2, PC3, PD2 | 51,33,54,2,3,4,9,10,11,55 | 空闲 | — | 留焊盘（HR-606）|

**核对结论**：PCB14 在用的 29 个 MCU 信号（不含 NRST 与只有网络标签的 PC1），在 `STM32G474R(B-C-E)Tx.xml` 上逐脚比对无功能丢失，
所以"换料能跑"成立；但 V2 的功能分配另有 6 处改动（模拟前端、6 路 PWM、CAN、UART、BKIN、ADC4），
**PCB14 的走线不能直接沿用**。上表是交给外包的唯一版本。

**引脚层面的两个附带发现**（来自网表，不是原理图）：
- PC6/PC7/PC8 在网表里没有任何网络——architecture §9 写的"预留 strobe"在 PCB14 上并不存在。
- PC1 只有一个孤立网络名 `ADC12_IN7`，没有其它连接，属未完成设计。

时钟树：HSE 16 M → PLL M=4/N=85/P=2 ⇒ 170 MHz（boost）；ADC 42.5 MHz；
USB 48 MHz 由 HSI48+CRS；CAN FD 位定时由 HSE 派生。

## 4. 仪器接线方案

### 4.1 示波器（双通道）四种配置

| 目的 | CH1 | CH2 | 判据 |
|---|---|---|---|
| 采样窗口与前端建立 | TP5（SOx_RAW）| STROBE1（J_DBG）| SOx 在 STROBE1 上升沿前 ≥ 0.5 µs 已稳定；窗口内三相低边共导 |
| 传递特性与饱和 | HR-206 分流器开尔文电压（20 mV/div）| TP8（ADC 侧）| ±5 A 内线性，偏差 < 1 %；超量程看削顶位置 |
| 故障时序 | EN_A（J_DBG）| BKIN 或 nFAULT（J_DBG）| BKIN 有效到 EN 变低 ≤ 2 µs；PWM 同周期无效 |
| 模拟遥测（无 LA 时）| TP17（DAC 输出 i_q）| TP5（SOx_RAW）| DAC 与 SOx 同相；STROBE1 处采样值与 DAC 同拍一致 |

### 4.2 逻辑分析仪（16 通道，≥100 MS/s）

| CH | 信号 | CH | 信号 |
|---|---|---|---|
| 0 | PWM_A | 8 | STROBE1（ADC 触发）|
| 1 | PWM_B | 9 | STROBE2（ISR 窗口）|
| 2 | PWM_C | 10 | STROBE3（标定更新）|
| 3 | EN_A | 11 | CAN_TX |
| 4 | EN_B | 12 | CAN_RX |
| 5 | EN_C | 13 | UART_TX |
| 6 | nFAULT | 14 | COMP2_OUT（过压内部比较器）|
| 7 | BKIN | 15 | GND |

判据：STROBE1 与 STROBE2 上升沿间隔 = 0；STROBE2 脉宽 ≤ 25 µs 且周期抖动 ≤ 1 µs；
EN 在 BKIN 后 ≤ 2 µs 变低；上电时 nSLEEP/EN/PWM 顺序符合 HR-404。

## 5. 测试点与注入网络

| TP | 网络 | 用途 |
|---|---|---|
| TP1 / TP2 | VBUS（电容后）/ PGND | 母线压降、开关振铃；TP2 是功率地参考 |
| TP3 / TP4 | +5 V / +3.3 V | 逻辑轨；**TP4 同时是 VREF 与偏置源，绝对刻度的依据** |
| TP5/6/7 | SOx_RAW（A/B/C）| 芯片输出，判断补偿/饱和 |
| TP8/9/10 | SOx_ADC（A/B/C，100 Ω 之后）| 判断前端网络与 ADC 输入 |
| TP11 | PH_A | 开关节点与 SOx 恢复时间的关系 |
| TP12 | VREF_2V5 | OVP 比较基准 |
| TP13 | OVP_DIV | 过压阈值实测点 |
| TP14 | NTC | 温度读数核对 |
| TP15 | VBUS_DIV | 与 DMM 比对换算系数 |
| TP16 | J_SYNC 注入节点 | HR-201/504 同步实测；可由 TP17 的 DAC 直接驱动 |
| TP17 | DAC1_OUT1（PA4）| 模拟遥测：i_a / i_q / 残差 / 标定状态 |
| TP18 | COMP2_OUT（PB9）| 内部过压比较器输出，LA 可抓 |

注入与替身网络：`JP_GAIN_A/B/C`（三档增益，HR-204）、`JP_OFF_A/B/C`（双向偏置，HR-205）、
`R_SHUNT_A/B/C`（10 mΩ + 旁路，HR-206）、`J_SYNC`（HR-504）。

## 6. 状态输出约定

LED（PB2）编码：灭 = 未使能；1 Hz 闪 = 待机；常亮 = 运行；5 Hz 闪 = 标定中；双闪 = 已冻结；
快闪 N 次后停顿 = 故障码 N。

固件把同一状态写成 32 bit 状态字（随每个 CAN 反馈帧和 USB 遥测帧发出）：

| 位 | 含义 |
|---|---|
| 0–3 | 模式：0 自检 / 1 待机 / 2 标定 / 3 运行 / 4 台架 / 5 故障 |
| 4–6 | 标定状态：Idle / Collecting / Converged / Frozen / LatchedFault |
| 7 | 采样有效（窗口内且三路对齐）|
| 8 | 采样窗口判据通过 |
| 9 | 激励充分 |
| 10 | 跨工况一致 |
| 11–14 | 编码器状态（E1/E2 各自 OK/异常）|
| 16–23 | 故障码 |
| 24–31 | 帧序号低 8 位（LA 与帧对时用）|

LED 只能看粗状态，**判据一律以状态字为准**；示波器/LA 用 STROBE + 帧序号对齐到同一个 50 µs 周期。

状态字之外，每帧还必须带三个标量（否则事后无法区分算法错与基准漂）：
`VREFINT 原始码`（HR-810，反算 VDDA）、`ISR 周期数`（HR-812）、`样本时间戳`（TIM2 32 bit，1 µs 分辨率）。

## 7. 需求 → 毕设实验

| 实验 | 依赖 | 仪器 | 通过判据 |
|---|---|---|---|
| E1 电流链静态标定（3 相 × ±7 点）| HR-203/206/207 | 电源 + 负载 + 10 mΩ + DMM | 线性度 R² > 0.999，重复性 < 1 LSB |
| E2 ADC 同步性 | HR-201/504/804 | J_SYNC（DAC 或信号源）| 偏斜 ≤ 50 ns 或给出固定值 |
| E3 采样窗口与建立 | HR-202/305/501/813 | 示波器 | d = 90 % 时窗口 ≥ 3 µs |
| E4 相对增益校准（普通 RLS vs 可信闸门）| HR-204 | 跳线 | 普通版把 −25 % 学成新参数；闸门版冻结 |
| E5 偏置注入响应 | HR-205 | 跳线 | 读数变化 0.80 A ±5 % |
| E6 故障时序（OVP / nFAULT）| HR-401/402/403/808 | LA + 可调电源 | 见 §4.2 |
| E7 CAN FD + USB 吞吐 | HR-106/107 | 上位机 + LA | 1 kHz 遥测无丢帧，trace 突发 ≥ 1 MB/s |
| E8 持续电流与温升 | HR-102/405 | 电子负载 + NTC | 5 A 10 min 不降额 |
| E9 三路 ADC 自检（无外部仪器）| HR-804/807/811 | 仅板子 + DAC 跳线 | 三路同看 DAC 斜坡偏差 ≤ 2 LSB |
| E10 故障现场保留 | HR-814/815 | 复位 + 上位机 | 复位后黑匣子样本完整 |

## 8. 待你确认的输入（会改数值，不改结构）

1. **母线电压**：24 V（沿用 PCB14：OVP 28.1 V、VBUS 满量程 44.2 V）还是 48 V
   （需改 R17/R19 → 55 V 翻转、R22/R24 → 60 V 满量程、电容换 63 V 档；48 V 已接近 MP6540HA 的 50 V 上限）。
2. **关节连续相电流**：5 A 还是 8 A（影响 MP6540HA 降额余量、铜宽、分流器功率档）。
3. **板尺寸与安装孔位**：需要 YHorizon-JM 结构图纸；在此之前 HR-604 只是暂定值。
4. **CAN 是否隔离**：非隔离（现方案）在 5 A 开关噪声下能否接受，取决于台架布线。
5. **是否装 HR-210 母线分流**：装了，架构 §10 的"是否需要第二观测通道"就从论证题变成实验题。
6. **VREF+ 取 3V3 还是内部 VREFBUF（2.5 V）**：取 VREFBUF 可去掉 LDO 的 ±1 % 对绝对刻度的支配，
   但偏置分压必须改从 VREF+ 取（2k/2k → 1.25 V，正向量程降到 ±11.5 A），且需核 VREFBUF 带载能力
   （2k/2k 要 1.25 mA）。**这条会改电阻值，必须在投板前定。**
7. **内部 COMP 是否承担过压判定**：若 RM0440 确认 COMP 输出可直连 TIM1_BKIN，外部 TLV7021 可删；
   但上电到固件配置完成之间无保护，建议保留外部器件作为默认硬保护（HR-808）。
8. **论文题目是否随主控更新**：现题目写的是 STM32G491，与 G474RET6 不一致；这是教务层面的事，需先确认。

## 9. 投板前检查清单

- [ ] §8 的 1–3 已给数值
- [ ] 三路 ADC 在三个不同 ADC 单元，触发源与采样时间已按 RM0440 核对
- [ ] VREF+ / VDDA / SOx 偏置同源，TP4 可达
- [ ] JP_GAIN、JP_OFF、R_SHUNT、J_SYNC 四类网络都在原理图上
- [ ] PC6/PC7/PC8 真的接到排针（PCB14 只是"写了预留"）
- [ ] PA4 同时引到 TP17 与 J_SYNC（跳线可断）
- [ ] PB9(COMP2_OUT)、PB2(LED) 引出
- [ ] BKIN 延迟与 EN 顺序的固件责任已标注在原理图（HR-403）
- [ ] nSLEEP 下拉、BOOT0 下拉 + 跳线、NRST 上拉与按键
- [ ] 上电默认态：nSLEEP=0 / EN=0 / PWM 无输出
- [ ] 4 层叠层与地分割；模拟前端地平面连续
- [ ] 相线宽度与过孔数满足 5 A；三相端接可插拔
- [ ] 注入组（电流）/规则组（慢通道）分配已在 CubeMX 定死
- [ ] AWD 阈值通道、PVD 阈值已定
- [ ] SRAM2 划出黑匣子段，启动代码不清该段
- [ ] CORDIC/FMAC 时钟与 DBGMCU 的 DWT 保持开启
- [ ] 丝印含 TP/J 编号、版本号、LED 编码简表
- [ ] Flash 预算按 release 复核（debug 64,632 B / release 16,080 B，模板实测，512 K 的 12 %/3 %）
