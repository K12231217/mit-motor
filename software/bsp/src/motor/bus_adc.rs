//! ADC1/2/3 注入组 + TIM1_TRGO2 + DMA。
//!
//! 三路必须同一拍触发；[`Sample::window_ok`] 由采集端按低边采样窗口判定，调用方
//! 不再自己推断时效。

use contracts::RawSample3;

/// 三路注入组的所有者。
pub struct Adc;

/// 一拍样本：原始码、拍号/时刻与窗口判据。
pub struct Sample {
    /// 三路原始码、拍号与采样时刻。
    pub raw: RawSample3,
    /// 低边采样窗口是否满足 `SAMPLE_WINDOW_MIN_NS`。
    pub window_ok: bool,
}

impl Adc {
    /// 取本拍样本；注入组未完成或窗口不足时返回 `None`。
    pub fn take_sample(&mut self) -> Option<Sample> {
        todo!()
    }
}
