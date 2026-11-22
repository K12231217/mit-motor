//! FOC 坐标变换：三相 ↔ 两相静止 ↔ 两相旋转。
//!
//! 公式与 Microsemi 应用笔记 MSS《Park, Inverse Park and Clarke, Inverse Clarke
//! Transformations》的 1–11 式一致。结构移植自
//! `calebfletcher/foc@1db0260` 的 `src/park_clarke.rs`（Apache-2.0）：定点 `I16F16`
//! 换成 `f32`，并给每个量加单位后缀（PRE-06）。
//!
//! 幅值不变约定（Clarke 不乘 `2/3`）：平衡三相输入下 `|i_αβ|` 等于相峰值。
//! 正余弦移植自 `tarcieri/micromath@eb73416` 的 `src/float/{sin,cos}.rs`
//! （MIT OR Apache-2.0），最大绝对误差 0.002 rad；`core` 没有 `floor`，这里用
//! 截断加一步修正替代，误差量级不变。

use core::f32::consts::FRAC_1_PI;

use contracts::types::PhaseCurrent;

/// 1/√3，Clarke 变换与 `SVPWM` 扇区/作用时间共用。
pub(crate) const FRAC_1_SQRT3: f32 = 0.577_350_27;

/// 两相静止坐标系电流，单位 安培。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AlphaBeta {
    /// α 轴电流，安培。
    pub alpha_amps: f32,
    /// β 轴电流，安培。
    pub beta_amps: f32,
}

/// 旋转坐标系电流，单位 安培。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dq {
    /// d 轴电流，安培。
    pub d_amps: f32,
    /// q 轴电流，安培。
    pub q_amps: f32,
}

/// 两相静止坐标系电压，单位 伏特。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AlphaBetaVolts {
    /// α 轴电压，伏特。
    pub alpha_v: f32,
    /// β 轴电压，伏特。
    pub beta_v: f32,
}

/// 幅值不变 Clarke 变换（源实现 Eq 3/4）：`i_α = i_a`，`i_β = (i_a + 2·i_b)/√3`。
///
/// 依赖 `i_a + i_b + i_c = 0`。三相和不为零时 β 会带上共模，那属于采样异常，
/// 由 `gate` 判定，本函数不做补偿。
#[inline]
pub fn clarke(currents: PhaseCurrent) -> AlphaBeta {
    AlphaBeta {
        alpha_amps: currents.a_amps,
        beta_amps: FRAC_1_SQRT3 * (currents.a_amps + 2.0 * currents.b_amps),
    }
}

/// 同一拍角度的 sin/cos 缓存；Park 与逆 Park 共用，避免算两次多项式。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SinCos {
    /// `sin(theta)`。
    pub sin: f32,
    /// `cos(theta)`。
    pub cos: f32,
}

/// Park 变换（源实现 Eq 8/9）：`d = α·cos + β·sin`，`q = β·cos − α·sin`。
pub fn park(currents: AlphaBeta, theta_rad: f32) -> Dq {
    let (sin, cos) = sin_cos(theta_rad);
    park_with(SinCos { sin, cos }, currents)
}

/// 用已算好的 [`SinCos`] 做 Park；同一拍先算一次，Park/逆 Park 复用。
#[inline]
pub fn park_with(frame: SinCos, currents: AlphaBeta) -> Dq {
    Dq {
        d_amps: frame.cos * currents.alpha_amps + frame.sin * currents.beta_amps,
        q_amps: frame.cos * currents.beta_amps - frame.sin * currents.alpha_amps,
    }
}

/// 逆 Park 变换（源实现 Eq 10/11）：`α = d·cos − q·sin`，`β = d·sin + q·cos`。
pub fn inverse_park(d_v: f32, q_v: f32, theta_rad: f32) -> AlphaBetaVolts {
    let (sin, cos) = sin_cos(theta_rad);
    inverse_park_with(SinCos { sin, cos }, d_v, q_v)
}

/// 用已算好的 [`SinCos`] 做逆 Park；与 [`park_with`] 共用同一拍缓存。
#[inline]
pub fn inverse_park_with(frame: SinCos, d_v: f32, q_v: f32) -> AlphaBetaVolts {
    AlphaBetaVolts {
        alpha_v: frame.cos * d_v - frame.sin * q_v,
        beta_v: frame.sin * d_v + frame.cos * q_v,
    }
}

/// 移植的抛物线近似：`|sin|` 与 `|cos|` 同时给出，最大绝对误差 0.002。
///
/// 只做一次 `theta / 2π` 缩放，并且只做一次 `floor`：
/// `cos` 的归约是 `v − 0.25 − floor(v + 0.25)`，`sin` 是 `v − 0.5 − floor(v)`；
/// `floor(v + 0.25)` 由 `floor(v)` 与小数部分 `≥ 0.75` 直接得到。
#[inline]
pub fn sin_cos(theta_rad: f32) -> (f32, f32) {
    let v = theta_rad * (FRAC_1_PI / 2.0);
    let base = floor_f32(v);
    let cos_floor = if v - base >= 0.75 {
        base + 1.0
    } else {
        base
    };
    (
        cos_poly(v - 0.5 - base),
        cos_poly(v - 0.25 - cos_floor),
    )
}

/// 归一化相位上的抛物线近似（源 `cos.rs` 的多项式部分）。
#[inline]
fn cos_poly(mut value: f32) -> f32 {
    value *= 16.0 * (value.abs() - 0.5);
    value + 0.225 * value * (value.abs() - 1.0)
}

/// `core` 没有 `floor`：截断后若超过原值再退一格。角度经过 `cos` 的缩放后量级很小，
/// 不会触及 `i32` 的饱和边界。
fn floor_f32(x: f32) -> f32 {
    let truncated = x as i32 as f32;
    if truncated > x {
        truncated - 1.0
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests;
