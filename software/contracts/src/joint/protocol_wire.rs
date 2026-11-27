//! 线协议 ↔ 内部类型的唯一换算入口。
//!
//! 三处断点都在这里闭合：`CmdFrame` → [`Cmd`]、[`Cmd`] → [`CmdView`]、物理量 ↔
//! 线上定点（mA / theta / d / z / Q1.14）。换算只做缩放、舍入与饱和，不含策略；
//! 越界值返回饱和码而不是 panic（PRE-05）。

use crate::joint::protocol_events::{CalibPayload, Cmd};
use crate::joint::protocol_icd::{
    CmdFrame, CMD_FLAG_BENCH_INJECT, CMD_FLAG_CLEAR_FAULT, CMD_FLAG_COMMIT_CALIB, CMD_FLAG_ENABLE,
    LSB_AMPS, LSB_BENCH_AMPS, LSB_THETA_RAD,
};
use crate::joint::protocol_snapshot::CmdView;
use crate::motor::config_hw as hw;
use crate::motor::protocol_types::{ClampedModulation, DqTarget, Mode, Q15};

/// 帧 → 命令失败的唯一原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    /// `COMMIT_CALIB` 标志需要外部提供载荷（`CmdFrame` 没有空间放它）。
    MissingCalibPayload,
    /// 模式编号未知或不可命令（自检/故障只能由固件进入）。
    InvalidMode,
    /// 台架相选择不是 0/1/2。
    InvalidBenchPhase,
}

/// 毫安 → 安培。
pub const fn ma_to_amps(ma: i16) -> f32 {
    ma as f32 * LSB_AMPS
}

/// 安培 → 毫安，四舍五入并饱和到 `i16`；`NaN → 0`。
pub fn amps_to_ma(amps: f32) -> i16 {
    round_sat_i16(amps / LSB_AMPS)
}

/// 弧度 → 16 bit 电角度（2π/65536 rad/LSB），先回绕到 `[0, 2π)`。
///
/// 非有限输入返回 0；`2π` 与 `0` 同码，主机按回绕处理。
pub fn rad_to_theta_u16(rad: f32) -> u16 {
    if !rad.is_finite() {
        return 0;
    }
    let mut wrapped = rad % core::f32::consts::TAU;
    if wrapped < 0.0 {
        wrapped += core::f32::consts::TAU;
    }
    let code = (wrapped / LSB_THETA_RAD + 0.5) as u32;
    (code & 0xFFFF) as u16
}

/// `d = det(S)/tr²(S)` → 1e-5/LSB 的无符号码，饱和。
pub fn det_to_1e5(d: f32) -> u16 {
    round_sat_u16(d * 1e5)
}

/// `z` → 1e-4/LSB 的无符号码，饱和。
pub fn z_to_1e4(z: f32) -> u16 {
    round_sat_u16(z * 1e4)
}

/// 相对增益 → Q1.14（1.0 = 16384），四舍五入并饱和。
pub fn gain_to_q14(gain: f32) -> u16 {
    round_sat_u16(gain * f32::from(crate::joint::protocol_icd::GAIN_Q14_ONE))
}

/// 比较值 → 受 `D_MAX` 约束的调制量。
///
/// `duty` 先夹到 [`hw::PWM_ARR`]，再按 `m = 2·duty/ARR − 1` 换算；`ClampedModulation`
/// 负责最后的 `D_MAX` 夹紧与 Q15 量化。调用方不再传满量程。
pub fn duty_to_modulation(duty: u16) -> ClampedModulation {
    let arr = f32::from(hw::PWM_ARR);
    let duty = duty.min(hw::PWM_ARR);
    let ratio = f32::from(duty) / arr;
    ClampedModulation::new_clamped(Q15::from_ratio(2.0 * ratio - 1.0))
}

/// 三相比较值 → 三路调制量（顺序 A/B/C）。
pub fn duty_words_to_modulations(duty: [u16; 3]) -> [ClampedModulation; 3] {
    duty.map(duty_to_modulation)
}

/// 调制量 → 占空比归一化值 `d ∈ [0, 1]`（`m = 2d − 1` 的逆）。
pub fn modulation_to_duty_norm(m: ClampedModulation) -> f32 {
    (m.get().to_ratio() + 1.0) * 0.5
}

