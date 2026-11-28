//! TIM1 三相互补 PWM、BDTR 死区与 MOE。
//!
//! 输出只接受 [`Duty`]：调制量在构造点已按 `D_MAX` 夹紧（PRE-08）。六路全低是
//! `Duty::Coast`，不能用零调制量冒充。
//!
//! 安全顺序：`init` 结束时 `nSLEEP` 低、`MOE = 0`、`AOE = 0`、六路输出关闭；`enable`
//! 先检查 `AOE` 与 `nFAULT`，再置零调制量、拉高 `nSLEEP`、等 1 ms、开通道、置 `MOE`。
//! 2026-11-27 网表**没有 TIM1_BKIN 网络**：`BKE = 0`，硬件过压 break 不存在（HR-401 缺口）；
//! `AOE = 0` 仍保留，防止未来接回 BKIN 后自动恢复。

use contracts::joint::config_budget::SYS_CLK_HZ;
use contracts::joint::protocol_wire::modulation_to_duty_norm;
use contracts::motor::config_hw::{PWM_ARR, PWM_DEADTIME_NS};
use contracts::{ClampedModulation, Duty};
use embassy_stm32::gpio::{Input, Level, Output, OutputType, Pull, Speed};
use embassy_stm32::pac::timer::vals::{Cms, Dir, Ocm, Ossi, Ossr};
use embassy_stm32::pac::{RCC, TIM1 as TIM1_REGS};
use embassy_stm32::peripherals::{PA10, PA8, PA9, PB13, PB14, PB15, PC7, PC8, TIM1};
use embassy_stm32::timer::complementary_pwm::ComplementaryPwmPin;
use embassy_stm32::timer::simple_pwm::{Ch1, Ch2, Ch3, PwmPin};

/// TIM1 六路栅极、`nSLEEP`、`nFAULT` 与 `BKIN` 引脚。
pub struct PwmPins {
    /// 高边 A（TIM1_CH1）。
    pub hs_a: PA8,
    /// 高边 B（TIM1_CH2）。
    pub hs_b: PA9,
    /// 高边 C（TIM1_CH3）。
    pub hs_c: PA10,
    /// 低边 A（TIM1_CH1N）。
    pub ls_a: PB13,
    /// 低边 B（TIM1_CH2N）。
    pub ls_b: PB14,
    /// 低边 C（TIM1_CH3N）。
    pub ls_c: PB15,
    /// 驱动使能，高有效；2026-11-27 网表 U1.39 / PC7。
    pub n_sleep: PC7,
    /// 驱动故障，开漏低有效；2026-11-27 网表 U1.40 / PC8。
    pub n_fault: PC8,
}

/// 使能失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwmFault {
    /// `BDTR.AOE` 被置位：break 后会自动恢复出力，禁止使能。
    AoeSet,
    /// `nFAULT` 为低：驱动已报故障。
    NfaultLow,
}

/// 只读回读，供 bring-up 日志与 HIL 记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PwmState {
    /// 自动重装值（中心对齐半周期计数）。
    pub arr: u16,
    /// 死区发生器 DTG。
    pub dtg: u8,
    /// 主输出使能。
    pub moe: bool,
    /// 自动输出使能（必须为 `false`）。
    pub aoe: bool,
    /// 硬件 break 输入使能（本板无 BKIN，必须为 `false`）。
    pub bke: bool,
    /// `nFAULT` 是否拉低。
    pub nfault: bool,
    /// `nSLEEP` 是否拉高。
    pub n_sleep: bool,
    /// `CCER` 原样（bit0/2/4 = CCxE，bit1/3/5 = CCxNE）。
    pub ccer: u32,
    /// 六路全关：`MOE=0` 且三对 `CCxE/CCxNE` 全 0。
    pub outputs_off: bool,
}

/// TIM1 与六路栅极输入的所有者。
pub struct Pwm {
    _tim: TIM1,
    _ch1: PwmPin<'static, TIM1, Ch1>,
    _ch1n: ComplementaryPwmPin<'static, TIM1, Ch1>,
    _ch2: PwmPin<'static, TIM1, Ch2>,
    _ch2n: ComplementaryPwmPin<'static, TIM1, Ch2>,
    _ch3: PwmPin<'static, TIM1, Ch3>,
    _ch3n: ComplementaryPwmPin<'static, TIM1, Ch3>,
    n_sleep: Output<'static, PC7>,
    n_fault: Input<'static, PC8>,
}

