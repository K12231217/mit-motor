# 固件文件架构（v0.1，2026-11-15 敲定）

> 本文是文件级唯一版本（crate 划分、任务表、数据流），取代旧 `architecture.md` 的 §4–§6；
> 硬件事实见 [hardware-facts.md](archive/hardware-facts.md) §2。需求编号见 [hardware-requirements.md](hardware-requirements.md)。
> **状态标记**：树中标 `[计划]` 的条目尚未实现；实际落地状态见 [as-built.md](archive/as-built.md)。
> 本轮只定**文件与契约**，不落实现。

## 1. 六条不变量（每条都有检查手段，不靠自觉）

| # | 不变量 | 检查 |
|---|---|---|
| I1 | 依赖单向：`{contracts} ← {bsp, system, algorithms} ← app` | `cargo tree -p algorithms` 里出现 `bsp` 即失败 |
| I2 | `unsafe` 只出现在 `software/bsp/src/**` 与 `software/app/src/{fault,sections}.rs` | 其余 crate 根写 `#![forbid(unsafe_code)]`；CI 里 `grep -rn "unsafe"` 白名单 |
| I3 | 无堆：链接产物无 `__rust_alloc` | `llvm-nm` 搜索结果为零；`cargo tree` 无 `alloc` 特性 |
| I4 | 20 kHz 路径只由硬件事件驱动，**不含 `await`/`defmt`/锁** | `grep -n "await\|defmt\|mutex" software/app/src/tasks/current.rs` 为零 |
| I5 | 跨优先级数据只用无锁快照/有界队列，满即丢并计数 | 每个队列有 `dropped` 计数器进状态字 |
| I6 | 每个 ISR 有周期上界，实测值进遥测 | DWT 进出打点，`isr_cycles` 字段 |

### 1.1 模块标识：名字由四轴推导，不允许同义词

**一个模块 = 一个目录 + 一个公共头 + 一个 owner + 一份契约。** 模块名不是文件名的别名，
而是下面四轴的唯一拼接；四轴之外不允许加词（`handler`、`manager`、`util`、`core` 都不是角色）。

| 轴 | 闭集 | 回答 |
|---|---|---|
| layer | `app` / `dev` / `bsp` / `lib` | 它住在哪一层？ |
| domain | `joint` / `motor` / `rc` / `imu` | 它是什么领域对象？ |
| role | `task` / `service` / `protocol` / `bus` / `config` / `chain` | 它扮演什么角色？ |
| owner | 一个任务/ISR 或 `lib` | 谁拥有它的状态？唯一写者 |

规范 ID：`<layer>.<domain>.<role>[_<qualifier>]`，全小写 `snake_case`；`qualifier` 是领域名词
（如 `gate`、`icd`、`encoder`），不是角色或同义词。目录必须落在
`<crate>/src/<domain>/<role>[_<qualifier>].rs` 或同名目录；`src/lib.rs`、`src/**/mod.rs` 与
`src/bin/*` 是公共头/入口例外。模块名只认 ID，路径是它的机械投影。

- **唯一 owner**：有可变状态的模块只能有一个写者；其他模块通过 `Snapshot` / 有界队列读，
  或只拿不可变引用。出现第二个写者就拆模块或改 role，不许“两边都写一点”。
- **一份契约**：公共头/注册表的 `contract` 列写该模块对外承诺的不变量（单写者、输出边界、
  单位、失败语义）。契约不写实现细节。
- **推导顺序**：先答 layer → domain → role → owner，再拼 ID；ID 已存在就是同一个模块，
  不允许再起第二个名字。新模块的目录名必须用 ID 的 `<role>_<qualifier>` 形式；存量按注册表迁移。

### 1.2 模块注册表（机器校验）

H6 检查这张表：每个源码文件恰好被一个 path 覆盖、ID 唯一且四轴合法、owner/contract 非空。
表是模块名与所有权的唯一真源；`cargo xtask hygiene` 失败即阻断。

