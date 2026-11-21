//! 闸门单测：合成三相激励、结构门、精度门与 Commit Gate。
//!
//! 测试支撑只服务本文件，不提升为跨模块夹具；换文件复用前先确认有第二个消费者。

use self::test_support::{
    gate, measured, push, raw, Lcg, Rot, ARC_STEP_COS, ARC_STEP_SIN, FULL_STEP_COS, FULL_STEP_SIN,
};
use super::accum::{GainAccum, HalfWindow};
use super::math::SQRT3;
use super::{GainGate, GateParams};
use crate::policy;
use contracts::Trust;

/// 1 A 相峰值对应的 LSB 码（2.97 mA/LSB，HR-203）：只用于构造测试数据。
const LSB_PER_AMP: f32 = 1.0 / 0.002_97;

fn frozen_gate() -> GainGate {
    gate()
}

// 合成状态的解析目标：`s_rms = 12.5 LSB`、`σ_ch = 0.6756 LSB`、`N_eq = 3999`。
// 反解依据 pred 公式：
//   pred(δo²) = 100·[√3·σ_ch/(s_rms·√N_eq) + (σ_ch² + δo²)/s_rms²]
//   pred(0)    ≈ 0.440 pp ≤ KAPPA·TARGET_PP = 0.45 pp
//   pred(1/12) ≈ 0.493 pp > 0.45 pp
const SYNTH_S_RMS: f32 = 12.5;
const SYNTH_SIGMA_CH: f32 = 0.6756;
const SYNTH_DOF: f32 = 2_000.0 - 2.0;

fn synth_sse() -> f32 {
    3.0 * SYNTH_DOF * SYNTH_SIGMA_CH * SYNTH_SIGMA_CH
}

// 把组窗填成指定状态：结构门全过（R = 0、d = 0.25、z = 0），半窗同 θ。
// 主累加器保持为空，样本由 Commit Gate 并入，测的是真实提交路径。
fn synth_group(gate: &mut GainGate, sse: f32) {
    let g = GateParams::FROZEN.group_samples as f32;
    gate.group.accum.groups = 199;
    gate.group.n = 199;
    gate.group.accum.a0 = 0.0;
    gate.group.accum.a1 = 3.0 * g * SYNTH_S_RMS * SYNTH_S_RMS;
    gate.group.accum.weight = 1_999.0;
    gate.group.accum.sse = sse;
    gate.group.accum.s = [1.0, 0.0, 0.0, 1.0];
    gate.group.accum.b = [0.0, 0.0];
    gate.group.accum.rsum = [0.0, 0.0];
    gate.group.accum.ri2 = 1.0;
    gate.group.accum.rn = 1;
    gate.group.accum.theta = [1.0, 1.0];
    let half = HalfWindow {
        s: [1.0, 0.0, 0.0, 1.0],
        b: [0.5, 0.5],
        weight: 100.0,
        n: 100,
    };
    gate.group.half_a = half;
    gate.group.half_b = half;
}

// 主累加器合成状态：只给 pred_pp 的数值对照用。
fn synth_accum(sse: f32) -> GainAccum {
    let g = GateParams::FROZEN.group_samples as f32;
    GainAccum {
        s: [1.0, 0.0, 0.0, 1.0],
        b: [0.0, 0.0],
        a0: 0.0,
        a1: 3.0 * g * SYNTH_S_RMS * SYNTH_S_RMS,
        groups: 200,
        weight: 2_000.0,
        sse,
        do2: 0.0,
        rsum: [0.0; 2],
        ri2: 1.0,
        rn: 1,
        theta: [1.0, 1.0],
    }
}

