//! S0/current bring-up: 20 kHz TIM1 current task with real ADC residual, DAC/TP_SYNC trace.
//!
//! The current task is hardware-driven by the TIM1 update event. Each ISR:
//! 1. clears UIF and raises `TP_SYNC_LOOP`,
//! 2. reads OPAMP1/2/3 → ADC1/2/3 (real samples),
//! 3. advances `GainGate`, writes the signed residual to DAC1_OUT1,
//! 4. pushes a 16 B frame into the pre/post-trigger ring,
//! 5. lowers `TP_SYNC_LOOP`; DWT records the ISR cycles.
//!
//! For this bench test the ADC is **software-started regular group** in the TIM1 ISR,
//! not the final injected-group/TIM1_TRGO2 low-side-window scheme (HR-805). It is a
//! real ADC residual, but the `window_ok` flag is provisional. MOE stays 0 and no
//! motor is connected.
//!
//! After 2000 valid samples the task triggers the recorder and DAC1_OUT2; 1536 post
//! samples later it unfreezes. A low-priority monitor dumps the first 8 real frames
//! and the summary after 1 s.

#![deny(unsafe_code)]
#![deny(warnings)]
#![no_main]
#![no_std]

mod trace;

use core::sync::atomic::{AtomicU32, Ordering};

use algorithms::{GainGate, GateParams};
use bsp::motor::bus_adc::{Adc, AdcPins, AMPS_PER_LSB};
use bsp::motor::bus_pwm::{Pwm, PwmPins};
use bsp::motor::service_clock;
use bsp::motor::service_dwt::Dwt;
use bsp::motor::service_timebase::{self, Timebase as ServiceTimebase};
use bsp::motor::service_trace::{Trace, TracePins};
use contracts::joint::config_budget::SYS_CLK_HZ;
use embassy_stm32::pac::TIM1 as TIM1_REGS;
use rtic::app;
use rtic_monotonics::systick::prelude::*;
use {defmt_rtt as _, panic_probe as _};

systick_monotonic!(Mono, 1_000);

/// 残差 DAC 满量程；0 A 始终在 2048（约 1.25 V，VREF+ = 2.5 V）。
const TRACE_R_FULL_SCALE_AMPS: f32 = 0.5;

/// 自检触发点：约 100 ms @20 kHz。
const AUTO_TRIGGER_SAMPLES: u32 = 2_000;

static ADC_ERRORS: AtomicU32 = AtomicU32::new(0);
static ISR_CYCLES: AtomicU32 = AtomicU32::new(0);
static ISR_COUNT: AtomicU32 = AtomicU32::new(0);

defmt::timestamp!("{=u32:us}", service_timebase::now_us());

pub mod pac {
    pub use embassy_stm32::pac::Interrupt as interrupt;
    pub use embassy_stm32::pac::*;
}

struct Current {
    adc: Adc,
    gate: GainGate,
    trace: Trace,
    dwt: Dwt,
    samples: u32,
    post: u32,
    triggered: bool,
    released: bool,
}

#[app(device = pac, peripherals = false, dispatchers = [USART1, USART2])]
mod app {
    use super::*;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        pwm: Pwm,
        current: Current,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        Mono::start(cx.core.SYST, SYS_CLK_HZ);

        let p = embassy_stm32::init(service_clock::config());

        // TIM5 must be configured before the first info! for real microsecond stamps.
        let _timebase = ServiceTimebase::init(p.TIM5);

        if !service_clock::verify() {
            defmt::error!("clock=bad: expected HSE 16M/PLL 170M, AHB/APB undivided");
            loop {
                core::hint::spin_loop();
            }
        }
        defmt::info!("clock=ok (HSE 16M / PLL 170M)");