<!-- module-registry:start -->
| module id | path | owner | contract |
|---|---|---|---|
| `lib.joint.protocol_icd` | `software/contracts/src/joint/protocol_icd.rs` | `lib` | 线协议布局与解码；只读、无策略 |
| `lib.joint.protocol_wire` | `software/contracts/src/joint/protocol_wire.rs` | `lib` | 线协议与内部类型换算的唯一入口；无策略 |
| `lib.joint.protocol_events` | `software/contracts/src/joint/protocol_events.rs` | `lib` | 队列事件与命令；单写者 |
| `lib.joint.protocol_faults` | `software/contracts/src/joint/protocol_faults.rs` | `lib` | 状态字位域；纯函数 |
| `lib.joint.protocol_snapshot` | `software/contracts/src/joint/protocol_snapshot.rs` | `lib` | seqlock；每个方向单写者 |
| `lib.joint.protocol_fsm` | `software/contracts/src/joint/protocol_fsm.rs` | `lib` | 状态机类型；不含迁移规则 |
| `lib.motor.protocol_trust` | `software/contracts/src/motor/protocol_trust.rs` | `lib` | Trust 枚举；Unknown=0 |
| `lib.motor.protocol_types` | `software/contracts/src/motor/protocol_types.rs` | `lib` | 物理量与定点类型；无策略 |
| `lib.motor.config_hw` | `software/contracts/src/motor/config_hw.rs` | `lib` | 器件事实与电气硬约束；编译期断言 |
| `lib.joint.config_budget` | `software/contracts/src/joint/config_budget.rs` | `lib` | 系统预算；只读 |
| `lib.joint.service_contracts` | `software/contracts/src/lib.rs` | `lib` | crate 根与 re-export；无状态 |
| `bsp.motor.service_clock` | `software/bsp/src/motor/service_clock.rs` | `init` | HSE 16M/PLL 170M 配置与读回；不匹配进 fault |
| `bsp.motor.bus_pwm` | `software/bsp/src/motor/bus_pwm.rs` | `current @8` | TIM1 20 kHz/500 ns；无 BKIN（BKE=0）；enable 检查 nFAULT |
| `bsp.motor.bus_adc` | `software/bsp/src/motor/bus_adc.rs` | `current @8` | ADC1/2/3 注入组 + TIM1_TRGO2；低边窗口判定待接入 |
| `bsp.motor.bus_encoder` | `software/bsp/src/motor/bus_encoder.rs` | `current @8` | SPI1/主编码器唯一所有者；只搬字节 |
| `bsp.motor.service_dwt` | `software/bsp/src/motor/service_dwt.rs` | `current @8` | DWT CYCCNT；先开 DCB.TRCENA，再使能计数器 |
| `bsp.motor.service_timebase` | `software/bsp/src/motor/service_timebase.rs` | `init` | TIM5 1 MHz，defmt 时间戳；32 位回绕 |
| `bsp.motor.service_trace` | `software/bsp/src/motor/service_trace.rs` | `current @8` | PB6/PB7 TP_SYNC、DAC1 残差/冻结；无锁无日志 |
| `bsp.joint.service_board` | `software/bsp/src/lib.rs` | `init` | Board 单实例；构造后 move 移交 |
| `lib.motor.service_gate` | `software/algorithms/src/motor/service_gate` | `calib @2` | GainGate 唯一写者；组窗不写主窗 |
| `lib.motor.service_encoder` | `software/algorithms/src/motor/service_encoder` | `current @8` | SPI1 唯一所有者；帧判据纯函数 |
| `lib.joint.service_fsm` | `software/algorithms/src/joint/service_fsm` | `app state owner` | 迁移矩阵唯一实现 |
| `lib.motor.config_motor` | `software/algorithms/src/motor/config_motor.rs` | `lib` | WK4310H 铭牌参数；每相/磁链换算 |
| `lib.motor.config_policy` | `software/algorithms/src/motor/config_policy.rs` | `lib` | 策略阈值唯一来源；不复制 contracts |
| `lib.motor.chain_foc` | `software/algorithms/src/motor/chain_foc` | `current @8` | Clarke/Park 纯函数；幅值不变 |
| `lib.motor.chain_svpwm` | `software/algorithms/src/motor/chain_svpwm` | `current @8` | 输出边界 ClampedModulation；D_MAX 夹紧 |
| `lib.motor.chain_sense` | `software/algorithms/src/motor/chain_sense` | `current @8` | 码到安培；不做零和修正 |
| `lib.motor.chain_pid` | `software/algorithms/src/motor/chain_pid` | `current @8` | PI 状态唯一写者；抗饱和 |
| `lib.motor.chain_current` | `software/algorithms/src/motor/chain_current` | `current @8` | dq 电流环；电压圆唯一限幅 |
| `lib.joint.service_supervisor` | `software/algorithms/src/joint/service_supervisor` | `supervisor @7` | 心跳与超时锁存；clear 显式 |
| `lib.motor.service_math` | `software/algorithms/src/motor/service_math.rs` | `lib` | 无 libm 数值；纯函数 |
| `lib.joint.service_user` | `software/algorithms/src/lib.rs` | `lib` | crate 根与 re-export；无状态 |
| `app.motor.task_bringup` | `software/firmware/src/bin/main.rs` | `bringup` | S0 探针；单 Board owner；无电机输出 |
<!-- module-registry:end -->