#[test]
fn zero_uncertainty_flips_precision_gate() {
    // P0 证据：同一状态，`do2 = 0` 时 pred = 0.440 pp ≤ 0.45（Trusted），
    // `do2 = 1/12` 时 pred = 0.493 pp > 0.45（UntrustedAmplitude）。
    let sse = synth_sse();
    let mut probe_without = gate();
    probe_without.accum = synth_accum(sse);
    probe_without.set_zero_uncertainty_lsb(0.0);
    let mut probe_with = gate();
    probe_with.accum = synth_accum(sse);
    probe_with.set_zero_uncertainty_lsb(policy::DO_LSB_HALF_WIDTH);
    let pred_without = match probe_without.pred_pp() {
        Some(v) => v,
        None => panic!("pred_pp is None"),
    };
    let pred_with = match probe_with.pred_pp() {
        Some(v) => v,
        None => panic!("pred_pp is None"),
    };
    let threshold = GateParams::FROZEN.kappa * GateParams::FROZEN.target_pp;
    assert!(
        pred_without <= threshold && pred_with > threshold,
        "pred(0) = {pred_without} pp, pred(1/12) = {pred_with} pp, threshold = {threshold} pp"
    );
    let expected_delta = 100.0 * policy::DO2_LSB2 / (SYNTH_S_RMS * SYNTH_S_RMS);
    assert!(
        (pred_with - pred_without - expected_delta).abs() < 1e-3,
        "do2 contribution = {} pp, expected {expected_delta} pp",
        pred_with - pred_without
    );

    // 真实组窗边界：组窗已有 G-1 拍，再推一拍触发 Commit Gate。
    let mut without = gate();
    without.set_zero_uncertainty_lsb(0.0);
    synth_group(&mut without, sse);
    let mut with = gate();
    with.set_zero_uncertainty_lsb(policy::DO_LSB_HALF_WIDTH);
    synth_group(&mut with, sse);
    let _ = without.step(raw([0.0, 0.0, 0.0]), true);
    let _ = with.step(raw([0.0, 0.0, 0.0]), true);

    assert!(without.pred_ok, "δo²=0 应通过精度门");
    assert!(!with.pred_ok, "δo²=1/12 应被精度门拒绝");
    assert_eq!(without.trust(), Trust::Trusted);
    assert_eq!(with.trust(), Trust::UntrustedAmplitude);
}

#[test]
fn zero_uncertainty_survives_reset() {
    // 零位不确定度是标定属性，不是窗内统计量：reset 只能清窗。
    let mut gate = frozen_gate();
    gate.set_zero_uncertainty_lsb(0.25);
    let expected = (0.5_f32 * 0.5_f32) / 12.0;
    assert!((gate.accum.do2 - expected).abs() < 1e-9);
    gate.reset();
    assert_eq!(gate.zero_uncertainty_lsb(), 0.25);
    assert!((gate.accum.do2 - expected).abs() < 1e-9);
}

#[test]
fn default_zero_uncertainty_is_one_twelfth_lsb2() {
    let gate = frozen_gate();
    assert_eq!(gate.zero_uncertainty_lsb(), policy::DO_LSB_HALF_WIDTH);
    assert!((gate.accum.do2 - 1.0 / 12.0).abs() < 1e-7);
}

#[test]
fn rotating_1a_recovers_injected_gain() {
    let mut gate = frozen_gate();
    let mut rng = Lcg::new(0x1234_5678);
    let mut rot = Rot::new();
    let mut last = None;
    let mut saw_trusted = false;

    for _ in 0..3_000 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let abc = rot.iabc(1.0 * LSB_PER_AMP);
        let s = measured(abc, 1.02, 0.0, 1.5, &mut rng);
        if let Some(g) = push(&mut gate, s) {
            last = Some(g);
        }
        if gate.trust().is_trusted() {
            saw_trusted = true;
        }
    }

    let (gb, gc) = match last {
        Some(v) => v,
        None => panic!("gate never produced a gain estimate"),
    };
    assert!(saw_trusted, "never saw Trusted");
    assert!((gb - 1.02).abs() < 1e-3, "gamma_b = {gb}");
    assert!((gc - 1.0).abs() < 1e-3, "gamma_c = {gc}");
    assert_eq!(gate.trust(), Trust::Trusted);
}

