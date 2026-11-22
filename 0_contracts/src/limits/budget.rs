//! 时序与容量预算。
//!
//! 这些量不是物理常数，而是全工程共同遵守的**系统预算**：环频率、ISR 时间余量、
//! 线上的帧率与队列深度、看门狗超时。跨 crate 必须一致（PRE-07），但它们的取值
//! 是设计选择，不是器件事实；改这里不会让电路不成立，只会让预算不再成立。

/// 电流环频率，单位 Hz。由 ADC 注入组完成事件驱动，不是软件定时器（HR-302）。
pub const CURRENT_LOOP_HZ: u32 = 20_000;

/// ISR 周期，单位 µs（1 / 20 kHz）。
pub const ISR_PERIOD_US: u32 = 50;

/// PWM 载波频率，单位 Hz（HR-302：TIM1 中心对齐 40 kHz）。
///
/// 控制环仍是 20 kHz：每 [`CONTROL_DECIMATION`] 个载波周期触发一次注入组采样。
pub const PWM_CARRIER_HZ: u32 = 40_000;

/// 控制环对载波的抽取比：40 kHz / 20 kHz = 2。
pub const CONTROL_DECIMATION: u32 = 2;

/// ISR 时间预算，单位 µs。
///
/// 预算含快照发布（< 0.5 µs）；DWT 实测值进遥测。
pub const ISR_BUDGET_US: u32 = 25;

/// 通信看门狗超时，单位 ms；也作为 CMD 帧 `ttl_ms = 0` 的默认值。
pub const CAN_TIMEOUT_MS: u16 = 20;

/// CAN FD 数据段波特率，单位 bit/s。
pub const CAN_DATA_BITRATE_BPS: u32 = 2_000_000;

/// 反馈帧速率，单位 Hz（1 kHz，E7 要求无丢帧）。
pub const TELEMETRY_HZ: u32 = 1_000;

/// 慢帧速率，单位 Hz（50 Hz，状态变化或请求时提前发）。
pub const SLOW_FRAME_HZ: u32 = 50;

/// 命令队列深度（2 的幂）。
pub const CMD_Q: usize = 16;

/// 事件队列深度（2 的幂）。
pub const EVT_Q: usize = 32;

/// 黑匣子 trace 队列深度（2 的幂）。
pub const TRC_Q: usize = 8;

/// 日志环条目数：12 B/条 × 512 = 6 KB 的 SRAM2 保留区。
pub const LOG_Q: usize = 512;

/// seqlock 读重试上限；超过就用上一拍值，绝不阻塞。
pub const SNAPSHOT_RETRY_MAX: u8 = 4;

// 编译期自检：预算内部必须自洽。
const _: () = assert!(CURRENT_LOOP_HZ == 1_000_000 / ISR_PERIOD_US);
const _: () = assert!(ISR_BUDGET_US < ISR_PERIOD_US);
const _: () = assert!(PWM_CARRIER_HZ == CURRENT_LOOP_HZ * CONTROL_DECIMATION);
const _: () = assert!(1_000_000_000 % PWM_CARRIER_HZ == 0);
const _: () = assert!(CMD_Q.is_power_of_two());
const _: () = assert!(EVT_Q.is_power_of_two());
const _: () = assert!(TRC_Q.is_power_of_two());
const _: () = assert!(LOG_Q > 0);
const _: () = assert!(SNAPSHOT_RETRY_MAX > 0);
const _: () = assert!(CAN_TIMEOUT_MS > 0);
const _: () = assert!(CAN_DATA_BITRATE_BPS > 0);
const _: () = assert!(TELEMETRY_HZ > 0 && TELEMETRY_HZ <= CURRENT_LOOP_HZ);
const _: () = assert!(SLOW_FRAME_HZ > 0 && SLOW_FRAME_HZ < TELEMETRY_HZ);
