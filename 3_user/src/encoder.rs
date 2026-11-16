//! MT6701 24 bit 帧解析、接受判据机与去抖。
//!
//! # 分层
//!
//! `1_bsp` 只负责 SPI 搬运三个字节（10.625 MHz，MSB 先出）。帧布局、CRC-6、状态位
//! 语义与"这一帧能不能用"都是纯函数或纯状态机，放在这里，host 可测。
//!
//! # 帧
//!
//! ```text
//! bit  23..10   9..6   5..0
//!      D[13:0]    Mg     CRC
//! ```
//!
//! - `D`：14 bit 绝对角，16384 计数 = 2π（[`hw::ENC_COUNTS_PER_REV`]）。
//! - `Mg[1:0]`：0 正常 / 1 磁场过强 / 2 磁场过弱 / 3 保留；`Mg[2]` 按压或气隙突变；
//!   `Mg[3]` 超速或失锁。不同手册版本对 `Mg[3]` 的命名不一致，这里只当"角度无效"，
//!   不按名字区分原因。
//! - CRC-6：`x^6 + x + 1`，初值 0，覆盖前 18 bit（D 加 Mg），MSB 先出；等价于
//!   `(payload << 6) mod poly` 的余数，`crc6` 用 24 bit 长除法实现。
//!
//! # 接受判据
//!
//! 1. CRC 残差为 0，否则 [`Fault::Crc`]；
//! 2. `Mg[3] == 0`，否则 [`Fault::MagLock`]；
//! 3. 全 0 帧护栏：`0x000000` 是合法帧（θ=0、Mg=0、CRC=0），与 MISO 恒低不可区分；
//!    连续 [`policy::ENC_ZERO_FRAME_LIMIT`] 帧全 0 且此前接受角非 0 时判
//!    [`Fault::ZeroFrame`]。靠近回绕处的前几帧全 0 还能通过跳变判据，正是它抓这些帧；
//! 4. 相邻接受帧的角度差 `|Δθ| ≤ (missed + 1) · B`，`B` 由 [`jump_limit_lsb`] 按
//!    `ceil(ω_max · T_s / (2π/16384)) + 4` 算出，否则 [`Fault::Jump`]。
//!
//! 软告警：`Mg[1:0] != 0` 不拒帧，输出 [`Health::MagWarn`] 由上层降额；连续
//! [`policy::ENC_MAG_WARN_LIMIT`] 帧不消失升级为 [`Fault::MagStuck`]。
//!
//! # 去抖与丢失窗口
//!
//! CRC 错只说明这一帧丢了，不说明传感器坏：它只计入 `missed`，不增加语义故障的连续
//! 数。语义故障（`Mg[3]`、全 0 护栏、跳变、磁场异常升级）连续
//! [`policy::ENC_FAIL_LATCH_FRAMES`] 帧锁存。任何未接受帧都计入 `missed`，连续超过
//! [`policy::ENC_MAX_MISS`] 帧直接锁存。
//!
//! 跳变界随丢失拍数线性放宽为 `(missed + 1) · B`：`B` 本来就是一个周期的最大位移，
//! 丢了几拍就该按几拍算，否则一次 CRC 抖动后的恢复帧会被误判成跳变（20 kHz 下这类
//! 抖动不是偶发）。超过 `MAX_MISS` 拍就锁存：此时角度不是"稍微多走了一点"，而是完全
//! 不知道。
//!
//! 锁存后角度冻结在最后一次接受的样本，只有 [`EncoderMonitor::reset`] 能清除。

use crate::policy;
use contracts::limits::{budget, hw};

// 一圈的计数（u32 形态，便于移位与取模）。
const COUNTS_PER_REV: u32 = hw::ENC_COUNTS_PER_REV as u32;

// 半圈计数：有符号 14 bit 差值的回绕中点。
const HALF_TURN: i32 = COUNTS_PER_REV as i32 / 2;

// 一圈掩码：`diff & TURN_MASK` 得到 0..16383。
const TURN_MASK: i32 = COUNTS_PER_REV as i32 - 1;

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

/// 解析阶段的错误（只管传输/完整性，语义判据在 [`EncoderMonitor`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// CRC-6 残差非 0：位翻转或 SPI 错位。
    Crc,
}

