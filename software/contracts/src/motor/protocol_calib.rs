//! 标定档案记录：固定 48 字节、CRC-32、A/B 双槽选择。
//!
//! 记录只放**事实**（零位、相对增益、标定时的母线 OVP 阈值、板 UID 与序号），
//! 不含策略。落盘位置与页大小在 `config_hw`（bank 2，两槽各 2 页）；本模块只
//! 定义字节布局与校验，读写由 `bsp::motor::service_calib` 负责。
//!
//! 布局（小端）：
//!
//! | 偏移 | 字段 | 类型 |
//! |---|---|---|
//! | 0 | `magic` | u32 = `0x4341_4C42`（"BALC" 反序，便于 hexdump 认出 CALB）|
//! | 4 | `version` | u16 |
//! | 6 | `rsvd0` | u16 |
//! | 8 | `seq` | u32，单调递增，A/B 选新 |
//! | 12 | `board_uid` | [u32; 3]（96 bit）|
//! | 24 | `zero_ma` | [i16; 3] |
//! | 30 | `gain_q14` | [u16; 2]（参考相 `g_a ≡ 1`）|
//! | 34 | `ovp_mv` | u16 |
//! | 36 | `rsvd1` | [u8; 8] |
//! | 44 | `crc32` | u32，覆盖 0..44 |
//!
//! 48 字节 = 6 个 64 bit 双字，正好是 G474 的编程粒度。

use crate::motor::config_hw::{CALIB_SLOT_BYTES, FLASH_PAGE_BYTES};

/// 记录魔数：`0x4341_4C42`，小端字节序列为 `42 4C 41 43` = "BALC"。
pub const CALIB_MAGIC: u32 = 0x4341_4C42;
/// 当前记录版本。
pub const CALIB_VERSION: u16 = 1;
/// 记录总字节数（含 CRC 与保留区）。
pub const CALIB_RECORD_BYTES: usize = 48;
/// CRC 覆盖的字节数。
pub const CALIB_CRC_BYTES: usize = 44;

// 槽必须能放下至少一条记录；下面还要保证 48 B 是 8 B 的整数倍（双字编程）。
const _: () = assert!(CALIB_SLOT_BYTES >= CALIB_RECORD_BYTES as u32);
const _: () = assert!(CALIB_RECORD_BYTES.is_multiple_of(8));
const _: () = assert!(CALIB_CRC_BYTES + 4 == CALIB_RECORD_BYTES);
const _: () = assert!(FLASH_PAGE_BYTES >= CALIB_RECORD_BYTES as u32);

/// 记录解码/校验失败的原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibError {
    /// 魔数不符（空白页、别的东西）。
    Magic,
    /// 版本不认识。
    Version,
    /// CRC-32 不符（写一半掉电、位翻转）。
    Crc,
}

/// 一条标定记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibRecord {
    /// 单调递增序号；A/B 选择时大的胜。
    pub seq: u32,
    /// 芯片 96 bit UID，防止把别的板的档案当成自己的。
    pub board_uid: [u32; 3],
    /// 三相零位，毫安（1 mA/LSB）。
    pub zero_ma: [i16; 3],
    /// 相对增益 `[g_b, g_c]`，Q1.14（`g_a ≡ 1`）。
    pub gain_q14: [u16; 2],
    /// 标定时的母线 OVP 阈值，mV。
    pub ovp_mv: u16,
}

impl CalibRecord {
    /// 序列化并按 [`CALIB_CRC_BYTES`] 计算、写入 CRC-32。
    pub fn encode(&self) -> [u8; CALIB_RECORD_BYTES] {
        let mut out = [0u8; CALIB_RECORD_BYTES];
        put_u32(&mut out, 0, CALIB_MAGIC);
        put_u16(&mut out, 4, CALIB_VERSION);
        put_u32(&mut out, 8, self.seq);
        for (i, v) in self.board_uid.iter().enumerate() {
            put_u32(&mut out, 12 + 4 * i, *v);
        }
        for (i, v) in self.zero_ma.iter().enumerate() {
            put_u16(&mut out, 24 + 2 * i, *v as u16);
        }
        for (i, v) in self.gain_q14.iter().enumerate() {
            put_u16(&mut out, 30 + 2 * i, *v);
        }
        put_u16(&mut out, 34, self.ovp_mv);
        let crc = crc32_ieee(&out[..CALIB_CRC_BYTES]);
        put_u32(&mut out, CALIB_CRC_BYTES, crc);
        out
    }

