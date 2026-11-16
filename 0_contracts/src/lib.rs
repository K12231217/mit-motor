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

    #[test]
    fn duty_coast_is_six_low_not_zero_modulation() {
        // 结论：MP6540HA 六路全低 = Hi-Z；Duty::Coast ≠ Duty::zero_modulation()。
        assert_ne!(Duty::coast(), Duty::zero_modulation());
        assert!(matches!(Duty::coast(), Duty::Coast));
        assert!(Duty::coast().is_within_limits());
        assert!(Duty::zero_modulation().is_within_limits());
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
}