/// 硬失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// CRC-6 校验不过。
    Crc,
    /// `Mg[3]` 置位：角度不可信。
    MagLock,
    /// 连续全 0 帧，且此前接受角非 0（MISO 恒低盲区护栏）。
    ZeroFrame,
    /// 相邻接受帧的角度变化超过 [`jump_limit_lsb`]。
    Jump,
    /// 磁场异常连续 [`policy::ENC_MAG_WARN_LIMIT`] 帧未消失。
    MagStuck,
}

/// 一拍的接受结论（健康度随角度一起上报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// 帧通过全部判据。
    Ok,
    /// 帧被接受，但 `Mg[1:0]` 报磁场异常；上层应降额使用。
    MagWarn {
        /// 原始 `Mg[1:0]` 码：1 过强 / 2 过弱 / 3 保留。
        code: u8,
    },
    /// 硬失败，连续计数未达门限；调用方应在同一拍内重读一次。
    Rejected(Fault),
    /// 硬失败已锁存，只有 [`EncoderMonitor::reset`] 能清除。
    Latched(Fault),
}

impl Health {
    /// 本拍是否硬失败（含未锁存的单帧失败）。
    pub const fn is_hard_failure(self) -> bool {
        matches!(self, Self::Rejected(_) | Self::Latched(_))
    }

    /// 硬失败是否已锁存。
    pub const fn is_latched(self) -> bool {
        matches!(self, Self::Latched(_))
    }
}

/// 一拍的输出：角度 + 健康度。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngleOutcome {
    /// 弧度，`[0, 2π)`。硬失败时保持最后一次接受的角度（冷启动未建立基准时为 0）。
    pub theta_rad: f32,
    /// 本拍健康度；非 [`Health::Ok`] 时上层不得按无故障继续。
    pub health: Health,
}

/// 计数 → 弧度。`angle_raw` 必须是 14 bit（[`Frame`] 的解析路径保证）。
pub fn angle_rad(angle_raw: u16) -> f32 {
    f32::from(angle_raw) * RAD_PER_COUNT
}

/// 单周期允许的最大角度变化 `B`（LSB）：
/// `ceil(ω_max · T_s / (2π/16384)) + ENC_JUMP_MARGIN_LSB`。
///
/// 用关节机械极限而不是 MT6701 的 55 000 rpm 传感器上限：传感器能转多快
/// 不重要，机械结构允许多快才决定"一拍内真实角度最多走多少计数"。
pub fn jump_limit_lsb(omega_max_rpm: f32, period_us: u32) -> u16 {
    let counts_per_second = omega_max_rpm / 60.0 * f32::from(hw::ENC_COUNTS_PER_REV);
    let counts_per_period = counts_per_second * (period_us as f32) * 1e-6;
    if !counts_per_period.is_finite() {
        return u16::MAX;
    }
    // NaN / 负数经 `as u32` 饱和为 0，落回"只有余量"的退化界。
    let whole = counts_per_period as u32;
    let ceiled = if counts_per_period > whole as f32 {
        whole.saturating_add(1)
    } else {
        whole
    };
    let total = ceiled.saturating_add(u32::from(policy::ENC_JUMP_MARGIN_LSB));
    total.min(u32::from(u16::MAX)) as u16
}

/// 跳变判据与去抖门限。计数类门限来自 [`policy`]，`jump_limit_lsb` 由
/// [`EncoderParams::new`] 按机械转速上限算出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderParams {
    /// 单周期允许的角度变化（LSB，含余量）。
    pub jump_limit_lsb: u16,
    /// 连续全 0 帧门限。
    pub zero_frame_limit: u8,
    /// 磁场异常连续帧门限。
    pub mag_warn_limit: u8,
    /// 硬失败锁存所需连续语义故障帧数。
    pub latch_frames: u8,
    /// 连续未接受帧上限；超过即锁存。
    pub max_miss: u8,
}

impl EncoderParams {
    /// 按机械转速上限（rpm）与环周期（µs）构造；计数门限取 [`policy`] 的默认值。
    pub fn new(omega_max_rpm: f32, period_us: u32) -> Self {
        Self {
            jump_limit_lsb: jump_limit_lsb(omega_max_rpm, period_us),
            zero_frame_limit: policy::ENC_ZERO_FRAME_LIMIT,
            mag_warn_limit: policy::ENC_MAG_WARN_LIMIT,
            latch_frames: policy::ENC_FAIL_LATCH_FRAMES,
            max_miss: policy::ENC_MAX_MISS,
        }
    }
}

