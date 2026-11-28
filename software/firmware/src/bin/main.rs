//! S0/current bring-up: 20 kHz TIM1 current task with real ADC residual, DAC/TP_SYNC trace.
//!
//! The current task is hardware-driven by the TIM1 update event. Each ISR:
//! 1. clears UIF and raises `TP_SYNC_LOOP`,
//! 2. waits for ADC1/2/3 injected JEOS, triggered together by TIM1_TRGO2,
//! 3. advances `GainGate`, writes the signed residual to DAC1_OUT1,
//! 4. pushes a 16 B frame into the pre/post-trigger ring,
//! 5. lowers `TP_SYNC_LOOP`; DWT records the ISR cycles.
//!
//! The ADC path is the injected-group + TIM1_TRGO2 scheme (HR-805); the three
//! JDR1 results are read in one TIM1_UP ISR. `window_ok` remains provisional until
//! the current loop enables PWM and checks the low-side window. MOE stays 0 and no
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
use bsp::motor::bus_encoder::{Encoder, EncoderId, EncoderPins};
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
static OVP_TRIPPED: AtomicU32 = AtomicU32::new(0);
static ISR_CYCLES: AtomicU32 = AtomicU32::new(0);
static ISR_COUNT: AtomicU32 = AtomicU32::new(0);

defmt::timestamp!("{=u32:us}", service_timebase::now_us());

pub mod pac {
    pub use embassy_stm32::pac::Interrupt as interrupt;
    pub use embassy_stm32::pac::*;
}

struct Current {
    pwm: Pwm,
    adc: Adc,
    gate: GainGate,
    trace: Trace,
    dwt: Dwt,
    samples: u32,
    post: u32,
    triggered: bool,
    released: bool,
}

/// 板上验证 `EncoderMonitor` 的接受/降额/锁存路径（HIL-only，`hil-encoder-fsm`）。
///
/// 无磁铁也成立：真实帧 `Mg[3]=1` 走 `MagLock`；弱场、跳变、全 0 用 host 预先算好
/// 的合法帧注入。所有常数的 CRC-6 残差为 0（由 `frame::crc6` 生成），板上不复制
/// CRC 表；这里只验证判据与锁存语义，不重复 host 已覆盖的位级解码。
#[cfg(feature = "hil-encoder-fsm")]
mod encoder_fsm {
    use algorithms::motor::service_encoder::{
        angle_rad, EncoderMonitor, EncoderParams, Fault, Health,
    };
    use bsp::motor::bus_encoder::{Encoder, EncoderId};

    // 24 bit 字：`(angle << 10) | (mag << 6) | crc`，crc 由 host 计算并核过残差。
    const NORMAL_1000: u32 = 0x0FA006;
    const NORMAL_1010: u32 = 0x0FC83B;
    const NORMAL_1040: u32 = 0x104028;
    const WEAK_1000: u32 = 0x0FA080;
    const NEAR_WRAP: u32 = 0xFFFC1F;

    const TAG_OK: u8 = 0;
    const TAG_WARN: u8 = 1;
    const TAG_REJECTED: u8 = 2;
    const TAG_LATCHED: u8 = 3;
    const F_NONE: u8 = 0;
    const F_MAG_LOCK: u8 = 2;
    const F_ZERO: u8 = 3;
    const F_JUMP: u8 = 4;
    const F_MAG_STUCK: u8 = 5;

    fn tag(h: Health) -> u8 {
        match h {
            Health::Ok => TAG_OK,
            Health::MagWarn { .. } => TAG_WARN,
            Health::Rejected(_) => TAG_REJECTED,
            Health::Latched(_) => TAG_LATCHED,
        }
    }

    fn fault(h: Health) -> u8 {
        match h {
            Health::Rejected(f) | Health::Latched(f) => match f {
                Fault::Crc => 1,
                Fault::MagLock => F_MAG_LOCK,
                Fault::ZeroFrame => F_ZERO,
                Fault::Jump => F_JUMP,
                Fault::MagStuck => F_MAG_STUCK,
            },
            Health::Ok | Health::MagWarn { .. } => F_NONE,
        }
    }

    fn feed(m: &mut EncoderMonitor, word: u32) -> Health {
        let b = word.to_be_bytes();
        m.update([b[1], b[2], b[3]]).health
    }

