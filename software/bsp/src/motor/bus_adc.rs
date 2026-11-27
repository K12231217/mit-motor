//! ADC1/2/3 电流采样（T-013：注入组 + TIM1_TRGO2）。
//!
//! 当前板：`SOC→OPAMP1→ADC1_IN13`、`SOB→OPAMP2→ADC2_IN16`、
//! `SOA→OPAMP3→ADC3_IN13`（A/C 交叉）。本模块把三路 OPAMP 配成 follower，
//! 三路 ADC 各配 1 个注入通道，由 TIM1_TRGO2 的更新事件硬件同拍启动；
//! 电流任务在 TIM1_UP 中断里依次等待 3 个 JEOS，并从各自的 JDR1 读回。
//!
//! TIM1_TRGO2 的 JEXTSEL 值 0b01000：RM0440 Rev 9 表 167（ADC1/2）与表 169
//! （ADC3/4/5）均为 `tim1_trgo2`。三路转换由硬件触发，不再软件写 ADSTART。
//!
//! `window_ok` 由 TIM1 的实时输出状态判定：MOE=1、AOE=0，且三路低边
//! （CC1NE/CC2NE/CC3NE）都使能。中心对齐 + [`D_MAX`](contracts::motor::config_hw::D_MAX)
//! 保证更新事件落在低边共导窗口内。

use contracts::motor::config_hw::SOX_AMPS_PER_LSB;
use contracts::RawSample3;
use embassy_stm32::pac;
use embassy_stm32::pac::adc::vals::{Adcaldif, Difsel, Exten, Jexten, SampleTime};
use embassy_stm32::pac::gpio::vals::Moder;
use embassy_stm32::pac::opamp::vals::{Opahsm, Opaintoen, PgaGain, VmSel, VpSel};
use embassy_stm32::peripherals::{ADC1, ADC2, ADC3, OPAMP1, OPAMP2, OPAMP3, PA3, PA7, PB0};

use crate::motor::service_timebase::now_us;

/// 每相每 LSB 对应的安培数（`r_lsb * SOX_AMPS_PER_LSB`）。
pub const AMPS_PER_LSB: f32 = SOX_AMPS_PER_LSB;

/// 三路 OPAMP/ADC 的所有者。
pub struct Adc {
    _adc1: ADC1,
    _adc2: ADC2,
    _adc3: ADC3,
    _opamp1: OPAMP1,
    _opamp2: OPAMP2,
    _opamp3: OPAMP3,
    _pa3: PA3,
    _pa7: PA7,
    _pb0: PB0,
    tick: u32,
}

/// 一拍样本：原始码、拍号/时刻与窗口判据。
pub struct Sample {
    /// 三路原始码、拍号与采样时刻。
    pub raw: RawSample3,
    /// 本拍是否落在低边共导窗口：MOE=1、AOE=0、三路 CCxNE=1。
    pub window_ok: bool,
}

/// ADC/OPAMP/模拟输入引脚的所有权。
pub struct AdcPins {
    /// ADC1。
    pub adc1: ADC1,
    /// ADC2。
    pub adc2: ADC2,
    /// ADC3。
    pub adc3: ADC3,
    /// OPAMP1（SOC/PA3）。
    pub opamp1: OPAMP1,
    /// OPAMP2（SOB/PA7）。
    pub opamp2: OPAMP2,
    /// OPAMP3（SOA/PB0）。
    pub opamp3: OPAMP3,
    /// OPAMP1 输入。
    pub pa3: PA3,
    /// OPAMP2 输入。
    pub pa7: PA7,
    /// OPAMP3 输入。
    pub pb0: PB0,
}

impl Adc {
    /// 配置三路 OPAMP follower 与 ADC 注入组。
    ///
    /// 返回 `None` 表示 ADC 校准/使能超时，调用方必须进 fault dump，不得继续。
    pub fn init(pins: AdcPins) -> Option<Self> {
        pac::RCC.ahb2enr().modify(|w| {
            w.set_adc12en(true);
            w.set_adc345en(true);
        });

        // 三路输入脚只作模拟输入；OPAMP 输出走内部 ADC 通道。
        pac::GPIOA.moder().modify(|w| w.set_moder(3, Moder::ANALOG));
        pac::GPIOA.moder().modify(|w| w.set_moder(7, Moder::ANALOG));
        pac::GPIOB.moder().modify(|w| w.set_moder(0, Moder::ANALOG));

        // OPAMP1=SOC/PA3, OPAMP2=SOB/PA7, OPAMP3=SOA/PB0，全部 follower。
        configure_opamp(&pac::OPAMP1, VpSel::VINP1);
        configure_opamp(&pac::OPAMP2, VpSel::VINP0);
        configure_opamp(&pac::OPAMP3, VpSel::VINP0);

        // 内部输出通道：OPAMP1→ADC1_IN13, OPAMP2→ADC2_IN16, OPAMP3→ADC3_IN13。
        if !configure_adc(&pac::ADC1, 13)
            || !configure_adc(&pac::ADC2, 16)
            || !configure_adc(&pac::ADC3, 13)
        {
            return None;
        }

        Some(Self {
            _adc1: pins.adc1,
            _adc2: pins.adc2,
            _adc3: pins.adc3,
            _opamp1: pins.opamp1,
            _opamp2: pins.opamp2,
            _opamp3: pins.opamp3,
            _pa3: pins.pa3,
            _pa7: pins.pa7,
            _pb0: pins.pb0,
            tick: 0,
        })
    }