#[test]
fn narrow_arc_with_dc_is_rejected() {
    let mut gate = frozen_gate();
    let mut rng = Lcg::new(21);
    let mut rot = Rot::new();
    let mut forward = true;
    for k in 0..400 {
        if k % 20 == 0 {
            forward = !forward;
        }
        if forward {
            rot.step(ARC_STEP_COS, ARC_STEP_SIN);
        } else {
            rot.step_back(ARC_STEP_COS, ARC_STEP_SIN);
        }
        let abc = rot.iabc(94.0);
        let s = measured(abc, 1.0, 30.0, 0.5, &mut rng);
        let _ = push(&mut gate, s);
    }
    assert!(!gate.trust().is_trusted());
}

#[test]
fn low_amplitude_is_rejected_by_precision_gate() {
    // 0.1 A 峰值（≈33.7 LSB）+ 3.5 LSB 噪声：pred > 0.45 pp。
    let mut gate = frozen_gate();
    let mut rng = Lcg::new(31);
    let mut rot = Rot::new();
    for _ in 0..400 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let abc = rot.iabc(0.1 * LSB_PER_AMP);
        let s = measured(abc, 1.0, 0.0, 3.5, &mut rng);
        let _ = push(&mut gate, s);
    }
    assert_eq!(gate.trust(), Trust::UntrustedAmplitude);
}

#[test]
fn two_percent_step_trips_change_gate_within_10ms() {
    let mut gate = frozen_gate();
    let mut rng = Lcg::new(51);
    let mut rot = Rot::new();

    for _ in 0..1_200 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let abc = rot.iabc(1.0 * LSB_PER_AMP);
        let s = measured(abc, 1.0, 0.0, 0.5, &mut rng);
        let _ = push(&mut gate, s);
    }
    assert_eq!(gate.trust(), Trust::Trusted);

    let mut saw_change = false;
    for k in 0..200 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let gamma_b = if k < 50 { 1.0 } else { 1.02 };
        let abc = rot.iabc(1.0 * LSB_PER_AMP);
        let s = measured(abc, gamma_b, 0.0, 0.5, &mut rng);
        let _ = push(&mut gate, s);
        if gate.trust() == Trust::UntrustedChange {
            saw_change = true;
            break;
        }
    }
    assert!(saw_change, "2 % step not caught within 200 samples (10 ms)");
}

/// 组首第一拍 25 % 增益突变：跨组一致性门必须拒绝，且历史累加器一个数都不能动。
#[test]
fn quarter_gain_jump_at_group_start_never_touches_history() {
    let mut gate = frozen_gate();
    let mut rng = Lcg::new(0x5eed);
    let mut rot = Rot::new();

    for _ in 0..1_200 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let s = measured(rot.iabc(1.0 * LSB_PER_AMP), 1.0, 0.0, 0.5, &mut rng);
        let _ = push(&mut gate, s);
    }
    assert_eq!(gate.trust(), Trust::Trusted);

    let weight_before = gate.accum.weight;
    let sse_before = gate.accum.sse;
    let active_before = gate.active_theta;

    let mut saw_change = false;
    for _ in 0..200 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let s = measured(rot.iabc(1.0 * LSB_PER_AMP), 1.25, 0.0, 0.5, &mut rng);
        let _ = push(&mut gate, s);
        if gate.trust() == Trust::UntrustedChange {
            saw_change = true;
        }
    }
    assert!(saw_change, "25 % step not caught within one group window");
    assert_eq!(gate.accum.weight, weight_before);
    assert_eq!(gate.accum.sse, sse_before);
    assert_eq!(gate.active_theta, active_before);
    assert!(gate.step(raw([0.0, 0.0, 0.0]), true).is_none());
}