    fn check(fail: u32, name: &'static str, got: Health, want_tag: u8, want_fault: u8) -> u32 {
        let (t, f) = (tag(got), fault(got));
        defmt::info!(
            "enc-fsm {=str} tag={=u8} fault={=u8} want={=u8}/{=u8}",
            name,
            t,
            f,
            want_tag,
            want_fault
        );
        if t == want_tag && f == want_fault {
            fail
        } else {
            fail + 1
        }
    }

    /// 跑完全部判据，返回失败计数（0 = 全部符合预期）。
    pub fn run(enc: &mut Encoder) -> u32 {
        let mut fail = 0u32;

        // 1) 真实帧（无磁铁）：Mg[3]=1，连续 2 帧锁存 MagLock。
        let mut live = EncoderMonitor::new(EncoderParams::default());
        let mut last = None;
        for _ in 0..4 {
            if let Some(raw) = enc.read_raw(EncoderId::E1) {
                last = Some(live.update(raw).health);
            }
        }
        match last {
            Some(h) => fail = check(fail, "live-maglock", h, TAG_LATCHED, F_MAG_LOCK),
            None => {
                defmt::error!("enc-fsm live: SPI returned no frame");
                fail += 1;
            }
        }

        // 2) 弱场：19 帧 MagWarn，第 20 帧 Rejected(MagStuck)，第 21 帧锁存。
        let mut weak = EncoderMonitor::new(EncoderParams::default());
        let _ = feed(&mut weak, NORMAL_1000);
        let mut warns = 0u32;
        let mut h = Health::Ok;
        for _ in 0..19 {
            h = feed(&mut weak, WEAK_1000);
            if tag(h) == TAG_WARN {
                warns += 1;
            }
        }
        fail = check(fail, "weak-19-warn", h, TAG_WARN, F_NONE);
        if warns != 19 {
            fail += 1;
            defmt::error!("enc-fsm weak: warn count {=u32} != 19", warns);
        }
        fail = check(
            fail,
            "weak-20-stuck",
            feed(&mut weak, WEAK_1000),
            TAG_REJECTED,
            F_MAG_STUCK,
        );
        fail = check(
            fail,
            "weak-21-latch",
            feed(&mut weak, WEAK_1000),
            TAG_LATCHED,
            F_MAG_STUCK,
        );

        // 3) 跳变：40 LSB > B_MAX_LSB(19)，第二帧锁存；reset 后立即可用。
        let mut jump = EncoderMonitor::new(EncoderParams::default());
        let _ = feed(&mut jump, NORMAL_1000);
        fail = check(
            fail,
            "jump-reject",
            feed(&mut jump, NORMAL_1040),
            TAG_REJECTED,
            F_JUMP,
        );
        fail = check(
            fail,
            "jump-latch",
            feed(&mut jump, NORMAL_1040),
            TAG_LATCHED,
            F_JUMP,
        );
        jump.reset();
        fail = check(
            fail,
            "jump-reset",
            feed(&mut jump, NORMAL_1000),
            TAG_OK,
            F_NONE,
        );

        // 4) 全 0 护栏：只有"此前接受角非 0"才计数；从 16383 回绕到 0 不过跳变判据。
        let mut zero = EncoderMonitor::new(EncoderParams::default());
        let _ = feed(&mut zero, NEAR_WRAP);
        let mut h = Health::Ok;
        for _ in 0..7 {
            h = feed(&mut zero, 0);
        }
        fail = check(fail, "zero-7-ok", h, TAG_OK, F_NONE);
        fail = check(
            fail,
            "zero-8-reject",
            feed(&mut zero, 0),
            TAG_REJECTED,
            F_ZERO,
        );
        fail = check(
            fail,
            "zero-9-latch",
            feed(&mut zero, 0),
            TAG_LATCHED,
            F_ZERO,
        );

        // 5) 接受路径的角度：14 bit 计数 → 弧度（16384 计数 = 2π）。
        let mut theta = EncoderMonitor::new(EncoderParams::default());
        let _ = feed(&mut theta, NORMAL_1000);
        let b = NORMAL_1010.to_be_bytes();
        let out = theta.update([b[1], b[2], b[3]]);
        let want = angle_rad(1010);
        if tag(out.health) != TAG_OK || (out.theta_rad - want).abs() > 1e-4 {
            fail += 1;
            defmt::error!(
                "enc-fsm theta: tag={=u8} got_mrad={=i32} want_mrad={=i32}",
                tag(out.health),
                (out.theta_rad * 1000.0) as i32,
                (want * 1000.0) as i32
            );
        } else {
            defmt::info!(
                "enc-fsm theta-ok mrad={=i32}",
                (out.theta_rad * 1000.0) as i32
            );
        }

        fail
    }
}

