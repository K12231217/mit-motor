//! 海泰机电 WK4310H 的电机参数。
//!
//! 铭牌给的是**相间**电阻/电感；`MotorParams` 用**每相**量，按星形、忽略互感的
//! 常规换算 `/ 2`。磁链由铭牌 `Kt` 反解：`T = 1.5 · p · ψ · iq` ⇒
//! `ψ = Kt / (1.5 · p)`。`Kv = 43 rpm/V` 与 `Kt = 0.23 N·m/A` 的 60/(2πKv) 值相差
//! 约 3.5 %，本文件以铭牌 `Kt` 为准。
//!
//! 来源与完整规格表：`docs/motor-wk4310h.md`。

use super::chain_current::MotorParams;

/// 相间电阻，单位 Ω（铭牌 10.32 Ω）。
pub const PHASE_TO_PHASE_R_OHM: f32 = 10.32;
/// 相间电感，单位 H（铭牌 4.76 mH）。
pub const PHASE_TO_PHASE_L_H: f32 = 4.76e-3;
/// 每相电阻，单位 Ω（相间 / 2）。
pub const R_OHM: f32 = PHASE_TO_PHASE_R_OHM / 2.0;
/// 每相电感，单位 H（相间 / 2，星形、忽略互感）。
pub const L_H: f32 = PHASE_TO_PHASE_L_H / 2.0;

/// 极对数（铭牌 14）。
pub const POLE_PAIRS: u32 = 14;
/// 扭矩常数，单位 N·m/A（铭牌 0.23）。
pub const KT_NM_PER_A: f32 = 0.23;
/// 转速常数，单位 rpm/V（铭牌 43）。
pub const KV_RPM_PER_V: f32 = 43.0;

/// 永磁磁链，单位 Wb：`Kt / (1.5 · p)`。
pub const FLUX_WB: f32 = KT_NM_PER_A / (1.5 * POLE_PAIRS as f32);

/// 额定电压，单位 V。
pub const NOMINAL_VOLTAGE_V: f32 = 24.0;
/// 额定电流，单位 A。
pub const NOMINAL_CURRENT_A: f32 = 0.9;
/// 额定扭矩，单位 N·m。
pub const NOMINAL_TORQUE_NM: f32 = 0.2;
/// 额定转速，单位 rpm。
pub const NOMINAL_SPEED_RPM: f32 = 504.0;
/// 最大转速，单位 rpm。
pub const MAX_SPEED_RPM: f32 = 1_028.0;
/// 堵转扭矩，单位 N·m。
pub const STALL_TORQUE_NM: f32 = 0.49;
/// 堵转电流，单位 A。
pub const STALL_CURRENT_A: f32 = 1.9;
/// 转子惯量，单位 kg·m²（铭牌 161 g·cm²）。
pub const ROTOR_INERTIA_KG_M2: f32 = 161.0e-7;

/// 电流环可直接消费的电机参数。
pub const WK4310H: MotorParams = MotorParams {
    r_ohm: R_OHM,
    ld_h: L_H,
    lq_h: L_H,
    flux_wb: FLUX_WB,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn wk4310h_phase_values_match_the_nameplate() {
        assert!(close(R_OHM, 5.16, 1e-6), "R = {R_OHM}");
        assert!(close(L_H, 2.38e-3, 1e-9), "L = {L_H}");
        assert!(close(FLUX_WB, 0.010_952_381, 1e-7), "psi = {FLUX_WB}");
        assert!(WK4310H.is_valid());
        assert_eq!(POLE_PAIRS, 14);
    }

    #[test]
    fn kt_and_kv_are_consistent_within_nameplate_rounding() {
        let kt_from_kv = 60.0 / (core::f32::consts::TAU * KV_RPM_PER_V);
        assert!(
            close(KT_NM_PER_A, kt_from_kv, 0.02),
            "Kt={KT_NM_PER_A}, 60/(2πKv)={kt_from_kv}"
        );
    }
}
