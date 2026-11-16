//! 增益闸门的线协议结论：只有枚举与判据。
//!
//! 估计器（`GateParams` / `GainAccum` / `GainGate`）在 `user` crate 里。这里只留
//! 线上要编码的那一个字节。

/// 相对增益的可信结论。
///
/// 任一维不通过就不允许提交新的相对增益。变体编号直接写进慢帧 `trust` 位与状态字
/// bit9/bit10（见 [`crate::icd`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Trust {
    /// 四维全过且 χ² 通过，允许冻结或更新增益。
    Trusted = 0,
    /// 方向覆盖 `R` 或信息矩阵 `D` 不通过：激励不足或共线，参数不可辨识。
    UntrustedExcitation = 1,
    /// `pred > KAPPA · target_pp`：估计值可能正确，但精度不足以使用。
    UntrustedAmplitude = 2,
    /// `z > Z_MAX`：DC 主导窗口，偏置会被学成增益。
    UntrustedBias = 3,
    /// χ² 超限：与已冻结参数跨工况不一致（样本变了或在漂）。
    UntrustedChange = 4,
}

impl Trust {
    /// 是否允许更新或冻结增益。
    pub const fn is_trusted(self) -> bool {
        matches!(self, Self::Trusted)
    }
}

// 慢帧 `trust` 字段是 1 B，`#[repr(u8)]` 必须名副其实。
const _: () = assert!(core::mem::size_of::<Trust>() == 1);