## 2. 工作区与文件树

**目录与 crate 同名，不再使用数字前缀**。Rust 标识符不能以数字开头，数字前缀本来就是历史包袱；
现行 crate 根：`software/contracts/`、`software/algorithms/`、`software/bsp/`、`software/firmware/`。
`software/system/` 与 `software/app/` 是目标位，落地后按同一规则命名。

```toml
# Cargo.toml（仓库根）
[workspace]
resolver = "2"
members = ["software/contracts", "software/algorithms"]          # system/app 落地后再加入
exclude = ["software/firmware", "software/xtask", "software/bsp"]         # 独立 thumb / host 工具 workspace
```

```
software/contracts/            # 零依赖、零 unsafe、host+target 双用（包名 contracts，layer lib）
  src/lib.rs            #   只做 re-export；公共头
  src/joint/mod.rs      #   joint 模块树
  src/joint/protocol_icd.rs      #   CmdFrame/FbFrame/SlowFrame
  src/joint/protocol_wire.rs     #   CmdFrame→Cmd→CmdView、定点缩放
  src/joint/protocol_events.rs   #   Event / Cmd + 队列容量
  src/joint/protocol_faults.rs   #   FaultCode + 32 bit 状态字
  src/joint/protocol_snapshot.rs #   Fast/Cmd/Gain seqlock
  src/joint/protocol_fsm.rs      #   MotorState / Event / 迁移输入
  src/joint/config_budget.rs     #   系统预算（环频率、帧率、队列容量、看门狗）
  src/motor/mod.rs               #   motor 模块树
  src/motor/protocol_trust.rs    #   Trust
  src/motor/protocol_types.rs    #   Duty / DqTarget / RawSample3 / PhaseCurrent / Mode
  src/motor/config_hw.rs         #   器件事实与电气硬约束（D_MAX、采样窗口、MT6701 位宽）

software/bsp/                  # 唯一 unsafe 层（包名 bsp）[T-012 clock/pwm 已实现]
  src/lib.rs            #   Board 类型、init 构造后 move 移交、所有权表
  src/motor/mod.rs      #   motor 模块树
  src/motor/service_clock.rs   #   RCC 读回校验（embassy 配，bsp 验）
  src/motor/bus_pwm.rs         #   TIM1：6 路 + BDTR 死区（本板无 BKIN）
  src/motor/bus_adc.rs         #   OPAMP1/2/3 + ADC1/2/3 注入组；TIM1_TRGO2 触发
  src/motor/bus_encoder.rs     #   SPI1 双 CS + MT6701 读帧
  src/motor/service_dwt.rs     #   CYCCNT 计时与剖面（先 DCB.TRCENA）
  src/motor/service_timebase.rs#   TIM5 @1 MHz；defmt 时间戳
  src/motor/service_trace.rs  #   PB6/PB7 TP_SYNC + DAC1_OUT1/2 遥测
  src/motor/bus_dma.rs         #   [计划] 3 通道搬运与完成序
  src/motor/bus_can.rs         #   [计划] FDCAN2（embassy-stm32 包装，类型不外泄）
  src/joint/bus_uart.rs        #   [计划] 调试串口：最简阻塞写，fault dump 专用
  src/joint/bus_usb.rs         #   [计划] USB FS（板载 USB 已砍，见 INDEX §1）
  src/joint/service_gpio.rs    #   [计划] PWM/CS/STROBE/LED 的静态配置
  src/joint/service_fault.rs   #   [计划] HardFault/panic 的裸入口（第二处 unsafe）

software/system/               # 服务层（包名 system），forbid(unsafe)
  src/lib.rs
  src/log.rs            #   try_log：无锁环，满则丢；flush 只在最低优先级
  src/time.rs           #   TIM5 @1 MHz + 64 位溢出扩展（TIM2 归 embassy 时基）
  src/queue.rs          #   事件/指令/日志队列封装（容量来自 contracts）
  src/trace.rs          #   [计划] Snapshot → TraceFrame 打包（含 flags 位；当前实现见 firmware/src/bin/trace/mod.rs）
  src/watchdog.rs       #   通信中断 > 20 ms 停机

> T-014 先把 TIM5/DWT/trace 放在 `bsp.motor.*` 与 `firmware/src/bin/trace`；`system` 建后迁移，不复制两份。
> 当前 TIM5 是 32 位直读（71.58 min 回绕），`system/time.rs` 的 64 位扩展尚未实现。

software/algorithms/                 # 纯算法（包名 algorithms），no_std + forbid(unsafe)，x86 可测
  src/lib.rs            #   #![no_std] #![forbid(unsafe_code)]；公共头
  src/joint/mod.rs      #   joint 模块树
  src/joint/service_fsm.rs       #   状态 × 事件迁移矩阵 [已实现]
  src/joint/service_fsm/tests.rs #   迁移矩阵全表穷举测试 [已实现]
  src/joint/service_supervisor.rs       #   心跳 + 命令超时 [已实现]
  src/joint/service_supervisor/tests.rs #   窗口边界/停摆/锁存 [已实现]
  src/motor/mod.rs      #   motor 模块树
  src/motor/config_motor.rs      #   WK4310H 铭牌参数与每相/磁链换算 [已实现]
  src/motor/config_policy.rs     #   策略参数 [已实现]
  src/motor/service_gate.rs      #   四维结构门 + χ² [已实现]
  src/motor/service_gate/accum.rs#   正规方程累加器/组窗/半窗 [已实现]
  src/motor/service_gate/math.rs #   闸门专用常量 [已实现]
  src/motor/service_gate/tests.rs#   门控回归测试 [已实现]
  src/motor/service_encoder.rs       #   MT6701 帧解析 + 监控器 [已实现]
  src/motor/service_encoder/frame.rs #   24 bit 帧/CRC-6/状态位 [已实现]
  src/motor/service_encoder/monitor.rs # 接受判据/丢失拍放宽 [已实现]
  src/motor/service_encoder/tests.rs #   编码器回归测试 [已实现]
  src/motor/chain_foc.rs       #   Clarke/Park/逆 Park [已实现]
  src/motor/chain_foc/tests.rs #   变换与正余弦精度测试 [已实现]
  src/motor/chain_svpwm.rs       #   几何扇区 SVPWM [已实现]
  src/motor/chain_svpwm/tests.rs #   矢量还原/扇区表/输出边界 [已实现]
  src/motor/chain_sense.rs       #   原始码到安培 [已实现]
  src/motor/chain_sense/tests.rs #   零位/标度/通道顺序 [已实现]
  src/motor/chain_pid.rs         #   电流环 PI [已实现]
  src/motor/chain_pid/tests.rs   #   比例/积分/抗饱和 [已实现]
  src/motor/chain_current.rs     #   dq 电流环 [已实现]
  src/motor/chain_current/tests.rs # 一阶 RL/电压圆/接管预置 [已实现]
  src/motor/service_math.rs      #   共享无 libm 原语 [已实现]
  # calib/safety 不另建：相对增益估计与可信性判据已在 service_gate（ADR-12）

software/app/                  # 装配（包名 app，bin）[未建]
  src/main.rs           #   #[app(device = ..., peripherals = false)]
  src/app/joint/service_init.rs    #   上电顺序：时钟校验→自检→外设→任务
  src/app/joint/task_io.rs         #   prio 1：executor.run()
  src/app/joint/service_fault.rs   #   #[panic_handler] + HardFault
  src/app/joint/config_sections.rs #   #[link_section] 保留段与 magic
  src/app/motor/task_current.rs    #   prio 8：20 kHz 电流环
  src/app/motor/task_calib.rs      #   prio 2：软件 dispatcher
  src/app/motor/task_can.rs        #   prio 5：CAN
  src/app/motor/task_telemetry.rs  #   prio 5：遥测

software/firmware/               # 现有探针（独立 workspace，单 bin main，无电机代码）
  src/bin/main.rs       #   S0 bring-up：时钟读回 / Board 所有权 / PWM 安全关断 / trace HIL
  src/bin/trace/mod.rs  #   TraceFrame 16 B + 512/1536 预/后触发环 + stride + DATA 行
```

