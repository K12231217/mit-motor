# 调试与录波：TP_SYNC / DAC / DWT / defmt RTT

> 状态：T-014 已实现并完成 RTT/寄存器级 HIL；**物理波形尚未测**。
> 引脚唯一来源：[`hardware-requirements.md`](hardware-requirements.md) §3.1、§4.3、§5。
> 工程红线：[`../AGENTS.md`](../AGENTS.md) PRE-02/PRE-04/PRE-05。

## 1. 引脚与信号

| 信号 | 引脚 | 方向/电平 | 语义 |
|---|---|---|---|
| `TP_SYNC_LOOP` | PB6 | 推挽 3.3 V | 20 kHz 控制 ISR 进入拉高、退出拉低；示波器/LA 的周期起点 |
| `TP_SYNC_FREEZE` | PB7 | 推挽 3.3 V | 校准冻结期间高；与 DAC1_OUT2 同相，供 LA 抓冻结窗口 |
| `DAC1_OUT1` / TP1 | PA4 | 0…3.3 V | 残差 `r_k`；0 A 固定 2048（约 1.65 V） |
| `DAC1_OUT2` / TP2 | PA5 | 0 / 3.3 V | 正常 0；冻结瞬间 4095，示波器 CH2 上升沿单次触发 |

PB6/PB7 在网表上仍叫 `USART3_TX/RX`，但**本季不启用 USART3**；不要再把这两个脚配成 UART AF。
调试日志只走 RTT（SWD 的 SWO 也不可用，PB3 是 SPI1_SCK）。

### 1.1 DAC 标度

```text
code = clamp(2048 + r_A * 2047 / R_FS, 0, 4095)
```

- `R_FS` 当前在 `software/firmware/src/bin/main.rs` 的 `TRACE_R_FULL_SCALE_AMPS = 0.5`。
- `r=0` → 2048；`r=+0.5 A` → 4095；`r=-0.5 A` → 0。
- 非有限值按 0 A（2048）处理；越界饱和；不在热路径 panic。
- 换量程只改 `TRACE_R_FULL_SCALE_AMPS`，不要改 DAC 中点。

### 1.2 冻结协议

1. 正常：DAC2=0、PB7=低、DAC1=残差。
2. 校准冻结瞬间：DAC2=4095、PB7=高；同一拍或前后 1 µs 内。
3. 示波器：CH2 上升沿单次触发；CH1 看残差；PB6 标 ISR 起止。
4. 解冻：PB7 低、DAC2=0；已录帧只导出，不再采。

## 2. 时间基准

| 用途 | 硬件 | 说明 |
|---|---|---|
| RTT 时间戳 | **TIM5**，PSC=169，1 MHz，32 位自由运行 | `defmt::timestamp!("{=u32:us}", now_us())`；32 位在 1 MHz 下 **71.58 min** 回绕；不合并回绕 |
| ISR/算法耗时 | **DWT CYCCNT**，170 MHz | `Dwt::new` 先 `DCB::enable_trace()` 再 `enable_cycle_counter()`；`elapsed_cycles` 处理回绕 |
| 毫秒调度 | SysTick | `rtic-monotonics`；TIM2 仍归 embassy，不碰 |

实测（2026-11-27 HIL）：

```text
0.000058 [INFO ] dwt=ok cycles=75
0.000071 [INFO ] trace=freeze dac1=2048 dac2=4095
0.070773 [INFO ] trace=ok dac1=2048 dac2=0
```

`cycles=0` 是错误信号：说明 `DEMCR.TRCENA` 没开或 CYCCNT 没使能；已修。

## 3. defmt RTT 约定

`.cargo/config.toml` 的编译期过滤：

```toml
[env]
DEFMT_LOG = "info"
```

分级边界：

| 级别 | 用途 | 示例 |
|---|---|---|
| `error!` | 不可恢复：nFAULT、校准冻结、ADC  watchdog、编码器 CRC 连续失败 | 默认常开 |
| `warn!` | 可恢复：单拍残差越界、CAN 邮箱满、母线接近阈值 | 联调常开 |
| `info!` | 低频状态：初始化、使能/失能、模式切换、冻结/解冻 | 正常开启 |
| `debug!` | 10–50 Hz 诊断：`ĝ`、`o_j`、单次 DWT 耗时 | 按模块打开 |
| `trace!` | 高频逐拍/寄存器；只排查特定 Bug | 单模块临时打开 |

示例过滤：

```text
DEFMT_LOG=info,main=debug
DEFMT_LOG=info,bsp::motor::bus_adc=trace
```

**20 kHz ISR 内禁止 `defmt!`**（PRE-02）。ISR 只写 GPIO/DAC/队列；文字行全部在低优先级任务输出。

当前实际格式（`probe-rs` 在行尾附模块/文件，不能把模块名放到行首）：

```text
0.070788 [INFO ] DATA,0,2048,2048,2048,0,129 (main bin/trace/mod.rs:66)
```

## 4. DATA 行与环形缓冲

### 4.1 行格式

