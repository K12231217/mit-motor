//! dq 电流环：PI + 交叉解耦 + 反电势前馈 + d 轴优先电压圆限幅。
//!
//! 输入是 Park 变换后的**未滤波** `i_dq`（[`crate::foc::park`]），输出是逆 Park 之前的
//! [`DqVolts`]。不碰寄存器、不读时钟、无全局状态；`dt` 与母线电压由调用方逐拍传入。
//!
//! 增益按 VESC `conf_general_calc_apply_foc_cc_kp_ki_gain()` 的口径整定：
//! `kp_d = Ld·BW`、`kp_q = Lq·BW`、`ki = R·BW`，`BW` 取 [`policy::CURRENT_BW_RAD_S`]。
//! 两个轴的电感不同，因此各用一个 [`Pi`]。
//!
//! # 边界
//!
//! 每次 [`CurrentLoop::current_loop`] 只在入口做一次有限性检查、在出口做一次电压圆
//! 限幅，并通过 [`Pi::back_calculate_integral`] 把积分器反算回该边界。热路径不接受
//! `NaN`/`±∞`：任一输入异常即清积分并返回 [`DqVolts::ZERO`]，由调用方按故障处理，
//! 不在中间层静默逐项归零。

use contracts::types::{DqTarget, DqVolts, MotorParams};

use crate::foc::Dq;
use crate::math::{sqrt_f32, SQRT3};
use crate::pid::Pi;
use crate::policy;

/// 一台电机的 dq 电流环状态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurrentLoop {
    params: MotorParams,
    pi_d: Pi,
    pi_q: Pi,
}

impl CurrentLoop {
    /// 用离线实测的电机参数与带宽构造。
    ///
    /// `bandwidth_rad_s` 必须有限且大于 0；参数不满足 [`MotorParams::is_valid`] 时返回
    /// `None`，由调用方进配置故障，不用默认值静默运行。
    pub fn new(params: MotorParams, bandwidth_rad_s: f32) -> Option<Self> {
        if !params.is_valid() || !bandwidth_rad_s.is_finite() || bandwidth_rad_s <= 0.0 {
            return None;
        }

        let ki = params.r_ohm * bandwidth_rad_s;
        Some(Self {
            params,
            pi_d: Pi::new(params.ld_h * bandwidth_rad_s, ki, f32::INFINITY),
            pi_q: Pi::new(params.lq_h * bandwidth_rad_s, ki, f32::INFINITY),
        })
    }

    /// 清零两个积分器。切模式、清故障、重新使能前必须调用。
    pub fn reset(&mut self) {
        self.pi_d.reset();
        self.pi_q.reset();
    }

    /// 带速接管：把积分器预置到“实测电压对应的 PI 输出”。
    ///
    /// `measured` 是重新使能前逆变器已经施加的 dq 电压（解耦与 BEMF 已包含在内）。
    /// 按 `pi = v_total ± 解耦项` 反推，再用当前 `i_dq`/`target` 反算积分器；这样下一拍
    /// 不会因为积分从 0 爬升而产生电流尖峰。输入非法时清积分并返回 `false`。
    pub fn preload_from_measured(
        &mut self,
        measured: DqVolts,
        i_dq: Dq,
        target: DqTarget,
        omega_e_rad_s: f32,
    ) -> bool {
        let valid = measured.d_v.is_finite()
            && measured.q_v.is_finite()
            && i_dq.d_amps.is_finite()
            && i_dq.q_amps.is_finite()
            && target.d_amps.is_finite()
            && target.q_amps.is_finite()
            && omega_e_rad_s.is_finite();
        if !valid {
            self.reset();
            return false;
        }

        let dec_vd = i_dq.q_amps * omega_e_rad_s * self.params.lq_h;
        let dec_vq = i_dq.d_amps * omega_e_rad_s * self.params.ld_h;
        let dec_bemf = omega_e_rad_s * self.params.flux_wb;

        let pi_d_v = measured.d_v + dec_vd;
        let pi_q_v = measured.q_v - dec_vq - dec_bemf;
        if !pi_d_v.is_finite() || !pi_q_v.is_finite() {
            self.reset();
            return false;
        }

        self.pi_d
            .back_calculate_integral(pi_d_v, target.d_amps - i_dq.d_amps);
        self.pi_q
            .back_calculate_integral(pi_q_v, target.q_amps - i_dq.q_amps);
        true
    }