/// `CmdFrame` → [`Cmd`]。
///
/// 优先级是线协议的一部分：清故障 > 提交标定 > 台架注入 > 模式/目标。没有动作标志时，
/// 零目标且未置 `CMD_FLAG_ENABLE` 的帧解释为模式命令，否则解释为 dq 目标。提交标定
/// 需要调用方提供 `CalibPayload`（帧里没有空间），缺失即报错而不是编造零载荷。
pub fn cmd_from_frame(frame: &CmdFrame, calib: Option<CalibPayload>) -> Result<Cmd, WireError> {
    if frame.flags & CMD_FLAG_CLEAR_FAULT != 0 {
        return Ok(Cmd::ClearFault);
    }
    if frame.flags & CMD_FLAG_COMMIT_CALIB != 0 {
        return calib
            .map(Cmd::CommitCalib)
            .ok_or(WireError::MissingCalibPayload);
    }
    if frame.flags & CMD_FLAG_BENCH_INJECT != 0 {
        let phase_sel = match frame.bench_phase {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => return Err(WireError::InvalidBenchPhase),
        };
        return Ok(Cmd::Bench {
            phase_sel,
            amps: f32::from(frame.bench_amps_10ma) * LSB_BENCH_AMPS,
        });
    }

    let mode = Mode::from_u8(frame.mode).ok_or(WireError::InvalidMode)?;
    if !mode.is_commandable() {
        return Err(WireError::InvalidMode);
    }
    if frame.i_d_ref_ma == 0 && frame.i_q_ref_ma == 0 && frame.flags & CMD_FLAG_ENABLE == 0 {
        return Ok(Cmd::SetMode(mode));
    }
    Ok(Cmd::SetTarget(DqTarget {
        d_amps: ma_to_amps(frame.i_d_ref_ma),
        q_amps: ma_to_amps(frame.i_q_ref_ma),
    }))
}

/// [`Cmd`] → [`CmdView`]，保留 `base` 的时间戳、序号、TTL 与模式。
///
/// 动作标志与台架字段由命令覆盖；`SetTarget` 置 `CMD_FLAG_ENABLE`，其余命令清掉它，
/// 避免"上一拍目标仍然有效"的隐式状态。
pub fn cmd_to_view(cmd: Cmd, mut base: CmdView) -> CmdView {
    base.i_dq_amps = [0.0, 0.0];
    base.bench_phase = 0xFF;
    base.bench_amps_10ma = 0;
    base.flags &=
        !(CMD_FLAG_ENABLE | CMD_FLAG_CLEAR_FAULT | CMD_FLAG_COMMIT_CALIB | CMD_FLAG_BENCH_INJECT);

    match cmd {
        Cmd::SetMode(mode) => base.mode = mode.as_u8(),
        Cmd::SetTarget(target) => {
            base.i_dq_amps = [target.d_amps, target.q_amps];
            base.flags |= CMD_FLAG_ENABLE;
        }
        Cmd::CommitCalib(_) => base.flags |= CMD_FLAG_COMMIT_CALIB,
        Cmd::Bench { phase_sel, amps } => {
            base.bench_phase = phase_sel;
            base.bench_amps_10ma = amps_to_10ma_i8(amps);
            base.flags |= CMD_FLAG_BENCH_INJECT;
        }
        Cmd::ClearFault => base.flags |= CMD_FLAG_CLEAR_FAULT,
    }
    base
}

fn amps_to_10ma_i8(amps: f32) -> i8 {
    let scaled = amps / LSB_BENCH_AMPS;
    if !scaled.is_finite() {
        return 0;
    }
    let rounded = if scaled.is_sign_negative() {
        scaled - 0.5
    } else {
        scaled + 0.5
    };
    rounded as i8
}

fn round_sat_i16(x: f32) -> i16 {
    if x.is_nan() {
        return 0;
    }
    if !x.is_finite() {
        return if x.is_sign_negative() {
            i16::MIN
        } else {
            i16::MAX
        };
    }
    let rounded = if x.is_sign_negative() {
        x - 0.5
    } else {
        x + 0.5
    };
    rounded as i16
}

