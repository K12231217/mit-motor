//! MT6701 24 bit 帧布局、CRC-6 与角度换算。
//!
//! ```text
//! bit  23..10   9..6   5..0
//!      D[13:0]    Mg     CRC
//! ```
//!
//! `D` 是 14 bit 绝对角（16384 计数 = 2π）；`Mg[1:0]` 是磁场码，`Mg[3]` 只当"角度
//! 无效"。CRC-6 为 `x^6 + x + 1`、初值 0、覆盖前 18 bit，MSB 先出。

use contracts::motor::config_hw as hw;

// 一圈的计数（u32 形态，便于移位与取模）。
pub(super) const COUNTS_PER_REV: u32 = hw::ENC_COUNTS_PER_REV as u32;

// CRC 域的掩码（6 bit）。
const CRC_MASK: u32 = (1 << hw::ENC_CRC_BITS) - 1;

// 每计数对应的弧度：`2π / 16384`；除数是 2 的幂，结果精确。
const RAD_PER_COUNT: f32 = core::f32::consts::TAU / COUNTS_PER_REV as f32;

/// 一帧 MT6701 的解码结果（CRC 已通过，尚未做语义判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// 14 bit 绝对角，16384 计数 = 2π。
    pub angle_raw: u16,
    /// 4 bit 状态字 `Mg`。
    pub mag: u8,
    /// 帧尾 6 bit CRC（已与计算值一致）。
    pub crc: u8,
}

impl Frame {
    /// `Mg[1:0]`：0 正常 / 1 磁场过强 / 2 磁场过弱 / 3 保留。
    pub const fn field_code(self) -> u8 {
        self.mag & 0b11
    }

    /// `Mg[3]`：超速 / 失锁。语义在手册版本间不一致，只当"角度无效"。
    pub const fn is_locked(self) -> bool {
        self.mag & 0b1000 != 0
    }
}

/// 解析阶段的错误（只管传输/完整性，语义判据在 [`super::EncoderMonitor`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// CRC-6 残差非 0：位翻转或 SPI 错位。
    Crc,
}

/// 计数 → 弧度。`angle_raw` 必须是 14 bit（[`Frame`] 的解析路径保证）。
pub fn angle_rad(angle_raw: u16) -> f32 {
    f32::from(angle_raw) * RAD_PER_COUNT
}

// CRC-6：`x^6 + x + 1`，初值 0，`payload` 为前 18 bit（D 与 Mg）。
// 24 bit 长除法：把 `payload << 6` 逐位除以 `0x43`，余数即 CRC。
pub(super) const fn crc6(payload: u32) -> u8 {
    const POLY: u32 = hw::ENC_CRC_POLY as u32;
    const SHIFT: u32 = hw::ENC_CRC_BITS as u32;
    const BITS: u32 = hw::ENC_FRAME_BITS as u32;
    let mut reg = payload << SHIFT;
    let mut i = BITS;
    while i > SHIFT {
        i -= 1;
        if (reg >> i) & 1 != 0 {
            reg ^= POLY << (i - SHIFT);
        }
    }
    (reg & CRC_MASK) as u8
}

/// 三个字节（MSB 先出）→ [`Frame`]，只校验 CRC。
///
/// # Errors
///
/// CRC-6 残差非 0 时返回 [`FrameError::Crc`]；语义判据由
/// [`super::EncoderMonitor`] 负责。
pub fn parse(raw: [u8; 3]) -> Result<Frame, FrameError> {
    let [hi, mid, lo] = raw;
    let word = (u32::from(hi) << 16) | (u32::from(mid) << 8) | u32::from(lo);
    let crc = (word & CRC_MASK) as u8;
    if crc6(word >> hw::ENC_CRC_BITS) != crc {
        return Err(FrameError::Crc);
    }
    Ok(Frame {
        angle_raw: (word >> (hw::ENC_MAG_BITS + hw::ENC_CRC_BITS)) as u16,
        mag: ((word >> hw::ENC_CRC_BITS) & 0x0F) as u8,
        crc,
    })
}

const _: () = assert!(COUNTS_PER_REV == 1 << hw::ENC_ANGLE_BITS);
