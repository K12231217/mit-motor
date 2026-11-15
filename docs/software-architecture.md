# 固件文件架构（v0.1，2026-11-15 敲定）

> 本文**取代** [architecture.md](architecture.md) §4–§6（crate 划分、任务表、数据流）作为文件级唯一版本；
> architecture.md 的 §2 硬件事实仍然有效。需求编号见 [hardware-requirements.md](hardware-requirements.md)。
> 本轮只定**文件与契约**，不落实现。

## 1. 六条不变量（每条都有检查手段，不靠自觉）

| # | 不变量 | 检查 |
|---|---|---|
| I1 | 依赖单向：`{0_contracts} ← {1_bsp, 2_system, 3_user} ← 4_app` | `cargo tree -p user` 里出现 `bsp` 即失败 |
| I2 | `unsafe` 只出现在 `1_bsp/src/**` 与 `4_app/src/{fault,sections}.rs` | 其余 crate 根写 `#![forbid(unsafe_code)]`；CI 里 `grep -rn "unsafe"` 白名单 |
| I3 | 无堆：链接产物无 `__rust_alloc` | `llvm-nm` 搜索结果为零；`cargo tree` 无 `alloc` 特性 |
| I4 | 20 kHz 路径只由硬件事件驱动，**不含 `await`/`defmt`/锁** | `grep -n "await\|defmt\|mutex" 4_app/src/tasks/current.rs` 为零 |
| I5 | 跨优先级数据只用无锁快照/有界队列，满即丢并计数 | 每个队列有 `dropped` 计数器进状态字 |
| I6 | 每个 ISR 有周期上界，实测值进遥测 | DWT 进出打点，`isr_cycles` 字段 |

## 2. 工作区与文件树

**目录可带数字前缀，crate 名不能**（Rust 标识符不能以数字开头）。所以 `Cargo.toml` 里用路径声明、
包名去掉前缀：`0_contracts/` → `name = "contracts"`，`4_app/` → `name = "app"`。

```toml
# Cargo.toml（仓库根）
[workspace]
resolver = "2"
members = ["0_contracts", "1_bsp", "2_system", "3_user", "4_app"]
```

```
0_contracts/            # 零依赖、零 unsafe、host+target 双用（包名 contracts）
  src/lib.rs            #   只做 re-export + ICD 版本常量
  src/types.rs          #   Duty / DqTarget / RawSample3 / PhaseCurrent / Mode
  src/snapshot.rs       #   Snapshot<T>：seqlock，快环↔慢环唯一的跨优先级通道
  src/events.rs         #   Event / Cmd 枚举 + 各队列容量常量
  src/faults.rs         #   FaultCode + 32 bit 状态字位定义
  src/icd.rs            #   TraceFrame / CanFrame 的 repr(C) 布局 + 编译期 size 断言
  src/traits.rs         #   Hw / Clock / LogSink / Persist（跨层抽象）
  src/limits.rs         #   D_MAX、窗口下限、电流阈值 —— 全工程唯一数值来源

1_bsp/                  # 唯一 unsafe 层（包名 bsp）
  src/lib.rs            #   Board 类型、外设单例 take、所有权表
  src/clock.rs          #   RCC 读回校验（embassy 配，bsp 验）
  src/gpio.rs           #   PWM/CS/STROBE/LED 的静态配置
  src/pwm.rs            #   TIM1：6 路 + BDTR 死区 + BKIN
  src/adc.rs            #   ADC1/2/3 注入组 + TIM1_TRGO2 + DMA + 窗口判据
  src/dma.rs            #   3 通道搬运与完成序
  src/encoder.rs        #   SPI1 双 CS + MT6701 读帧
  src/can.rs            #   FDCAN2（embassy-stm32 包装，类型不外泄）
  src/usb.rs            #   USB FS（embassy-usb 包装）；**热路径不碰**
  src/uart.rs           #   调试串口：最简阻塞写，fault dump 专用
  src/dwt.rs            #   CYCCNT 计时与剖面
  src/faultentry.rs     #   HardFault/panic 的裸入口（第二处 unsafe）

2_system/               # 服务层（包名 system），forbid(unsafe)
  src/lib.rs
  src/log.rs            #   try_log：无锁环，满则丢；flush 只在最低优先级
  src/time.rs           #   TIM2 @1 MHz + 64 位溢出扩展
  src/queue.rs          #   事件/指令/日志队列封装（容量来自 contracts）
  src/fsm.rs            #   状态机骨架：trait Step + 迁移表
  src/trace.rs          #   Snapshot → TraceFrame 打包（含 flags 位）
  src/watchdog.rs       #   通信中断 > 20 ms 停机

3_user/                 # 纯算法（包名 user），no_std + forbid(unsafe)，x86 可测
  src/lib.rs            #   #![no_std] #![forbid(unsafe_code)]
  src/foc.rs            #   Clarke/Park 与逆变换
  src/svpwm.rs          #   七段式 + 占空比夹紧
  src/sense.rs          #   code → 安培（含零位/增益表）
  src/calib.rs          #   RLS 相对增益估计 + 双缓冲
  src/safety.rs         #   窗口/激励/跨工况一致性判据 → 冻结/锁存
  src/pid.rs
  tests/                #   host 测试 + 回放数据集（fixtures/）

4_app/                  # 装配（包名 app，bin）
  src/main.rs           #   #[app(device = ..., peripherals = false)]
  src/init.rs           #   上电顺序：时钟校验→自检→外设→任务
  src/tasks/current.rs  #   prio 8：20 kHz 电流环（唯一碰 ADC/DMA 的任务）
  src/tasks/can.rs      #   prio 5
  src/tasks/calib.rs    #   prio 2（软件 dispatcher）
  src/tasks/io.rs       #   prio 1：executor.run()，跑 USB/UART 的 async 外壳
  src/fault.rs          #   #[panic_handler] + HardFault（unsafe 第二处）
  src/sections.rs       #   #[link_section] 保留段与 magic
```

