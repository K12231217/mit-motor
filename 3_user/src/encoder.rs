//! MT6701 24 bit 帧解析、接受判据机与去抖。
//!
//! `1_bsp` 只负责 SPI 搬运三个字节（10.625 MHz，MSB 先出）。帧布局、CRC-6 与角度
//! 换算在 `frame`，语义判据、去抖与丢失窗口在 `monitor`；两者都是纯函数或纯状态机，
//! host 可测。

mod frame;
mod monitor;
#[cfg(test)]
mod tests;

pub use self::frame::{angle_rad, parse, Frame, FrameError};
pub use self::monitor::{
    jump_limit_lsb, AngleOutcome, EncoderMonitor, EncoderParams, Fault, Health,
};
