//! TIM5 1 MHz 自由运行时基，供 `defmt` 时间戳使用。
//!
//! TIM2 由 embassy 的 `time-driver-any` 占用，毫秒调度改用 SysTick；TIM5 是 32 位
//! 自由运行计数器，PSC = 170 - 1，CNT 直接读就是微秒。32 位在 1 MHz 下 71.58 min
//! 回绕一次，`defmt` 时间戳不合并回绕；需要跨回绕的日志由上位机解绕。
//!
//! 初始化放在 `embassy_stm32::init` 之后、第一条 `info!` 之前；`now_us()` 在
//! 初始化前返回 0，避免在 APB1 时钟关闭时读 TIM5。

use core::sync::atomic::{AtomicBool, Ordering};

use contracts::joint::config_budget::SYS_CLK_HZ;
use embassy_stm32::pac::timer::vals::Dir;
use embassy_stm32::pac::{RCC, TIM5 as TIM5_REGS};
use embassy_stm32::peripherals::TIM5;

/// 1 MHz 计数：每 tick 1 µs。
const TIM5_HZ: u32 = 1_000_000;

/// APB1 定时器时钟 170 MHz / 170 = 1 MHz。
const TIM5_PSC: u16 = (SYS_CLK_HZ / TIM5_HZ - 1) as u16;

/// 时基是否已配置；`now_us` 在 false 时返回 0。
static READY: AtomicBool = AtomicBool::new(false);

const _: () = assert!(SYS_CLK_HZ.is_multiple_of(TIM5_HZ));
const _: () = assert!(SYS_CLK_HZ / TIM5_HZ <= u16::MAX as u32 + 1);

/// TIM5 的所有者；只负责初始化，计数器本身由 `now_us()` 直接读。
pub struct Timebase {
    _tim: TIM5,
}

impl Timebase {
    /// 配置 TIM5 为 1 MHz、向上计数、ARR = u32::MAX。
    ///
    /// 调用前 APB1 定时器时钟必须是 [`SYS_CLK_HZ`]；`service_clock::verify` 负责
    /// 守住这一点。
    pub fn init(tim: TIM5) -> Self {
        RCC.apb1enr1().modify(|w| w.set_tim5en(true));

        let r = TIM5_REGS;
        r.cr1().modify(|w| {
            w.set_cen(false);
            w.set_dir(Dir::UP);
        });
        r.psc().write(|w| w.set_psc(TIM5_PSC));
        r.arr().write(|w| w.set_arr(u32::MAX));
        r.egr().write(|w| w.set_ug(true));
        r.cr1().modify(|w| w.set_cen(true));

        READY.store(true, Ordering::Release);
        Self { _tim: tim }
    }
}

/// 当前微秒计数；未初始化时返回 0。
#[inline]
pub fn now_us() -> u32 {
    if !READY.load(Ordering::Acquire) {
        return 0;
    }
    TIM5_REGS.cnt().read().cnt()
}