> **一处修正**：async 任务的**外壳**放 `software/app/src/tasks/io.rs`，业务函数放 `algorithms`。
> 若把 `#[embassy_executor::task]` 写进 `algorithms`，user 就依赖了执行器，I1 与 I2 同时破。

## 3. `contracts`：三个模块的分工

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

上行快照（24 B）：`{ t_us: u64, dq: [f32;2], mode, seq, flags, rsvd0, ttl_ms, bench_phase, bench_amps_10ma }`（`CmdView`，无填充）。

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

### 3.3 跨层接口 —— 具体类型 + enum，不做运行时分发

设备族在本板是封闭的：一种桥（MP6540HA）、一种编码器（MT6701）。整机三类电机
（RM/DM/UT）同样按 enum 分派，不写成通用库。驱动对象只有一个主人：`bsp::init()`
构造，move 进 `app`；`algorithms` 只收定长数据与具体算法类型（AGENTS §1.1 MECH-01/02）。

```rust
/// 构造点产出确定已初始化的具体类型；热路径不查 Option。
pub struct Board { pub pwm: Pwm, pub adc: Adc, pub can: Can }

/// 多实现只在编译期封闭的枚举里切换。
pub enum Motor { Rm(RmMotor), Dm(DmMotor), Ut(UtMotor) }

impl Board {
    pub fn start(&mut self) { /* -> () */ }
    pub fn set_duty(&mut self, d: Duty) { /* d 已在构造点夹限 */ }
}
```

