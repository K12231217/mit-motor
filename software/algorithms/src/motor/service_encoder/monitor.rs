//! MT6701 接受判据机、去抖与丢失窗口。
//!
//! # 接受判据
//!
//! 1. CRC 残差为 0，否则 [`Fault::Crc`]；
//! 2. `Mg[3] == 0`，否则 [`Fault::MagLock`]；
//! 3. 全 0 帧护栏：`0x000000` 是合法帧，与 MISO 恒低不可区分；连续
//!    [`policy::ENC_ZERO_FRAME_LIMIT`] 帧全 0 且此前接受角非 0 时判
//!    [`Fault::ZeroFrame`]；
//! 4. 相邻接受帧的角度差 `|Δθ| ≤ (missed + 1)·B`，`B` 由 [`jump_limit_lsb`] 按
//!    `ceil(ω_max·T_s/(2π/16384)) + 4` 算出，否则 [`Fault::Jump`]。
//!
//! `Mg[1:0] != 0` 不拒帧，输出 [`Health::MagWarn`] 由上层降额；连续
//! [`policy::ENC_MAG_WARN_LIMIT`] 帧不消失升级为 [`Fault::MagStuck`]。
//!
//! # 去抖与丢失窗口
//!
//! CRC 错只计入 `missed`，不增加语义故障连续数。语义故障（`Mg[3]`、全 0 护栏、
//! 跳变、磁场异常升级）连续 [`policy::ENC_FAIL_LATCH_FRAMES`] 帧锁存。任何未接受帧
//! 都计入 `missed`，连续超过 [`policy::ENC_MAX_MISS`] 帧直接锁存。锁存后角度冻结在
//! 最后一次接受的样本，只有 [`EncoderMonitor::reset`] 能清除。

use super::frame::{angle_rad, parse, Frame, FrameError, COUNTS_PER_REV};
use crate::motor::config_policy as policy;
use contracts::joint::config_budget as budget;
use contracts::motor::config_hw as hw;

// 半圈计数：有符号 14 bit 差值的回绕中点。
const HALF_TURN: i32 = COUNTS_PER_REV as i32 / 2;

// 一圈掩码：`diff & TURN_MASK` 得到 0..16383。
const TURN_MASK: i32 = COUNTS_PER_REV as i32 - 1;

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

/// 单周期允许的最大角度变化 `B`（LSB）：
/// `ceil(ω_max·T_s/(2π/16384)) + ENC_JUMP_MARGIN_LSB`。
///
/// 用关节机械极限而不是 MT6701 的 55 000 rpm 传感器上限：机械结构允许多快才决定
/// "一拍内真实角度最多走多少计数"。
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

const _: () = assert!(HALF_TURN * 2 == COUNTS_PER_REV as i32);
