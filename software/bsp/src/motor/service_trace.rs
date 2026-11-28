//! 示波器/逻辑分析仪遥测：TP_SYNC 与 DAC1 双通道。
//!
//! - `PB6`：`TP_SYNC_LOOP`，20 kHz 控制 ISR 进入时拉高、退出时拉低。只做 GPIO，
//!   不参与 UART（最新板不再把 PB6/PB7 当调试串口）。
//! - `PB7`：`TP_SYNC_FREEZE`，校准冻结期间为高，和 DAC1_OUT2 同相，供 LA 抓取。
//! - `PA4 / DAC1_OUT1`：残差 r_k 的模拟输出，0 A = 2048（约 1.25 V，VREF+ = 2.5 V），
//!   ±`full_scale_amps` 映射到 0…4095。
//! - `PA5 / DAC1_OUT2`：数字指示器，正常 0，冻结瞬间写 4095（约 2.49 V）。示波器用
//!   CH2 上升沿单次触发。
//!
//! 所有写操作都是 `GPIO->BSRR` / `DAC->DHR12R`，无锁、无分配、无日志；可在 20 kHz
//! ISR 里调用（PRE-02）。DAC 的 `set` 不做 `critical_section`。

use embassy_stm32::dac::{Dac, DacCh1, DacCh2, Value};
use embassy_stm32::dma::NoDma;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::peripherals::{DAC1, PA4, PA5, PB6, PB7};

/// 12 bit DAC 中点码（0 A）。
const DAC_MID_CODE: u16 = 0x0800;
/// 12 bit DAC 满量程码。
const DAC_FULL_SCALE_CODE: u16 = 0x0FFF;

/// 遥测引脚所有权。
pub struct TracePins {
    /// DAC1 外设。
    pub dac: DAC1,
    /// DAC1_OUT1 / TP1，残差模拟输出。
    pub dac_out1: PA4,
    /// DAC1_OUT2 / TP2，冻结数字指示。
    pub dac_out2: PA5,
    /// TP_SYNC_LOOP。
    pub sync_loop: PB6,
    /// TP_SYNC_FREEZE。
    pub sync_freeze: PB7,
}

/// 已初始化的遥测输出；只由 20 kHz 任务持有。
pub struct Trace {
    residual: DacCh1<'static, DAC1>,
    freeze_flag: DacCh2<'static, DAC1>,
    sync_loop: Output<'static, PB6>,
    sync_freeze: Output<'static, PB7>,
    residual_full_scale_amps: f32,
}

impl Trace {
    /// 初始化 DAC1 双通道和两根同步 GPIO；上电态：残差 0 A、冻结 0 V、两根同步低。
    ///
    /// `residual_full_scale_amps` 是 DAC 正/负满量程对应的残差幅值；0 A 始终在中点。
    pub fn init(pins: TracePins, residual_full_scale_amps: f32) -> Self {
        let dac: Dac<'static, DAC1> =
            Dac::new(pins.dac, NoDma, NoDma, pins.dac_out1, pins.dac_out2);
        let (mut residual, mut freeze_flag) = dac.split();
        // `Dac::new` 只开 RCC 引用计数；两个通道必须显式使能。
        residual.enable();
        freeze_flag.enable();
        residual.set(Value::Bit12Right(DAC_MID_CODE));
        freeze_flag.set(Value::Bit12Right(0));

        Self {
            residual,
            freeze_flag,
            sync_loop: Output::new(pins.sync_loop, Level::Low, Speed::VeryHigh),
            sync_freeze: Output::new(pins.sync_freeze, Level::Low, Speed::VeryHigh),
            residual_full_scale_amps,
        }
    }

    /// 写残差（安培）。非有限值按 0 A 处理；超出满量程饱和，不 panic。
    #[inline]
    pub fn set_residual_amps(&mut self, amps: f32) {
        let code = residual_to_code(amps, self.residual_full_scale_amps);
        self.residual.set(Value::Bit12Right(code));
    }

    /// 直接写 DAC1 原始码（12 bit，超出饱和）。只给台架自检用：HR-804 的
    /// DAC → 三路 ADC 斜坡验证需要指定码，而不是安培。
    pub fn set_dac1_code(&mut self, code: u16) {
        self.residual.set(Value::Bit12Right(code.min(4095)));
    }

    /// 20 kHz ISR 入口：TP_SYNC_LOOP 高。
    #[inline]
    pub fn sync_loop_high(&mut self) {
        self.sync_loop.set_high();
    }

    /// 20 kHz ISR 出口：TP_SYNC_LOOP 低。
    #[inline]
    pub fn sync_loop_low(&mut self) {
        self.sync_loop.set_low();
    }

    /// 校准冻结瞬间：DAC1_OUT2 拉满，TP_SYNC_FREEZE 拉高。
    #[inline]
    pub fn freeze(&mut self) {
        self.freeze_flag.set(Value::Bit12Right(DAC_FULL_SCALE_CODE));
        self.sync_freeze.set_high();
    }

    /// 冻结结束：DAC1_OUT2 归零，TP_SYNC_FREEZE 拉低。
    #[inline]
    pub fn unfreeze(&mut self) {
        self.sync_freeze.set_low();
        self.freeze_flag.set(Value::Bit12Right(0));
    }

    /// DAC 输出数据寄存器回读，供 HIL 证明写入生效。
    pub fn dac_codes(&self) -> (u16, u16) {
        (self.residual.read(), self.freeze_flag.read())
    }
}

/// 安培 → 12 bit DAC 码：0 A = 2048，±full_scale = 0/4095。
fn residual_to_code(amps: f32, full_scale_amps: f32) -> u16 {
    if !amps.is_finite() || !full_scale_amps.is_finite() || full_scale_amps <= 0.0 {
        return DAC_MID_CODE;
    }
    let mid = f32::from(DAC_MID_CODE);
    let full = f32::from(DAC_FULL_SCALE_CODE);
    let code = mid + amps * ((full - mid) / full_scale_amps);
    if code <= 0.0 {
        0
    } else if code >= full {
        DAC_FULL_SCALE_CODE
    } else {
        (code + 0.5) as u16
    }
}
