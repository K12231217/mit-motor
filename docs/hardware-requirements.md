# 硬件需求规格（V2 板，STM32G474RBT6 + MP6540HA）

> 状态：草稿 v0.4（2026-11-15）。主控 **STM32G474RBT6**（LQFP64，128 K Flash / 128 K SRAM），
> 驱动 **MP6540HA**（订货号 `MP6540HGU-A-Z`，HS/LS 六输入，见 [hardware-facts.md](archive/hardware-facts.md) §2.2）。
> 与 PCB14 的关系：同封装、引脚功能逐脚核对兼容（§1），**但毕设必需的三类设施在 PCB14 上不存在**（§1.3）。
> 相关：[hardware-facts.md](archive/hardware-facts.md) 硬件事实、[software-architecture.md](software-architecture.md) 固件结构；[research-plan-v1.1-2026-11-04.md](archive/research-plan-v1.1-2026-11-04.md) 研究方案；
> 外包任务书的对抗审核与错误清单见 [taskbook-review.md](archive/taskbook-review.md)。

## 0. 这份文档怎么用

V2 板不是为了换 MCU。换 MCU 是换料级改动；V2 板要做的是**补齐三个实验的物理条件**：
物理增益注入没有位置、ADC 同步性无法实测、台架标定缺电流注入点。

每条需求都带验收方法。没有验收方法的需求不写——毕设的可信性来自可测，不是来自设计意图。

## 1. 主控更换的实际影响（已逐脚核对）

| 项 | PCB14 / G491RET6 | V2 / G474RBT6 | 后果 |
|---|---|---|---|
| 封装 | LQFP64 | **LQFP64** | 同封装，PCB14 可**直接换料**，不必重排信号 |
| 引脚功能 | G491RET6 | G474RBT6 | G491 在用的 29 个信号功能**全部存在**（功能集合无丢失）|
| Flash | 512 K | **128 K** | 少 384 K；debug 构型吃 49 %，见 §1.1 的三条对冲手段 |
| SRAM | 112 K | **128 K**（SRAM1 80 + SRAM2 16 + CCM 32，DS12288 §3.5）| CCM 在 `0x1000_0000` 只有 CPU 可达；同一块内存在 `0x2001_8000` 有 SBUS 别名，DMA1/2 可达 |
| 外设 | 3 ADC | 5 ADC / 7 COMP / 4 DAC / 6 OP-AMP / HRTIM / CORDIC / FMAC | §2.H 的调试设施因此可以白拿 |

核对依据：CubeMX 器件库 `db/mcu/STM32G474R(B-C-E)Tx.xml`、`STM32G491R(C-E)Tx.xml` 两个器件文件
按脚位比对，以及 PCB14 网表 `hardware/pcb/Netlist_PCB14_2026-11-11.tel`（**在 `hw/pcb14` 分支上，本分支没有这个文件**）；`software/firmware/` 已切到 `stm32g474rb`，`cargo build` / `cargo build --release` 在该特性下均已通过（2026-11-15，rustc 1.93.0-nightly gnu；ELF 68,732 B debug / 17,184 B release，镜像 69,320 / 17,768 B，比 1.99.1 的旧数据大约 6 %，来自工具链差异，见 `software/firmware/README.md`）。

**两条路径，建议都走**：
- 路径 A（1 天）：现有 PCB14 换焊 G474RBT6，验证固件能否跑起来；
  代价是没有 §1.3 的任何设施，只能测"能不能跑"。
- 路径 B（1–2 周）：打 V2 板，把 §1.3 的缺口一次补齐。**只有路径 B 能产出论文的实验数据。**

### 1.1 Flash 预算（已实测；RBT6 是最终料号，约束写在这里）

烧进 flash 的镜像大小（`llvm-objcopy -O binary`，不是 ELF 大小）：

| 构建 | flash 镜像 | 占 128 K |
|---|---|---|
| debug（仅模板 bin + defmt）| **65,216 B** | 50 % |
| release（同一模板）| **16,664 B** | 13 % |

独立复测（rustc 1.93.0-nightly gnu，`stm32g474rb` 特性）：**69,320 B / 17,768 B**（53 % / 14 %）。
两次差 ~6 %，来自工具链版本，不是特性差异；两个构型都在 128 K 内链接成功。