> **一处修正**：async 任务的**外壳**放 `4_app/src/tasks/io.rs`，业务函数放 `3_user`。
> 若把 `#[embassy_executor::task]` 写进 `3_user`，user 就依赖了执行器，I1 与 I2 同时破。

## 3. `0_contracts`：三个模块的分工

### 3.1 `snapshot.rs` —— 快环↔慢环的唯一通道

两个方向各一个 `Snapshot<T>`，**只有一个写者**，方向由优先级决定：

| 方向 | 写者 | 读者 | 读者代价 |
|---|---|---|---|
| 下行（电流环 → 慢环）| `current_loop` @8 | can/calib/io @1–5 | 被写者抢占 → seq 重试，最多 4 次 |
| 上行（慢环 → 电流环）| `calib`/`can` @2–5 | `current_loop` @8 | 写者被抢占 → 读失败，用上一拍设定值 |

协议（`T` 按 `[AtomicU32; 16]` 存储，**全程安全代码**，因此 `contracts` 能保持 `forbid(unsafe_code)`）：

```rust
pub struct Snapshot { seq: AtomicU32, w: [AtomicU32; 16] }   // 64 B payload

// 写：odd 表示"正在写"。Release 保证 payload 写入不会被排到末尾的 seq 之后
seq.store(odd,  Ordering::Release);            // 1
for (i, word) in v.encode().iter().enumerate() { w[i].store(*word, Ordering::Relaxed); }
seq.store(even, Ordering::Release);            // 2

// 读：有界重试，绝不等待。Acquire 保证 payload 读取不会被提到开头的 seq 之前
for _ in 0..4 {
    let s0 = seq.load(Ordering::Acquire);
    if s0 & 1 != 0 { continue; }
    let mut buf = [0u32; 16];
    for (i, word) in buf.iter_mut().enumerate() { *word = w[i].load(Ordering::Relaxed); }
    if seq.load(Ordering::Acquire) == s0 { return Some(FastView::decode(&buf)); }
}
None
```

三条论证（写进代码注释，别让人以为是随手写的）：

1. **为什么不用 `UnsafeCell` + `ptr::write`**：那样 `contracts` 就得开 `unsafe`，I2 破功。用 `[AtomicU32; 16]` +
   `f32::to_bits/from_bits` 存取，编译后就是普通的 `LDR`/`STR`，零额外代价，且全程安全代码。
   M4 单核、无 D-cache、快照不与 DMA 共享 ⇒ 只需 `Acquire/Release` 约束编译器与总线顺序，无需额外 DMB。
