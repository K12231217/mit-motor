//! 队列事件与下行命令枚举（PRE-04）。
//!
//! RTIC 的 `spawn` 只有一个 pending 位，重发即丢，所以有序列的消息必须走有界队列。
//! 容量常量在 [`crate::limits`] 定义，这里只 re-export 名字。每条队列都配 `dropped`
//! 计数并进状态字。

use crate::faults::FaultCode;
use crate::types::{DqTarget, Mode};

pub use crate::limits::{CMD_Q, EVT_Q, LOG_Q, TRC_Q};

/// 标定提交载荷（定长、定点化、可 CRC）。
///
/// 与慢帧同一套缩放：零位 1 mA/LSB，相对增益 Q1.14（1.0 = 16384）。无标定档案路径，
/// 不写 flash，只活过复位（SRAM2 / 快照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct CalibPayload {
    /// 三相零位，毫安（1 mA/LSB）。
    pub zero_ma: [i16; 3],
    /// 相对增益 `[g_b, g_c]`，Q1.14（参考相 `g_a ≡ 1`）。
    pub gain_q14: [u16; 2],
    /// 上述字段的 CRC-16，防止半新半旧提交。
    pub crc: u16,
}

/// 下行命令（经队列时保留顺序）。
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Cmd {
    /// 切换模式。只能给出 `Mode::is_commandable()` 为真的编号（见 [`crate::icd`]）。
    SetMode(Mode),
    /// 更新 dq 电流目标，安培。
    SetTarget(DqTarget),
    /// 提交标定结果（需同时满足"无故障持续 T"与 `Trust::Trusted`）。
    CommitCalib(CalibPayload),
    /// 台架注入：指定相（0=A/1=B/2=C）与电流，安培。
    Bench {
        /// 相选择：0=A / 1=B / 2=C；其它值由消费端丢弃。
        phase_sel: u8,
        /// 注入电流，安培（由主机按 10 mA/LSB 转来）。
        amps: f32,
    },
    /// 清故障（还需"无故障持续 T"，且只在 `Mode::Fault` 下有效）。
    ClearFault,
}

/// 慢速任务之间的事件。
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Event {
    /// 收到一帧 CAN 命令（已通过 ID、长度与 `rsvd` 校验）。
    Can(Cmd),
    /// 遥测节拍（1 kHz）。
    TelemetryDue,
    /// 标定节拍（10 ms，与组窗 `G` 对齐）。
    CalibTick,
    /// 故障锁存（事件本身不可丢；队列满即设计错误）。
    Fault(FaultCode),
    /// 一批样本已就绪（数量饱和 u16）。
    SampleBatch(u16),
}

// 编译期断言：事件必须塞进 32 B 的队列槽（`docs/software-architecture.md` §8）。
const _: () = assert!(core::mem::size_of::<CalibPayload>() == 12);
const _: () = assert!(core::mem::offset_of!(CalibPayload, zero_ma) == 0);
const _: () = assert!(core::mem::offset_of!(CalibPayload, gain_q14) == 6);
const _: () = assert!(core::mem::offset_of!(CalibPayload, crc) == 10);
const _: () = assert!(core::mem::size_of::<Cmd>() <= 24);
const _: () = assert!(core::mem::size_of::<Event>() <= 32);
const _: () = assert!(CMD_Q.is_power_of_two());
const _: () = assert!(EVT_Q.is_power_of_two());