```text
DATA,k,ia_raw,ib_raw,ic_raw,r_lsb,flags
```

- 前缀固定 `DATA,`；无空格；`grep '^DATA,' run.log > data.csv` 直接得到 CSV。
- `ia_raw/ib_raw/ic_raw` 是 12 bit ADC 原始码，不是安培；在主机用零位/相对增益换算。
- `r_lsb` 是残差 LSB（有符号），DAC 标度前使用。
- `flags`：bit0=`FROZEN`，bit7=`SELFTEST`，其余 0。
- 帧固定 16 B（`TraceFrame` 编译期 `size_of == 16`）。

### 4.2 预/后触发窗口

2048 槽覆盖环形，固定 **512 帧预触发 + 1536 帧后触发**：

| `stride` | 采样率 | 预触发 | 后触发 | 总窗口 |
|---:|---:|---:|---:|---:|
| 1（默认） | 20 kHz | 512×50 µs = **25.6 ms** | 1536×50 µs = **76.8 ms** | **102.4 ms** |
| 10 | 2 kHz | 512×0.5 ms = **256 ms** | 1536×0.5 ms = **768 ms** | **1.024 s** |

- 未触发：写者覆盖最旧槽；环形始终保留最近 2048 帧。
- 触发瞬间记录 `trigger_head`；再写 1536 帧后 `locked=true`。
- 导出窗口从 `trigger_head - 512` 到 `trigger_head + 1536`，正好 2048 帧。
- 触发前不足 512 帧时 `summary.pre` 会小于 512，不伪造 25.6 ms。
- RAM 固定 **32 KiB**（2048×16 B）。没有 0.5 s @20 kHz 的能力：10,000 帧需要 160 KB，超过 96 KiB 链接 RAM。已裁决保留 2048 帧，不改 8 B 帧、不加外存；慢速收敛曲线用 `stride=10` 换 1.024 s。

### 4.3 写入、触发与导出

- `trace::TRACE` 是唯一环形；槽为 4×`AtomicU32`（16 B），不使用 `UnsafeCell`。
- 20 kHz 任务每拍调用 `TRACE.push(frame)`；内部 `period` 计数按 `stride` 降采样。
- `push` 无锁、无分配、无 `defmt`；未触发时覆盖最旧，触发后写满 1536 帧锁存。
- 标定任务调用 `TRACE.trigger()`；只有第一次生效。
- 导出任务先看 `summary().locked`，再 `TRACE.dump(limit)`；`limit=usize::MAX` 导出完整窗口。
- 导出完成后 `TRACE.arm()` 才能开始下一轮；未锁存时 `arm` 返回 false，避免与写者竞争。
- HIL 自检帧 flags=0x81；真实数据应为 0x00/0x01（bit0=`FROZEN`）。

## 5. 所有权与热路径

- `Trace`（DAC + 两根 GPIO）由 20 kHz 任务独占；`set_residual_amps`、`sync_loop_high/low`、`freeze/unfreeze` 都是寄存器写，无锁/无分配/无日志。
- `trace::TRACE` 是纯数据环，单写者（20 kHz）、单触发者（标定）、单读者（导出）；不共享 DAC/GPIO 对象（MECH-01）。
- `Trace` 不放进 `static Option`；`Timebase` 只负责一次性初始化 TIM5；`Dwt` 持有 CYCCNT。
- 未来 `software/system` 的 `trace.rs` 若接管环形，必须保持 512/1536、16 B 帧和 `stride` 语义。

## 6. HIL 记录与未测项

已见（2026-11-27，J-Link）：

```text
0.000059 [INFO ] dwt=ok cycles=73
0.000073 [INFO ] trace=freeze dac1=2048 dac2=4095
0.071463 [INFO ] trace=ok dac1=2048 dac2=0
0.072926 [INFO ] DATA,1536,2048,2048,2048,0,129
0.073294 [INFO ] recorder pre=512 post=1536 len=2048 total=3584 trigger=true locked=true stride=1 rearm=true
0.082778 [INFO ] recorder stride=10 pre=512 post=1536 len=2048 total=3584 trigger=true locked=true rearm=true
```

- `stride=1` 自检先把 2048 帧写满再触发，导出首帧 `k=1536`，证明最旧帧被覆盖、窗口为最后 512 预触发帧。
- `stride=10` 自检按 20,480 个 PWM 周期产生 2048 帧，证明 1/10 降采样与 1.024 s 窗口换算。
- 这些是**同步自检计数**，不是真实 2 kHz 运行时序；真实采集需在 20 kHz 任务里按 50 µs 调用 `push`。

未测：

- PB6/PB7 物理边沿、DAC1/2 实际电压（示波器 `192.168.1.77:5025` 本会话不可达；只有 DOR 回读）。
- 20 kHz `enable` 后的 HS/LS 互补与 500 ns 死区。
- 真实 20 kHz 写者/真实冻结窗口；当前只有自检。

下一步：接示波器 CH1=PA4、CH2=PA5、CH3=PB6、CH4=PB7，CH2 上升沿单次；先跑本探针自检，再接 20 kHz 任务。
