//! S0 bring-up + HIL instrumentation: clock, PWM safe-off, DWT, DAC/TP_SYNC, RTT trace.
//!
//! This is **not** the motor application. It proves:
//!
//! 1. Clock: HSE 16 MHz → PLL 170 MHz (ADC PLLP/8 = 42.5 MHz), read back before use.
//! 2. Ownership: `Board` is built in `init` and moved into RTIC `Local`; no static driver.
//! 3. PWM: TIM1 20 kHz center-aligned, 500 ns dead time, `AOE = 0`, `MOE = 0`,
//!    `nSLEEP` low, `BKE = 0`; no `enable` call, so no gate output.
//! 4. Trace: PB6/PB7 TP_SYNC, DAC1_OUT1 residual (0 A = midscale), DAC1_OUT2 freeze
//!    marker, DWT cycle timing, pre/post-trigger 2048-frame recorder with `DATA,` dump.
//!
//! PB6/PB7 are deliberately **not** USART3 here. The latest board routes those pins
//! to test points; UART debug is RTT only. See `docs/instrumentation.md`.
//!
//! ADC is absent: `embassy-stm32 0.1.0` unwraps `pll1_p` in `ADC1::frequency()`, and
//! the G4 RCC leaves it `None` with `adc12sel = PLL1_P`. `bus_adc` (T-013) will use
//! the PAC injected-group path instead of that HAL constructor.

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

mod trace;

use bsp::motor::bus_pwm::{Pwm, PwmPins};
use bsp::motor::service_clock;
use bsp::motor::service_dwt::Dwt;
use bsp::motor::service_timebase::{self, Timebase as ServiceTimebase};
use bsp::motor::service_trace::{Trace, TracePins};
use contracts::joint::config_budget::SYS_CLK_HZ;
use rtic::app;
use rtic_monotonics::systick::prelude::*;
use {defmt_rtt as _, panic_probe as _};

systick_monotonic!(Mono, 1_000);

/// 残差 DAC 满量程；0 A 始终在 2048（约 1.65 V）。
const TRACE_R_FULL_SCALE_AMPS: f32 = 0.5;

defmt::timestamp!("{=u32:us}", service_timebase::now_us());

pub mod pac {
    pub use embassy_stm32::pac::Interrupt as interrupt;
    pub use embassy_stm32::pac::*;
}

/// One owner for every resource this probe touches.
struct Board {
    pwm: Pwm,
    trace: Trace,
    dwt: Dwt,
}

#[app(device = pac, peripherals = false, dispatchers = [USART1])]
mod app {
    use super::*;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        board: Board,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        // Keep in sync with `service_clock::config()`: HSE 16 MHz, PLL 170 MHz.
        Mono::start(cx.core.SYST, SYS_CLK_HZ);

        let p = embassy_stm32::init(service_clock::config());

        // Configure TIM5 before the first info! so timestamps are real microseconds.
        // The object owns TIM5 for the rest of init; the counter keeps running.
        let _timebase = ServiceTimebase::init(p.TIM5);

        if !service_clock::verify() {
            defmt::error!("clock=bad: expected HSE 16M/PLL 170M, AHB/APB undivided");
            loop {
                core::hint::spin_loop();
            }
        }
        defmt::info!("clock=ok (HSE 16M / PLL 170M)");

        let dwt = Dwt::new(cx.core.DWT, cx.core.DCB, SYS_CLK_HZ);

        let pwm = Pwm::init(
            p.TIM1,
            PwmPins {
                hs_a: p.PA8,
                hs_b: p.PA9,
                hs_c: p.PA10,
                ls_a: p.PB13,
                ls_b: p.PB14,
                ls_c: p.PB15,
                n_sleep: p.PC7,
                n_fault: p.PC8,
            },
        );

        let trace = Trace::init(
            TracePins {
                dac: p.DAC1,
                dac_out1: p.PA4,
                dac_out2: p.PA5,
                sync_loop: p.PB6,
                sync_freeze: p.PB7,
            },
            TRACE_R_FULL_SCALE_AMPS,
        );

        let board = Board { pwm, trace, dwt };

        bringup::spawn().ok();
        (Shared {}, Local { board })
    }

    #[task(local = [board])]
    async fn bringup(cx: bringup::Context) {
        let board = cx.local.board;
        defmt::info!("owner=ok (Board moved into Local)");

        let pwm = board.pwm.state();
        defmt::info!(
            "pwm=ok arr={} dtg={} moe={} aoe={} bke={} nfault={}",
            pwm.arr,
            pwm.dtg,
            pwm.moe,
            pwm.aoe,
            pwm.bke,
            pwm.nfault
        );

        // DWT sanity check around a known register read.
        board.trace.sync_loop_high();
        let t0 = board.dwt.now_cycles();
        let _ = board.pwm.state();
        let cycles = board.dwt.elapsed_cycles(t0);
        board.trace.sync_loop_low();
        defmt::info!("dwt=ok cycles={=u32}", cycles);

        // DAC/TP_SYNC self-test: 0 A is midscale; freeze marker is 4095.
        board.trace.set_residual_amps(0.0);
        board.trace.freeze();
        let (dac1, dac2) = board.trace.dac_codes();
        defmt::info!("trace=freeze dac1={=u16} dac2={=u16}", dac1, dac2);
        Mono::delay(50.millis()).await;
        board.trace.unfreeze();
        let (dac1, dac2) = board.trace.dac_codes();
        defmt::info!("trace=ok dac1={=u16} dac2={=u16}", dac1, dac2);

        trace::TRACE.set_stride(trace::DEFAULT_STRIDE);

        // Pre/post-trigger recorder self-test. Fill the whole ring before trigger,
        // so the 512-frame pre-window proves oldest-frame overwrite.
        let frame = |k: u32| trace::TraceFrame {
            k,
            ia_raw: 2048 + (k % 3) as u16,
            ib_raw: 2048,
            ic_raw: 2048,
            r_lsb: 0,
            flags: trace::TRACE_FLAG_FROZEN | trace::TRACE_FLAG_SELFTEST,
            _rsvd: 0,
        };
        let run = |stride: u32| {
            trace::TRACE.set_stride(stride);
            let pre_periods = trace::TRACE_FRAMES as u32 * stride;
            let post_periods = trace::POST_FRAMES as u32 * stride;
            for k in 0..pre_periods {
                trace::TRACE.push(frame(k));
            }
            if !trace::TRACE.trigger() {
                defmt::error!("recorder trigger rejected");
            }
            for k in pre_periods..pre_periods + post_periods {
                trace::TRACE.push(frame(k));
            }
            let summary = trace::TRACE.summary();
            trace::TRACE.dump(3);
            let armed = trace::TRACE.arm();
            (summary, armed)
        };

        let (s, armed) = run(1);
        defmt::info!(
            "recorder pre={=u32} post={=u32} len={=u32} total={=u32} trigger={} locked={} stride={=u32} rearm={}",
            s.pre,
            s.post,
            s.len,
            s.total,
            s.triggered,
            s.locked,
            s.stride,
            armed
        );

        let (s, armed) = run(10);
        defmt::info!(
            "recorder stride={=u32} pre={=u32} post={=u32} len={=u32} total={=u32} trigger={} locked={} rearm={}",
            s.stride,
            s.pre,
            s.post,
            s.len,
            s.total,
            s.triggered,
            s.locked,
            armed
        );

        defmt::info!("bringup done; waiting for the next test");
    }
}
