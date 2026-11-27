//! ADC1/2/3 电流采样（T-013 过渡实现）。
//!
//! 当前板：`SOC→OPAMP1→ADC1_IN13`、`SOB→OPAMP2→ADC2_IN16`、
//! `SOA→OPAMP3→ADC3_IN13`（A/C 交叉）。本模块把三路 OPAMP 配成 follower，
//! 在 TIM1 20 kHz 更新中断里**软件启动规则组**并同步读回三路。
//!
//! 这是真实 ADC 路径，但**不是最终低边窗口方案**：注入组 + TIM1_TRGO2 的硬件
//! 同拍触发尚未落地（HR-805）。`window_ok` 只表示转换完成，不表示落在低边
//! 采样窗口；20 kHz 闭环前必须替换。

use contracts::motor::config_hw::SOX_AMPS_PER_LSB;
use contracts::RawSample3;
use embassy_stm32::pac;
use embassy_stm32::pac::adc::vals::{Adcaldif, Difsel, Exten, SampleTime};
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
    /// 低边采样窗口是否满足 `SAMPLE_WINDOW_MIN_NS`。
    ///
    /// T-013 过渡实现只表示转换完成；最终注入组触发后改由硬件窗口判定。
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
    /// 配置三路 OPAMP follower 与 ADC 规则组。
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
    pub fn take_sample(&mut self) -> Option<Sample> {
        start(&pac::ADC1);
        start(&pac::ADC2);
        start(&pac::ADC3);

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
            window_ok: true,
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

    // 规则组只排 1 个通道；通道 10..19 的采样时间在 SMPR2（下标 1）。
    adc.sqr1().modify(|w| {
        w.set_l(0);
        w.set_sq(0, channel);
    });
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

    true
}

fn start(adc: &pac::adc::Adc) {
    adc.cr().modify(|w| w.set_adstart(true));
}

fn wait_result(adc: &pac::adc::Adc) -> Option<u16> {
    if !wait_until(|| adc.isr().read().eoc(), 4_000) {
        return None;
    }
    Some(adc.dr().read().rdata())
}

fn wait_until(mut done: impl FnMut() -> bool, limit: u32) -> bool {
    for _ in 0..limit {
        if done() {
            return true;
        }
    }
    false
}