#[app(device = pac, peripherals = false, dispatchers = [USART1, USART2])]
mod app {
    use super::*;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        current: Current,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        // Earliest boot marker before embassy clock setup: PB7 high means the
        // core reached init. Trace::init later reconfigures PB7 low.
        pac::RCC.ahb2enr().modify(|w| w.set_gpioben(true));
        pac::GPIOB
            .moder()
            .modify(|w| w.set_moder(7, pac::gpio::vals::Moder::OUTPUT));
        pac::GPIOB.bsrr().write(|w| w.set_bs(7, true));

        Mono::start(cx.core.SYST, SYS_CLK_HZ);

        let p = embassy_stm32::init(service_clock::config());

        // TIM5 must be configured before the first info! for real microsecond stamps.
        let _timebase = ServiceTimebase::init(p.TIM5);

        let mut clock_ok = false;
        for _ in 0..10 {
            if service_clock::verify() {
                clock_ok = true;
                break;
            }
            cortex_m::asm::delay(SYS_CLK_HZ / 300);
        }
        if !clock_ok {
            defmt::error!("clock=bad: expected HSE 16M/PLL 170M, AHB/APB undivided");
            loop {
                core::hint::spin_loop();
            }
        }
        defmt::info!("clock=ok (HSE 16M / PLL 170M)");

        // 先把 DAC/TP_SYNC 拉起来：即使后面的驱动唤醒或 ADC 初始化失败，
        // TP1 也应保持 1.25 V；失败路径把 TP2/PB7 拉高作为可见故障码，
        // 不再出现“所有输出都没有”的静默死循环。
        let mut trace = Trace::init(
            TracePins {
                dac: p.DAC1,
                dac_out1: p.PA4,
                dac_out2: p.PA5,
                sync_loop: p.PB6,
                sync_freeze: p.PB7,
            },
            TRACE_R_FULL_SCALE_AMPS,
        );

        // 编码器 bring-up：两片 MT6701 已焊、暂无磁铁。这里只验证软件片选/SPI 时序与
        // CRC；角度语义（零位、跳变、磁场码）要等磁铁装好再测。SPI1 用完即交还，
        // FOC 落地后由 20 kHz 任务独占（ADR-08）。
        //
        // MT6701 的 T_PwrUp 上限 32 ms（Rev.1.8 §5）；3V3 与 MCU 同时上电，复位后
        // 立即读会拿到未就绪的 DO。`asm::delay` 每圈约 3 周期，2e6 圈 ≈ 35 ms。
        cortex_m::asm::delay(2_000_000);
        let mut enc = Encoder::new(
            EncoderPins {
                spi: p.SPI1,
                sck: p.PB3,
                miso: p.PB4,
                cs1: p.PC12,
                cs2: p.PC10,
            },
            algorithms::motor::config_policy::ENC_SPI_HZ,
        );
        for id in [EncoderId::E1, EncoderId::E2] {
            let mut spi_ok = 0u32;
            let mut crc_ok = 0u32;
            let mut last = 0u32;
            let mut angle = 0u16;
            let mut mag = 0u8;
            for _ in 0..8 {
                if let Some(raw) = enc.read_raw(id) {
                    spi_ok += 1;
                    last = (u32::from(raw[0]) << 16) | (u32::from(raw[1]) << 8) | u32::from(raw[2]);
                    if let Ok(frame) = algorithms::motor::service_encoder::parse(raw) {
                        crc_ok += 1;
                        angle = frame.angle_raw;
                        mag = frame.mag;
                    }
                }
            }
            defmt::info!(
                "enc id={=u8} spi_ok={=u32}/8 crc_ok={=u32}/8 last=0x{=u32:x} ang={=u16} mag={=u8}",
                id.number(),
                spi_ok,
                crc_ok,
                last,
                angle,
                mag
            );
        }
        #[cfg(feature = "hil-encoder-fsm")]
        {
            let failures = encoder_fsm::run(&mut enc);
            if failures == 0 {
                defmt::info!("enc-fsm pass");
            } else {
                trace.freeze();
                defmt::error!("enc-fsm fail={=u32}; TP2 held high", failures);
            }
        }
        drop(enc);

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
        // `hil-safety` 用 PB6 把唤醒窗口标在 LA 上：PC7(nSLEEP) 上升沿 → PC8(nFAULT)
        // 释放应落在 PB6 高电平窗口内；默认态（六路关断）另有一条 RTT 快照。
        #[cfg(feature = "hil-safety")]
        {
            let s = pwm.state();
            defmt::info!(
                "safety-off moe={=bool} aoe={=bool} bke={=bool} ccer=0x{=u32:x} off={=bool} nsleep={=bool} nfault={=bool}",
                s.moe,
                s.aoe,
                s.bke,
                s.ccer,
                s.outputs_off,
                s.n_sleep,
                s.nfault
            );
            if !s.outputs_off || s.aoe || s.bke || s.nfault || s.n_sleep {
                trace.freeze();
                defmt::error!("safety-off: 六路关断/默认态不成立");
            }
            trace.sync_loop_high();
        }

