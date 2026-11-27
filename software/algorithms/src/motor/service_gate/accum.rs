//! 正规方程累加器与两级一致性 χ²。
//!
//! 模型 `y = φᵀθ`：`φ = [s_b, s_c]`、`y = -s_a`、`θ = [γ_b, γ_c]`；全部量是减零位后的
//! ADC LSB（码），闸门内部不做安培换算。共模增益不可观测，因此未知量只有两个相对增益。
//!
//! 主累加器、组窗与半窗共用 [`Regressor::push`]：同一套正规方程与残差统计只有一份
//! 实现，遗忘因子与权重上限由参数决定，而不是各写一遍。

use core::ops::{Deref, DerefMut};

use super::math::DET_EPS;
use crate::motor::config_policy as policy;
use crate::motor::service_math::{sqrt_f32, SQRT3};

/// 正规方程的共享状态。`GainAccum` / `GroupWindow` / `HalfWindow` 都只包这一份。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Regressor {
    /// 信息矩阵 `S = Σ w·φφᵀ`，行主序 `[s00, s01, s10, s11]`。
    pub(crate) s: [f32; 4],
    /// 正规方程右端 `b = Σ w·φ·y`。
    pub(crate) b: [f32; 2],
    /// `A0 = Σ(s_a + s_b + s_c)`：z 门分子，对共模偏移敏感。
    pub(crate) a0: f32,
    /// `A1 = Σ(s_a² + s_b² + s_c²)`：z 门分母。
    pub(crate) a1: f32,
    /// 窗口拍数（主累加器里是最近一次并入的组窗拍数）。
    pub(crate) groups: u32,
    /// 权重和 `Σw`。
    pub(crate) weight: f32,
    /// 残差平方和 `Σe²`。
    pub(crate) sse: f32,
    /// `Σi_α`（Clarke 幅值不变变换，`i_α = s_a`）。
    pub(crate) rsum: [f32; 2],
    /// `Σ(i_α² + i_β²)`，方向集中度门的归一化分母。
    pub(crate) ri2: f32,
    /// 方向集中度门的样本数。
    pub(crate) rn: u32,
}

impl Regressor {
    pub(crate) const fn empty() -> Self {
        Self {
            s: [0.0; 4],
            b: [0.0; 2],
            a0: 0.0,
            a1: 0.0,
            groups: 0,
            weight: 0.0,
            sse: 0.0,
            rsum: [0.0; 2],
            ri2: 0.0,
            rn: 0,
        }
    }

    /// 累加一个样本（已减零位，单位 LSB）。
    ///
    /// 信息形式：`S ← λS + w·φφᵀ`、`b ← λb + w·φ·y`、`w ← λw + w`。组窗与半窗传
    /// `lambda = 1.0`、`weight_cap = None`；主累加器不再直接吃样本，只通过
    /// `absorb_group` 并入通过 Commit Gate 的组窗。
    pub(crate) fn push(
        &mut self,
        s_lsb: [f32; 3],
        weight: f32,
        theta: [f32; 2],
        lambda: f32,
        weight_cap: Option<f32>,
    ) {
        if !weight.is_finite() || weight <= 0.0 {
            return;
        }
        let [sa, sb, sc] = s_lsb;

        let [s00, s01, s10, s11] = self.s;
        self.s = [
            lambda * s00 + weight * sb * sb,
            lambda * s01 + weight * sb * sc,
            lambda * s10 + weight * sc * sb,
            lambda * s11 + weight * sc * sc,
        ];

        let y = -sa;
        let [b0, b1] = self.b;
        self.b = [lambda * b0 + weight * sb * y, lambda * b1 + weight * sc * y];

        let cap = weight_cap.unwrap_or(f32::INFINITY);
        let next_weight = lambda * self.weight + weight;
        self.weight = if next_weight >= cap { cap } else { next_weight };

        self.a0 += sa + sb + sc;
        self.a1 += sa * sa + sb * sb + sc * sc;
        self.groups = self.groups.saturating_add(1);

        let i_alpha = sa;
        let i_beta = (sa + 2.0 * sb) / SQRT3;
        let [r0, r1] = self.rsum;
        self.rsum = [r0 + i_alpha, r1 + i_beta];
        self.ri2 += i_alpha * i_alpha + i_beta * i_beta;
        self.rn = self.rn.saturating_add(1);

        let [t0, t1] = theta;
        let residual = y - (sb * t0 + sc * t1);
        self.sse = lambda * self.sse + residual * residual;
    }