        let mut pwm = Pwm::init(
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

        // Wake the driver only (no MOE); the current-sense bias needs nSLEEP high.
        if pwm.wake().is_err() {
            defmt::error!(
                "driver=wake fail: nFAULT low, nSLEEP returned low; current task stopped"
            );
            loop {
                core::hint::spin_loop();
            }
        }

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

        trace::TRACE.set_stride(trace::DEFAULT_STRIDE);
        let dwt = Dwt::new(cx.core.DWT, cx.core.DCB, SYS_CLK_HZ);

        let Some(mut adc) = Adc::init(AdcPins {
            adc1: p.ADC1,
            adc2: p.ADC2,
            adc3: p.ADC3,
            opamp1: p.OPAMP1,
            opamp2: p.OPAMP2,
            opamp3: p.OPAMP3,
            pa3: p.PA3,
            pa7: p.PA7,
            pb0: p.PB0,
        }) else {
            defmt::error!("adc=bad: OPAMP/ADC calibrate or ready timeout");
            loop {
                core::hint::spin_loop();
            }
        };
        defmt::info!("adc=ok (OPAMP1/2/3 -> ADC1/2/3)");

        // Bench zero snapshot: no PWM output, driver awake. This is diagnostic only;
        // the gate still requires a zero inside its plausibility window.
        let mut sum = [0u32; 3];
        let mut n = 0u32;
        for _ in 0..32 {
            if let Some(sample) = adc.take_sample() {
                sum[0] += u32::from(sample.raw.code[0]);
                sum[1] += u32::from(sample.raw.code[1]);
                sum[2] += u32::from(sample.raw.code[2]);
                n += 1;
            }
        }
        if n == 0 {
            defmt::error!("adc0=bad: no sample during zero snapshot");
            loop {
                core::hint::spin_loop();
            }
        }
        let zero = [
            sum[0] as f32 / n as f32,
            sum[1] as f32 / n as f32,
            sum[2] as f32 / n as f32,
        ];
        defmt::info!(
            "adc0 n={=u32} a={=f32} b={=f32} c={=f32}",
            n,
            zero[0],
            zero[1],
            zero[2]
        );

        let op1 = embassy_stm32::pac::OPAMP1.csr().read();
        let op2 = embassy_stm32::pac::OPAMP2.csr().read();
        let op3 = embassy_stm32::pac::OPAMP3.csr().read();
        defmt::info!(
            "opamp en={}{}{} into={}{}{} vm={}{}{}",
            op1.opampen(),
            op2.opampen(),
            op3.opampen(),
            op1.opaintoen() == embassy_stm32::pac::opamp::vals::Opaintoen::ADCCHANNEL,
            op2.opaintoen() == embassy_stm32::pac::opamp::vals::Opaintoen::ADCCHANNEL,
            op3.opaintoen() == embassy_stm32::pac::opamp::vals::Opaintoen::ADCCHANNEL,
            op1.vm_sel() == embassy_stm32::pac::opamp::vals::VmSel::OUTPUT,
            op2.vm_sel() == embassy_stm32::pac::opamp::vals::VmSel::OUTPUT,
            op3.vm_sel() == embassy_stm32::pac::opamp::vals::VmSel::OUTPUT
        );

        let mut gate = GainGate::new(GateParams::FROZEN);
        if !gate.set_zero_lsb(zero) {
            defmt::error!("gate=bad: zero outside plausible window");
            loop {
                core::hint::spin_loop();
            }
        }

        let current = Current {
            adc,
            gate,
            trace,
            dwt,
            samples: 0,
            post: 0,
            triggered: false,
            released: false,
        };

        bringup::spawn().ok();
        monitor::spawn().ok();
        (Shared {}, Local { pwm, current })
    }

    /// Logs the static safe-off state; owns TIM1 for the program lifetime.
    #[task(priority = 1, local = [pwm])]
    async fn bringup(cx: bringup::Context) {
        let pwm = cx.local.pwm.state();
        defmt::info!(
            "pwm=ok arr={} dtg={} moe={} aoe={} bke={} nfault={} n_sleep={}",
            pwm.arr,
            pwm.dtg,
            pwm.moe,
            pwm.aoe,
            pwm.bke,
            pwm.nfault,
            pwm.n_sleep
        );
        defmt::info!("current=go (20 kHz TIM1_UP)");
    }

    /// 20 kHz hardware task. No defmt, no await, no lock.
    #[task(binds = TIM1_UP_TIM16, priority = 8, local = [current])]
    fn current(cx: current::Context) {
        TIM1_REGS.sr().modify(|w| w.set_uif(false));

        let current = cx.local.current;
        ISR_COUNT.fetch_add(1, Ordering::Relaxed);
        let t0 = current.dwt.now_cycles();
        current.trace.sync_loop_high();

        if let Some(sample) = current.adc.take_sample() {
            let _ = current.gate.step(sample.raw, sample.window_ok);
            if let Some(r_lsb) = current.gate.last_residual_lsb() {
                let r_amps = r_lsb * AMPS_PER_LSB;
                current.trace.set_residual_amps(r_amps);

                let flags = if current.triggered && !current.released {
                    trace::TRACE_FLAG_FROZEN
                } else {
                    0
                };
                trace::TRACE.push(trace::TraceFrame {
                    k: sample.raw.tick,
                    ia_raw: sample.raw.code[0],
                    ib_raw: sample.raw.code[1],
                    ic_raw: sample.raw.code[2],
                    r_lsb: r_lsb as i16,
                    flags,
                    _rsvd: 0,
                });

                current.samples = current.samples.wrapping_add(1);

                if !current.triggered
                    && current.samples >= AUTO_TRIGGER_SAMPLES
                    && trace::TRACE.trigger()
                {
                    current.trace.freeze();
                    current.triggered = true;
                }

                if current.triggered && !current.released {
                    current.post = current.post.wrapping_add(1);
                    if current.post >= trace::POST_FRAMES as u32 {
                        current.trace.unfreeze();
                        current.released = true;
                    }
                }
            }
        } else {
            ADC_ERRORS.fetch_add(1, Ordering::Relaxed);
        }

        current.trace.sync_loop_low();
        ISR_CYCLES.store(current.dwt.elapsed_cycles(t0), Ordering::Relaxed);
    }

    /// Lowest-priority exporter; never runs in the 20 kHz path.
    #[task(priority = 1)]
    async fn monitor(_cx: monitor::Context) {
        Mono::delay(1_000.millis()).await;

        let s = trace::TRACE.summary();
        defmt::info!(
            "current errors={=u32} isr_cycles={=u32} isr_count={=u32} recorder pre={=u32} post={=u32} len={=u32} total={=u32} trigger={} locked={} stride={=u32}",
            ADC_ERRORS.load(Ordering::Relaxed),
            ISR_CYCLES.load(Ordering::Relaxed),
            ISR_COUNT.load(Ordering::Relaxed),
            s.pre,
            s.post,
            s.len,
            s.total,
            s.triggered,
            s.locked,
            s.stride
        );
        trace::TRACE.dump(8);
    }
}