`algorithms` 只依赖 `contracts`；需要硬件时由 `app` 把 `&mut Board` 传进去，单板单实例，
最多一个泛型参数，禁 trait object。`Snapshot<T>` 是跨任务唯一的最新值通道（PRE-04）。

## 4. `bsp`：所有权表与两处 unsafe

| 外设 | 归属 | 理由 |
|---|---|---|
| TIM1 / ADC1-3 / DMA1 / PWM GPIO | **只有 bsp 碰**，走裸寄存器 | 20 kHz 热路径，不许经过 HAL 的动态检查 |
| FDCAN2 / USART3 / USB FS / SPI1 | 只有 bsp 碰，内部用 embassy-stm32/embassy-usb | 冷路径，不值得手写 |
| RCC / PWR | embassy 配，bsp **读回校验** | 时钟树只能一个主人 |

两条不可协商的硬件契约（寄存器级，`init()` 必须逐条读回校验）：

- **`TIM1_BDTR.AOE = 0`**：BKIN 触发硬件清 MOE 后，禁止下一个更新事件自动恢复出力；
  故障后只能由显式清故障流程重新使能。
- **电流采样路径**：三相电流走 ADC1/2/3 注入组，由 **TIM1_TRGO2** 同步触发；单个 TIM1_UP ISR 等待 JEOS
  中断一次性读取三路 JDR1，并给三路样本打同一个 `RawSample3.tick`（`contracts::types`）。
  跨 ADC、跨中断拼出的样本一律判无效。