    /// 解 2×2 正规方程，返回 `(g_b, g_c)`；`det(S)` 不足或非有限时 `None`。
    pub(crate) fn solve(&self) -> Option<(f32, f32)> {
        solve_2x2(self.s, self.b)
    }

    /// 有效样本数 `N_eq = floor((1+λ)Σw)`，饱和到 `N_EQ_MAX`。
    pub(crate) fn n_eq(&self) -> u32 {
        if !self.weight.is_finite() || self.weight <= 0.0 {
            return 0;
        }
        let n = (1.0 + policy::LAMBDA) * self.weight;
        if !n.is_finite() || n <= 0.0 {
            0
        } else {
            (n as u32).min(policy::N_EQ_MAX)
        }
    }

    // 方向集中度平方 `R²`；无效时返回 0，随后会被 `D_MIN` 门拦下。
    pub(crate) fn direction_r2(&self) -> f32 {
        let [r0, r1] = self.rsum;
        let n = self.rn as f32;
        if n <= 0.0 || self.ri2 <= 0.0 || !self.ri2.is_finite() {
            return 0.0;
        }
        let r2 = (r0 * r0 + r1 * r1) / (n * self.ri2);
        if r2.is_finite() {
            r2
        } else {
            0.0
        }
    }

    // DC 偏置平方 `z² = 3·A0² / (G·A1)`；无激励时返回 0。
    pub(crate) fn bias_z2(&self) -> f32 {
        let g = self.groups as f32;
        if g <= 0.0 || self.a1 <= 0.0 || !self.a1.is_finite() {
            return 0.0;
        }
        let z2 = 3.0 * self.a0 * self.a0 / (g * self.a1);
        if z2.is_finite() {
            z2
        } else {
            0.0
        }
    }

    // 信息矩阵规整度 `d = det(S) / tr²(S)`。
    pub(crate) fn info_d(&self) -> f32 {
        let [s00, s01, s10, s11] = self.s;
        let det = s00 * s11 - s01 * s10;
        let tr = s00 + s11;
        if tr <= 0.0 || !tr.is_finite() || !det.is_finite() {
            return 0.0;
        }
        let d = det / (tr * tr);
        if d.is_finite() {
            d.max(0.0)
        } else {
            0.0
        }
    }

    // 残差方差 `σ_res² = sse/(weight-2) = 3σ_ch²`，单位 LSB²；给 χ² 与突变门用。
    pub(crate) fn sigma_res2_lsb2(&self) -> Option<f32> {
        let dof = self.weight - 2.0;
        if dof <= 0.0 || !dof.is_finite() || !self.sse.is_finite() {
            return None;
        }
        let v = self.sse / dof;
        if v > 0.0 && v.is_finite() {
            Some(v)
        } else {
            None
        }
    }

    // 单通道残差标准差（1σ、单位 LSB）。残差 `e = -ε_a - γ_b ε_b - γ_c ε_c`，
    // 三路独立时 `Var(e) ≈ 3σ²`，故 `σ_ch = √(sse/(3(weight-2)))`。
    pub(crate) fn sigma_ch_std_lsb(&self) -> Option<f32> {
        let dof = self.weight - 2.0;
        let denom = dof * 3.0;
        if dof <= 0.0 || denom <= 0.0 || !denom.is_finite() || !self.sse.is_finite() {
            return None;
        }
        Some(sqrt_f32(self.sse / denom))
    }

    // 三相等权均方根 `s_rms = √(A1/(3G))`，单位 LSB（按相）。平衡旋转时等于相 RMS
    //（幅值/√2），单相激励时等于幅值/√3。
    pub(crate) fn s_rms_lsb(&self) -> Option<f32> {
        let g = self.groups as f32;
        if g <= 0.0 || !self.a1.is_finite() {
            return None;
        }
        let ms = self.a1 / (3.0 * g);
        if ms <= 0.0 || !ms.is_finite() {
            return None;
        }
        Some(sqrt_f32(ms))
    }

    // 预测精度界（单位 pp）；`N_eq` 不足或 `s_rms` 为 0 时 `None`。
    pub(crate) fn pred_pp(&self, do2: f32) -> Option<f32> {
        let n_eq = self.n_eq();
        if n_eq == 0 {
            return None;
        }
        let sigma_ch_std_lsb = self.sigma_ch_std_lsb()?;
        let s_rms = self.s_rms_lsb()?;
        let n = n_eq as f32;
        let term1 = SQRT3 * sigma_ch_std_lsb / (s_rms * sqrt_f32(n));
        let term2 = (sigma_ch_std_lsb * sigma_ch_std_lsb + do2) / (s_rms * s_rms);
        let pred = 100.0 * (term1 + term2);
        if pred.is_finite() {
            Some(pred)
        } else {
            None
        }
    }
}