impl Default for EncoderParams {
    /// `ω_max = ENC_MECH_RPM_MAX`（3000 rpm）、`T_s = ISR_PERIOD_US`（50 µs）。
    fn default() -> Self {
        Self::new(policy::ENC_MECH_RPM_MAX, budget::ISR_PERIOD_US)
    }
}

/// 有状态接受机：每拍输入三个原始字节，输出角度 + 健康度。
///
/// 只持有上一接受角、连续零帧 / 磁场告警 / 语义故障 / 丢失拍计数与锁存原因。
#[derive(Clone, Debug)]
pub struct EncoderMonitor {
    params: EncoderParams,
    last_angle_raw: Option<u16>,
    zero_run: u8,
    zero_run_after_nonzero: bool,
    mag_warn_run: u8,
    fail_run: u8,
    missed: u8,
    latched: Option<Fault>,
}

impl EncoderMonitor {
    /// 冷启动：无上一角度基准，锁存打开。
    pub fn new(params: EncoderParams) -> Self {
        Self {
            params,
            last_angle_raw: None,
            zero_run: 0,
            zero_run_after_nonzero: false,
            mag_warn_run: 0,
            fail_run: 0,
            missed: 0,
            latched: None,
        }
    }

    /// 推进一拍。`raw` 为 MSB 先出的三个字节。
    pub fn update(&mut self, raw: [u8; 3]) -> AngleOutcome {
        if let Some(reason) = self.latched {
            return AngleOutcome {
                theta_rad: self.hold_theta_rad(),
                health: Health::Latched(reason),
            };
        }

        // 全 0 帧计数与 CRC 结果无关：0x000000 的 CRC 恒通过。
        let is_zero_frame = raw == [0, 0, 0];
        if is_zero_frame {
            if self.zero_run == 0 {
                self.zero_run_after_nonzero = self.last_angle_raw.is_some_and(|a| a != 0);
            }
            self.zero_run = self.zero_run.saturating_add(1);
        } else {
            self.zero_run = 0;
        }

        let frame = match parse(raw) {
            Ok(frame) => frame,
            Err(FrameError::Crc) => return self.miss(Fault::Crc),
        };

        if frame.is_locked() {
            return self.reject(Fault::MagLock);
        }

        // 磁场恢复正常即打断"连续异常"计数（即使这一帧随后因跳变被拒）。
        let code = frame.field_code();
        if code == 0 {
            self.mag_warn_run = 0;
        }

        if is_zero_frame
            && self.zero_run_after_nonzero
            && self.zero_run >= self.params.zero_frame_limit
        {
            return self.reject(Fault::ZeroFrame);
        }

        if let Some(last) = self.last_angle_raw {
            // `B` 是单位时间的最大位移：距上一个接受帧过了 `missed + 1` 拍，
            // 界就乘这个拍数。饱和到 u16，窗口上限由 missed 锁存兜底。
            let periods = u16::from(self.missed).saturating_add(1);
            let allow = self.params.jump_limit_lsb.saturating_mul(periods);
            if wrap_delta(frame.angle_raw, last).unsigned_abs() > u32::from(allow) {
                return self.reject(Fault::Jump);
            }
        }

        if code != 0 {
            self.mag_warn_run = self.mag_warn_run.saturating_add(1);
            if self.mag_warn_run >= self.params.mag_warn_limit {
                return self.reject(Fault::MagStuck);
            }
            self.accept(frame);
            return AngleOutcome {
                theta_rad: angle_rad(frame.angle_raw),
                health: Health::MagWarn { code },
            };
        }

        self.accept(frame);
        AngleOutcome {
            theta_rad: angle_rad(frame.angle_raw),
            health: Health::Ok,
        }
    }

    /// 硬失败是否已锁存（锁存后 [`Self::update`] 恒定返回 [`Health::Latched`]）。
    pub fn is_latched(&self) -> bool {
        self.latched.is_some()
    }

    /// 锁存原因（无锁存时为 `None`）。
    pub const fn latched_fault(&self) -> Option<Fault> {
        self.latched
    }