        let t0 = service_timebase::now_us();
        let wake = pwm.wake();
        let wake_us = service_timebase::now_us().wrapping_sub(t0);

        #[cfg(feature = "hil-safety")]
        trace.sync_loop_low();

        if wake.is_err() {
            trace.freeze();
            defmt::error!(
                "driver=wake fail: nFAULT low after 100ms; TP2/PB7 held high as fault code"
            );
            loop {
                core::hint::spin_loop();
            }
        }
        defmt::info!(
            "driver woken us={=u32} nsleep={=bool} nfault={=bool}",
            wake_us,
            pwm.state().n_sleep,
            pwm.state().nfault
        );

        #[cfg(feature = "hil-safety")]
        {
            let s = pwm.state();
            defmt::info!(
                "safety-wake moe={=bool} ccer=0x{=u32:x} off={=bool} nsleep={=bool} nfault={=bool} wake_us={=u32}",
                s.moe,
                s.ccer,
                s.outputs_off,
                s.n_sleep,
                s.nfault,
                wake_us
            );
            if !s.outputs_off || s.nfault || !s.n_sleep {
                trace.freeze();
                defmt::error!("safety-wake: 唤醒后仍应六路关断");
            }
        }

        #[cfg(feature = "hil-enable-zero")]
        {
            if pwm.enable().is_err() {
                trace.freeze();
                defmt::error!("hil=enable-zero fail: nFAULT/AOE; TP2/PB7 fault");
                loop {
                    core::hint::spin_loop();
                }
            }
            defmt::info!("hil=enable-zero moe=1 (zero vector, no motor)");
        }