`src/lib.rs` 顶部必须写死这段注释，并在 `init()` 里运行：

```
// 所有权：TIM1/ADC/DMA 只由本 crate 以裸寄存器访问；embassy 驱动不得接管。
// 时钟：由 embassy_stm32::init() 配置，本 crate 只校验 PLLN/PLLR/VOS 与预期一致，
//      不一致 → 直接走 fault dump（不静默继续）。
```

## 5. `system` / `algorithms` 的边界

- `system` 里**不许出现** `stm32*` / `embassy_stm32` / `embassy_executor` / `rtic*` 字样
  （`embassy-sync` 允许：它是纯 `no_std`、无芯片依赖、无分配，队列就靠它）；
  它只从 `contracts` 取类型；需要硬件时由 `app` 以 `&mut` 传入具体实现（单板单实例，
  最多一个泛型参数，禁 trait object）。
- `algorithms` 里**不许出现** `bsp`、不许 `usize` 下标越界风险（`deny(clippy::indexing_slicing)`）、不许 `unwrap`。
- `system` 与 `algorithms` 均 `deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)`；
  `arithmetic_side_effects` 只对整数部分启用（对 `f32` 太严）。

## 6. `app`：任务表与上电顺序

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
| `.uninit.trace` | SRAM2 @0x20014000 | `TraceFrame[1024]`（16 B/帧）| 16 KB（**规划**；当前 T-014 用 `.bss` 2048 帧=32 KB） |
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

## 9. 检查手段（CI，`cargo xtask arch`）

实现在 `software/xtask/src/checks.rs`（纯 Rust，无 shell 依赖）。

1. `cargo tree -p algorithms | grep -E "bsp|embassy-stm32|embassy-executor|rtic"` → 必须为空；
2. `cargo tree -p system | grep -E "stm32|embassy-stm32|embassy-executor|rtic"` → 必须为空；
3. `grep -rn "unsafe" software/contracts/src software/system/src software/algorithms/src` → 必须为空；
4. `grep -rn "await\|defmt\|\.lock()" software/app/src/tasks/current.rs` → 必须为空；
5. `llvm-nm target/.../app | grep __rust_alloc` → 必须为空；
6. `cargo build --release && llvm-objcopy -O binary`，统计 Flash/RAM 占用写进 CI 日志；
7. I7：`MotorState` 只允许在 `software/contracts/src/joint/protocol_fsm.rs`、`software/algorithms/src/joint/service_fsm.rs` 与两处 crate 根出现
   （其余文件拿到它就能写第二条迁移路径）。

8. **待落地**（`bsp`/`system`/`app` 建好后再接进 `cargo xtask arch`）：MECH-01 的
   `static mut`/`static Option<驱动>` grep、MECH-02 的 `dyn` grep、MECH-03 的热路径签名 grep、
   MECH-04 的 `core::fmt` 符号审查。未建 crate 的检查记为 N/A，不计 PASS。

9. 协作门禁（`cargo xtask hygiene`，落点见 [`agent-guardrails.md`](archive/agent-guardrails.md)）：
   H2 的 `TODO(owner, trigger)`、H5 的注释标记，以及 H4 —— 每个 `<crate>/src/**/*.rs`
   必须出现在上面 §2 的文件树块里。新增模块不写文档即 FAIL。

## 10. 未定项（不影响文件架构，但影响第一行实现）

1. **三路 ADC 的"采完"信号**：挂在 ADC3 EOC 还是读上一拍（建议后者，确定性最好）→ 决定 `take_sample()` 语义。
2. **`embassy_stm32::init()` 与裸 PAC 单例的衔接**：需 30 行探针确认（`steal()` 还是从返回的 Peripherals 里取）。
3. **`algorithms` 的测试载体**：`tests/` 用 host `cargo test` + `fixtures/*.bin`，还是单独 `tools/replay`。建议后者，`algorithms` 只留最小单测。