    /// 一拍电流环：`i_dq` 与目标 → `DqVolts`。
    ///
    /// 控制律顺序固定：PI → 交叉解耦 → 反电势前馈 → 电压圆限幅 → 积分器反算。
    /// `omega_e_rad_s` 是电角速度（`[INFERENCE]` 由编码器/观测器提供），`vbus_v` 必须
    /// 为正且有限。输入非法时清零积分并返回零电压。
    pub fn current_loop(
        &mut self,
        i_dq: Dq,
        target: DqTarget,
        omega_e_rad_s: f32,
        vbus_v: f32,
        dt_s: f32,
    ) -> DqVolts {
        if !inputs_valid(i_dq, target, omega_e_rad_s, vbus_v, dt_s) {
            self.reset();
            return DqVolts::ZERO;
        }

        let max_v_mag = policy::VOLTAGE_MARGIN * vbus_v / SQRT3;
        let error_d_amps = target.d_amps - i_dq.d_amps;
        let error_q_amps = target.q_amps - i_dq.q_amps;

        let vd_pi = self.pi_d.update(target.d_amps, i_dq.d_amps, dt_s);
        let vq_pi = self.pi_q.update(target.q_amps, i_dq.q_amps, dt_s);

        // 与 VESC 同号：vd -= iq·ωe·Lq，vq += id·ωe·Ld + ωe·ψm。
        let dec_vd = i_dq.q_amps * omega_e_rad_s * self.params.lq_h;
        let dec_vq = i_dq.d_amps * omega_e_rad_s * self.params.ld_h;
        let dec_bemf = omega_e_rad_s * self.params.flux_wb;

        let vd_unclamped = vd_pi - dec_vd;
        let vq_unclamped = vq_pi + dec_vq + dec_bemf;
        if !vd_unclamped.is_finite() || !vq_unclamped.is_finite() {
            self.reset();
            return DqVolts::ZERO;
        }

        // d 轴优先：先把 vd 夹到 max_v_mag，再用剩余长度夹 vq。
        let vd_v = vd_unclamped.clamp(-max_v_mag, max_v_mag);
        let remaining_v2 = max_v_mag * max_v_mag - vd_v * vd_v;
        let max_vq = if remaining_v2 > 0.0 {
            sqrt_f32(remaining_v2)
        } else {
            0.0
        };
        let vq_v = vq_unclamped.clamp(-max_vq, max_vq);

        // 反算到 PI 输出侧：总电压 = pi - dec_vd，或 pi + dec_vq + dec_bemf。
        self.pi_d
            .back_calculate_integral(vd_v + dec_vd, error_d_amps);
        self.pi_q
            .back_calculate_integral(vq_v - dec_vq - dec_bemf, error_q_amps);

        DqVolts {
            d_v: vd_v,
            q_v: vq_v,
        }
    }
}

/// 入口唯一一次有限性检查。
fn inputs_valid(i_dq: Dq, target: DqTarget, omega_e_rad_s: f32, vbus_v: f32, dt_s: f32) -> bool {
    i_dq.d_amps.is_finite()
        && i_dq.q_amps.is_finite()
        && target.d_amps.is_finite()
        && target.q_amps.is_finite()
        && omega_e_rad_s.is_finite()
        && vbus_v.is_finite()
        && vbus_v > 0.0
        && dt_s.is_finite()
        && dt_s > 0.0
}

#[cfg(test)]
mod tests;
