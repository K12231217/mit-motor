//! 全工程接口事实表的唯一来源。
//!
//! 物理量与定点约定（[`types`]）、限值与容量（[`limits`]）、线上帧布局与状态字
//! （[`icd`]、[`faults`]）、跨优先级通道（[`snapshot`]、[`events`]）都只在这里定义一次，
//! 其它 crate 只许引用（PRE-07）。
//!
//! 零依赖、`no_std`、无堆、无不安全代码，host 上可直接 `cargo test`。每个线上结构都有
//! `size_of` 与逐字段 `offset_of!` 编译期断言：改字段会在这里编译失败，而不是在台架上
//! 表现为"数据看起来不对"。
//!
//! # 通道
//!
//! 跨优先级只有 [`snapshot`] 的 seqlock 与 [`events`] 的有界队列（PRE-04），都是单写者。

#![no_std]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(
    clippy::doc_markdown,
    clippy::too_long_first_doc_paragraph,
    clippy::doc_lazy_continuation
)]

pub mod events;
pub mod faults;
pub mod icd;
pub mod limits;
pub mod snapshot;
pub mod trust;
pub mod types;

pub use crate::events::{CalibPayload, Cmd, Event};
pub use crate::faults::{CalibState, FaultCode, StatusWord};
pub use crate::icd::{CmdFrame, DecodeError, FbFrame, SlowFrame, ICD_VERSION};
pub use crate::snapshot::{CmdSnapshot, CmdView, FastSnapshot, FastView, GainSnapshot, GainView};
pub use crate::trust::Trust;
pub use crate::types::{CalibStep, DqTarget, Duty, Mode, PhaseCurrent, RawSample3, Q15};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    //! 跨模块契约的可执行版本；字段级用例在各模块自己的测试模块里。

    use super::*;
    use crate::faults;
    use crate::icd;
    use crate::limits;
    use crate::types;

    fn sample_fast_view() -> FastView {
        FastView {
            t_us: 0x0102_0304_0506_0708,
            i_abc_amps: [1.0, -0.5, -0.5],
            i_dq_amps: [0.25, -1.5],
            duty_norm: [-0.6, 0.0, 0.6],
            theta_e_rad: 3.0,
            vbus_v: 24.0,
            isr_cycles: 4_200,
            state: 0x0A00_0000,
            margin_permille: 500,
            dropped: 2,
            seq_hint: 0xCAFE_BABE,
        }
    }

    fn sample_cmd_view() -> CmdView {
        CmdView {
            t_us: 0x1122_3344_5566_7788,
            i_dq_amps: [0.0, 0.75],
            mode: Mode::Run.as_u8(),
            seq: 0x5A,
            flags: icd::CMD_FLAG_ENABLE,
            rsvd0: 0,
            ttl_ms: limits::CAN_TIMEOUT_MS,
            rsvd1: 0,
        }
    }

    fn sample_gain_view() -> GainView {
        GainView {
            t_us: 0xDEAD_BEEF,
            gain_bc: [1.001, 0.999],
            zero_amps: [0.001, -0.001, 0.0],
            n_eq: 3_999,
            trust: icd::SLOW_TRUST_TRUSTED,
            step: CalibStep::C6Freeze.as_u8(),
            chi2: 9.21,
        }
    }

    fn sample_cmd_frame() -> CmdFrame {
        CmdFrame {
            mode: Mode::Standby.as_u8(),
            flags: icd::CMD_FLAG_ENABLE,
            ttl_ms: limits::CAN_TIMEOUT_MS,
            i_d_ref_ma: 0,
            i_q_ref_ma: 500,
            seq: 7,
            bench_phase: 0xFF,
            bench_amps_10ma: 0,
            ver: ICD_VERSION,
            rsvd: [0; 3],
        }
    }

    fn sample_fb_frame() -> FbFrame {
        FbFrame {
            state: 0,
            t_us: 1,
            isr_cycles: 4_000,
            vrefint: 1_500,
            i_abc_ma: [0, 0, 0],
            i_dq_ma: [0, 0],
            theta_e: 0,
            vel_crad_s: 0,
            vbus_10mv: 2_400,
            margin_permille: 250,
            dropped: 0,
            rsvd: 0,
        }
    }

    fn sample_slow_frame() -> SlowFrame {
        SlowFrame {
            g_ref_phase: 0,
            trust: icd::SLOW_TRUST_TRUSTED,
            ver: ICD_VERSION,
            rsvd0: 0,
            g_b_q14: icd::GAIN_Q14_ONE,
            g_c_q14: icd::GAIN_Q14_ONE,
            d_1e5: 200,
            z_1e4: 0,
            pred_pp_centi: 5,
            chi2_centi: 921,
            n_samples: 2_000,
            sigma_e_centi: 260,
            temp_dc: 250,
            rsvd2: 0,
            t_commit_us: 0,
            rsvd1: [0; 4],
        }
    }

    /// 测试专用：把 `raw` 的某字节改成 `value`（越界直接失败，不静默）。
    fn poke<const N: usize>(mut raw: [u8; N], at: usize, value: u8) -> [u8; N] {
        let Some(slot) = raw.get_mut(at) else {
            panic!("offset {at} out of range for {N} bytes")
        };
        *slot = value;
        raw
    }

    #[test]
    fn duty_coast_is_six_low_not_zero_modulation() {
        // 结论：MP6540HA 六路全低 = Hi-Z；Duty::Coast ≠ Duty::zero_modulation()。
        assert_ne!(Duty::coast(), Duty::zero_modulation());
        assert!(matches!(Duty::coast(), Duty::Coast));
        assert!(Duty::coast().is_within_limits());
        assert!(Duty::zero_modulation().is_within_limits());
    }

    #[test]
    fn duty_clamp_holds_d_max_0p80() {
        // 结论：D_MAX = 0.80 ⇒ |m| ≤ MODULATION_MAX = 0.6；量化边界是 19661。
        assert_eq!(limits::D_MAX, 0.80);
        assert!((types::MODULATION_MAX - 0.6).abs() < f32::EPSILON);
        assert_eq!(types::MODULATION_MAX_RAW, 19_661);

        let pwm = |m: i16| Duty::Pwm {
            a: Q15::from_raw(m),
            b: Q15::ZERO,
            c: Q15::ZERO,
        };
        assert!(pwm(types::MODULATION_MAX_RAW).is_within_limits());
        assert!(pwm(-types::MODULATION_MAX_RAW).is_within_limits());
        assert!(!pwm(types::MODULATION_MAX_RAW + 1).is_within_limits());
        assert!(!pwm(-types::MODULATION_MAX_RAW - 1).is_within_limits());
    }

    #[test]
    fn mode_and_calib_encoding_matches_status_word_bits() {
        // 结论：Mode 0–5 对应 bit0–3；CalibState 0–4 对应 bit4–6（HR §6）。
        for raw in 0u8..=5 {
            let Some(mode) = Mode::from_u8(raw) else {
                panic!("mode {raw} must exist")
            };
            assert_eq!(mode.as_u8(), raw);
            assert_eq!(StatusWord::from_raw(u32::from(raw)).mode(), Some(mode));
        }
        for raw in 0u8..=4 {
            let Some(state) = CalibState::from_u8(raw) else {
                panic!("calib state {raw} must exist")
            };
            let word = StatusWord::from_raw(u32::from(raw) << faults::CALIB_SHIFT);
            assert_eq!(word.calib_state(), Some(state));
            assert_eq!(word.mode_bits(), 0, "标定字段不得污染模式位");
        }
    }

    #[test]
    fn status_word_bit_fields_do_not_overlap() {
        // 结论：模式 0–3 / 标定 4–6 / 有效 7 / 窗口 8 / 激励 9 / 一致 10 /
        //       编码器 11–14 / 故障码 16–23 / 序号 24–31。
        let all = StatusWord::from_raw(u32::MAX);
        assert_eq!(all.mode_bits(), 0xF);
        assert_eq!(all.calib_bits(), 0x7);
        assert!(all.sample_valid());
        assert!(all.window_ok());
        assert!(all.excitation_ok());
        assert!(all.consistent());
        assert_eq!(all.encoder_bits(), 0xF);
        assert_eq!(all.fault_code_bits(), 0xFF);
        assert_eq!(all.frame_seq(), 0xFF);

        // 单独置位只影响自己那一段。
        let calib = StatusWord::from_raw(1 << faults::CALIB_SHIFT);
        assert_eq!(calib.calib_bits(), 1);
        assert_eq!(calib.mode_bits(), 0);
        assert_eq!(calib.encoder_bits(), 0);
        assert_eq!(calib.fault_code_bits(), 0);
        assert_eq!(calib.frame_seq(), 0);
        assert!(!calib.sample_valid());

        let fault =
            StatusWord::from_raw(u32::from(FaultCode::OverCurrent.as_u8()) << faults::FAULT_SHIFT);
        assert_eq!(fault.fault_code(), Some(FaultCode::OverCurrent));
        assert_eq!(fault.frame_seq(), 0);
        assert_eq!(fault.mode_bits(), 0);
    }

    #[test]
    fn slow_frame_trust_bits_complement_status_bits() {
        // 结论：慢帧 trust b1–b4 与状态字 bit9/bit10 及闸门维度一一对应。
        assert_eq!(icd::SLOW_TRUST_TRUSTED, 1 << 0);
        assert_eq!(icd::SLOW_TRUST_EXCITATION_OK, 1 << 1);
        assert_eq!(icd::SLOW_TRUST_AMPLITUDE_OK, 1 << 2);
        assert_eq!(icd::SLOW_TRUST_BIAS_OK, 1 << 3);
        assert_eq!(icd::SLOW_TRUST_CHANGE_OK, 1 << 4);
        assert_eq!(icd::SLOW_TRUST_COMMITTED, 1 << 5);
        assert_eq!(
            icd::SLOW_TRUST_TRUSTED
                | icd::SLOW_TRUST_EXCITATION_OK
                | icd::SLOW_TRUST_AMPLITUDE_OK
                | icd::SLOW_TRUST_BIAS_OK
                | icd::SLOW_TRUST_CHANGE_OK
                | icd::SLOW_TRUST_COMMITTED,
            0x3F
        );

        // 激励充分：trust b1 ↔ 状态字 bit9。
        assert!(StatusWord::from_raw(1 << faults::EXCITATION_OK_BIT).excitation_ok());
        // 跨工况一致（χ²）：trust b4 ↔ 状态字 bit10。
        assert!(StatusWord::from_raw(1 << faults::CONSISTENT_BIT).consistent());
        // 精度/偏置两维只在慢帧里表达，状态字没有对应位。
        assert_eq!(icd::SLOW_TRUST_RESERVED_MASK, 0xC0);
        assert_eq!(u32::from(icd::SLOW_TRUST_RESERVED_MASK) & 0x3F, 0);
    }

    #[test]
    fn can_command_frame_is_16_bytes() {
        // 结论：CMD 定长 16 B（字段偏移见 icd.rs 断言）。
        assert_eq!(core::mem::size_of::<CmdFrame>(), 16);
        assert_eq!(core::mem::size_of::<[u8; 16]>(), 16);
        assert_eq!(ICD_VERSION, 1);
    }

    #[test]
    fn can_feedback_frame_is_32_bytes() {
        // 结论：FB 定长 32 B，含状态字、时间戳、VREFINT、ISR cycles。
        assert_eq!(core::mem::size_of::<FbFrame>(), 32);
        assert_eq!(core::mem::offset_of!(FbFrame, state), 0);
        assert_eq!(core::mem::offset_of!(FbFrame, t_us), 4);
        assert_eq!(core::mem::offset_of!(FbFrame, rsvd), 30);
    }

    #[test]
    fn can_slow_frame_is_32_bytes() {
        // 结论：SLOW 定长 32 B；自然对齐（不是 packed），t_commit_us 在偏移 24。
        assert_eq!(core::mem::size_of::<SlowFrame>(), 32);
        assert_eq!(core::mem::align_of::<SlowFrame>(), 4);
        assert_eq!(core::mem::offset_of!(SlowFrame, rsvd2), 22);
        assert_eq!(core::mem::offset_of!(SlowFrame, t_commit_us), 24);
    }

    #[test]
    fn slow_frame_reports_relative_gain_only() {
        // 结论：g_b/g_c 是相对增益；共模增益不可观测，帧里没有平均增益字段。
        assert_eq!(core::mem::offset_of!(SlowFrame, g_b_q14), 4);
        assert_eq!(core::mem::offset_of!(SlowFrame, g_c_q14), 6);
        assert_eq!(icd::GAIN_Q14_ONE, 16_384);
        // 字段一直排到第 32 字节，没有剩余空间再塞一个"平均增益"。
        assert_eq!(core::mem::offset_of!(SlowFrame, rsvd1) + 4, 32);
        // 参考相 ≡ 1.0：慢帧只携带 B/C 两路相对增益。
        let frame = sample_slow_frame();
        assert_eq!(frame.g_b_q14, icd::GAIN_Q14_ONE);
        assert_eq!(frame.g_c_q14, icd::GAIN_Q14_ONE);
    }

    #[test]
    fn fast_snapshot_is_64_byte_payload() {
        // 结论：FastView = 64 B / 16 word；FastSnapshot = 4 + 64 B。
        assert_eq!(core::mem::size_of::<FastView>(), 64);
        assert_eq!(core::mem::size_of::<FastSnapshot>(), 68);
        assert_eq!(core::mem::size_of::<[u32; snapshot::FAST_WORDS]>(), 64);

        let view = sample_fast_view();
        let words = view.encode();
        assert_eq!(FastView::decode(&words).encode(), words);
    }

    #[test]
    fn snapshot_views_roundtrip_through_words() {
        // 结论：三个视图 encode→decode 必须逐位等价（f32 用 to_bits/from_bits）。
        let fast = sample_fast_view();
        assert_eq!(FastView::decode(&fast.encode()).encode(), fast.encode());

        let cmd = sample_cmd_view();
        assert_eq!(CmdView::decode(&cmd.encode()).encode(), cmd.encode());

        let gain = sample_gain_view();
        assert_eq!(GainView::decode(&gain.encode()).encode(), gain.encode());
    }

    #[test]
    fn event_fits_32_byte_queue_slot() {
        // 结论：size_of::<Event>() ≤ 32，size_of::<Cmd>() ≤ 24。
        assert!(core::mem::size_of::<Event>() <= 32);
        assert!(core::mem::size_of::<Cmd>() <= 24);
        assert_eq!(core::mem::size_of::<CalibPayload>(), 12);
        // 最大变体：Bench 携带 u8 + f32，CommitCalib 携带 12 B 载荷。
        let largest_event = Event::Can(Cmd::CommitCalib(CalibPayload {
            zero_ma: [0; 3],
            gain_q14: [icd::GAIN_Q14_ONE; 2],
            crc: 0,
        }));
        assert_eq!(
            core::mem::size_of_val(&largest_event),
            core::mem::size_of::<Event>()
        );
    }

    #[test]
    fn mt6701_frame_is_14_plus_4_plus_6_bits() {
        // 结论：帧 = 14b 角度 + 4b Mg + 6b CRC = 24 bit，SPI 10.625 MHz。
        assert_eq!(limits::ENC_FRAME_BITS, 24);
        assert_eq!(limits::ENC_ANGLE_BITS, 14);
        assert_eq!(limits::ENC_MAG_BITS, 4);
        assert_eq!(limits::ENC_CRC_BITS, 6);
        assert_eq!(
            limits::ENC_FRAME_BITS,
            limits::ENC_ANGLE_BITS + limits::ENC_MAG_BITS + limits::ENC_CRC_BITS
        );
        assert_eq!(limits::ENC_SPI_HZ, 10_625_000);
    }

    #[test]
    fn frame_decode_rejects_nonzero_reserved_bytes() {
        // 契约：三个帧的全部 rsvd* 非 0 → DecodeError::Rejected；解析路径不 panic。
        let raw = sample_cmd_frame().encode();
        let base = core::mem::offset_of!(CmdFrame, rsvd);
        for i in 0..3 {
            assert_eq!(
                CmdFrame::decode(&poke(raw, base + i, 1)),
                Err(DecodeError::Rejected),
                "CMD rsvd byte {i}"
            );
        }

        let raw = sample_fb_frame().encode();
        let base = core::mem::offset_of!(FbFrame, rsvd);
        for i in 0..2 {
            assert_eq!(
                FbFrame::decode(&poke(raw, base + i, 1)),
                Err(DecodeError::Rejected),
                "FB rsvd byte {i}"
            );
        }

        let raw = sample_slow_frame().encode();
        let rsvd0 = core::mem::offset_of!(SlowFrame, rsvd0);
        let rsvd2 = core::mem::offset_of!(SlowFrame, rsvd2);
        let rsvd1 = core::mem::offset_of!(SlowFrame, rsvd1);
        assert_eq!(
            SlowFrame::decode(&poke(raw, rsvd0, 1)),
            Err(DecodeError::Rejected),
            "SLOW rsvd0"
        );
        for i in 0..2 {
            assert_eq!(
                SlowFrame::decode(&poke(raw, rsvd2 + i, 1)),
                Err(DecodeError::Rejected),
                "SLOW rsvd2 byte {i}"
            );
        }
        for i in 0..4 {
            assert_eq!(
                SlowFrame::decode(&poke(raw, rsvd1 + i, 1)),
                Err(DecodeError::Rejected),
                "SLOW rsvd1 byte {i}"
            );
        }
    }

    #[test]
    fn status_word_unknown_codes_are_not_silently_accepted() {
        // 结论：未知模式/标定/故障编号一律 None，禁止静默继续（PRE-05）。
        for raw in 6u8..=15 {
            assert_eq!(Mode::from_u8(raw), None, "mode {raw}");
            assert_eq!(StatusWord::from_raw(u32::from(raw)).mode(), None);
        }
        for raw in 5u8..=7 {
            assert_eq!(CalibState::from_u8(raw), None, "calib {raw}");
            assert_eq!(
                StatusWord::from_raw(u32::from(raw) << faults::CALIB_SHIFT).calib_state(),
                None
            );
        }
        for raw in 12u8..=255 {
            assert_eq!(FaultCode::from_u8(raw), None, "fault {raw}");
        }
        assert_eq!(
            StatusWord::from_raw(u32::from(12u8) << faults::FAULT_SHIFT).fault_code(),
            None
        );
    }

    #[test]
    fn can_ids_are_derived_from_node_address() {
        // 结论：CMD=0x080|n、FB=0x100|n、SLOW=0x180|n，n ∈ 1..=127。
        assert_eq!(icd::NODE_ADDR_MIN, 1);
        assert_eq!(icd::NODE_ADDR_MAX, 127);
        assert_eq!(icd::cmd_id(1), 0x081);
        assert_eq!(icd::cmd_id(127), 0x0FF);
        assert_eq!(icd::fb_id(1), 0x101);
        assert_eq!(icd::fb_id(127), 0x17F);
        assert_eq!(icd::slow_id(1), 0x181);
        assert_eq!(icd::slow_id(127), 0x1FF);
    }
}