        #[cfg(feature = "hil-boot-freeze")]
        {
            trace.freeze();
            defmt::info!("hil=boot-freeze 1s (TP2/PB7 high)");
            // cortex_m::asm::delay's loop is ~3 cycles; /3 gives ~1 s at 170 MHz.
            cortex_m::asm::delay(SYS_CLK_HZ / 3);
            trace.unfreeze();
            defmt::info!("hil=boot-freeze done");
        }

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
            pb11: p.PB11,
        }) else {
            trace.freeze();
            defmt::error!("adc=bad: OPAMP/ADC calibrate or ready timeout; TP2/PB7 fault");
            loop {
                core::hint::spin_loop();
            }
        };
        defmt::info!("adc=ok (OPAMP1/2/3 -> ADC1/2/3)");

        // HR-804：PA4(TP1) 经 J_SYNC 跳线进三路电流采样输入。扫三个 DAC 码，
        // 每码平均 32 拍；同码三通道偏差 ≤ 2 LSB，且绝对值应跟随 DAC 码
        // （没有跳线时三路都读偏置 ~2048，绝对值检查会报 "no correlation"）。
        #[cfg(feature = "hil-dac-ramp")]
        {
            let mut fail = 0u32;
            for code in [512u16, 2048, 3584] {
                trace.set_dac1_code(code);
                cortex_m::asm::delay(SYS_CLK_HZ / 10_000); // ~10 µs 建立
                let mut sum = [0u32; 3];
                let mut n = 0u32;
                for _ in 0..32 {
                    if let Some(sample) = adc.take_sample() {
                        for (dst, src) in sum.iter_mut().zip(sample.raw.code.iter()) {
                            *dst += u32::from(*src);
                        }
                        n += 1;
                    }
                }
                if n == 0 {
                    fail += 1;
                    defmt::error!("dac-ramp code={=u16} no samples", code);
                    continue;
                }
                let avg = [sum[0] / n, sum[1] / n, sum[2] / n];
                let mut hi = 0u32;
                let mut lo = u32::MAX;
                for v in avg {
                    hi = if v > hi { v } else { hi };
                    lo = if v < lo { v } else { lo };
                }
                let follow = avg.iter().all(|v| v.abs_diff(u32::from(code)) <= 50);
                defmt::info!(
                    "dac-ramp code={=u16} a={=u32} b={=u32} c={=u32} spread={=u32} follow={=bool}",
                    code,
                    avg[0],
                    avg[1],
                    avg[2],
                    hi - lo,
                    follow
                );
                if hi - lo > 2 || !follow {
                    fail += 1;
                }
            }
            trace.set_residual_amps(0.0);
            if fail == 0 {
                defmt::info!("dac-ramp pass");
            } else {
                trace.freeze();
                defmt::error!("dac-ramp fail={=u32}（先确认 J_SYNC 跳线）", fail);
            }
        }

        // Bench zero snapshot: no PWM output, driver awake. This is diagnostic only;
        // the gate still requires a zero inside its plausibility window.
        let mut sum = [0u32; 3];
        let mut vbus_sum = 0u32;
        let mut n = 0u32;
        for _ in 0..32 {
            if let Some(sample) = adc.take_sample() {
                sum[0] += u32::from(sample.raw.code[0]);
                sum[1] += u32::from(sample.raw.code[1]);
                sum[2] += u32::from(sample.raw.code[2]);
                vbus_sum += u32::from(sample.vbus_raw);
                n += 1;
            }
        }
        if n == 0 {
            trace.freeze();
            defmt::error!("adc0=bad: no sample during zero snapshot; TP2/PB7 fault");
            loop {
                core::hint::spin_loop();
            }
        }
        let zero = [
            sum[0] as f32 / n as f32,
            sum[1] as f32 / n as f32,
            sum[2] as f32 / n as f32,
        ];
        let vbus_raw = vbus_sum / n;
        defmt::info!(
            "adc0 n={=u32} a={=f32} b={=f32} c={=f32} vbus={=u32} mv={=i32} ovp_raw={=u16}",
            n,
            zero[0],
            zero[1],
            zero[2],
            vbus_raw,
            (vbus_raw as f32 * contracts::motor::config_hw::VBUS_MV_PER_LSB) as i32,
            algorithms::motor::config_policy::VBUS_OVP_RAW
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
            trace.freeze();
            defmt::error!("gate=bad: zero outside plausible window; TP2/PB7 fault");
            loop {
                core::hint::spin_loop();
            }
        }

        let st = pwm.state();
        defmt::info!(
            "pwm=ok arr={} dtg={} moe={} aoe={} bke={} ccer=0x{:x} off={} nfault={} n_sleep={}",
            st.arr,
            st.dtg,
            st.moe,
            st.aoe,
            st.bke,
            st.ccer,
            st.outputs_off,
            st.nfault,
            st.n_sleep
        );
        defmt::info!("current=go (20 kHz TIM1_UP)");

        let current = Current {
            pwm,
            adc,
            gate,
            trace,
            dwt,
            samples: 0,
            post: 0,
            triggered: false,
            released: false,
        };

        monitor::spawn().ok();
        (Shared {}, Local { current })
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
            // 母线过压：锁存 + 立即关 MOE/六路 + 拉低 nSLEEP；只做一次，避免反复写寄存器。
            if algorithms::motor::config_policy::VBUS_OVP_ENABLED
                && sample.vbus_raw >= algorithms::motor::config_policy::VBUS_OVP_RAW
                && OVP_TRIPPED.swap(1, Ordering::Relaxed) == 0
            {
                current.pwm.disable();
                current.trace.freeze();
            }

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
            "current errors={=u32} isr_cycles={=u32} isr_count={=u32} ovp={=u32} recorder pre={=u32} post={=u32} len={=u32} total={=u32} trigger={} locked={} stride={=u32}",
            ADC_ERRORS.load(Ordering::Relaxed),
            ISR_CYCLES.load(Ordering::Relaxed),
            ISR_COUNT.load(Ordering::Relaxed),
            OVP_TRIPPED.load(Ordering::Relaxed),
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
