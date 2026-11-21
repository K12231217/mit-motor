use super::Pi;

const DT_S: f32 = 50e-6;

#[test]
fn proportional_only_is_linear_and_clamped() {
    let mut pi = Pi::new(2.0, 0.0, 10.0);
    assert!((pi.update(3.0, 0.0, DT_S) - 6.0).abs() < 1e-6);
    assert_eq!(pi.update(100.0, 0.0, DT_S), 10.0);
    assert_eq!(pi.update(-100.0, 0.0, DT_S), -10.0);
}

#[test]
fn integral_closes_the_steady_state_error() {
    // 被控对象取单位直流增益（y = u），只考察积分项能否把误差压到零。
    let mut pi = Pi::new(0.0, 2_000.0, 10.0);
    let target_amps = 1.0;
    let mut measurement_amps = 0.0f32;
    for _ in 0..4_000 {
        measurement_amps = pi.update(target_amps, measurement_amps, DT_S);
    }
    assert!((target_amps - measurement_amps).abs() < 1e-3);
    assert!((pi.integral_v() - target_amps).abs() < 1e-3);
}

#[test]
fn saturation_freezes_the_integrator() {
    let mut pi = Pi::new(1.0, 1_000.0, 1.0);
    for _ in 0..200 {
        assert_eq!(pi.update(5.0, 0.0, DT_S), 1.0);
    }
    assert!(
        pi.update(0.1, 0.0, DT_S) < 0.2,
        "integrator = {}",
        pi.integral_v()
    );
}

#[test]
fn reverse_error_unwinds_immediately() {
    let mut pi = Pi::new(1.0, 1_000.0, 1.0);
    for _ in 0..200 {
        let _ = pi.update(5.0, 0.0, DT_S);
    }
    assert!(pi.update(-5.0, 0.0, DT_S) < 0.0);
    assert!(pi.integral_v() <= 0.0);
}

#[test]
fn reset_clears_the_integrator() {
    let mut pi = Pi::new(0.0, 2_000.0, 10.0);
    let _ = pi.update(1.0, 0.0, DT_S);
    assert!(pi.integral_v() > 0.0);
    pi.reset();
    assert_eq!(pi.integral_v(), 0.0);
}

#[test]
fn output_never_leaves_the_limit() {
    let mut pi = Pi::new(3.0, 500.0, 2.0);
    let mut setpoint = -1.0f32;
    for _ in 0..1_000 {
        let u = pi.update(setpoint, 0.0, DT_S);
        assert!((-2.0..=2.0).contains(&u), "u = {u}");
        setpoint = -setpoint;
    }
}