    /// 距上一个接受帧已连续丢失的拍数（接受一帧即归零）。
    pub const fn missed_frames(&self) -> u8 {
        self.missed
    }

    /// 故障清除后重新建立基准；清空全部连续计数与锁存。
    pub fn reset(&mut self) {
        self.last_angle_raw = None;
        self.zero_run = 0;
        self.zero_run_after_nonzero = false;
        self.mag_warn_run = 0;
        self.fail_run = 0;
        self.missed = 0;
        self.latched = None;
    }

    // 接受一帧：更新基准角并清空丢失与语义故障连续计数。
    fn accept(&mut self, frame: Frame) {
        self.last_angle_raw = Some(frame.angle_raw);
        self.fail_run = 0;
        self.missed = 0;
    }

    // 记一次传输层丢失（CRC 错）：只增加丢失拍数，不增加语义故障连续数。
    fn miss(&mut self, fault: Fault) -> AngleOutcome {
        self.missed = self.missed.saturating_add(1);
        if self.missed > self.params.max_miss {
            return self.latch(fault);
        }
        AngleOutcome {
            theta_rad: self.hold_theta_rad(),
            health: Health::Rejected(fault),
        }
    }

    // 记一次语义硬失败：丢失拍数与语义故障连续数同时增加。
    //
    // 连续 `latch_frames` 帧，或丢失拍数超过 `max_miss` 拍，即锁存。
    fn reject(&mut self, fault: Fault) -> AngleOutcome {
        self.missed = self.missed.saturating_add(1);
        self.fail_run = self.fail_run.saturating_add(1);
        if self.fail_run >= self.params.latch_frames || self.missed > self.params.max_miss {
            return self.latch(fault);
        }
        AngleOutcome {
            theta_rad: self.hold_theta_rad(),
            health: Health::Rejected(fault),
        }
    }

    // 锁存：此后 `update` 恒定返回 `Health::Latched`，直到 `reset`。
    fn latch(&mut self, fault: Fault) -> AngleOutcome {
        self.latched = Some(fault);
        AngleOutcome {
            theta_rad: self.hold_theta_rad(),
            health: Health::Latched(fault),
        }
    }

    // 硬失败期间保持最后一次接受的角度。
    fn hold_theta_rad(&self) -> f32 {
        match self.last_angle_raw {
            Some(angle_raw) => angle_rad(angle_raw),
            None => 0.0,
        }
    }
}

// 有符号 14 bit 角度差：`16383 → 0` 是 `+1` 而不是 `-16383`。
fn wrap_delta(new: u16, old: u16) -> i32 {
    let diff = i32::from(new) - i32::from(old);
    ((diff + HALF_TURN) & TURN_MASK) - HALF_TURN
}