/// 相对增益正规方程累加器。
///
/// 未知量固定为 `x = [g_b, g_c]`（参考相 `g_a ≡ 1`）；共模增益不可观测，因此没有
/// "平均增益"字段。所有回归量都是减零位后的 ADC LSB，不是安培。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainAccum {
    pub(crate) reg: Regressor,
    /// 零位残余不确定度 `δo²`，单位 LSB²，`pred` 第二项。
    pub(crate) do2: f32,
    /// 当前相对增益估计 `[γ_b, γ_c]`，初值 `[1, 1]`。
    pub(crate) theta: [f32; 2],
}

impl Deref for GainAccum {
    type Target = Regressor;

    fn deref(&self) -> &Self::Target {
        &self.reg
    }
}

impl DerefMut for GainAccum {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.reg
    }
}

impl GainAccum {
    /// 纯构造：全零，相对增益按名义值 1。
    pub const fn empty() -> Self {
        Self {
            reg: Regressor::empty(),
            do2: 0.0,
            theta: [1.0, 1.0],
        }
    }

    /// 清空累加器（换工况/换模式/提交后）。
    ///
    /// `do2` 是零位标定的属性，不是窗内统计量：清窗不能把零位不确定度重置成 0。
    pub fn reset(&mut self) {
        let do2 = self.do2;
        *self = Self::empty();
        self.do2 = do2;
    }

    /// 预测精度界，单位 pp。
    pub fn pred_pp(&self) -> Option<f32> {
        self.reg.pred_pp(self.do2)
    }

    /// 有效样本数 `N_eq`。
    pub fn n_eq(&self) -> u32 {
        self.reg.n_eq()
    }

    // 把通过 Commit Gate 的组窗并入主累加器：组间按 `G` 拍遗忘一次，组内无遗忘。
    // 结构门的 a0/a1/R 快照取本组，`pred` 的 `s_rms` 与 `z²` 都定义在完整组窗上。
    pub(super) fn absorb_group(&mut self, group: &Regressor, decay: f32) {
        let decay = if decay.is_finite() && decay > 0.0 {
            decay
        } else {
            1.0
        };

        let [s00, s01, s10, s11] = self.reg.s;
        let [g00, g01, g10, g11] = group.s;
        self.reg.s = [
            decay * s00 + g00,
            decay * s01 + g01,
            decay * s10 + g10,
            decay * s11 + g11,
        ];

        let [b0, b1] = self.reg.b;
        let [gb0, gb1] = group.b;
        self.reg.b = [decay * b0 + gb0, decay * b1 + gb1];

        self.reg.sse = decay * self.reg.sse + group.sse;

        let cap = policy::WINDOW_SAMPLES as f32;
        let next_weight = decay * self.reg.weight + group.weight;
        self.reg.weight = if next_weight >= cap { cap } else { next_weight };

        self.reg.a0 = group.a0;
        self.reg.a1 = group.a1;
        self.reg.rsum = group.rsum;
        self.reg.ri2 = group.ri2;
        self.reg.rn = group.rn;
        self.reg.groups = group.groups;
    }
}

/// 半组窗（χ² 用）：普通求和，不带 λ 遗忘。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct HalfWindow {
    pub(super) reg: Regressor,
}

impl HalfWindow {
    pub(super) const fn empty() -> Self {
        Self {
            reg: Regressor::empty(),
        }
    }

    pub(super) fn push(&mut self, s_lsb: [f32; 3], weight: f32) {
        self.reg.push(s_lsb, weight, [1.0, 1.0], 1.0, None);
    }

    pub(super) fn solve(&self) -> Option<(f32, f32)> {
        self.reg.solve()
    }

    pub(super) fn inv(&self) -> Option<[f32; 4]> {
        invert_2x2(self.reg.s)
    }
}