    /// 取本拍样本；任一 ADC 超时返回 `None`。
    ///
    /// 三路按 A/B/C 顺序打包：ADC3=SOA、ADC2=SOB、ADC1=SOC。
    ///
    /// 三路注入组由 TIM1_TRGO2 同一个更新事件触发；这里只等待并读回，
    /// 不软件启动转换。
    pub fn take_sample(&mut self) -> Option<Sample> {
        let window_ok = low_side_window();
        let c = wait_result(&pac::ADC1)?;
        let b = wait_result(&pac::ADC2)?;
        let a = wait_result(&pac::ADC3)?;

        self.tick = self.tick.wrapping_add(1);
        Some(Sample {
            raw: RawSample3 {
                tick: self.tick,
                t_us: now_us(),
                code: [a, b, c],
            },
            window_ok,
        })
    }
}

fn configure_opamp(opamp: &pac::opamp::Opamp, vp: VpSel) {
    opamp.csr().modify(|w| {
        w.set_opampen(true);
        w.set_vp_sel(vp);
        w.set_vm_sel(VmSel::OUTPUT);
        w.set_pga_gain(PgaGain::GAIN2);
        w.set_opahsm(Opahsm::HIGHSPEED);
        w.set_opaintoen(Opaintoen::ADCCHANNEL);
    });
}

fn configure_adc(adc: &pac::adc::Adc, channel: u8) -> bool {
    // 退出 deep power，开内部稳压器；10 µs 稳定时间。
    adc.cr().modify(|w| {
        w.set_deeppwd(false);
        w.set_advregen(true);
    });
    cortex_m::asm::delay(1_700);

    adc.difsel().modify(|w| {
        for n in 0..20 {
            w.set_difsel(n, Difsel::SINGLEENDED);
        }
    });

    adc.cr().modify(|w| {
        w.set_adcaldif(Adcaldif::SINGLEENDED);
        w.set_adcallin(true);
        w.set_adcal(true);
    });
    if !wait_until(|| !adc.cr().read().adcal(), 100_000) {
        return false;
    }

    // RES 必须在 ADEN=0 时写。G474 的 RES 是 CFGR[4:3]，00 = 12 bit；
    // PAC 的 adc_v4 `Res` 编码与 G4 不一致（TWELVEBIT 会写成 10 bit，raw 509）。
    adc.cr().modify(|w| w.set_aden(false));
    if !wait_until(|| !adc.cr().read().aden(), 100_000) {
        return false;
    }
    adc.cfgr().modify(|w| {
        // G474 RM0440: RES is CFGR[4:3], 00 = 12 bit. The PAC's adc_v4 `Res`
        // encoding is not the G4 layout (TWELVEBIT would write 10 bit here).
        w.0 &= !(0x3 << 3);
        w.set_cont(false);
        w.set_exten(Exten::DISABLED);
    });

    // 注入组只排 1 个通道；通道 10..19 的采样时间在 SMPR2（下标 1）。
    // 规则组保持禁用；慢通道扫描后续另配，不混进 20 kHz 电流采样。
    let smpr = if channel >= 10 { 1 } else { 0 };
    let smp_index = if channel >= 10 { channel - 10 } else { channel };
    adc.smpr(smpr)
        .modify(|w| w.set_smp(smp_index as usize, SampleTime::CYCLES64_5));

    adc.isr().write(|w| w.set_adrdy(true));
    adc.cr().modify(|w| w.set_aden(true));
    if !wait_until(|| adc.isr().read().adrdy(), 100_000) {
        return false;
    }
    adc.isr().write(|w| w.set_adrdy(true));

    // 注入组：JL=0（1 个转换），JEXTSEL=8（TIM1_TRGO2），JEXTEN=上升沿。
    // JRQ 只能在 JADSTART=0 时改；此时尚未启动。
    adc.jsqr().modify(|w| {
        w.set_jl(0);
        w.set_jextsel(8);
        w.set_jexten(Jexten::RISINGEDGE);
        w.set_jsq1(0, channel);
    });
    // RM0440 §21.4.15/§21.4.18：注入硬件触发必须置 JADSTART=1，否则
    // JEXTEN 的边沿被忽略。这里保持 JQDIS=1（队列关闭），JEXTEN=01
    // 仍是硬件触发，且 JSQR 不会被队列消费。
    adc.cfgr().modify(|w| w.set_jqdis(true));
    adc.cr().modify(|w| w.set_jadstart(true));

    // 清掉 ADC 使能期间可能残留的注入标志。
    adc.isr().write(|w| {
        w.set_jeoc(true);
        w.set_jeos(true);
        w.set_jqovf(true);
    });

    true
}

/// TIM1 当前是否处于低边共导窗口。
///
/// `D_MAX` 保证更新事件（TRGO2）距低边关断至少 5 µs；这里只判输出是否真的
/// 使能，防止在 `MOE=0` 或通道关闭时把样本送进 GainGate。
fn low_side_window() -> bool {
    let tim = pac::TIM1;
    let bdtr = tim.bdtr().read();
    let ccer = tim.ccer().read();
    bdtr.moe() && !bdtr.aoe() && ccer.ccne(0) && ccer.ccne(1) && ccer.ccne(2)
}

fn wait_result(adc: &pac::adc::Adc) -> Option<u16> {
    if !wait_until(|| adc.isr().read().jeos(), 4_000) {
        return None;
    }
    let value = adc.jdr(0).read().jdata() as u16;
    adc.isr().write(|w| w.set_jeos(true));
    Some(value)
}

fn wait_until(mut done: impl FnMut() -> bool, limit: u32) -> bool {
    for _ in 0..limit {
        if done() {
            return true;
        }
    }
    false
}
