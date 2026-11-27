//! S0 bring-up: 170 MHz clock readback, driver ownership, PWM safe-off.
//!
//! This is **not** the motor application and it is **not** a full HIL result.
//! It compiles for `thumbv7em-none-eabihf`; the checks below become evidence when
//! the binary runs on a G474RBT6 board and RTT shows `clock=ok pwm=ok`.
//!
//! 1. Clock: `service_clock::config()` selects HSE 16 MHz → PLL 170 MHz
//!    (ADC PLLP/8 = 42.5 MHz). The firmware reads the tree back and halts on
//!    mismatch instead of continuing silently (PRE-05).
//! 2. Ownership: `Board` is constructed in `init` and moved into RTIC `Local`.
//!    PWM and the pins have exactly one owner; no `static mut`, no `Option`
//!    take in the hot path (MECH-01).
//! 3. PWM: TIM1 20 kHz center-aligned, 500 ns dead time, `AOE = 0`, `MOE = 0`,
//!    `nSLEEP` low, `BKE = 0` (latest netlist has no TIM1_BKIN). The task only
//!    reads back `PwmState`; it never calls `enable`, so no gate output.
//!
//! ADC is intentionally absent: `embassy-stm32` 0.1.0's `ADC1::frequency()`
//! unwraps `pll1_p`, which the G4 RCC leaves `None` when `adc12sel = PLL1_P`.
//! The ADC injected group belongs to `bus_adc` (T-013), not this probe.

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use bsp::motor::bus_pwm::{Pwm, PwmPins};
use bsp::motor::service_clock;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::peripherals::PB2;
use rtic::app;
use rtic_monotonics::systick::prelude::*;
use {defmt_rtt as _, panic_probe as _};

systick_monotonic!(Mono, 1_000);

pub mod pac {
    pub use embassy_stm32::pac::Interrupt as interrupt;
    pub use embassy_stm32::pac::*;
}

/// One owner for every resource this probe touches.
struct Board {
    led: Output<'static, PB2>,
    pwm: Pwm,
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
        Mono::start(cx.core.SYST, 170_000_000);

        let p = embassy_stm32::init(service_clock::config());
        if !service_clock::verify() {
            defmt::error!("clock=bad: expected HSE 16M/PLL 170M, AHB/APB undivided");
            loop {
                core::hint::spin_loop();
            }
        }
        defmt::info!("clock=ok (HSE 16M / PLL 170M)");

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

        let board = Board {
            led: Output::new(p.PB2, Level::High, Speed::Low),
            pwm,
        };

        // The only place the Board is moved: into RTIC's Local resource.
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

        // Heartbeat: high for ~1 s, then low. No control output is generated.
        board.led.set_low();
        Mono::delay(1_000.millis()).await;
        board.led.set_high();
        defmt::info!("bringup done; waiting for the next test");
    }
}