fn round_sat_u16(x: f32) -> u16 {
    if x.is_nan() || x <= 0.0 {
        return 0;
    }
    if !x.is_finite() || x >= f32::from(u16::MAX) {
        return u16::MAX;
    }
    (x + 0.5) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::joint::protocol_icd::{CmdFrame, ICD_VERSION};

    fn frame() -> CmdFrame {
        CmdFrame {
            mode: Mode::Run.as_u8(),
            flags: 0,
            ttl_ms: 20,
            i_d_ref_ma: 0,
            i_q_ref_ma: 500,
            seq: 7,
            bench_phase: 0xFF,
            bench_amps_10ma: 0,
            ver: ICD_VERSION,
            rsvd: [0; 3],
        }
    }

    #[test]
    fn physical_scalings_round_and_saturate() {
        assert_eq!(amps_to_ma(0.4994), 499);
        assert_eq!(amps_to_ma(0.4996), 500);
        assert_eq!(amps_to_ma(-0.4996), -500);
        assert_eq!(amps_to_ma(f32::NAN), 0);
        assert_eq!(amps_to_ma(1e9), i16::MAX);
        assert_eq!(amps_to_ma(-1e9), i16::MIN);
        assert_eq!(ma_to_amps(-500), -0.5);

        assert_eq!(rad_to_theta_u16(0.0), 0);
        assert_eq!(rad_to_theta_u16(core::f32::consts::FRAC_PI_2), 16_384);
        assert_eq!(rad_to_theta_u16(core::f32::consts::PI), 32_768);
        assert_eq!(rad_to_theta_u16(-core::f32::consts::FRAC_PI_2), 49_152);
        assert_eq!(rad_to_theta_u16(core::f32::consts::TAU), 0);
        assert_eq!(rad_to_theta_u16(f32::NAN), 0);

        assert_eq!(det_to_1e5(0.25), 25_000);
        assert_eq!(det_to_1e5(-1.0), 0);
        assert_eq!(det_to_1e5(f32::INFINITY), u16::MAX);
        assert_eq!(z_to_1e4(0.02), 200);
        assert_eq!(gain_to_q14(1.0), 16_384);
        assert_eq!(gain_to_q14(1.5), 24_576);
        assert_eq!(gain_to_q14(100.0), u16::MAX);
    }

    #[test]
    fn duty_norm_and_clamped_modulation_round_trip() {
        let min = (f32::from(hw::PWM_ARR) * (1.0 - hw::D_MAX)) as u16;
        let max = (f32::from(hw::PWM_ARR) * hw::D_MAX) as u16;
        for duty in [0, 850, 2_125, 3_400, hw::PWM_ARR] {
            let expected = duty.clamp(min, max);
            let m = duty_to_modulation(duty);
            let back = modulation_to_duty_norm(m) * f32::from(hw::PWM_ARR);
            assert!(
                (back - f32::from(expected)).abs() <= 1.0,
                "duty {duty} clamped {expected} -> {back}"
            );
            assert!(m.get().raw().abs() <= crate::motor::protocol_types::MODULATION_MAX_RAW);
        }
        let [a, b, c] = duty_words_to_modulations([0, 3_400, hw::PWM_ARR]);
        assert_eq!(
            a.get().raw(),
            -crate::motor::protocol_types::MODULATION_MAX_RAW
        );
        assert_eq!(
            c.get().raw(),
            crate::motor::protocol_types::MODULATION_MAX_RAW
        );
        assert_eq!(
            b.get().raw(),
            crate::motor::protocol_types::MODULATION_MAX_RAW
        );
    }

    #[test]
    fn frame_precedence_is_explicit() {
        let mut f = frame();
        f.flags = CMD_FLAG_CLEAR_FAULT | CMD_FLAG_BENCH_INJECT;
        assert_eq!(cmd_from_frame(&f, None), Ok(Cmd::ClearFault));

        f.flags = CMD_FLAG_COMMIT_CALIB;
        assert_eq!(
            cmd_from_frame(&f, None),
            Err(WireError::MissingCalibPayload)
        );
        let payload = CalibPayload {
            zero_ma: [1, 2, 3],
            gain_q14: [16_384, 16_000],
        };
        assert_eq!(
            cmd_from_frame(&f, Some(payload)),
            Ok(Cmd::CommitCalib(payload))
        );

        f.flags = CMD_FLAG_BENCH_INJECT;
        f.bench_phase = 2;
        f.bench_amps_10ma = -12;
        assert_eq!(
            cmd_from_frame(&f, None),
            Ok(Cmd::Bench {
                phase_sel: 2,
                amps: -0.12
            })
        );

        f.flags = CMD_FLAG_ENABLE;
        f.bench_phase = 0xFF;
        assert_eq!(
            cmd_from_frame(&f, None),
            Ok(Cmd::SetTarget(DqTarget {
                d_amps: 0.0,
                q_amps: 0.5
            }))
        );

        f.flags = 0;
        f.i_q_ref_ma = 0;
        assert_eq!(
            cmd_from_frame(&f, None),
            Ok(Cmd::SetMode(Mode::Run)),
            "零目标且未使能 = 模式命令"
        );
    }

    #[test]
    fn cmd_to_view_keeps_ttl_and_bench_fields() {
        let base = CmdView {
            t_us: 0x1122_3344,
            i_dq_amps: [9.0, 9.0],
            mode: Mode::Standby.as_u8(),
            seq: 0xA5,
            flags: CMD_FLAG_ENABLE,
            rsvd0: 0,
            ttl_ms: 20,
            bench_phase: 0xFF,
            bench_amps_10ma: 0,
        };
        let bench = cmd_to_view(
            Cmd::Bench {
                phase_sel: 1,
                amps: 0.25,
            },
            base,
        );
        assert_eq!(bench.ttl_ms, 20);
        assert_eq!(bench.seq, 0xA5);
        assert_eq!(bench.bench_phase, 1);
        assert_eq!(bench.bench_amps_10ma, 25);
        assert_eq!(bench.flags, CMD_FLAG_BENCH_INJECT);
        assert_eq!(bench.i_dq_amps, [0.0, 0.0]);

        let target = cmd_to_view(
            Cmd::SetTarget(DqTarget {
                d_amps: 0.1,
                q_amps: -0.2,
            }),
            base,
        );
        assert_eq!(target.flags, CMD_FLAG_ENABLE);
        assert_eq!(target.i_dq_amps, [0.1, -0.2]);
        assert_eq!(target.bench_phase, 0xFF);
        assert_eq!(target.bench_amps_10ma, 0);
    }
}