    /// 解析并校验魔数、版本与 CRC。
    pub fn decode(raw: &[u8; CALIB_RECORD_BYTES]) -> Result<Self, CalibError> {
        if get_u32(raw, 0) != CALIB_MAGIC {
            return Err(CalibError::Magic);
        }
        if get_u16(raw, 4) != CALIB_VERSION {
            return Err(CalibError::Version);
        }
        if get_u32(raw, CALIB_CRC_BYTES) != crc32_ieee(&raw[..CALIB_CRC_BYTES]) {
            return Err(CalibError::Crc);
        }
        let mut board_uid = [0u32; 3];
        for (i, v) in board_uid.iter_mut().enumerate() {
            *v = get_u32(raw, 12 + 4 * i);
        }
        let mut zero_ma = [0i16; 3];
        for (i, v) in zero_ma.iter_mut().enumerate() {
            *v = get_u16(raw, 24 + 2 * i) as i16;
        }
        let mut gain_q14 = [0u16; 2];
        for (i, v) in gain_q14.iter_mut().enumerate() {
            *v = get_u16(raw, 30 + 2 * i);
        }
        Ok(Self {
            seq: get_u32(raw, 8),
            board_uid,
            zero_ma,
            gain_q14,
            ovp_mv: get_u16(raw, 34),
        })
    }
}

/// A/B 两槽里选一条可用记录：优先 `seq` 大的；相等时取 A（确定性）。
///
/// 只比较已经通过校验的记录；调用方负责丢弃 `None`。
pub fn pick(a: Option<CalibRecord>, b: Option<CalibRecord>) -> Option<CalibRecord> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.seq > a.seq { b } else { a }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// CRC-32（IEEE 802.3，反射多项式 `0xEDB8_8320`，初值/异或全 1）。
///
/// 逐位实现，host 与 M4 结果一致；48 B 记录只算一次，不进热路径。
pub fn crc32_ieee(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn put_u16(out: &mut [u8], at: usize, value: u16) {
    let bytes = value.to_le_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if let Some(slot) = out.get_mut(at + i) {
            *slot = *b;
        }
    }
}

fn put_u32(out: &mut [u8], at: usize, value: u32) {
    let bytes = value.to_le_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if let Some(slot) = out.get_mut(at + i) {
            *slot = *b;
        }
    }
}

fn get_u16(bytes: &[u8], at: usize) -> u16 {
    let mut buf = [0u8; 2];
    for (i, slot) in buf.iter_mut().enumerate() {
        if let Some(b) = bytes.get(at + i) {
            *slot = *b;
        }
    }
    u16::from_le_bytes(buf)
}

fn get_u32(bytes: &[u8], at: usize) -> u32 {
    let mut buf = [0u8; 4];
    for (i, slot) in buf.iter_mut().enumerate() {
        if let Some(b) = bytes.get(at + i) {
            *slot = *b;
        }
    }
    u32::from_le_bytes(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(seq: u32) -> CalibRecord {
        CalibRecord {
            seq,
            board_uid: [0x1122_3344, 0x5566_7788, 0x99AA_BBCC],
            zero_ma: [11, -22, 33],
            gain_q14: [16_384, 16_000],
            ovp_mv: 27_000,
        }
    }

    #[test]
    fn record_roundtrips_through_bytes() {
        let rec = sample(7);
        let raw = rec.encode();
        assert_eq!(CalibRecord::decode(&raw), Ok(rec));
    }

    #[test]
    fn record_rejects_bad_magic_version_and_crc() {
        let raw = sample(1).encode();

        let mut bad = raw;
        bad[0] ^= 0xFF;
        assert_eq!(CalibRecord::decode(&bad), Err(CalibError::Magic));

        let mut bad = raw;
        bad[4] = 9;
        assert_eq!(CalibRecord::decode(&bad), Err(CalibError::Version));

        let mut bad = raw;
        bad[24] ^= 0x01;
        assert_eq!(CalibRecord::decode(&bad), Err(CalibError::Crc));
    }

    #[test]
    fn pick_prefers_higher_seq_and_is_deterministic_on_tie() {
        let a = sample(3);
        let b = sample(4);
        assert_eq!(pick(Some(a), Some(b)), Some(b));
        assert_eq!(pick(Some(b), Some(a)), Some(b));
        assert_eq!(pick(Some(a), Some(a)), Some(a));
        assert_eq!(pick(None, Some(a)), Some(a));
        assert_eq!(pick(Some(b), None), Some(b));
        assert_eq!(pick(None, None), None);
    }
}
