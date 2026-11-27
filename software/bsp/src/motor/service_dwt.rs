//! DWT CYCCNT 计时。
//!
//! 只做周期计数；µs 时基由 TIM5 提供（ADR-05）。`isr_cycles` 进遥测（I6）。

use cortex_m::peripheral::{DCB, DWT};

/// DWT 周期计数器的所有者。
pub struct Dwt {
    dwt: DWT,
    cycles_per_us: u32,
}

impl Dwt {
    /// 打开 CYCCNT；`sys_clk_hz` 必须与内核时钟一致（当前 170 MHz）。
    pub fn new(mut dwt: DWT, mut dcb: DCB, sys_clk_hz: u32) -> Self {
        // DEMCR.TRCENA gates DWT on Cortex-M4; enable it before CYCCNTENA.
        dcb.enable_trace();
        dwt.enable_cycle_counter();
        Self {
            dwt,
            cycles_per_us: (sys_clk_hz / 1_000_000).max(1),
        }
    }

    /// 当前周期计数。
    #[inline]
    pub fn now_cycles(&self) -> u32 {
        self.dwt.cyccnt.read()
    }

    /// 清零计数。
    pub fn reset(&mut self) {
        // Volatile write is unsafe in cortex-m 0.7; this crate is the unsafe whitelist.
        unsafe { self.dwt.cyccnt.write(0) };
    }

    /// 从 `start` 到现在的周期数，自动处理 u32 回绕。
    #[inline]
    pub fn elapsed_cycles(&self, start: u32) -> u32 {
        self.now_cycles().wrapping_sub(start)
    }

    /// 从 `start` 到现在的微秒数；只用于 ISR 预算，不作为日志时间戳。
    #[inline]
    pub fn elapsed_us_since(&self, start: u32) -> u32 {
        self.elapsed_cycles(start) / self.cycles_per_us
    }
}
