use super::CurrentLoop;
use crate::foc::Dq;
use crate::math::SQRT3;
use crate::policy;
use contracts::types::{DqTarget, DqVolts, MotorParams};

const DT_S: f32 = 50e-6;
const VBUS_V: f32 = 48.0;
const R_OHM: f32 = 0.1;
const LD_H: f32 = 100e-6;
const LQ_H: f32 = 150e-6;
const FLUX_WB: f32 = 0.01;

fn params() -> MotorParams {
    MotorParams {
        r_ohm: R_OHM,
        ld_h: LD_H,
        lq_h: LQ_H,
        flux_wb: FLUX_WB,
    }
}

fn new_loop() -> CurrentLoop {
    let Some(current_loop) = CurrentLoop::new(params(), policy::CURRENT_BW_RAD_S) else {
        panic!("valid motor parameters rejected");
    };
    current_loop
}

fn voltage_limit(vbus_v: f32) -> f32 {
    policy::VOLTAGE_MARGIN * vbus_v / SQRT3
}

/// 简化 d 轴被控对象：`Ld·di/dt = v − R·i`（ω = 0，q 轴不参与）。
fn rl_step(current_a: f32, voltage_v: f32) -> f32 {
    current_a + DT_S * (voltage_v - R_OHM * current_a) / LD_H
}

#[test]
fn first_order_rl_step_tracks_reference() {
    let mut current_loop = new_loop();
    let mut current_a = 0.0f32;

    for _ in 0..4_000 {
        let volts = current_loop.current_loop(
            Dq {
                d_amps: current_a,
                q_amps: 0.0,
            },
            DqTarget {
                d_amps: 1.0,
                q_amps: 0.0,
            },
            0.0,
            VBUS_V,
            DT_S,
        );
        current_a = rl_step(current_a, volts.d_v);
    }

    assert!(current_a.is_finite());
    assert!((current_a - 1.0).abs() < 0.01, "i_d = {current_a}");
}

#[test]
fn voltage_vector_stays_within_limit_and_recovers() {
    let mut current_loop = new_loop();
    let vbus_v = 1.0;
    let limit_v = voltage_limit(vbus_v);
    let mut current_a = 0.0f32;

    for _ in 0..1_000 {
        let volts = current_loop.current_loop(
            Dq {
                d_amps: current_a,
                q_amps: 0.0,
            },
            DqTarget {
                d_amps: 50.0,
                q_amps: 0.0,
            },
            0.0,
            vbus_v,
            DT_S,
        );
        let magnitude_v2 = volts.d_v * volts.d_v + volts.q_v * volts.q_v;
        assert!(
            magnitude_v2 <= limit_v * limit_v + 1e-6,
            "|v| exceeds limit: {magnitude_v2}"
        );
        assert!(volts.d_v.abs() <= limit_v + 1e-6);
        current_a = rl_step(current_a, volts.d_v);
    }

    // 撤掉目标后积分器必须能退出饱和，而不是把电压锁在限幅值。
    for _ in 0..4_000 {
        let volts = current_loop.current_loop(
            Dq {
                d_amps: current_a,
                q_amps: 0.0,
            },
            DqTarget {
                d_amps: 0.0,
                q_amps: 0.0,
            },
            0.0,
            vbus_v,
            DT_S,
        );
        current_a = rl_step(current_a, volts.d_v);
    }

    assert!(current_a.abs() < 0.01, "i_d after recovery = {current_a}");
}

#[test]
fn d_axis_saturation_shrinks_q_axis_limit() {
    let mut current_loop = new_loop();
    let limit_v = voltage_limit(VBUS_V);
    let mut last = DqVolts::ZERO;

    for _ in 0..400 {
        last = current_loop.current_loop(
            Dq {
                d_amps: 0.0,
                q_amps: 0.0,
            },
            DqTarget {
                d_amps: 50.0,
                q_amps: 50.0,
            },
            0.0,
            VBUS_V,
            DT_S,
        );
    }

    assert!(last.d_v.abs() <= limit_v + 1e-6);
    assert!((last.d_v.abs() - limit_v).abs() < 1e-3);

    let remaining_v2 = (limit_v * limit_v - last.d_v * last.d_v).max(0.0);
    assert!(last.q_v.abs() <= remaining_v2.sqrt() + 1e-3);
    assert!(last.q_v.abs() < 1e-3, "q_v = {}", last.q_v);
}

