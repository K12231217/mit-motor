//! 增益闸门的线协议结论：只有枚举与判据。
//!
//! 估计器（`GateParams` / `GainAccum` / `GainGate`）在 `user` crate 里。这里只留
//! 线上要编码的那一个字节。

/// 相对增益的可信结论。
///
/// `Unknown = 0` 是**未初始化状态**，不是"可信"：全零快照、全零状态字与复位后的
/// 默认值都必须落在这里。任一维不通过就不允许提交新的相对增益。变体编号写进
/// `GainView::trust`（`0 = Unknown`），状态位与慢帧位由 [`Self::status_bits`] /
/// [`Self::slow_bits`] 派生。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Trust {
    /// 尚未产生任何闸门结论（全零快照的默认值）。
    Unknown = 0,
    /// 四维全过且 χ² 通过，允许冻结或更新增益。
    Trusted = 1,
    /// 方向覆盖 `R` 或信息矩阵 `D` 不通过：方向单一、共线或秩亏，参数不可辨识。
    UntrustedExcitation = 2,
    /// `pred > KAPPA · target_pp`：估计值可能正确，但精度不足以使用。
    UntrustedAmplitude = 3,
    /// `z > Z_MAX`：DC 主导窗口，偏置会被学成增益。
    UntrustedBias = 4,
    /// χ² 超限：与已冻结参数跨工况不一致（样本变了或在漂）。
    UntrustedChange = 5,
}

impl Trust {
    /// 编号 → 结论；未知编号返回 `None`（线协议版本探测，不静默继续）。
    pub const fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Unknown),
            1 => Some(Self::Trusted),
            2 => Some(Self::UntrustedExcitation),
            3 => Some(Self::UntrustedAmplitude),
            4 => Some(Self::UntrustedBias),
            5 => Some(Self::UntrustedChange),
            _ => None,
        }
    }

    /// 是否允许更新或冻结增益。
    pub const fn is_trusted(self) -> bool {
        matches!(self, Self::Trusted)
    }

    /// 状态字 bit9（激励充分）与 bit10（跨工况一致）。
    ///
    /// 只置位**确定已通过**的维度：失败变体保留此前维度的证据，不猜测后续门的结果。
    pub const fn status_bits(self) -> u32 {
        use crate::joint::protocol_faults::{CONSISTENT_BIT, EXCITATION_OK_BIT};

        let excitation = 1u32 << EXCITATION_OK_BIT;
        let consistent = 1u32 << CONSISTENT_BIT;
        match self {
            Self::Trusted | Self::UntrustedAmplitude => excitation | consistent,
            Self::UntrustedBias | Self::UntrustedChange => excitation,
            Self::Unknown | Self::UntrustedExcitation => 0,
        }
    }

    /// 慢帧 `trust` 位（`SLOW_TRUST_*`）。
    ///
    /// 与状态字同口径：只表达确定通过的维度。`COMMITTED` 只在 [`Self::Trusted`] 置位；
    /// 慢帧的逐维位不能由一个枚举唯一还原时，宁可少报也不虚报。
    pub const fn slow_bits(self) -> u8 {
        use crate::joint::protocol_icd::{
            SLOW_TRUST_AMPLITUDE_OK, SLOW_TRUST_BIAS_OK, SLOW_TRUST_CHANGE_OK,
            SLOW_TRUST_COMMITTED, SLOW_TRUST_EXCITATION_OK, SLOW_TRUST_TRUSTED,
        };

        match self {
            Self::Trusted => {
                SLOW_TRUST_TRUSTED
                    | SLOW_TRUST_EXCITATION_OK
                    | SLOW_TRUST_AMPLITUDE_OK
                    | SLOW_TRUST_BIAS_OK
                    | SLOW_TRUST_CHANGE_OK
                    | SLOW_TRUST_COMMITTED
            }
            Self::UntrustedAmplitude => {
                SLOW_TRUST_EXCITATION_OK | SLOW_TRUST_BIAS_OK | SLOW_TRUST_CHANGE_OK
            }
            Self::UntrustedChange => SLOW_TRUST_EXCITATION_OK | SLOW_TRUST_BIAS_OK,
            Self::UntrustedBias => SLOW_TRUST_EXCITATION_OK,
            Self::Unknown | Self::UntrustedExcitation => 0,
        }
    }
}

// 慢帧 `trust` 字段是 1 B，`#[repr(u8)]` 必须名副其实。
const _: () = assert!(core::mem::size_of::<Trust>() == 1);

#[cfg(test)]
mod tests {
    use super::Trust;

    #[test]
    fn unknown_is_the_zero_value_and_never_trusted() {
        assert_eq!(Trust::Unknown as u8, 0);
        assert!(!Trust::Unknown.is_trusted());
        assert_eq!(Trust::from_u8(0), Some(Trust::Unknown));
        assert_eq!(Trust::from_u8(6), None);
        assert_eq!(Trust::from_u8(255), None);
    }

    #[test]
    fn status_and_slow_bits_only_claim_passed_dimensions() {
        use crate::joint::protocol_faults::{CONSISTENT_BIT, EXCITATION_OK_BIT};
        use crate::joint::protocol_icd::{
            SLOW_TRUST_AMPLITUDE_OK, SLOW_TRUST_BIAS_OK, SLOW_TRUST_CHANGE_OK,
            SLOW_TRUST_COMMITTED, SLOW_TRUST_EXCITATION_OK, SLOW_TRUST_TRUSTED,
        };

        assert_eq!(Trust::Unknown.status_bits(), 0);
        assert_eq!(Trust::UntrustedExcitation.status_bits(), 0);
        assert_eq!(
            Trust::UntrustedBias.status_bits(),
            1 << EXCITATION_OK_BIT,
            "z 门失败时 R/D 已通过"
        );
        assert_eq!(
            Trust::UntrustedChange.status_bits(),
            1 << EXCITATION_OK_BIT,
            "χ² 失败不得报一致"
        );
        assert_eq!(
            Trust::Trusted.status_bits(),
            (1 << EXCITATION_OK_BIT) | (1 << CONSISTENT_BIT)
        );

        assert_eq!(Trust::Unknown.slow_bits(), 0);
        assert_eq!(Trust::UntrustedBias.slow_bits(), SLOW_TRUST_EXCITATION_OK);
        assert_eq!(
            Trust::UntrustedAmplitude.slow_bits(),
            SLOW_TRUST_EXCITATION_OK | SLOW_TRUST_BIAS_OK | SLOW_TRUST_CHANGE_OK
        );
        assert_eq!(
            Trust::Trusted.slow_bits(),
            SLOW_TRUST_TRUSTED
                | SLOW_TRUST_EXCITATION_OK
                | SLOW_TRUST_AMPLITUDE_OK
                | SLOW_TRUST_BIAS_OK
                | SLOW_TRUST_CHANGE_OK
                | SLOW_TRUST_COMMITTED
        );
        assert_eq!(Trust::Trusted.slow_bits() & 0xC0, 0, "保留位不得置位");
    }
}