#[test]
fn invalid_sample_does_not_decay() {
    let mut gate = frozen_gate();
    let mut rng = Lcg::new(71);
    let mut rot = Rot::new();
    for _ in 0..300 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let abc = rot.iabc(200.0);
        let s = measured(abc, 1.0, 0.0, 1.0, &mut rng);
        let _ = push(&mut gate, s);
    }
    let weight = gate.accum.weight;
    let sse = gate.accum.sse;
    let groups = gate.accum.groups;
    let n_eq = gate.accum.n_eq();

    for _ in 0..32 {
        assert!(gate.step(raw([10.0, -5.0, -5.0]), false).is_none());
    }

    assert_eq!(gate.accum.weight, weight);
    assert_eq!(gate.accum.sse, sse);
    assert_eq!(gate.accum.groups, groups);
    assert_eq!(gate.accum.n_eq(), n_eq);
}

#[test]
fn s_rms_is_equal_weight_three_phase() {
    // 平衡旋转：三相等权均方根 = 相峰值/√2。
    let amp = 100.0;
    let mut acc = GainAccum::empty();
    let mut rot = Rot::new();
    for _ in 0..64 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let [a, b, c] = rot.iabc(amp);
        acc.push_sample([a, b, c], 1.0);
    }
    let s_rot = match acc.s_rms_lsb() {
        Some(v) => v,
        None => panic!("s_rms is None"),
    };
    let sqrt2 = core::f32::consts::SQRT_2;
    assert!((s_rot - amp / sqrt2).abs() < 1e-2, "s_rot = {s_rot}");

    // 单相激励：= 幅值/√3，明确不是幅值/√2。
    let mut acc_one = GainAccum::empty();
    for _ in 0..64 {
        acc_one.push_sample([amp, 0.0, 0.0], 1.0);
    }
    let s_one = match acc_one.s_rms_lsb() {
        Some(v) => v,
        None => panic!("s_rms is None"),
    };
    assert!((s_one - amp / SQRT3).abs() < 1e-3, "s_one = {s_one}");
    assert!((s_one - amp / sqrt2).abs() > 5.0, "s_one read as peak/√2");
}

#[test]
fn step_returns_none_until_zero_is_set() {
    let mut gate = GainGate::new(GateParams::FROZEN);
    let s = [100.0, -50.0, -50.0];
    assert!(gate.step(raw(s), true).is_none());
    assert_eq!(gate.trust(), Trust::UntrustedExcitation);
    assert_eq!(gate.accum.weight, 0.0);
    assert_eq!(gate.group.accum.weight, 0.0);

    gate.set_zero_lsb([2_048.0, 2_044.0, 2_051.0]);
    let _ = gate.step(raw(s), true);
    // 首组未满：历史累加器不得被提前修改，样本只能在组窗里。
    assert_eq!(gate.accum.weight, 0.0);
    assert_eq!(gate.group.accum.weight, 1.0);
}

#[test]
fn gate_rejects_dc_biased_window() {
    // z = |A0|·√3/√(G·A1) > Z_MAX = 0.02 时必须 UntrustedBias。
    let mut gate = gate();
    let mut rng = Lcg::new(11);
    let mut rot = Rot::new();
    for _ in 0..400 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let abc = rot.iabc(94.0);
        let s = measured(abc, 1.01, 5.0, 0.5, &mut rng);
        let _ = push(&mut gate, s);
    }
    assert_eq!(gate.trust(), Trust::UntrustedBias);
}

#[test]
fn gate_rejects_collinear_current_excitation() {
    // 秩 1 噪声（方向单一、S 秩亏）时 d < D_MIN = 0.01，必须 UntrustedExcitation。
    let mut gate = gate();
    let mut rng = Lcg::new(41);
    for _ in 0..400 {
        let n = rng.noise(4.0);
        let _ = push(&mut gate, [n, -0.5 * n, -0.5 * n]);
    }
    assert_eq!(gate.trust(), Trust::UntrustedExcitation);
}

#[test]
fn gate_rejects_single_direction_rotation() {
    // R 门抓"整段同一方向"：R² = 1 > R_MAX² 时必须 UntrustedExcitation。
    let mut gate = gate();
    let mut rng = Lcg::new(43);
    for _ in 0..400 {
        let s = measured([94.0, -47.0, -47.0], 1.0, 0.0, 0.5, &mut rng);
        let _ = push(&mut gate, s);
    }
    assert_eq!(gate.trust(), Trust::UntrustedExcitation);
}