**128 K 够不够，取决于构型，不取决于代码量：**

- 全量应用（FOC + 标定状态机 + FDCAN + USB CDC + 遥测 + 编码器）release 估算 40–50 KB，
  其中**自己写的算法只占 10–15 KB**，其余是 embassy 外设层与 USB 协议栈 ⇒ 128 K 在 release 下够用（约 35 %）。
- 但本模板的 **debug/release = 3.9×**（65,216 / 16,664，未优化 + 未内联 + defmt 格式化代码），
  同一套全量应用在 debug 构型就是 110–175 KB ⇒ **128 K 在 debug 构型下不够**。
- 而 bring-up 与硬件在环实验恰恰是"flash debug 构型、看 RTT"的阶段。

**最终料号：`STM32G474RBT6`（128 K）**（2026-11-15 定稿）。因此上面那条不是"选型理由"，而是**工程约束**，
必须在固件侧对冲，三条都可测量：
1. **HIL 与出数一律 release 构型**：16,664 B + 全量应用 40–50 KB，余量 > 50 %。
2. **必须看 RTT 时用带优化的 debug 构型**：`[profile.dev] opt-level = 2`（或只对依赖开优化）把 3.9× 压下来，
   压不下来就先降 PWM 频率/砍遥测，而不是砍标定。
3. **每次镜像增长都复核**：`llvm-objcopy -O binary` 后超过 100 KB 即在 §9 清单置红，不允许"先塞进去再说"。

RC（256 K）/RE（512 K）与 RBT6 同封装同脚位、只差 Flash 密度：这是零改动板的升级路径，
升级只需改 `software/firmware/memory.x` 的 FLASH 长度与 `stm32g474rb` → `stm32g474rc`/`stm32g474re`。

### 1.2 三个必须在投板前确认的技术风险

1. **ADC1/2（ADC12 组）与 ADC3（ADC345 组）跨组是否同拍启动**。毕设"三 ADC 无采样偏斜"的立论依赖它，
   而 G4 的两个组有独立的触发选择寄存器与时钟源（按 RM0440 ADC 触发表逐条核对）。
   本设计不假设为零：HR-201 要求**实测偏斜并写入文档**，HR-504/804 提供两种实测手段。
   若偏斜不可忽略（> 50 ns），论文表述改为"偏斜固定且可标定"，并给出补偿量与补偿后残差。
2. **USB FS 时钟**：170 MHz 下 VCO = 340 MHz，PLLQ 只能取 /2/4/6/8（42.5/56.7），得不到 48 MHz。
   V2 用 HSI48 + CRS 供 USB；HSE 16 MHz 只服务 CAN FD 位定时。这是时钟树的前提，不是优化项。
3. **RAM 的分块**：128 K 总量 = SRAM1 80 K + SRAM2 16 K + CCM 32 K（DS12288 §3.5）。CCM 在
   `0x1000_0000` 只经 I/D-code 总线供 CPU 使用，DMA 不可达；但同一块内存在 `0x2001_8000` 有
   SBUS 别名，DMA1/2 可达。ADC/SPI 缓冲放 SRAM1/SRAM2（零等待且无别名歧义），黑匣子（HR-814）
   放 SRAM2；`stm32-metapac` 对 G4 SRAM 的数字是错的（`g474rb/re` 给 96 K），所以 `memory.x`
   必须手写（已做）。

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
| HR-108 | 编码器接口 | 2× MT6701 共用 SPI1（SCK/MISO）+ 2 个独立 CS（**两个 CS 各 10 kΩ 上拉到 3V3**：软件片选下，复位后到 GPIO 配置完成之间 CS 浮空，两片可能同时被选通而争用 MISO，与 PWM 六路缺下拉同类问题），4 pin 排线 ×2，MISO 上拉 | 读角 1 h 无错帧 |
| HR-109 | 相线端接 | 三相各一个 2 pin 端子（缺口 11），配合 HR-206 | 插拔 50 次接触电阻 < 5 mΩ |

### B. 电流采样链（HR-2xx，毕设核心）

单通道拓扑（三相完全同构）：

