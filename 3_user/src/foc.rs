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

use core::f32::consts::{FRAC_1_PI, FRAC_PI_2};

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
pub fn clarke(currents: PhaseCurrent) -> AlphaBeta {
    AlphaBeta {
        alpha_amps: currents.a_amps,
        beta_amps: FRAC_1_SQRT3 * (currents.a_amps + 2.0 * currents.b_amps),
    }
}

/// Park 变换（源实现 Eq 8/9）：`d = α·cos + β·sin`，`q = β·cos − α·sin`。
pub fn park(currents: AlphaBeta, theta_rad: f32) -> Dq {
    let (sin, cos) = sin_cos(theta_rad);
    Dq {
        d_amps: cos * currents.alpha_amps + sin * currents.beta_amps,
        q_amps: cos * currents.beta_amps - sin * currents.alpha_amps,
    }
}

/// 逆 Park 变换（源实现 Eq 10/11）：`α = d·cos − q·sin`，`β = d·sin + q·cos`。
pub fn inverse_park(d_v: f32, q_v: f32, theta_rad: f32) -> AlphaBetaVolts {
    let (sin, cos) = sin_cos(theta_rad);
    AlphaBetaVolts {
        alpha_v: cos * d_v - sin * q_v,
        beta_v: sin * d_v + cos * q_v,
    }
}

/// 移植的抛物线近似：`|sin|` 与 `|cos|` 同时给出，最大绝对误差 0.002。
fn sin_cos(theta_rad: f32) -> (f32, f32) {
    (sin_f32(theta_rad), cos_f32(theta_rad))
}

/// `sin(x)` 由 `cos(x − π/2)` 得到（源 `sin.rs`）。
fn sin_f32(theta_rad: f32) -> f32 {
    cos_f32(theta_rad - FRAC_PI_2)
}

/// 抛物线近似的 `cos(x)`（源 `cos.rs`，`floor` 换成本地截断实现）。
fn cos_f32(x: f32) -> f32 {
    let mut value = x * (FRAC_1_PI / 2.0);
    value -= 0.25 + floor_f32(value + 0.25);
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
