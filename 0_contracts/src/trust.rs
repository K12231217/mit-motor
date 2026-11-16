//! 增益闸门的**线协议结论**（只有枚举与判据，没有估计器）。
//!
//! 估计器（`GateParams`/`GainAccum`/`GainGate`/`sqrt_f32`）在 `3_user` 里：
//! 它们是运行时算数的算法，不属于接口事实表。这里只留线上要编码的那一个字节。

/// 闸门结论。只要有一维没过就**不允许**提交新的相对增益。
///
/// 变体与慢帧 `trust` 位、状态字 bit9/bit10 一一对应，编码固定（见 `icd.rs`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Trust {
    /// 四维全过且 χ² 通过：允许冻结/更新增益。
    Trusted = 0,
    /// 方向覆盖 `R` 或信息矩阵 `D` 不过：激励不足/共线，参数不可辨识。
    UntrustedExcitation = 1,
    /// `pred > KAPPA·target_pp`：精度不够，estimate 可能是对的但不可用。
    UntrustedAmplitude = 2,
    /// `z > Z_MAX`：DC 主导窗口，偏置会被学成增益。
    UntrustedBias = 3,
    /// χ² 超限：与已冻结参数跨工况不一致（样本变了或在漂）。
    UntrustedChange = 4,
}

impl Trust {
    /// 纯判据：是否可以更新/冻结增益。
    pub const fn is_trusted(self) -> bool {
        matches!(self, Self::Trusted)
    }
}

// 线上编码宽度：慢帧 trust 字段是 1 B，`#[repr(u8)]` 必须名副其实。
const _: () = assert!(core::mem::size_of::<Trust>() == 1);
