//! 相电流归算：12 bit 原始码 → 安培。
//!
//! 结构移植自 `okhsunrog/oxifoc@3d48332` 的
//! `oxifoc-core/src/foc/current_sense.rs`（Apache-2.0）：把「码 → 伏 → 安」的除法
//! 折成一个 `amps_per_count`（源码注释：每拍三次 `vdiv` 约 180 周期，折一次即可），
//! 零位以**码**为单位保存，逐相减后缩放。
//!
//! 与源码的差异：MP6540HA 是器件内部电流检测（`SOx` 比例 1/9200 + 板级分压），
//! 没有外部分流电阻与运放增益，因此标度直接来自 `contracts::limits::hw`，
//! 不引入 `shunt_ohms`/`opamp_gain` 两个参数。

use contracts::limits::hw;
use contracts::types::PhaseCurrent;

/// 每相零位码与折算后的标度。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SenseParams {
    /// 三相零位码，顺序 A/B/C（C0 取平均后非整数）。
    pub offset_code: [f32; 3],
    /// 每个 ADC 码对应的相电流，单位 A/LSB。
    pub amps_per_count: f32,
}

impl SenseParams {
    /// 标定缺失时的复位值：`ADC_MIDSCALE_CODE` + HR-203 标度。
    pub const fn nominal() -> Self {
        Self {
            offset_code: [hw::ADC_MIDSCALE_CODE; 3],
            amps_per_count: hw::SOX_AMPS_PER_LSB,
        }
    }
}

impl Default for SenseParams {
    fn default() -> Self {
        Self::nominal()
    }
}

/// 一拍原始码 → 三相电流，单位 安培。
///
/// 只接收码数组，不接收 `RawSample3`：拍号与时效判据由采集端负责，归算只做线性换算。
/// 三相独立换算，不做零和修正：共模分量必须留给 `gate` 看见。正电流方向（`SOx` 极性）
/// 尚未上台架验证，见 `docs/INDEX.md` 的未决项。
pub fn currents(params: &SenseParams, code: [u16; 3]) -> PhaseCurrent {
    let [code_a, code_b, code_c] = code;
    let [offset_a, offset_b, offset_c] = params.offset_code;
    let scale = params.amps_per_count;

    PhaseCurrent {
        a_amps: (code_a as f32 - offset_a) * scale,
        b_amps: (code_b as f32 - offset_b) * scale,
        c_amps: (code_c as f32 - offset_c) * scale,
    }
}

#[cfg(test)]
mod tests;