// CRC-6：`x^6 + x + 1`，初值 0，`payload` 为前 18 bit（D 与 Mg）。
//
// 24 bit 长除法：把 `payload << 6` 逐位除以 `0x43`，余数即 CRC。
const fn crc6(payload: u32) -> u8 {
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
/// CRC-6 残差非 0 时返回 [`FrameError::Crc`]；语义判据由 [`EncoderMonitor`] 负责。
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

// 常数与 `limits` 的一致性。
const _: () = assert!(COUNTS_PER_REV == 1 << hw::ENC_ANGLE_BITS);
const _: () = assert!(HALF_TURN * 2 == COUNTS_PER_REV as i32);

#[cfg(test)]
mod tests {
    use super::*;

    /// 24 bit 字 → 三个 MSB 先出的字节（高 8 bit 恒为 0）。
    fn bytes_of(word: u32) -> [u8; 3] {
        let [_, hi, mid, lo] = word.to_be_bytes();
        [hi, mid, lo]
    }

    /// 构造合法帧：14 bit 角 + 4 bit Mg + 6 bit CRC。
    fn encode(angle_raw: u16, mag: u8) -> [u8; 3] {
        let payload = (u32::from(angle_raw) << hw::ENC_MAG_BITS) | u32::from(mag & 0x0F);
        let word = (payload << hw::ENC_CRC_BITS) | u32::from(crc6(payload));
        bytes_of(word)
    }

    #[test]
    fn known_frame_parses_to_frozen_fields() {
        // 一手手册样例帧：θ=16030、Mg=0、CRC=0x1B。
        let word = 0x00FA_781B;
        assert_eq!(crc6(word >> hw::ENC_CRC_BITS), 0x1B);

        let raw = bytes_of(word);
        let frame = match parse(raw) {
            Ok(frame) => frame,
            Err(_) => panic!("样例帧必须通过 CRC"),
        };
        assert_eq!(frame.angle_raw, 16_030);
        assert_eq!(frame.mag, 0);
        assert_eq!(frame.crc, 0x1B);
        assert_eq!(frame.field_code(), 0);
        assert!(!frame.is_locked());

        let expected = 16_030.0 * core::f32::consts::TAU / f32::from(hw::ENC_COUNTS_PER_REV);
        assert!((angle_rad(frame.angle_raw) - expected).abs() < 1e-6);
    }

    #[test]
    fn all_zero_frame_is_a_legal_frame() {
        // 0x000000：θ=0、Mg=0、CRC=0，CRC 必须通过（盲区由判据机护栏负责）。
        assert_eq!(crc6(0), 0);
        let frame = match parse([0, 0, 0]) {
            Ok(frame) => frame,
            Err(_) => panic!("全 0 帧是合法帧"),
        };
        assert_eq!(frame.angle_raw, 0);
        assert_eq!(frame.mag, 0);
        assert_eq!(frame.crc, 0);
    }

    #[test]
    fn every_single_bit_flip_is_rejected() {
        // 24 个位置逐一翻转（角度 14 bit、Mg 4 bit、CRC 6 bit 全覆盖）。
        let word = 0x00FA_781Bu32;
        let mut pos = 0;
        while pos < u32::from(hw::ENC_FRAME_BITS) {
            let flipped = word ^ (1u32 << pos);
            assert_eq!(parse(bytes_of(flipped)), Err(FrameError::Crc), "bit {pos}");
            pos += 1;
        }
    }

    #[test]
    fn mag_lock_bit_is_a_hard_failure() {
        let raw = encode(16_030, 0b1000);
        let frame = match parse(raw) {
            Ok(frame) => frame,
            Err(_) => panic!("构造的帧必须通过 CRC"),
        };
        assert!(frame.is_locked());

        let mut monitor = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(monitor.update(raw).health, Health::Rejected(Fault::MagLock));
        assert_eq!(monitor.update(raw).health, Health::Latched(Fault::MagLock));
        assert!(monitor.is_latched());
        assert_eq!(monitor.latched_fault(), Some(Fault::MagLock));
    }

    #[test]
    fn zero_frame_after_nonzero_angle_fails_but_startup_zero_does_not() {
        // 冷启动就是 0：合法帧，连续 40 帧也不误报（此前没有非 0 角）。
        let mut startup = EncoderMonitor::new(EncoderParams::default());
        let mut i = 0;
        while i < 40 {
            let out = startup.update([0, 0, 0]);
            assert_eq!(out.health, Health::Ok, "startup zero {i}");
            assert_eq!(out.theta_rad, 0.0);
            i += 1;
        }
        assert!(!startup.is_latched());

        // 上一接受角非 0：全 0 帧进入硬失败（这里是跳变判据先抓；回绕护栏另有测试）。
        let mut monitor = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(monitor.update(encode(16_030, 0)).health, Health::Ok);
        let first = monitor.update([0, 0, 0]);
        assert!(first.health.is_hard_failure());
        assert!(!first.health.is_latched());
        // 连续 2 帧锁存。
        assert!(monitor.update([0, 0, 0]).health.is_latched());
    }

    #[test]
    fn zero_run_guard_catches_stuck_low_miso_near_wrap() {
        // 16380 → 0 是合法回绕（Δ=4），前 7 帧全 0 都能通过跳变判据；
        // 第 8 帧由全 0 计数抓出，这正是 MISO 恒低盲区的护栏。
        let mut monitor = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(monitor.update(encode(16_380, 0)).health, Health::Ok);
        let mut i = 0;
        while i < 7 {
            assert_eq!(
                monitor.update([0, 0, 0]).health,
                Health::Ok,
                "wrap zero {i}"
            );
            i += 1;
        }
        assert_eq!(
            monitor.update([0, 0, 0]).health,
            Health::Rejected(Fault::ZeroFrame)
        );
        assert_eq!(
            monitor.update([0, 0, 0]).health,
            Health::Latched(Fault::ZeroFrame)
        );
    }

    #[test]
    fn jump_over_limit_rejects_then_latches() {
        let mut monitor = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(monitor.update(encode(1_000, 0)).health, Health::Ok);

        let jumped = encode(6_000, 0);
        let first = monitor.update(jumped);
        assert_eq!(first.health, Health::Rejected(Fault::Jump));
        assert!(!first.health.is_latched());
        // 失败时不更新基准：输出保持上一接受角，控制环不会看到假跳变。
        assert!((first.theta_rad - angle_rad(1_000)).abs() < 1e-6);

        assert_eq!(monitor.update(jumped).health, Health::Latched(Fault::Jump));
    }

    #[test]
    fn wrap_around_pi_is_not_a_jump() {
        let mut forward = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(forward.update(encode(16_383, 0)).health, Health::Ok);
        assert_eq!(forward.update(encode(0, 0)).health, Health::Ok);

        let mut backward = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(backward.update(encode(0, 0)).health, Health::Ok);
        assert_eq!(backward.update(encode(16_383, 0)).health, Health::Ok);
    }

    #[test]
    fn weak_field_is_reported_but_not_rejected_until_persistent() {
        // Mg[1:0] = 2（磁场过弱）：不拒帧，健康度上报；连续 20 帧升级为硬失败。
        let weak = encode(1_000, 0b0010);
        let mut monitor = EncoderMonitor::new(EncoderParams::default());
        let mut i = 0;
        while i < 19 {
            let out = monitor.update(weak);
            assert_eq!(out.health, Health::MagWarn { code: 2 }, "frame {i}");
            assert!((out.theta_rad - angle_rad(1_000)).abs() < 1e-6);
            i += 1;
        }
        assert_eq!(
            monitor.update(weak).health,
            Health::Rejected(Fault::MagStuck)
        );
        assert_eq!(
            monitor.update(weak).health,
            Health::Latched(Fault::MagStuck)
        );
    }

    #[test]
    fn clean_frame_resets_magnetic_warning_streak() {
        let weak = encode(1_000, 0b0010);
        let clean = encode(1_000, 0);
        let mut monitor = EncoderMonitor::new(EncoderParams::default());

        let mut i = 0;
        while i < 10 {
            assert_eq!(
                monitor.update(weak).health,
                Health::MagWarn { code: 2 },
                "warm-up {i}"
            );
            i += 1;
        }
        assert_eq!(monitor.update(clean).health, Health::Ok);

        // 重置后的 19 帧告警仍在软告警区间（连续计数确实被清零）。
        let mut j = 0;
        while j < 19 {
            assert_eq!(
                monitor.update(weak).health,
                Health::MagWarn { code: 2 },
                "after reset {j}"
            );
            j += 1;
        }
        assert_eq!(
            monitor.update(weak).health,
            Health::Rejected(Fault::MagStuck)
        );
    }

    #[test]
    fn jump_limit_matches_hand_computed_counts() {
        // 3000 rpm = 50 rev/s ⇒ 50·16384 = 819200 count/s ⇒ 40.96 count/50 µs
        // ⇒ ceil 41 + 4 = 45 LSB。
        assert_eq!(jump_limit_lsb(3_000.0, 50), 45);
        // 1000 rpm ⇒ 13.65 count/50 µs ⇒ ceil 14 + 4 = 18 LSB。
        assert_eq!(jump_limit_lsb(1_000.0, 50), 18);
        // 周期翻倍 ⇒ 预算翻倍。
        assert_eq!(jump_limit_lsb(3_000.0, 100), 86);
        // 默认参数就是机械极限 3000 rpm / 50 µs。
        assert_eq!(EncoderParams::default().jump_limit_lsb, 45);
        // `policy::B_MAX_LSB` 是同一算式的 const 手算镜像（f32 版本不能进 const），
        // 盲推断言依赖它，所以这里钉住两者相等。
        assert_eq!(policy::B_MAX_LSB, 45);
        assert_eq!(
            jump_limit_lsb(policy::ENC_MECH_RPM_MAX, budget::ISR_PERIOD_US),
            policy::B_MAX_LSB
        );
    }

    /// 单个 CRC 位翻转的坏帧（payload 合法但帧尾错）。
    fn broken_frame() -> [u8; 3] {
        bytes_of(0x00FA_781B ^ 1)
    }

    #[test]
    fn single_crc_glitch_does_not_latch() {
        let b = EncoderParams::default().jump_limit_lsb;
        let mut monitor = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(monitor.update(encode(1_000, 0)).health, Health::Ok);

        // 一帧 CRC 错：只算丢失，不锁存。
        assert_eq!(
            monitor.update(broken_frame()).health,
            Health::Rejected(Fault::Crc)
        );
        assert!(!monitor.is_latched());
        assert_eq!(monitor.missed_frames(), 1);

        // 下一帧位移落在 2B 内：接受，丢失计数归零（一次抖动不升级为停机）。
        let out = monitor.update(encode(1_000 + 2 * b - 1, 0));
        assert_eq!(out.health, Health::Ok);
        assert_eq!(monitor.missed_frames(), 0);
    }

    #[test]
    fn allowance_grows_with_missed_frames() {
        let b = EncoderParams::default().jump_limit_lsb;
        let mut monitor = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(monitor.update(encode(2_000, 0)).health, Health::Ok);

        // 盲推窗口 = MAX_BLIND_US / ISR_PERIOD_US = 1 拍：先丢一帧。
        let mut i = 0;
        while i < policy::ENC_MAX_MISS {
            assert_eq!(
                monitor.update(broken_frame()).health,
                Health::Rejected(Fault::Crc),
                "miss {i}"
            );
            i += 1;
        }
        assert_eq!(monitor.missed_frames(), policy::ENC_MAX_MISS);

        // missed = 1 ⇒ allow = 2B；2B 内的位移必须接受，而不是误判跳变。
        let out = monitor.update(encode(2_000 + 2 * b - 1, 0));
        assert_eq!(out.health, Health::Ok);
        assert_eq!(monitor.missed_frames(), 0);

        // 放宽是按丢失拍数线性给的，不会多给：同样一次丢失后位移 2B+1 必须拒，
        // 且这一次失败把 missed 推到 max_miss 之上，直接锁存。
        let mut strict = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(strict.update(encode(2_000, 0)).health, Health::Ok);
        assert_eq!(
            strict.update(broken_frame()).health,
            Health::Rejected(Fault::Crc)
        );
        let out = strict.update(encode(2_000 + 2 * b + 1, 0));
        assert_eq!(out.health, Health::Latched(Fault::Jump));
    }

    #[test]
    fn too_many_missed_latches() {
        let mut monitor = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(monitor.update(encode(3_000, 0)).health, Health::Ok);

        let mut i = 0;
        while i < policy::ENC_MAX_MISS {
            let out = monitor.update(broken_frame());
            assert_eq!(out.health, Health::Rejected(Fault::Crc), "miss {i}");
            assert!(!out.health.is_latched());
            i += 1;
        }
        // MAX_MISS + 1 帧：丢失窗口耗尽，直接锁存。
        assert_eq!(
            monitor.update(broken_frame()).health,
            Health::Latched(Fault::Crc)
        );
        assert_eq!(monitor.missed_frames(), policy::ENC_MAX_MISS + 1);
        // 锁存后即使来好帧也不接受（只能显式 reset）。
        assert_eq!(
            monitor.update(encode(3_000, 0)).health,
            Health::Latched(Fault::Crc)
        );
    }

    #[test]
    fn jump_beyond_scaled_bound_latches() {
        let b = EncoderParams::default().jump_limit_lsb;
        let mut monitor = EncoderMonitor::new(EncoderParams::default());
        assert_eq!(monitor.update(encode(4_000, 0)).health, Health::Ok);

        // 造成 missed = 1 的语义跳变（#1，未锁存）。
        assert_eq!(
            monitor.update(encode(4_000 + 8 * b, 0)).health,
            Health::Rejected(Fault::Jump)
        );
        assert_eq!(monitor.missed_frames(), 1);

        // missed = 1 ⇒ allow = 2B；来帧位移 2.5B 仍超界 ⇒ 语义故障 #2 ⇒ 锁存。
        let displacement = b * 5 / 2;
        assert_eq!(
            monitor.update(encode(4_000 + displacement, 0)).health,
            Health::Latched(Fault::Jump)
        );
    }
}
