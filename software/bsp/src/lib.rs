//! 板级支持层：外设所有权与寄存器配置的唯一入口。
//!
//! `init` 构造确定已初始化的具体类型，move 给任务；热路径不查 `Option`、不加锁
//! （AGENTS §1.1 MECH-01/03）。本 crate 是 `unsafe` 白名单；其余 crate 保持
//! `forbid(unsafe_code)`。
//!
//! `service_clock`、`bus_pwm`、`bus_adc`、`service_dwt`、`service_timebase`、
//! `service_trace` 已实现；`bus_encoder` 与 `init()` 仍是 `todo!()`。
//! `software/firmware/src/bin/main.rs` 目前手工按序装配这些驱动，不调用 `init()`。

#![no_std]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod motor;

pub use motor::bus_adc::Sample;

/// 已初始化的板级资源。字段 move 给任务，构造后不再有第二份句柄。
pub struct Board {
    /// 三相互补 PWM 与使能/故障关断。
    pub pwm: motor::bus_pwm::Pwm,
    /// 三相注入组采样。
    pub adc: motor::bus_adc::Adc,
    /// MT6701 原始帧读取。
    pub enc: motor::bus_encoder::Encoder,
    /// DWT 周期计数。
    pub dwt: motor::service_dwt::Dwt,
    /// TIM5 1 MHz 时间戳。
    pub timebase: motor::service_timebase::Timebase,
    /// DAC/TP_SYNC 遥测。
    pub trace: motor::service_trace::Trace,
}

/// 上电初始化：时钟读回 → GPIO/PWM/ADC/SPI/DWT。
///
/// 任一步失败进 fault dump，不返回半初始化对象（PRE-05/PRE-08）。
pub fn init() -> Board {
    todo!()
}
