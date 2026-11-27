// 上游 `oxifoc-core/src/foc/svpwm.rs` 的测试，改为调用本地 API。

use super::{get_sector, normalize_volts, normalize_volts_with_inv, space_vector_pwm, Sector};
use contracts::joint::protocol_wire::modulation_to_duty_norm;
use contracts::motor::config_hw as hw;
use contracts::motor::protocol_types::MODULATION_MAX_RAW;

const ONE_BY_SQRT3: f32 = 0.577_350_27;
const ARR: f32 = hw::PWM_ARR as f32;

fn duty_norm(m: contracts::motor::protocol_types::ClampedModulation) -> f32 {
    modulation_to_duty_norm(m)
}

fn duty_counts(m: contracts::motor::protocol_types::ClampedModulation) -> i32 {
    (duty_norm(m) * ARR) as i32
}

#[test]
fn zero_voltage_is_midpoint_on_all_phases() {
    let duties = space_vector_pwm(0.0, 0.0);
    let expected = hw::PWM_ARR / 2;
    for m in duties {
        let duty = duty_counts(m);
        assert!((duty - i32::from(expected)).abs() <= 1, "duty = {duty}");
    }
}

/// 占空比必须真正实现请求的矢量：三相分数做 Clarke 后乘 1.5 应还原调制量。
#[test]
fn duties_realize_the_requested_vector() {
    for angle_deg in 0..360 {
        let angle_rad = f32::to_radians(angle_deg as f32);
        // D_MAX=0.80 时线性区只到 |m| ≈ 0.52（30° 处相占空比最高）；再大就被
        // 输出边界夹紧，矢量还原测试不再成立。
        for magnitude in [0.05, 0.3, 0.5] {
            let alpha = magnitude * angle_rad.cos();
            let beta = magnitude * angle_rad.sin();
            let [a, b, c] = space_vector_pwm(alpha, beta).map(duty_norm);
            let got_alpha = 1.5 * (2.0 * a - b - c) / 3.0;
            let got_beta = 1.5 * (b - c) * ONE_BY_SQRT3;
            assert!(
                (got_alpha - alpha).abs() < 4.0 / ARR && (got_beta - beta).abs() < 4.0 / ARR,
                "{angle_deg}° m={magnitude}: asked ({alpha}, {beta}), got ({got_alpha}, {got_beta})"
            );
        }
    }
}

#[test]
fn zero_vectors_are_split_evenly() {
    for angle_deg in (0..360).step_by(7) {
        let angle_rad = f32::to_radians(angle_deg as f32);
        let duties = space_vector_pwm(0.6 * angle_rad.cos(), 0.6 * angle_rad.sin());
        let hi = duties.iter().copied().map(duty_counts).max().unwrap_or(0);
        let lo = duties.iter().copied().map(duty_counts).min().unwrap_or(0);
        assert!(
            (hi + lo - i32::from(hw::PWM_ARR)).abs() <= 2,
            "{angle_deg}°: {hi} + {lo} != {}",
            hw::PWM_ARR
        );
    }
}

#[test]
fn sector_detection_matches_the_source_table() {
    let cases = [
        (0.5, 0.1, Sector::S1),
        (0.1, 0.5, Sector::S2),
        (-0.4, 0.4, Sector::S3),
        (-0.5, -0.1, Sector::S4),
        (-0.1, -0.5, Sector::S5),
        (0.4, -0.4, Sector::S6),
    ];
    for (alpha, beta, expected) in cases {
        assert_eq!(get_sector(alpha, beta), expected, "({alpha}, {beta})");
    }
}

#[test]
fn duties_stay_within_the_register_and_dmax_range() {
    let min_duty = (f32::from(hw::PWM_ARR) * (1.0 - hw::D_MAX)) as i32;
    let max_duty = (f32::from(hw::PWM_ARR) * hw::D_MAX) as i32;
    for alpha in [-4.0, -1.0, 0.0, 1.0, 4.0] {
        for beta in [-4.0, -1.0, 0.0, 1.0, 4.0] {
            for m in space_vector_pwm(alpha, beta) {
                let raw = m.get().raw();
                assert!(raw.abs() <= MODULATION_MAX_RAW, "raw = {raw}");
                let duty = duty_counts(m);
                assert!(
                    (min_duty..=max_duty).contains(&duty),
                    "duty = {duty} outside [{min_duty}, {max_duty}]"
                );
            }
        }
    }
}

/// `duty_norm` 与 `ClampedModulation` 必须按 `Q15` 量化往返，误差不超过 1 个计数。
#[test]
fn duty_norm_and_clamped_modulation_round_trip() {
    let max_duty = (f32::from(hw::PWM_ARR) * hw::D_MAX) as i32;
    let min_duty = (f32::from(hw::PWM_ARR) * (1.0 - hw::D_MAX)) as i32;
    for duty in [min_duty, 0, 1_000, 2_125, 3_000, max_duty] {
        let expected = duty.clamp(min_duty, max_duty);
        let ratio = expected as f32 / ARR;
        let m = contracts::motor::protocol_types::ClampedModulation::new_clamped(
            contracts::motor::protocol_types::Q15::from_ratio(2.0 * ratio - 1.0),
        );
        let back = duty_norm(m) * ARR;
        assert!(
            (back - expected as f32).abs() <= 1.0,
            "duty {duty} clamped {expected} -> {back}"
        );
    }
}

#[test]
fn normalizing_volts_uses_the_source_contract() {
    let Some((alpha, beta)) = normalize_volts(2.0, 3.0, 24.0) else {
        panic!("valid bus must normalize");
    };
    assert!((alpha - 1.5 * 2.0 / 24.0).abs() < 1e-6);
    assert!((beta - 1.5 * 3.0 / 24.0).abs() < 1e-6);

    assert!(normalize_volts(0.0, 0.0, 0.0).is_none());
    assert!(normalize_volts(0.0, 0.0, -24.0).is_none());
    assert!(normalize_volts(0.0, 0.0, f32::NAN).is_none());
    assert!(normalize_volts(f32::NAN, 0.0, 24.0).is_none());
}

#[test]
fn cached_inverse_bus_matches_direct_normalization() {
    let vbus = 24.0f32;
    let direct = normalize_volts(3.0, -2.0, vbus);
    let cached = normalize_volts_with_inv(3.0, -2.0, 1.0 / vbus);
    assert_eq!(direct, cached);

    assert_eq!(normalize_volts_with_inv(3.0, -2.0, 0.0), None);
    assert_eq!(normalize_volts_with_inv(3.0, -2.0, f32::NAN), None);
}