#[test]
fn non_finite_inputs_fall_back_to_zero() {
    let mut current_loop = new_loop();
    let good_current = Dq {
        d_amps: 0.0,
        q_amps: 0.0,
    };
    let good_target = DqTarget {
        d_amps: 0.0,
        q_amps: 0.0,
    };

    let bad_current = Dq {
        d_amps: f32::NAN,
        q_amps: 0.0,
    };
    assert_eq!(
        current_loop.current_loop(bad_current, good_target, 0.0, VBUS_V, DT_S),
        DqVolts::ZERO
    );

    let bad_target = DqTarget {
        d_amps: f32::INFINITY,
        q_amps: 0.0,
    };
    assert_eq!(
        current_loop.current_loop(good_current, bad_target, 0.0, VBUS_V, DT_S),
        DqVolts::ZERO
    );

    assert_eq!(
        current_loop.current_loop(good_current, good_target, f32::NAN, VBUS_V, DT_S),
        DqVolts::ZERO
    );

    assert_eq!(
        current_loop.current_loop(good_current, good_target, 0.0, f32::NAN, DT_S),
        DqVolts::ZERO
    );

    assert_eq!(
        current_loop.current_loop(good_current, good_target, 0.0, VBUS_V, 0.0),
        DqVolts::ZERO
    );

    let volts = current_loop.current_loop(good_current, good_target, 0.0, VBUS_V, DT_S);
    assert!(volts.d_v.is_finite() && volts.q_v.is_finite());

    assert!(CurrentLoop::new(
        MotorParams {
            r_ohm: 0.0,
            ..params()
        },
        policy::CURRENT_BW_RAD_S
    )
    .is_none());
    assert!(CurrentLoop::new(params(), 0.0).is_none());
}

#[test]
fn takeover_preloads_from_measured_voltage() {
    let mut current_loop = new_loop();
    let measured = DqVolts {
        d_v: 0.0,
        q_v: 10.5,
    };
    let current = Dq {
        d_amps: 0.0,
        q_amps: 0.0,
    };
    let target = DqTarget {
        d_amps: 0.0,
        q_amps: 0.0,
    };

    assert!(current_loop.preload_from_measured(measured, current, target, 1_000.0));
    let preloaded = current_loop.current_loop(current, target, 1_000.0, VBUS_V, DT_S);
    assert!((preloaded.q_v - measured.q_v).abs() < 1e-3);

    let mut fresh = new_loop();
    let without = fresh.current_loop(current, target, 1_000.0, VBUS_V, DT_S);
    assert!((without.q_v - 10.0).abs() < 1e-3);

    assert!(!current_loop.preload_from_measured(
        DqVolts {
            d_v: f32::NAN,
            q_v: 0.0,
        },
        current,
        target,
        0.0,
    ));
    let safe = current_loop.current_loop(current, target, 0.0, VBUS_V, DT_S);
    assert!(safe.d_v.is_finite() && safe.q_v.is_finite());
}

#[test]
fn back_emf_feedforward_uses_flux_linkage() {
    let mut current_loop = new_loop();
    let volts = current_loop.current_loop(
        Dq {
            d_amps: 0.0,
            q_amps: 0.0,
        },
        DqTarget {
            d_amps: 0.0,
            q_amps: 0.0,
        },
        1_000.0,
        VBUS_V,
        DT_S,
    );

    assert!(volts.d_v.abs() < 1e-6, "d_v = {}", volts.d_v);
    assert!((volts.q_v - 10.0).abs() < 1e-4, "q_v = {}", volts.q_v);
}
