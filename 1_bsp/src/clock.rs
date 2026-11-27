//! RCC/PWR 配置与读回校验。
//!
//! 目标：HSE 16 MHz → PLL 170 MHz；ADC 42.5 MHz。embassy 配时钟，本模块只读回
//! PLLN/PLLR/VOS/SWS；不一致进 fault dump，不静默继续（PRE-05）。

/// 已校验的时钟树句柄。
pub struct Clock;

impl Clock {
    /// 配置并返回时钟句柄；失败由调用方进 fault dump。
    pub fn configure() -> Self {
        todo!()
    }

    /// 读回校验：目标频率、PLL 源、VOS 与分频全部匹配才返回 `true`。
    pub fn verify(&self) -> bool {
        todo!()
    }
}