#[test]
fn gate_derates_prediction_by_kappa() {
    let mut gate = gate();
    let mut rng = Lcg::new(61);
    let mut rot = Rot::new();
    let amp = 0.14 * LSB_PER_AMP;
    for _ in 0..1_999 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let abc = rot.iabc(amp);
        let s = measured(abc, 1.0, 0.0, 1.75, &mut rng);
        let _ = push(&mut gate, s);
    }
    let pred = match gate.pred_pp() {
        Some(v) => v,
        None => panic!("pred_pp is None"),
    };
    assert!(pred > 0.45 && pred < 0.5, "pred = {pred} pp not marginal");

    rot.step(FULL_STEP_COS, FULL_STEP_SIN);
    let abc = rot.iabc(amp);
    let s = measured(abc, 1.0, 0.0, 1.75, &mut rng);
    let _ = push(&mut gate, s);
    assert_eq!(gate.trust(), Trust::UntrustedAmplitude);

    // 放宽目标后同一数据必须能过，证明降额真的在起作用。
    gate.params.target_pp = 2.0;
    let mut saw_trusted = false;
    for _ in 0..200 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let abc = rot.iabc(amp);
        let s = measured(abc, 1.0, 0.0, 1.75, &mut rng);
        let _ = push(&mut gate, s);
        if gate.trust().is_trusted() {
            saw_trusted = true;
        }
    }
    assert!(saw_trusted, "relaxed target did not pass");
}

#[test]
fn chi2_gate_only_runs_after_structural_gates() {
    // CHI2_MAX 只在 r/d/z 全过的样本组上计算，否则不得报 UntrustedChange。
    let mut gate = gate();
    let mut rng = Lcg::new(105);
    let mut rot = Rot::new();
    for _ in 0..600 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let s = measured(rot.iabc(337.0), 1.0, 0.0, 0.5, &mut rng);
        let _ = push(&mut gate, s);
    }
    // 同时注入共模 DC（z 门失败）与 2 % 增益阶跃（本会触发 χ²）。
    for _ in 0..400 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let s = measured(rot.iabc(337.0), 1.02, 20.0, 0.5, &mut rng);
        let _ = push(&mut gate, s);
    }
    assert_eq!(gate.trust(), Trust::UntrustedBias);
}

#[test]
fn n_eq_saturates_at_3999() {
    // λ = 0.9995，Σw = 2000 ⇒ N_eq = (1+λ)·Σw = 3999，且饱和。
    let mut accum = GainAccum::empty();
    accum.push_sample([100.0, -50.0, -50.0], 2000.0);
    assert_eq!(accum.weight, 2000.0);
    assert_eq!(accum.n_eq(), 3_999);
    accum.push_sample([100.0, -50.0, -50.0], 1.0);
    assert_eq!(accum.weight, 2000.0);
    assert_eq!(accum.n_eq(), 3_999);
}

#[test]
fn invalid_samples_above_limit_force_previous_gain() {
    let mut gate = gate();
    let mut rng = Lcg::new(0x1234_5678);
    let mut rot = Rot::new();
    for _ in 0..3_000 {
        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let s = measured(rot.iabc(1.0 * LSB_PER_AMP), 1.0, 0.0, 1.5, &mut rng);
        let _ = push(&mut gate, s);
    }
    let weight = gate.accum.weight;
    let n_invalid = u32::from(policy::INVALID_SAMPLE_LIMIT) + 1;
    for _ in 0..n_invalid {
        assert!(gate.step(raw([0.0, 0.0, 0.0]), false).is_none());
    }
    assert_eq!(gate.accum.weight, weight);
}

// 固定种子 LCG 与三相旋转发生器，不引入 rand；只服务本文件。
mod test_support {
    use super::super::math::ADC_MIDSCALE_CODE;
    use super::super::{GainGate, GateParams};
    use contracts::types::RawSample3;

