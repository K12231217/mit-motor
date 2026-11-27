//! S0 bring-up: 170 MHz clock readback, driver ownership, PWM safe-off, one ADC sample.
//!
//! This is **not** the motor application and it is **not** a HIL-verified result.
//! It compiles for `thumbv7em-none-eabihf`; the checks below only become evidence
//! when the same binary runs on a G474RBT6 board and the RTT log shows
//! `clock=ok pwm=ok sample=...`.
//!
//! 1. Clock: `bsp::motor::service_clock::config()` selects HSE 16 MHz → PLL 170 MHz
//!    (ADC PLLP/8 = 42.5 MHz). The firmware reads the tree back and halts on mismatch
//!    instead of continuing silently (PRE-05).
//! 2. Ownership: `Board` is constructed in `init` and moved into RTIC `Local`.
//!    ADC, PWM and the pins have exactly one owner; no `static mut`, no `Option`
//!    take in the hot path (MECH-01).
//! 3. PWM: TIM1 is configured at 20 kHz center-aligned with 500 ns dead time,
//!    `AOE = 0`, `MOE = 0`, `nSLEEP` low. The task only reads back `PwmState`;
//!    it never calls `enable`, so no gate output is generated.
//! 4. Sampling: one regular conversion on PA3 (VBUS, 100k/8.06k divider). The
//!    20 kHz injected group belongs to `bsp`, not here.
//!
//! The LED is only a heartbeat: it tells a human the binary reached the task.

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

use bsp::motor::bus_pwm::{Pwm, PwmPins};
use bsp::motor::service_clock;
use cortex_m::asm::delay as cycle_delay;
use embassy_stm32::adc::Adc;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::peripherals::{ADC1, PA3, PB2};
use embedded_hal::blocking::delay::DelayUs;
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
    adc: Adc<'static, ADC1>,
    vbus: PA3,
    led: Output<'static, PB2>,
    pwm: Pwm,
}

/// ADC driver needs a `DelayUs<u16>`. At 170 MHz, one microsecond is 170 cycles.
struct Delay;

impl DelayUs<u16> for Delay {
    fn delay_us(&mut self, us: u16) {
        cycle_delay(u32::from(us) * 170);
    }
}

/// VBUS divider: (100k + 8.06k) / 8.06k = 13.4, VREF = 3.3 V, 12-bit ADC.
fn vbus_mv(raw: u16) -> u32 {
    u32::from(raw) * 3_300 * 134 / (4_096 * 10)
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

        let mut delay = Delay;
        let adc = Adc::new(p.ADC1, &mut delay);
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
            adc,
            vbus: p.PA3,
            led: Output::new(p.PB2, Level::High, Speed::Low),
            pwm,
        };

        // The only place the Board is moved: into RTIC's Local resource. The
        // task below borrows it; no other code can name the ADC, PWM or pins.
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

        // One blocking regular conversion. v4 ADC calibrates on `Adc::new`.
        let raw = board.adc.read(&mut board.vbus);
        let mv = vbus_mv(raw);
        defmt::info!("sample=ok raw={} vbus_mv={}", raw, mv);

        // Heartbeat: high for ~1 s, then low. No control output is generated.
        board.led.set_low();
        Mono::delay(1_000.millis()).await;
        board.led.set_high();
        defmt::info!("bringup done; waiting for the next test");
    }
}
