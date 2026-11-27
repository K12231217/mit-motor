//! RCC/PWR 配置与读回校验。
//!
//! 目标：HSE 16 MHz → PLL 170 MHz；ADC 42.5 MHz。embassy 配时钟，本模块只读回
//! PLLN/PLLR/VOS/SWS；不一致进 fault dump，不静默继续（PRE-05）。

use embassy_stm32::pac::rcc::vals::{Pllsrc, Sw};
use embassy_stm32::pac::{PWR, RCC};
use embassy_stm32::rcc::{
    AHBPrescaler, APBPrescaler, AdcClockSource, ClockSrc, Pll, PllM, PllN, PllP, PllR, PllSource,
};
use embassy_stm32::time::Hertz;

use contracts::joint::config_budget::SYS_CLK_HZ;
use contracts::motor::config_hw::HSE_HZ;

// PLL 参数与预算必须同源：HSE / M × N / R。改任一处都先在这里编译失败。
const _: () = assert!(SYS_CLK_HZ == HSE_HZ / 4 * 85 / 2);

/// 170 MHz 的 embassy 时钟配置。
///
/// HSE 16 MHz / M4 = 4 MHz VCO 输入；× N85 = 340 MHz；R2 = 170 MHz。
/// PLLP = 340 / 8 = 42.5 MHz 供 ADC；PLLQ 关闭（板载 USB 已砍）。
pub fn config() -> embassy_stm32::Config {
    let rcc = embassy_stm32::rcc::Config {
        mux: ClockSrc::PLL,
        ahb_pre: AHBPrescaler::DIV1,
        apb1_pre: APBPrescaler::DIV1,
        apb2_pre: APBPrescaler::DIV1,
        pll: Some(Pll {
            source: PllSource::HSE(Hertz(HSE_HZ)),
            prediv_m: PllM::DIV4,
            mul_n: PllN::MUL85,
            div_p: Some(PllP::DIV8),
            div_q: None,
            div_r: Some(PllR::DIV2),
        }),
        clock_48mhz_src: None,
        adc12_clock_source: AdcClockSource::PLL1_P,
        adc345_clock_source: AdcClockSource::PLL1_P,
        ..Default::default()
    };

    let mut config = embassy_stm32::Config::default();
    config.rcc = rcc;
    config
}

/// 读回时钟树：PLL 源/倍频/分频、SWS、AHB/APB 分频与 boost 模式全部匹配才返回 `true`。
///
/// 调用方在 `embassy_stm32::init(config())` 之后调用；失败进 fault dump，不继续跑。
pub fn verify() -> bool {
    let cfgr = RCC.cfgr().read();
    let pll = RCC.pllcfgr().read();

    cfgr.sws() == Sw::PLL1_R
        && cfgr.hpre() == AHBPrescaler::DIV1
        && cfgr.ppre1() == APBPrescaler::DIV1
        && cfgr.ppre2() == APBPrescaler::DIV1
        && pll.pllsrc() == Pllsrc::HSE
        && pll.pllm() == PllM::DIV4
        && pll.plln() == PllN::MUL85
        && pll.pllr() == PllR::DIV2
        && pll.pllren()
        && RCC.cr().read().pllrdy()
        && !PWR.cr5().read().r1mode()
}