impl Pwm {
    /// 配置 TIM1：20 kHz 中心对齐、PWM1、500 ns 死区、`AOE = 0`、输出关闭。
    ///
    /// 结束后 `nSLEEP` 低、`MOE = 0`，六路按 `CR2.OISx` 为低（`OSSI/OSSR = idle`）。
    pub fn init(tim: TIM1, pins: PwmPins) -> Self {
        let ch1: PwmPin<'static, TIM1, Ch1> = PwmPin::new_ch1(pins.hs_a, OutputType::PushPull);
        let ch1n: ComplementaryPwmPin<'static, TIM1, Ch1> =
            ComplementaryPwmPin::new_ch1(pins.ls_a, OutputType::PushPull);
        let ch2: PwmPin<'static, TIM1, Ch2> = PwmPin::new_ch2(pins.hs_b, OutputType::PushPull);
        let ch2n: ComplementaryPwmPin<'static, TIM1, Ch2> =
            ComplementaryPwmPin::new_ch2(pins.ls_b, OutputType::PushPull);
        let ch3: PwmPin<'static, TIM1, Ch3> = PwmPin::new_ch3(pins.hs_c, OutputType::PushPull);
        let ch3n: ComplementaryPwmPin<'static, TIM1, Ch3> =
            ComplementaryPwmPin::new_ch3(pins.ls_c, OutputType::PushPull);

        let n_sleep = Output::new(pins.n_sleep, Level::Low, Speed::Low);
        let n_fault = Input::new(pins.n_fault, Pull::Up);

        // 2026-11-27 网表没有 TIM1_BKIN；不要配置 PB10 AF，也不要开 BKE。
        RCC.apb2enr().modify(|w| w.set_tim1en(true));

        let r = TIM1_REGS;
        r.cr1().modify(|w| {
            w.set_cen(false);
            w.set_cms(Cms::CENTERALIGNED1);
            w.set_dir(Dir::UP);
            w.set_arpe(true);
        });
        r.psc().write(|w| w.set_psc(0));
        r.arr().write(|w| w.set_arr(PWM_ARR));
        r.rcr().write(|w| w.set_rep(1));

        r.ccmr_output(0).modify(|w| {
            w.set_ocm(0, Ocm::PWMMODE1);
            w.set_ocpe(0, true);
            w.set_ocm(1, Ocm::PWMMODE1);
            w.set_ocpe(1, true);
        });
        r.ccmr_output(1).modify(|w| {
            w.set_ocm(0, Ocm::PWMMODE1);
            w.set_ocpe(0, true);
        });

        r.ccer().modify(|w| {
            for n in 0..3 {
                w.set_cce(n, false);
                w.set_ccne(n, false);
                w.set_ccp(n, false);
                w.set_ccnp(n, false);
            }
        });
        r.cr2().modify(|w| {
            for n in 0..3 {
                w.set_ois(n, false);
            }
            w.set_ois1n(false);
            w.set_ois2n(false);
            w.set_ois3n(false);
            // MMS2=0010: update event drives TIM1_TRGO2, the injected-group
            // trigger for ADC1/2/3 (RM0440 §29.3.31, CR2.MMS2).
            let mut bits = w.0;
            bits &= !(0xF << 20);
            bits |= 0b0010 << 20;
            w.0 = bits;
        });
        r.bdtr().modify(|w| {
            w.set_dtg(deadtime_dtg());
            w.set_aoe(false);
            w.set_moe(false);
            w.set_bke(false);
            w.set_ossi(Ossi::IDLELEVEL);
            w.set_ossr(Ossr::IDLELEVEL);
        });
        r.egr().write(|w| w.set_ug(true));
        // 20 kHz 电流环由 TIM1 更新事件驱动；清掉 UG 产生的 pending。
        r.sr().modify(|w| w.set_uif(false));
        r.dier().modify(|w| w.set_uie(true));
        r.cr1().modify(|w| w.set_cen(true));

        Self {
            _tim: tim,
            _ch1: ch1,
            _ch1n: ch1n,
            _ch2: ch2,
            _ch2n: ch2n,
            _ch3: ch3,
            _ch3n: ch3n,
            n_sleep,
            n_fault,
        }
    }

    /// 设置三相输出；`Duty::Coast` 等价于六路全低。
    pub fn set_duty(&mut self, duty: Duty) {
        match duty {
            Duty::Coast => self.coast(),
            Duty::Pwm { a, b, c } => {
                let r = TIM1_REGS;
                r.ccr(0).write(|w| w.set_ccr(ccr_from_modulation(a)));
                r.ccr(1).write(|w| w.set_ccr(ccr_from_modulation(b)));
                r.ccr(2).write(|w| w.set_ccr(ccr_from_modulation(c)));
            }
        }
    }