/// 当前组窗：`G` 拍独立累加器，**不带遗忘因子**，与主 [`GainAccum`] 完全隔离。
///
/// 组内只累加、不发布。组末 Commit Gate 全过才 `absorb_group` 进主累加器；任一检查
/// 失败整组丢弃，历史累加器一个数都不改。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GroupWindow {
    /// 无遗忘的正规方程与结构门统计。
    pub(super) accum: Regressor,
    /// 组内前半窗。
    pub(super) half_a: HalfWindow,
    /// 组内后半窗。
    pub(super) half_b: HalfWindow,
    /// `Σr`：组内模型残差之和，组末减均值消掉常量 DC。
    pub(super) change_sum: f32,
    /// `Σr²`：组内模型残差平方和；减去 `(Σr)²/n` 后才是突变检测方差。
    pub(super) change_sse: f32,
    /// 最新一拍的 `r²`，单拍门用。
    pub(super) last_change_sq: f32,
}

impl GroupWindow {
    pub(super) const fn empty() -> Self {
        Self {
            accum: Regressor::empty(),
            half_a: HalfWindow::empty(),
            half_b: HalfWindow::empty(),
            change_sum: 0.0,
            change_sse: 0.0,
            last_change_sq: 0.0,
        }
    }

    /// 组内累加一拍。`theta` 是当前生效的 `active_theta`，组内冻结不变。
    pub(super) fn push(&mut self, s_lsb: [f32; 3], theta: [f32; 2], group_samples: u32) {
        let [sa, sb, sc] = s_lsb;
        let [t0, t1] = theta;
        self.accum.push(s_lsb, 1.0, theta, 1.0, None);

        // Regressor 已经累加 sse；这里再算一次同一残差，供组内突变门使用。
        let residual = -sa - (sb * t0 + sc * t1);
        let residual_sq = residual * residual;
        self.change_sum += residual;
        self.change_sse += residual_sq;
        self.last_change_sq = residual_sq;

        let n = self.accum.groups;
        let half = group_samples / 2;
        if n <= half {
            self.half_a.push(s_lsb, 1.0);
        } else {
            self.half_b.push(s_lsb, 1.0);
        }
    }
}

// 2×2 解与逆共用同一行列式判据，避免主累加器、组窗与半窗各写一份。
fn solve_2x2(s: [f32; 4], b: [f32; 2]) -> Option<(f32, f32)> {
    let [s00, s01, s10, s11] = s;
    let det = s00 * s11 - s01 * s10;
    if !det.is_finite() || det.abs() < DET_EPS {
        return None;
    }
    let [b0, b1] = b;
    let x0 = (s11 * b0 - s01 * b1) / det;
    let x1 = (s00 * b1 - s10 * b0) / det;
    if x0.is_finite() && x1.is_finite() {
        Some((x0, x1))
    } else {
        None
    }
}

pub(super) fn invert_2x2(s: [f32; 4]) -> Option<[f32; 4]> {
    let [s00, s01, s10, s11] = s;
    let det = s00 * s11 - s01 * s10;
    if !det.is_finite() || det.abs() < DET_EPS {
        return None;
    }
    let inverse = [s11 / det, -s01 / det, -s10 / det, s00 / det];
    if inverse.iter().all(|v| v.is_finite()) {
        Some(inverse)
    } else {
        None
    }
}

// χ²：两个 θ 估计之差与各自协方差，`(θ1-θ2)ᵀ (Σ1+Σ2)⁻¹ (θ1-θ2)`，
// `Σi = Si⁻¹·σ_res²`。θ 的协方差用残差方差，不是单通道方差。
pub(super) fn chi2_estimates(
    t1: [f32; 2],
    inv1: [f32; 4],
    t2: [f32; 2],
    inv2: [f32; 4],
    sigma2: f32,
) -> Option<f32> {
    if sigma2 <= 0.0 || !sigma2.is_finite() {
        return None;
    }
    let [a00, a01, a10, a11] = inv1;
    let [b00, b01, b10, b11] = inv2;

    let m00 = a00 + b00;
    let m01 = a01 + b01;
    let m10 = a10 + b10;
    let m11 = a11 + b11;
    let det = m00 * m11 - m01 * m10;
    let scale = (m00 * m11).abs() + (m01 * m10).abs();
    if !det.is_finite() || scale <= 0.0 || !scale.is_finite() || det.abs() < 1e-12 * scale {
        return None;
    }

    let d0 = t1[0] - t2[0];
    let d1 = t1[1] - t2[1];
    let q0 = (m11 * d0 - m01 * d1) / det;
    let q1 = (m00 * d1 - m10 * d0) / det;
    let stat = (d0 * q0 + d1 * q1) / sigma2;
    if stat.is_finite() {
        Some(stat)
    } else {
        None
    }
}