2. **上行方向的失败是常态**：prio 8 读者抢占 prio 2–5 写者时，写者被冻结，seq 一直是奇数 —— 4 次重试必然失败，
   这是设计内行为（用上一拍设定值，50 µs 的滞后对设定值无害）。
3. **最坏代价可算**：4 ×（读 seq + 16 word 拷贝 + 读 seq）≈ 80 cycles ≈ 0.47 µs @170 MHz，写进 I6 的预算表。

下行快照的字段（**恰好 64 B，无填充**，编译期断言）：

| 字段 | 类型 | 用途 |
|---|---|---|
| `t_us` | u64 | 采样时刻 |
| `i_abc` / `i_dq` | [f32;3] / [f32;2] | 反馈 |
| `duty` | [f32;3] | 实际占空比（含夹紧）|
| `theta_e` / `vbus` | f32 | 电角度 / 母线 |
| `isr_cycles` | u32 | DWT 实测（I6）|
| `state` | u32 | 状态字（faults.rs 定义）|
| `window_margin` / `dropped` | u16 | 窗口余量 / 丢弃计数 |
| `seq_hint` | u32 | 帧序号低 32 位，LA 与帧对时用 |

上行快照（≤24 B）：`{ t_us: u64, dq: [f32;2], mode: u8, seq: u8 }` + 填充到 24。

两个快照各自定长、各自一个具体类型（`FastSnapshot` = 16 word / `CmdSnapshot` = 6 word），
**不用 `const N` 泛型参数**——避免同一份代码在不同 N 上被单态化多次。

### 3.2 `events.rs` —— 慢速任务之间的事件与指令

**不经过快照**的是"有序列的事件"，必须走队列（RTIC 的 `spawn` 只有一个 pending 位，重发即丢）。

```rust
#[repr(u8)]
pub enum Event {
    Can(Cmd),            // ≤24 B
    TelemetryDue,
    CalibTick,
    Fault(FaultCode),
    SampleBatch(u16),
}

pub enum Cmd {
    SetMode(Mode),
    SetTarget(DqTarget),
    CommitCalib(CalibPayload),   // 定长，内部定点化
    Bench { phase: u8, amps: f32 },
    ClearFault,
}

pub const CMD_Q: usize = 16;      // 2 的幂
pub const EVT_Q: usize = 32;
pub const TRC_Q: usize = 8;
```

每条队列配 `dropped: u16` 并进状态字；**禁止**重试循环。

### 3.3 `traits.rs` —— 跨层抽象（`3_user` 只认这些）

```rust
pub trait Hw  { fn start(&mut self); fn take_sample(&mut self) -> Option<RawSample3>;
                fn set_duty(&mut self, d: Duty); }
pub trait Clock { fn now_us(&self) -> u64; fn cycles(&self) -> u32; }
pub trait LogSink { fn try_push(&self, e: LogEntry) -> bool; }
pub trait Persist { fn load(&self, out: &mut CalibBlob) -> bool; fn store(&self, b: &CalibBlob) -> bool; }
```

`3_user` 只依赖 `contracts`，用泛型接收实现（单板单实例 ⇒ 只单态化一次，零额外代码量）。

## 4. `1_bsp`：所有权表与两处 unsafe

| 外设 | 归属 | 理由 |
|---|---|---|
| TIM1 / ADC1-3 / DMA1 / PWM GPIO | **只有 bsp 碰**，走裸寄存器 | 20 kHz 热路径，不许经过 HAL 的动态检查 |
| FDCAN2 / USART3 / USB FS / SPI1 | 只有 bsp 碰，内部用 embassy-stm32/embassy-usb | 冷路径，不值得手写 |
| RCC / PWR | embassy 配，bsp **读回校验** | 时钟树只能一个主人 |

`src/lib.rs` 顶部必须写死这段注释，并在 `init()` 里运行：

```
// 所有权：TIM1/ADC/DMA 只由本 crate 以裸寄存器访问；embassy 驱动不得接管。
// 时钟：由 embassy_stm32::init() 配置，本 crate 只校验 PLLN/PLLR/VOS 与预期一致，
//      不一致 → 直接走 fault dump（不静默继续）。
```

## 5. `2_system` / `3_user` 的边界

