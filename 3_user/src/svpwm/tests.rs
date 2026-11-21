// 上游 `oxifoc-core/src/foc/svpwm.rs` 的测试，改为调用本地 API。

use super::{get_sector, normalize_volts, space_vector_pwm, Sector};

const MAX_DUTY: u16 = 1_000;
const ONE_BY_SQRT3: f32 = 0.577_350_27;

#[test]
fn zero_voltage_is_midpoint_on_all_phases() {
    let duties = space_vector_pwm(0.0, 0.0, MAX_DUTY);
    let expected = MAX_DUTY / 2;
    for duty in duties {
        assert!(
            (i32::from(duty) - i32::from(expected)).abs() <= 1,
            "duty = {duty}"
        );
    }
}

/// 占空比必须真正实现请求的矢量：三相分数做 Clarke 后乘 1.5 应还原调制量。
#[test]
fn duties_realize_the_requested_vector() {
    let max = f32::from(MAX_DUTY);
    for angle_deg in 0..360 {
        let angle_rad = f32::to_radians(angle_deg as f32);
        for magnitude in [0.05, 0.3, 0.6, 0.86] {
            let alpha = magnitude * angle_rad.cos();
            let beta = magnitude * angle_rad.sin();
            let [a, b, c] = space_vector_pwm(alpha, beta, MAX_DUTY).map(|d| f32::from(d) / max);
            let got_alpha = 1.5 * (2.0 * a - b - c) / 3.0;
            let got_beta = 1.5 * (b - c) * ONE_BY_SQRT3;
            assert!(
                (got_alpha - alpha).abs() < 4.0 / max && (got_beta - beta).abs() < 4.0 / max,
                "{angle_deg}° m={magnitude}: asked ({alpha}, {beta}), got ({got_alpha}, {got_beta})"
            );
        }
    }
}

#[test]
fn zero_vectors_are_split_evenly() {
    for angle_deg in (0..360).step_by(7) {
        let angle_rad = f32::to_radians(angle_deg as f32);
        let duties = space_vector_pwm(0.7 * angle_rad.cos(), 0.7 * angle_rad.sin(), MAX_DUTY);
        let hi = i32::from(duties.iter().copied().max().unwrap_or(0));
        let lo = i32::from(duties.iter().copied().min().unwrap_or(0));
        assert!(
            (hi + lo - i32::from(MAX_DUTY)).abs() <= 2,
            "{angle_deg}°: {duties:?}"
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
fn duties_stay_within_the_register_range() {
    for alpha in [-4.0, -1.0, 0.0, 1.0, 4.0] {
        for beta in [-4.0, -1.0, 0.0, 1.0, 4.0] {
            for duty in space_vector_pwm(alpha, beta, MAX_DUTY) {
                assert!(duty <= MAX_DUTY);
            }
        }
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
