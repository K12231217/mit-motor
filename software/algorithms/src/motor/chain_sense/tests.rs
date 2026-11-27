use super::{currents, SenseParams};
use contracts::motor::config_hw as hw;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn default_is_the_nominal_calibration() {
    assert_eq!(SenseParams::default(), SenseParams::nominal());
}

#[test]
fn nominal_offset_reads_zero_amps() {
    let mid = hw::ADC_MIDSCALE_CODE as u16;
    let c = currents(&SenseParams::nominal(), [mid; 3]);
    assert_eq!((c.a_amps, c.b_amps, c.c_amps), (0.0, 0.0, 0.0));
}

#[test]
fn one_count_is_the_hardware_scale() {
    let mid = hw::ADC_MIDSCALE_CODE as u16;
    let c = currents(&SenseParams::nominal(), [mid + 1, mid, mid - 1]);
    assert!(close(c.a_amps, hw::SOX_AMPS_PER_LSB));
    assert_eq!(c.b_amps, 0.0);
    assert!(close(c.c_amps, -hw::SOX_AMPS_PER_LSB));
}

#[test]
fn channels_are_mapped_in_a_b_c_order() {
    let params = SenseParams {
        offset_code: [2_000.0, 2_100.0, 2_200.0],
        amps_per_count: hw::SOX_AMPS_PER_LSB,
    };
    let c = currents(&params, [2_010, 2_110, 2_210]);
    let expected = 10.0 * hw::SOX_AMPS_PER_LSB;
    assert!(close(c.a_amps, expected));
    assert!(close(c.b_amps, expected));
    assert!(close(c.c_amps, expected));
}

#[test]
fn scale_is_configurable_for_calibration() {
    let params = SenseParams {
        offset_code: [hw::ADC_MIDSCALE_CODE; 3],
        amps_per_count: 0.001,
    };
    let mid = hw::ADC_MIDSCALE_CODE as u16;
    let c = currents(&params, [mid + 500; 3]);
    assert!(close(c.a_amps, 0.5));
    assert!(close(c.b_amps, 0.5));
    assert!(close(c.c_amps, 0.5));
}