- `2_system` 里**不许出现** `stm32*` / `embassy_stm32` / `embassy_executor` / `rtic*` 字样
  （`embassy-sync` 允许：它是纯 `no_std`、无芯片依赖、无分配，队列就靠它）；
  它只从 `contracts` 取类型、从 `1_bsp` 取实现（通过泛型参数传入）。
- `3_user` 里**不许出现** `1_bsp`、不许 `usize` 下标越界风险（`deny(clippy::indexing_slicing)`）、不许 `unwrap`。
- `2_system` 与 `3_user` 均 `deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)`；
  `arithmetic_side_effects` 只对整数部分启用（对 `f32` 太严）。

## 6. `4_app`：任务表与上电顺序

| prio | 任务 | 绑定 | 周期 | 预算 |
|---|---|---|---|---|
| 8 | `current` | ADC DMA 完成 | 50 µs | ≤25 µs（含快照发布 <0.5 µs）|
| 7 | `brk` / `nfault` | TIM1_BRK / EXTI | 异步 | ≤2 µs（只锁存）|
| 5 | `can` | FDCAN2_IT0 | 帧驱动 | ≤10 µs |
| 2 | `calib` | 软件 dispatcher | 10 ms | 剩余预算 |
| 1 | `io` | executor，常驻 | 事件驱动 | 仅冷路径 |
| idle | **不可达** | — | — | WFE 在 executor 内部完成 |

上电顺序（`init.rs`，失败一律进 fault dump 而不是"继续跑"）：
`RCC 读回校验 → DWT → UART（能喷字）→ 保留段 magic/CRC → 自检 S0–S5 → TIM1/ADC/DMA → 使能`

## 7. 段与保留区

| 区 | 位置 | 内容 | 大小 |
|---|---|---|---|
| `.uninit.trace` | SRAM2 @0x20014000 | `TraceFrame[1024]`（16 B/帧）| 16 KB |
| `.uninit.log` | SRAM2 +16K | `LogEntry[512]`（12 B/条）| 6 KB |
| `.uninit.magic` | SRAM2 +22K | magic + 版本 + CRC | 256 B |
| BKPR0..15 | RTC 备份寄存器 | 标定摘要（定点零位/增益、板 UID、OVP 实测点）| 64 B |

- **限制必须写进文档**：PCB14 的 `VBAT` 并在 `+3V3` 上，备份寄存器与 SRAM2 一样**只活过复位、活不过掉电**；
  要掉电保存只能写 flash（20 ms，不能进 fault handler，只能重启后回写）。
- 写 BKPR 前必须 `PWR->CR1 |= DBP`；冷启动时 magic/CRC 不匹配即视为无效并清零。

## 8. 三个数字契约（编译期锁死）

```rust
const _: () = assert!(size_of::<FastView>() == 64);
const _: () = assert!(size_of::<TraceFrame>() == 16);
const _: () = assert!(size_of::<Event>() <= 32);
```

任何字段改动都会**编译失败**——这是"给 AI 填空"最有效的一道锁。

## 9. 检查手段（CI，`ci/arch_check.sh`）

1. `cargo tree -p user | grep -E "bsp|embassy-stm32|embassy-executor|rtic"` → 必须为空；
2. `cargo tree -p system | grep -E "stm32|embassy-stm32|embassy-executor|rtic"` → 必须为空；
3. `grep -rn "unsafe" 0_contracts/src 2_system/src 3_user/src` → 必须为空；
4. `grep -rn "await\|defmt\|\.lock()" 4_app/src/tasks/current.rs` → 必须为空；
5. `llvm-nm target/.../app | grep __rust_alloc` → 必须为空；
6. `cargo build --release && llvm-objcopy -O binary`，统计 Flash/RAM 占用写进 CI 日志。

## 10. 未定项（不影响文件架构，但影响第一行实现）

1. **三路 ADC 的"采完"信号**：挂在 ADC3 EOC 还是读上一拍（建议后者，确定性最好）→ 决定 `take_sample()` 语义。
2. **`embassy_stm32::init()` 与裸 PAC 单例的衔接**：需 30 行探针确认（`steal()` 还是从返回的 Peripherals 里取）。
3. **`3_user` 的测试载体**：`tests/` 用 host `cargo test` + `fixtures/*.bin`，还是单独 `tools/replay`。建议后者，`3_user` 只留最小单测。