    /// 只唤醒驱动（nSLEEP 高），保持 MOE=0、六路关闭；用于电流零位/采样链自检。
    ///
    /// # Errors
    ///
    /// 唤醒后 `nFAULT` 仍为低：立刻把 `nSLEEP` 拉回低并返回错误，不继续采样。
    pub fn wake(&mut self) -> Result<(), PwmFault> {
        self.n_sleep.set_high();
        // nSLEEP 的 1 ms 是 DS 的最小值，实测 nFAULT 会晚于 1 ms 才释放；
        // 手动复位时 5V/驱动状态可能更慢，最多等 100 ms。仍低才判故障，
        // 避免把正常的驱动唤醒当成 nFAULT。
        for _ in 0..10 {
            // ~10 ms per iteration at the observed 3-cycle delay loop.
            cortex_m::asm::delay(SYS_CLK_HZ / 300);
            if !self.n_fault.is_low() {
                return Ok(());
            }
        }
        self.n_sleep.set_low();
        Err(PwmFault::NfaultLow)
    }

    /// 六路全低，三相高阻。
    pub fn coast(&mut self) {
        self.disable();
    }

    /// 置 MOE；只允许在自检通过或故障清除后调用。
    ///
    /// # Errors
    ///
    /// `AOE` 读回为真，或 `nFAULT` 为低时返回错误并保持输出关闭。
    pub fn enable(&mut self) -> Result<(), PwmFault> {
        let r = TIM1_REGS;
        if r.bdtr().read().aoe() {
            return Err(PwmFault::AoeSet);
        }
        if self.n_fault.is_low() {
            return Err(PwmFault::NfaultLow);
        }

        // 先在输出关闭时写入 50 % 零矢量，再开通道与 MOE。
        self.set_duty(Duty::zero_modulation());
        self.n_sleep.set_high();
        cortex_m::asm::delay(SYS_CLK_HZ / 1_000);
        r.ccer().modify(|w| {
            for n in 0..3 {
                w.set_cce(n, true);
                w.set_ccne(n, true);
            }
        });
        r.bdtr().modify(|w| w.set_moe(true));

        if r.bdtr().read().aoe() {
            self.disable();
            return Err(PwmFault::AoeSet);
        }
        Ok(())
    }

    /// 清 MOE、关通道、拉低 `nSLEEP`；`AOE = 0` 保证不会自动恢复。
    pub fn disable(&mut self) {
        let r = TIM1_REGS;
        r.bdtr().modify(|w| w.set_moe(false));
        r.ccer().modify(|w| {
            for n in 0..3 {
                w.set_cce(n, false);
                w.set_ccne(n, false);
            }
        });
        self.n_sleep.set_low();
    }

    /// 当前寄存器回读。
    pub fn state(&self) -> PwmState {
        let r = TIM1_REGS;
        let bdtr = r.bdtr().read();
        PwmState {
            arr: r.arr().read().arr(),
            dtg: bdtr.dtg(),
            moe: bdtr.moe(),
            aoe: bdtr.aoe(),
            bke: bdtr.bke(),
            nfault: self.n_fault.is_low(),
            n_sleep: self.n_sleep.is_set_high(),
            ccer: r.ccer().read().0,
            outputs_off: !bdtr.moe() && r.ccer().read().0 & 0x3F == 0,
        }
    }
}

/// `Duty` 调制量 → 中心对齐比较值 `CCR = (m + 1) / 2 · ARR`。
fn ccr_from_modulation(m: ClampedModulation) -> u16 {
    let duty = modulation_to_duty_norm(m);
    let raw = duty * f32::from(PWM_ARR) + 0.5;
    let max = f32::from(PWM_ARR);
    if raw <= 0.0 {
        0
    } else if raw >= max {
        PWM_ARR
    } else {
        raw as u16
    }
}

/// `PWM_DEADTIME_NS` → BDTR.DTG（t_dts = 1 / SYS_CLK，DTG[7:5] = 0xx 区间）。
const fn deadtime_dtg() -> u8 {
    let ticks = (PWM_DEADTIME_NS as u64 * SYS_CLK_HZ as u64) / 1_000_000_000;
    assert!(ticks <= 127, "deadtime exceeds 7-bit DTG range");
    ticks as u8
}