```svg
<svg viewBox="0 0 760 300" xmlns="http://www.w3.org/2000/svg" font-family="ui-sans-serif,system-ui,sans-serif" font-size="12">
  <line x1="180" y1="48" x2="400" y2="48" stroke="var(--muted)" stroke-width="1.2"/>
  <text x="404" y="52" fill="var(--muted)">TL431 2.5 V（VREF+）</text>
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
| HR-203 | 偏置与标度 | **4.99 kΩ/4.99 kΩ（±0.1 %，25 ppm）** → 静态 VREF/2、戴维南 2.495 kΩ、0.2712 V/A；12 bit → **2.25 mA/LSB**；±4.6 A 摆幅 ±1.247 V，电气满量程 ±4.61 A | 台架 ±7 点 × 双向（E1）|
| HR-204 | **增益注入网络**（每相）| 默认 4.99k/4.99k；**上下各并联**一颗（串联会把 Rth 抬高 10 倍，见 [taskbook-review](archive/taskbook-review.md) §2）：并 47 k ⇒ **−9.5 %**，并 1.5 k ⇒ −25 %，换 6.8 k ⇒ +35 %；等值成对 ⇒ 零位保持 VREF/2 | 切换后零位漂移 < 5 mV（E4）|
| HR-205 | 偏置注入网络（每相）| 18 kΩ→+3V3 ⇒ +0.74 A，**但增益同时 −12 %**（18 k 拉低戴维南）；100 kΩ ⇒ +0.15 A / −2.4 %；"纯偏置"应改单个分压电阻的贴装值而非并联注入 | 静态读数变化与预期差 < 5 %，增益耦合量实测记录（E5）|
| HR-206 | 串联分流位置（每相）| 2512 **10 mΩ / 1 W** + 四线开尔文焊盘 + 0 Ω 旁路跳线；短接时相线直通 | 5 A 下 DMM 200 mV 档，误差 < 0.5 % |
| HR-207 | 基准同源 | VREF+ 接 TL431 2.5 V（U10），VDDA 仍为 3V3 LDO；SOx 偏置取自 VREF+；不装外部基准芯片 | TP4 实测 2.49 V ±1 % |
| HR-208 | 通道对称性 | 三路之间不允许任何单侧滤波、TVS 或不同封装；如需滤波必须三路同值 | 目视 + BOM 比对 |
| HR-209 | SOx 节点可注入 | 保留裸 SOx 焊盘 + 100 Ω 隔离，允许外部源经串阻注入而不拆电阻 | 注入 ±1 mA 时读数线性 |
| HR-210 | **（决策项）母线分流** | 1 mΩ + 差分放大 → PA0(ADC1_IN1)，默认不装；这是架构 §10 未决"是否需要第二观测通道"唯一的硬件入口 | 装机后与三路和电流对比 |
| HR-211 | 外部电流真值通道 | **PB12(ADC4_IN3)** + 3 pin 接口（只出 `EXT_I_REF` 与 GND，**不要把 VREF+ 引到连接器**，见 [taskbook-review](archive/taskbook-review.md) P1-5）；100 Ω 串阻 + 钳位；与三路同帧采样，用来消掉跨仪器对时误差 | 与 DMM/INA240 读数在同一帧内对比，偏差 < 1 % |

> HR-203 的边界：±4.61 A 时 SOx 节点贴到 0 V/2.5 V 轨，**工作区按 ±4.6 A 设计**；这是 VREF+=2.5 V 的电气边界。
> 这一条必须写进论文，否则"2.25 mA/LSB"会被误读成全量程可用。

### C. 时钟与时序（HR-3xx）

| 编号 | 需求 | 指标 | 验收 |
|---|---|---|---|
| HR-301 | HSE | 16 MHz 晶振（CAN FD 位定时；HSI16 ±1 % 不够）| 频率实测 ±50 ppm |
| HR-302 | 主频与 PWM | SYSCLK 170 MHz（boost）；TIM1 中心对齐 20 kHz，ARR = 4250；**MP6540HA 是 HSx/LSx 六输入、无内部死区，死区必须由 TIM1 BDTR 产生（建议 200–500 ns，按手册 t_d 定）**；**`TIM1_BDTR.AOE = 0`**。2026-11-27 网表无 TIM1_BKIN，硬件 break 缺口；当前 `BKE=0` | 周期 50 µs；死区用 LA 实测；AOE/BKE 读回为 0 |
| HR-303 | ADC 时钟 | 42.5 MHz（PLLP 170/4）；三个 ADC 同源同频（不同源则 HR-201 不成立）| 寄存器读回 + 时序实测 |
| HR-304 | USB 时钟 | **最新网表无 USB**（PA11/PA12 为 CAN）；本条仅保留历史 | — |
| HR-305 | 采样窗口 | 全低边共导窗口 ≥ 3 µs（ADC 2.5 µs + 余量）⇒ 占空比上限 d ≤ 90 %，由固件夹紧 | 示波器测窗口（E3）|

### D. 保护与故障（HR-4xx）

| 编号 | 需求 | 指标 | 验收 |
|---|---|---|---|
| HR-401 | 母线过压 | **2026-11-27 网表未把比较器接到 TIM1_BKIN**；当前 `BKE=0`，硬件封锁缺口。软件 OVP 阈值待定 | 补硬件或软件停机后再测（E6）|
| HR-402 | 驱动故障 | nFAULT 开漏 + 10 kΩ 上拉，**PC8**（GPIO/EXTI，软件读故障）；最新网表未接 TIM1_BKIN。MP6540 的 nFAULT 自清行为按手册确认 | 拉低软件锁存并关 MOE |
| HR-403 | 低边通道 | LS_A/B/C = TIM1_CH1N/2N/3N（`PB13/PB14/PB15`，与 HS 互补）；**没有独立 EN 引脚**，相高阻 = 上下输入全低 | LA 测六路时序与死区 |
| HR-404 | 上电默认态 | nSLEEP（**PC7**）10 kΩ 下拉、六路输入全低、PWM 无输出；只有固件显式释放 | 冷上电用 LA 抓时序 |
| HR-405 | 温度 | NTC 10 kΩ 分压（PC0），贴在 MP6540HA 与母线电容之间 | 两点校准 |
| HR-406 | 母线电压检测 | **PB11**，分压比按最新网表；满量程/每 LSB 待按阻值复核 | 与 DMM 比对 5 点 |
| HR-407 | 参数保存 | 不需要掉电保存（每次上电自校准）；不设 EEPROM | — |
| HR-408 | 停车语义 | 六路全无效 = 上下管全关 = **自由停车**（不是刹车）；要刹车需主动拉高三相 LS。最新网表无硬件 break，故障动作由软件锁存 + `disable` | 堵转下触发 OVP/nFAULT，确认自由滑行 |

### E. 外部调试设施（HR-5xx）

| 编号 | 需求 | 指标 | 验收 |
|---|---|---|---|
| HR-501 | 三个 strobe | **最新网表未引出**；PC7/PC8 已改 nSLEEP/nFAULT。本条仅保留历史 | — |
| HR-502 | 调试排针 J_DBG | 2×9：PWM_A/B/C、EN_A/B/C、nFAULT、BKIN、STROBE1/2/3、CAN_TX/RX、UART_TX、VBUS_DIV、GND×2 | 逐针通断 |
| HR-503 | 模拟测试点 | 见 §5 表；丝印标网络名 + TP 编号 | 逐点通断/电压 |
| HR-504 | J_SYNC | 三路 ADC 输入经 3 位跳线可共接到同一注入节点（外部信号源或 TP17 的 DAC 驱动）| 方波能同时出现在三路 |
| HR-505 | 状态指示 | **最新网表无 MCU LED**（只有电源指示 LED1）；状态以 RTT/帧为准 | — |
| HR-506 | SWD | PA13/PA14 + NRST(7) + 3V3 + GND，5 pin 2.54 或 10 pin 1.27；PB3 = SPI1_SCK 故 **SWO 不可用**，调试输出走 RTT | probe-rs 烧写 + RTT |
| HR-507 | 调试日志 | **RTT only**；PB6/PB7 的 USART3 网络本季不启用，改作 TP_SYNC 推挽输出（见 HR-510） | RTT 可读；PB6/PB7 静态低 |
| HR-510 | TP_SYNC | **PB6 = TP_SYNC_LOOP**（20 kHz ISR 内高，退出低）；**PB7 = TP_SYNC_FREEZE**（冻结期间高，与 DAC1_OUT2 同相）。3.3 V 推挽，禁止再分配给 USART3 | 示波器/LA 上升沿对齐 DAC；20 kHz 周期 50 µs |

### F. 结构与工艺（HR-6xx）

| 编号 | 需求 | 指标 | 验收 |
|---|---|---|---|
| HR-601 | 叠层 | 4 层：信号 / 地 / 电源 / 信号；模拟前端下方完整地平面；功率地与模拟地单点连接 | 叠层图评审 |
| HR-602 | 可手换 | 关键电阻 0603；功率器件与分流器 2512 | BOM 核对 |
| HR-603 | 载流 | 5 A 连续下相线温升 < 20 K（1 oz 至少 3 mm 宽，或改 2 oz）| 5 A 10 min 红外/热电偶 |
| HR-604 | 机械 | 尺寸/孔位由 YHorizon-JM 关节结构决定（**待图纸**），暂定 ≤ Ø45 mm、M2.5 ×4 | 与结构件试装 |
| HR-605 | 标识 | 丝印：版本号、日期、TP/J 编号、LED 编码简表 | 目视 |
| HR-606 | 改板余量 | 其余空闲脚留焊盘，其中预留 3 个焊盘（PA0、PC1、PA15）+ 1 组备用 UART | 原理图评审 |

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
| HR-803 | DAC 模拟遥测 | **PA4（DAC1_OUT1）** 输出残差 r_k（0 A=2048，±0.5 A=0/4095）；**PA5（DAC1_OUT2）** 数字指示：正常 0，校准冻结 4095。1 µs 内可更新，TP1/TP2 | 示波器 CH2 上升沿单次触发；DAC 码与 RTT 回读一致 |
| HR-804 | DAC → J_SYNC 自检 | PA4 经跳线驱动 J_SYNC 共点：三路 ADC 看同一 DAC 斜坡，**不需要外部信号源**即可测同步、线性与偏斜 | 三路读同一斜坡偏差 ≤ 2 LSB；扫相位得偏斜 |
| HR-805 | 注入组 / 规则组分工 | 三路电流走 **ADC1/2/3 注入组**，由 **TIM1_TRGO2 同步触发**；单个 TIM1_UP ISR 内等待三路 JEOS，一次性读三路 JDR1 并打同一 `RawSample3.tick`。VBUS/NTC/VREFINT 走**规则组 + DMA 慢扫**。T-013 已改为注入组/TIM1_TRGO2；低边窗口由 MOE+CCNE 判定，按互补 PWM 默认三相同时 | 20 kHz 下电流点不丢；三路 `tick` 相同；慢通道 1 ms 更新 |
| HR-806 | ADC 模拟看门狗 | AWD1/2/3 设过流与采样越限阈值，超限直接进硬件中断（不依赖软件残差）| 注入超限电流，中断命中且动作在下一周期内 |
| HR-807 | 硬件过采样 | 台架模式 4–16× 过采样（与 DMM 对齐）；控制环单次采样；模式可切 | 同一电流下读数稳定度 < 1 LSB |
| HR-808 | 内部比较器 + DAC 阈值 | COMP2_INP 与 VBUS_ADC **同脚 PA3**，不需额外分压；DAC 作可调阈值 ⇒ 过压/欠压实验直接改固件数值；COMP2_OUT 引到空闲脚 **PA2** 供 LA 抓取（`PA2` 同时是 `COMP2_INM`，本设计用内部 DAC 作阈值，不占用该输入）。外部 TLV7021 保留为不依赖固件的硬保护 | 改 DAC 值即改翻转点；COMP→TIM1_BKIN 内部互连按 RM0440 §21 核对，不可用则仅作可观测 |
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

## 3. 引脚分配

引脚唯一真源是 §3.1 的**一张表**。两处一手材料，缺一不可：

- `hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel`（U1 = G474RBT6 LQFP64）：网络名与 U1 脚号；
- `docs/DS_stm32g474rb/Pinouts_and_pin_description.md` Table 12（LQFP64 列）：脚号 → PXn 与复用/附加功能。

V2 需求表（与 PCB14 的逐脚差异、曾交给外包的版本）已移入
[`archive/pin-map-v2.md`](archive/pin-map-v2.md)；它与本表冲突的行一律作废，且不再维护。

### 3.1 引脚表（Schematic14_1 网表 + DS Table 12，2026-11-28 复核）

| 功能 | 引脚 | U1 脚 | 网表网络 | DS Table 12 复用/附加功能 |
|---|---|---|---|---|
| PWM_HS A/B/C | PA8 / PA9 / PA10 | 42/43/44 | `PWM_AH/BH/CH` | TIM1_CH1/2/3；PA8 另有 ADC5_IN1、OPAMP5_VOUT，PA9 ADC5_IN2，PA10 PVD_IN |
| PWM_LS A/B/C | PB13 / PB14 / PB15 | 35/36/37 | `PWM_AL/BL/CL` | TIM1_CH1N/2N/3N；PB13 ADC3_IN5，PB14 ADC4_IN4/ADC1_IN5，PB15 ADC4_IN5/ADC2_IN15 |
| nSLEEP | PC7 | 39 | `NSLEEP` | 无模拟复用；输出高有效（`.ioc` 写 PC8 的是旧版） |
| nFAULT | PC8 | 40 | `NFAULT` | 无模拟复用；开漏低有效（`.ioc` 写 PC7 的是旧版） |
| SOC 采样（OPA1） | PA3 | 17 | `OPA1` | ADC1_IN4、COMP2_INP、OPAMP1_VINM/VINP、OPAMP5_VINM |
| SOB 采样（OPA2） | PA7 | 21 | `OPA2` | ADC2_IN4、COMP2_INP、OPAMP1_VINP、OPAMP2_VINP |
| SOA 采样（OPA3） | PB0 | 24 | `OPA3` | ADC3_IN12/ADC1_IN15、COMP4_INP、OPAMP2_VINP、OPAMP3_VINP |
| VBUS_ADC | PB11 | 33 | `VBUS_ADC` | ADC12_IN14、COMP6_INP、OPAMP4_VINP |
| DAC1_OUT1 / OUT2 | PA4 / PA5 | 18/19 | `DAC1_OUT1`→TP1、`DAC1_OUT2`→TP2 | DAC1_OUT1/2；PA4 另有 ADC2_IN17，PA5 ADC2_IN13 |
| SPI1_SCK / MISO | PB3 / PB4 | 56/57 | `SPI1_SCK` / `SPI1_MISO` | SPI1_SCK / SPI1_MISO；两片 MT6701 共用 MISO，不要 MOSI |
| ENC1_CS / ENC2_CS | PC12 / PC10 | 54/52 | `ENC1_CS` / `ENC2_CS` | 纯 GPIO |
| FDCAN1_RX / TX | PA11 / PA12 | 45/46 | `CAN_RX` / `CAN_TX` | FDCAN1_RX / FDCAN1_TX；同脚还有 USB_DM/DP，本板无 USB 连接器 |
| USART3_TX / RX | PB6 / PB7 | 59/60 | `USART3_TX`→CN1.1、`USART3_RX`→CN1.2 | 本季不启用 USART3，配推挽：PB6=TP_SYNC_LOOP、PB7=TP_SYNC_FREEZE（HR-510） |
| BOOT0 | PB8 | 61 | `BOOT0` | 10 kΩ 下拉 + 跳线 |
| SWDIO / SWCLK | PA13 / PA14 | 49/50 | `SWDIO` / `SWCLK` | SWD；复位按键 SW1 接 NRST（脚 7，PG10-NRST） |
| HSE | PF0 / PF1 | 5/6 | `OSC_IN` / `OSC_OUT` | 16 MHz 晶振 X1 |
| VREF+ | VREF+ | 28 | `VREF+` | 供 OPAMP 偏置与 ADC 参考 |

**网表里没有连接**（固件不得假设其存在）：TIM1_BKIN、STROBE1/2/3、EXT_I_REF（ADC4）、
COMP2_OUT、母线分流（PA0）、MCU 状态 LED、USB 连接器、OPAMP4/5/6 引出。

**待确认（网表与 DS 冲突，未裁决）**：网络 `ADC12_IN6` 只连 U1.11 一个节点，没有分压或热敏电阻；
且 LQFP64 脚 11 是 PC3，其附加功能为 `ADC12_IN9`/`OPAMP5_VINP`，没有 `ADC12_IN6`。
也就是说本版网表上 NTC 既未接、通道名也对不上，需原理图/实物确认后才能写进需求。

## 4. HIL 探针（2026-11-27 网表）

### 4.1 T-012：clock + PWM 安全关断态

| 探针 | 位置 | 期望 |
|---|---|---|
| +3V3 / GND | 板级 3V3/GND | 3.30 V |
| HSE（可选） | PF0 / PF1（5/6） | 16 MHz |
| HS A/B/C | PA8/9/10（42/43/44）→ U9.6/7/8 | 低 |
| LS A/B/C | PB13/14/15（35/36/37）→ U9.3/4/5 | 低 |
| nSLEEP | PC7（39）→ U9.2 | 低 |
| nFAULT | PC8（40）→ U9.1 | 高 |
| BKIN | 无 | 本板无 BKIN，`BKE=0` |

RTT：`clock=ok`、`pwm=ok arr=4250 dtg=85 moe=false aoe=false bke=false`。
安全关断态通过后，才允许不接电机、限流下临时 `enable`：HS/LS 互补、周期 50 µs、死区约 500 ns。

> 手册对照（2026-11-27）：RM0440 §29.3.5 中心对齐模式下 RCR=1 使 UEV 每两个半周期
> 一次 → 20 kHz；BDTR DTG=0x55=85，`t_DTS=1/170 MHz`，在 `DTG[7:5]=0xx` 区间
> `DT=85×5.882 ns=500 ns`。`isr_count=20013/s` 与 20 kHz 吻合。
> 不测 U9 门极波形；低边三相同时按互补 PWM 默认。台上只用 TP1/TP2/PB6/PB7。

### 4.2 T-013：电流采样 / DAC / 片内 OPAMP

- `SOA/SOB/SOC`（U9.15/16/17）经 4.99 kΩ 网络进 `OPA3/OPA2/OPA1`（PB0/PA7/PA3）；**A/C 交叉**。
- 静态偏置约 1.25 V（VREF+/2 = 2.5/2）；HR-203 标度 0.2712 V/A、2.25 mA/LSB。
- `DAC1_OUT1/2`（PA4/PA5，TP1/TP2）只到外部脚；T-014 用 OUT1 输出残差、OUT2 作冻结指示（HR-803），不再作内部采样链。
- 片内 OPAMP1/2/3 follower，输出内部到 ADC1/2/3；无 OPAMP4/COMP2_OUT。

### 4.3 T-014：TP_SYNC / DAC / RTT 时间戳

| 探针 | 位置 | 期望 |
|---|---|---|
| TP_SYNC_LOOP | PB6（59） | 20 kHz ISR 内高、退出低；bringup 自检只给短脉冲 |
| TP_SYNC_FREEZE | PB7（60） | 冻结 50 ms 内高，随后低 |
| DAC1_OUT1 | PA4（18）/ TP1 | 0 A = 1.25 V；自检回读 2048 |
| DAC1_OUT2 | PA5（19）/ TP2 | 正常 0 V；冻结瞬间 2.49 V；自检回读 4095→0 |

RTT 已见 `dwt=ok cycles=75`、`trace=freeze dac1=2048 dac2=4095`、`DATA,...`；
**示波器/LA 物理波形尚未测**（192.168.1.77:5025 本会话不可达）。

### 4.4 T-015：20 kHz current task HIL（PASS，ADC/rate）

- `TIM1_UP_TIM16`；`RCR=1` 后 `isr_count=20013/s`（约 20.013 kHz），`isr_cycles≈1952`（约 11.5 µs）。
- ADC 12 bit：`adc0 a/b/c≈2038`，残差 `-1 LSB`；录波 `pre=512 post=1536 len=2048 total=3536 locked=true`。
- 根因：PAC `adc_v4` 的 `RES` 编码与 G474 不一致；`Res::TWELVEBIT` 实际写成 10 bit，
  raw 509 是 1.247 V 的 10-bit 码。改为按 `CFGR[4:3]` 清 0 后恢复 12 bit。
- `Pwm::wake()` 的 nSLEEP 等待改为 10 ms，消除 nFAULT 偶发低。
- 已测：TP1=1.2623 V；TP2 boot-freeze 2.49 V/1 s，随后 100 ms 自动触发 76.8 ms/2.49 V。
- 已实现并 HIL 通过：ADC1/2/3 注入组、TIM1_TRGO2 更新触发、三路 JDR1 读取。
- HIL：`adc0 a/b/c≈2039`、`isr_count=20004/s`、`isr_cycles≈2005`、`errors=0`；`JQDIS=1` + `JADSTART=1` 才能接收注入边沿。
- 复位故障码：最早期 PB7 拉高标记已进 init；Trace 先于 Pwm/ADC 初始化；`Pwm::wake` 的 nFAULT 等待放宽到 100 ms；时钟校验重试 10×10 ms；wake/ADC/零位/门控失败时 TP2+PB7 常高。20 次 SWD 复位均正常，手动复位“全无输出”未复现。
- **HIL PASS（enable-zero）**：`moe=true`、`nfault=false`、`isr_count=20004/s`、`errors=0`、`adc0≈2048`。
- **HIL PASS（低边窗口判据）**：`--enable-zero` 时 `trigger=true` 并有 `DATA`；普通 `hil`（MOE=0）时 `trigger=false`。判据来自 TIM1 `MOE + CC1NE/2NE/3NE`；低边同时按互补 PWM 默认，不测门极。

## 5. 测试点与注入网络

| TP | 网络 | 用途 |
|---|---|---|
| TP1 | DAC1_OUT1（PA4） | 残差 r_k 模拟输出；0 A≈1.25 V |
| TP2 | DAC1_OUT2（PA5） | 冻结指示：0 V 正常 / 2.49 V 冻结 |
| TP3 | 未分配 | 按实物确认 |
| PB6/PB7 | TP_SYNC_LOOP/FREEZE | 推挽 GPIO；不再作 USART3 |

`SOA/SOB/SOC` 测试脚：按丝印/通断确认是 U9.15/16/17（驱动侧）还是 OPA1/2/3（运放输入侧）。
PCB14 的 TP5/6/7 是滤波后 ADC 侧，与最新板不是同一套，不能混用。

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
`VREFINT 原始码`（HR-810，反算 VDDA）、`ISR 周期数`（HR-812）、`样本时间戳`（TIM5 32 bit，1 µs 分辨率；TIM2 被 embassy 时基占用，见 software/firmware/README.md）。

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
8. **论文题目是否随主控更新**：现题目写的是 STM32G491，与 G474RBT6 不一致；这是教务层面的事，需先确认。
9. **NTC 通道**：2026-11-27 网表里 `ADC12_IN6` 只连 U1.11 一个节点，没有分压/热敏电阻；LQFP64 脚 11 是
   PC3，附加功能为 `ADC12_IN9`/`OPAMP5_VINP`，不存在 `ADC12_IN6`。要么补上 NTC 与分压并改用 PC3 的
   `ADC12_IN9`，要么删掉 HR-405 的 NTC 依赖；在确认前不得把 NTC 当可用输入。

## 9. 投板前检查清单

- [ ] §8 的 1–3 已给数值
- [ ] 三路 ADC 在三个不同 ADC 单元，触发源与采样时间已按 RM0440 核对
- [ ] VREF+ / VDDA / SOx 偏置同源，TP4 可达
- [ ] JP_GAIN、JP_OFF、R_SHUNT、J_SYNC 四类网络都在原理图上
- [ ] PC6/PC7/PC8 真的接到排针（PCB14 只是"写了预留"）
- [ ] PA4 同时引到 TP17 与 J_SYNC（跳线可断）
- [ ] PA2(COMP2_OUT)、PB2(LED) 引出
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
- [ ] Flash 预算按 release 复核（数字只在 `cargo xtask size` 输出里，本文件不复写；PRE-09）