    pub(super) fn gate() -> GainGate {
        let mut gate = GainGate::new(GateParams::FROZEN);
        gate.set_zero_lsb([ADC_MIDSCALE_CODE; 3]);
        gate
    }

    /// 每 64 拍转一圈（2π/64）。
    pub(super) const FULL_STEP_COS: f32 = 0.995_184_7;
    pub(super) const FULL_STEP_SIN: f32 = 0.098_017_14;
    /// 每拍 0.5°（±10° 弧用）。
    pub(super) const ARC_STEP_COS: f32 = 0.999_961_9;
    pub(super) const ARC_STEP_SIN: f32 = 0.008_726_536;

    /// 线性同余发生器（Numerical Recipes 常量），确定性、无依赖。
    pub(super) struct Lcg {
        state: u32,
    }

    impl Lcg {
        pub(super) fn new(seed: u32) -> Self {
            Self { state: seed | 1 }
        }

        fn next_u32(&mut self) -> u32 {
            self.state = self
                .state
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            self.state
        }

        fn uniform(&mut self) -> f32 {
            let top = self.next_u32() >> 8;
            (top as f32) / 16_777_216.0
        }

        /// 近似零均值、标准差 `sigma` 的噪声（4 个均匀分布之和）。
        pub(super) fn noise(&mut self, sigma: f32) -> f32 {
            let mut acc = 0.0;
            let mut i = 0;
            while i < 4 {
                acc += self.uniform();
                i += 1;
            }
            (acc - 2.0) * sigma * 1.732_050_8
        }
    }

    /// 单位复数旋转器：不需要 libm 的 sin/cos。
    #[derive(Clone, Copy)]
    pub(super) struct Rot {
        x: f32,
        y: f32,
    }

    impl Rot {
        pub(super) fn new() -> Self {
            Self { x: 1.0, y: 0.0 }
        }

        pub(super) fn step(&mut self, cos_step: f32, sin_step: f32) {
            let x = self.x * cos_step - self.y * sin_step;
            let y = self.x * sin_step + self.y * cos_step;
            self.x = x;
            self.y = y;
        }

        pub(super) fn step_back(&mut self, cos_step: f32, sin_step: f32) {
            let x = self.x * cos_step + self.y * sin_step;
            let y = -self.x * sin_step + self.y * cos_step;
            self.x = x;
            self.y = y;
        }

        /// 幅值不变 Clarke 逆变换：相峰值 `amp`，三相和恒为 0。
        pub(super) fn iabc(&self, amp: f32) -> [f32; 3] {
            let alpha = amp * self.x;
            let beta = amp * self.y;
            [
                alpha,
                -0.5 * alpha + 0.866_025_4 * beta,
                -0.5 * alpha - 0.866_025_4 * beta,
            ]
        }
    }

    /// 注入相对增益（`s_b /= gamma_b`）、共模 DC 与噪声。
    pub(super) fn measured(
        abc: [f32; 3],
        gamma_b: f32,
        dc: f32,
        sigma: f32,
        rng: &mut Lcg,
    ) -> [f32; 3] {
        let [a, b, c] = abc;
        [
            a + dc + rng.noise(sigma),
            b / gamma_b + dc + rng.noise(sigma),
            c + dc + rng.noise(sigma),
        ]
    }

    /// LSB 码（含标称中点）→ 原始样本。
    pub(super) fn raw(s_lsb: [f32; 3]) -> RawSample3 {
        let [a, b, c] = s_lsb;
        RawSample3 {
            tick: 0,
            t_us: 0,
            code: [to_code(a), to_code(b), to_code(c)],
        }
    }

    pub(super) fn push(gate: &mut GainGate, s_lsb: [f32; 3]) -> Option<(f32, f32)> {
        gate.step(raw(s_lsb), true)
    }

    fn to_code(x: f32) -> u16 {
        let v = x + ADC_MIDSCALE_CODE;
        if v <= 0.0 || v.is_nan() {
            0
        } else if v >= 65_535.0 {
            65_535
        } else {
            (v + 0.5) as u16
        }
    }
}
