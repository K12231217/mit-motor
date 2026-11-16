//! 故障码与 32 bit 状态字。
//!
//! # 唯一真源
//!
//! 位定义**必须**与 `docs/hardware-requirements.md` §6 一致（PRE-10）。
//! 改位定义 = 改线协议 = 必须同步上位机与 LA 对时脚本。
//!
//! | 位 | 含义 |
//! |---|---|
//! | 0–3 | 模式：0 自检 / 1 待机 / 2 标定 / 3 运行 / 4 台架 / 5 故障 |
//! | 4–6 | 标定状态：Idle / Collecting / Converged / Frozen / LatchedFault |
//! | 7 | 采样有效（窗口内且三路对齐） |
//! | 8 | 采样窗口判据通过 |
//! | 9 | 激励充分 |
//! | 10 | 跨工况一致 |
//! | 11–14 | 编码器状态（E1/E2 各自 OK/异常） |
//! | 16–23 | 故障码 |
//! | 24–31 | 帧序号低 8 位（LA 与帧对时用） |
//!
//! LED 只能看粗状态，判据一律以状态字为准。

use crate::types::Mode;

/// 故障码（状态字 bit16–23）。`None = 0` 表示无故障。
///
/// 编码是线协议的一部分：新码只能追加，不能重排。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FaultCode {
    /// 0 无故障。
    None = 0,
    /// 1 母线过压（OVP）。
    OverVoltage = 1,
    /// 2 母线欠压。
    UnderVoltage = 2,
    /// 3 过流（硬件比较器或软件阈值）。
    OverCurrent = 3,
    /// 4 过温（NTC）。
    OverTemp = 4,
    /// 5 MP6540 nFAULT 拉低。
    NfaultPin = 5,
    /// 6 编码器帧 CRC-6 或帧格式错。
    EncoderCrc = 6,
    /// 7 时钟树读回校验失败（PLLN/PLLR/VOS 不符）。
    ClockMismatch = 7,
    /// 8 ADC 注入组转换未在窗口内完成。
    AdcTimeout = 8,
    /// 9 标定被锁存（跨工况不一致 / 闸门反复失败）。
    CalibLatched = 9,
    /// 10 通信看门狗超时（> `limits::CAN_TIMEOUT_MS`）。
    CommTimeout = 10,
    /// 11 HardFault / panic 入口（故障现场保留路径）。
    HardFault = 11,
}

impl FaultCode {
    /// 纯取值：状态字 bit16–23 的编号。
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// 纯构造：编号 → 故障码；未知编号返回 `None`（上位机兼容用）。
    pub const fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::None),
            1 => Some(Self::OverVoltage),
            2 => Some(Self::UnderVoltage),
            3 => Some(Self::OverCurrent),
            4 => Some(Self::OverTemp),
            5 => Some(Self::NfaultPin),
            6 => Some(Self::EncoderCrc),
            7 => Some(Self::ClockMismatch),
            8 => Some(Self::AdcTimeout),
            9 => Some(Self::CalibLatched),
            10 => Some(Self::CommTimeout),
            11 => Some(Self::HardFault),
            _ => None,
        }
    }
}

/// 标定状态（状态字 bit4–6）。与 `CalibStep`（C0–C6）正交：
/// `CalibStep` 说"在做哪一步"，`CalibState` 说"这批样本结论如何"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CalibState {
    /// 0 空闲。
    Idle = 0,
    /// 1 采集中。
    Collecting = 1,
    /// 2 已收敛（参数可用，未冻结）。
    Converged = 2,
    /// 3 已冻结（当前生效）。
    Frozen = 3,
    /// 4 触发锁存故障（必须显式清故障）。
    LatchedFault = 4,
}

impl CalibState {
    /// 纯构造：编号 → 状态；0–4 之外返回 `None`。
    pub const fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Idle),
            1 => Some(Self::Collecting),
            2 => Some(Self::Converged),
            3 => Some(Self::Frozen),
            4 => Some(Self::LatchedFault),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 位定义（改这里 = 改线协议）
// ---------------------------------------------------------------------------

/// 模式字段起始位。
pub const MODE_SHIFT: u32 = 0;
/// 模式字段掩码（4 bit）。
pub const MODE_MASK: u32 = 0xF;
/// 标定状态字段起始位。
pub const CALIB_SHIFT: u32 = 4;
/// 标定状态字段掩码（3 bit）。
pub const CALIB_MASK: u32 = 0x7;
/// 采样有效位：窗口内且三路对齐。
pub const SAMPLE_VALID_BIT: u32 = 7;
/// 采样窗口判据通过位。
pub const WINDOW_OK_BIT: u32 = 8;
/// 激励充分位（对应闸门 `R`/`D` 门）。
pub const EXCITATION_OK_BIT: u32 = 9;
/// 跨工况一致位（对应 χ² 门）。
pub const CONSISTENT_BIT: u32 = 10;
/// 编码器状态字段起始位。
pub const ENCODER_SHIFT: u32 = 11;
/// 编码器状态字段掩码（4 bit）。
pub const ENCODER_MASK: u32 = 0xF;
/// E1 采样 OK。
pub const ENC1_OK_BIT: u32 = 11;
/// E1 异常（CRC/无响应）。
pub const ENC1_FAULT_BIT: u32 = 12;
/// E2 采样 OK。
pub const ENC2_OK_BIT: u32 = 13;
/// E2 异常。
pub const ENC2_FAULT_BIT: u32 = 14;
/// 故障码字段起始位。
pub const FAULT_SHIFT: u32 = 16;
/// 故障码字段掩码（8 bit）。
pub const FAULT_MASK: u32 = 0xFF;
/// 帧序号字段起始位。
pub const SEQ_SHIFT: u32 = 24;
/// 帧序号字段掩码（8 bit）。
pub const SEQ_MASK: u32 = 0xFF;

