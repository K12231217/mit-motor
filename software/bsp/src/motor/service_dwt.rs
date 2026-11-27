//! DWT CYCCNT 计时。
//!
//! 只做周期计数；µs 时基仍由 TIM5 提供（ADR-05）。`isr_cycles` 进遥测（I6）。

/// DWT 周期计数器的所有者。
pub struct Dwt;

impl Dwt {
    /// 当前周期计数。
    pub fn now_cycles(&self) -> u32 {
        todo!()
    }

    /// 清零计数。
    pub fn reset(&mut self) {
        todo!()
    }
}
