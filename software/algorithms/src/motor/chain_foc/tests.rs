use super::{
    clarke, inverse_park, inverse_park_with, park, park_with, sin_cos, AlphaBeta, Dq, SinCos,
};
use contracts::motor::protocol_types::PhaseCurrent;
use std::f32::consts::{PI, TAU};

/// 移植的抛物线与 `std` 的偏差上限（源 crate 自述 0.002，留一点余量）。
const TRIG_TOL: f32 = 2.5e-3;

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn sin_cos_matches_std_over_two_turns() {
    let mut worst: f32 = 0.0;
    let mut x = -2.0 * TAU;
    while x <= 2.0 * TAU {
        let (sin, cos) = sin_cos(x);
        worst = worst.max((sin - x.sin()).abs()).max((cos - x.cos()).abs());
        x += 0.01;
    }
    assert!(worst <= TRIG_TOL, "max error {worst}");
}

#[test]
fn sin_cos_is_periodic_beyond_one_turn() {
    let (sin, cos) = sin_cos(TAU + 0.3);
    assert!(close(sin, 0.3f32.sin(), TRIG_TOL));
    assert!(close(cos, 0.3f32.cos(), TRIG_TOL));
}

#[test]
fn clarke_balanced_three_phase_gives_alpha_peak() {
    let ab = clarke(PhaseCurrent {
        a_amps: 1.0,
        b_amps: -0.5,
        c_amps: -0.5,
    });
    assert!(close(ab.alpha_amps, 1.0, 1e-6));
    assert!(close(ab.beta_amps, 0.0, 1e-6));
}

#[test]
fn clarke_beta_peak_is_at_ninety_degrees() {
    let ab = clarke(PhaseCurrent {
        a_amps: 0.0,
        b_amps: 0.866_025_4,
        c_amps: -0.866_025_4,
    });
    assert!(close(ab.alpha_amps, 0.0, 1e-6));
    assert!(close(ab.beta_amps, 1.0, 1e-6));
}

#[test]
fn park_is_identity_at_zero_angle() {
    let ab = AlphaBeta {
        alpha_amps: 0.7,
        beta_amps: -0.3,
    };
    let dq = park(ab, 0.0);
    assert!(close(dq.d_amps, 0.7, TRIG_TOL));
    assert!(close(dq.q_amps, -0.3, TRIG_TOL));
}

#[test]
fn park_rotates_alpha_into_q_at_ninety_degrees() {
    let ab = AlphaBeta {
        alpha_amps: 1.0,
        beta_amps: 0.0,
    };
    let dq = park(ab, PI / 2.0);
    assert!(close(dq.d_amps, 0.0, TRIG_TOL));
    assert!(close(dq.q_amps, -1.0, TRIG_TOL));
}

#[test]
fn inverse_park_and_park_round_trip() {
    let expected = Dq {
        d_amps: 0.7,
        q_amps: -0.3,
    };
    let mut theta = -PI;
    while theta <= PI {
        let ab = inverse_park(expected.d_amps, expected.q_amps, theta);
        let back = park(
            AlphaBeta {
                alpha_amps: ab.alpha_v,
                beta_amps: ab.beta_v,
            },
            theta,
        );
        assert!(close(back.d_amps, expected.d_amps, 1e-2), "d at {theta}");
        assert!(close(back.q_amps, expected.q_amps, 1e-2), "q at {theta}");
        theta += 0.05;
    }
}

#[test]
fn cached_sincos_matches_direct_paths() {
    let theta = 0.7f32;
    let (sin, cos) = sin_cos(theta);
    let frame = SinCos { sin, cos };
    let ab = AlphaBeta {
        alpha_amps: 1.2,
        beta_amps: -0.8,
    };

    assert_eq!(park_with(frame, ab), park(ab, theta));
    assert_eq!(
        inverse_park_with(frame, 0.9, -0.3),
        inverse_park(0.9, -0.3, theta)
    );
}