/// 32 bit 状态字。纯位域读写，无策略。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct StatusWord(u32);

impl StatusWord {
    /// 全零状态字（模式 = 0 自检、无故障）。
    pub const ZERO: Self = Self(0);

    /// 纯构造：从原始 u32 取。
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// 纯取值：原始 u32（写进 `FastView::state` 与 FB 帧）。
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// 模式编号（bit0–3）。
    pub const fn mode_bits(self) -> u8 {
        ((self.0 >> MODE_SHIFT) & MODE_MASK) as u8
    }

    /// 标定状态编号（bit4–6）。
    pub const fn calib_bits(self) -> u8 {
        ((self.0 >> CALIB_SHIFT) & CALIB_MASK) as u8
    }

    /// 采样有效位。
    pub const fn sample_valid(self) -> bool {
        bit_set(self.0, SAMPLE_VALID_BIT)
    }

    /// 采样窗口判据位。
    pub const fn window_ok(self) -> bool {
        bit_set(self.0, WINDOW_OK_BIT)
    }

    /// 激励充分位。
    pub const fn excitation_ok(self) -> bool {
        bit_set(self.0, EXCITATION_OK_BIT)
    }

    /// 跨工况一致位。
    pub const fn consistent(self) -> bool {
        bit_set(self.0, CONSISTENT_BIT)
    }

    /// 编码器 4 bit 状态（bit11–14）。
    pub const fn encoder_bits(self) -> u8 {
        ((self.0 >> ENCODER_SHIFT) & ENCODER_MASK) as u8
    }

    /// 故障码编号（bit16–23）。
    pub const fn fault_code_bits(self) -> u8 {
        ((self.0 >> FAULT_SHIFT) & FAULT_MASK) as u8
    }

    /// 帧序号低 8 位（bit24–31）。
    pub const fn frame_seq(self) -> u8 {
        ((self.0 >> SEQ_SHIFT) & SEQ_MASK) as u8
    }

    /// 模式枚举（未知编号返回 `None`，不静默继续）。
    pub const fn mode(self) -> Option<Mode> {
        Mode::from_u8(self.mode_bits())
    }

    /// 标定状态枚举。
    pub const fn calib_state(self) -> Option<CalibState> {
        CalibState::from_u8(self.calib_bits())
    }

    /// 故障码枚举。
    pub const fn fault_code(self) -> Option<FaultCode> {
        FaultCode::from_u8(self.fault_code_bits())
    }
}

/// 纯取值：某一位是否置位。
const fn bit_set(raw: u32, bit: u32) -> bool {
    raw & (1 << bit) != 0
}

// ---------------------------------------------------------------------------
// 编译期断言：字段不重叠、位置与 HR §6 一致
// ---------------------------------------------------------------------------

const _: () = assert!(core::mem::size_of::<StatusWord>() == 4);
const _: () = assert!(core::mem::size_of::<FaultCode>() == 1);
const _: () = assert!(core::mem::size_of::<CalibState>() == 1);

const _: () = assert!(MODE_MASK << MODE_SHIFT == 0x0000_000F);
const _: () = assert!(CALIB_MASK << CALIB_SHIFT == 0x0000_0070);
const _: () = assert!(FAULT_MASK << FAULT_SHIFT == 0x00FF_0000);
const _: () = assert!(SEQ_MASK << SEQ_SHIFT == 0xFF00_0000);

const _: () = assert!(MODE_SHIFT + 4 == CALIB_SHIFT);
const _: () = assert!(CALIB_SHIFT + 3 == SAMPLE_VALID_BIT);
const _: () = assert!(SAMPLE_VALID_BIT + 1 == WINDOW_OK_BIT);
const _: () = assert!(WINDOW_OK_BIT + 1 == EXCITATION_OK_BIT);
const _: () = assert!(EXCITATION_OK_BIT + 1 == CONSISTENT_BIT);
const _: () = assert!(CONSISTENT_BIT + 1 == ENCODER_SHIFT);
const _: () = assert!(ENCODER_SHIFT + 4 == 15);
const _: () = assert!(FAULT_SHIFT + 8 == SEQ_SHIFT);
const _: () = assert!(SEQ_SHIFT + 8 == 32);
