//! 跨优先级唯一通道：seqlock 快照（PRE-04）。
//!
//! # 协议
//!
//! `seq` 为奇数表示正在写。单写者：`current_loop` @8 写 [`FastSnapshot`]，
//! `calib` / `can` @2–5 写 [`CmdSnapshot`] / [`GainSnapshot`]，读者可任意多。
//! 全程用 `AtomicU32` 存 f32 的位模式（`to_bits` / `from_bits`），保持
//! `forbid(unsafe_code)`，编译后就是普通 `LDR` / `STR`。M4 单核、无 D-cache、快照不与
//! DMA 共享，因此 `Acquire` / `Release` 足以约束编译器与总线顺序。
//!
//! 发布序列：`seq` 加 1 变奇（`Release`），逐 word 存 payload（`Relaxed`），`seq` 再
//! 加 1 变偶（`Release`）。上行方向必须拆成 [`CmdSnapshot`] 与 [`GainSnapshot`]，
//! 同一个快照由两个任务写会破坏单写者前提。
//!
//! # 读法
//!
//! | 快照 | 写者 | 读者 | 允许的读法 |
//! |---|---|---|---|
//! | [`FastSnapshot`] | prio 8 | prio 1–5 | `read_retry`；`read_once` 仅用于丢一拍也行的日志 |
//! | [`CmdSnapshot`] | prio 2–5 | prio 8 | 只允许 `read_once` |
//! | [`GainSnapshot`] | prio 2 | prio 5 | `read_retry` |
//!
//! 高优先级读者运行时低优先级写者被冻结，`seq` 不可能在两次 load 之间变化，一次判定
//! 就够；低优先级读者会被写者抢占，只能从 `s0` 重试到 `s1 == s0`，上限
//! [`crate::limits::SNAPSHOT_RETRY_MAX`] 次，仍不收敛就用上一拍值，绝不阻塞。
//!
//! 最坏 4 ×（读 `seq` + 16 word 拷贝 + 读 `seq`）≈ 80 cycles ≈ 0.47 µs @170 MHz，
//! 已计入 25 µs 的 ISR 预算。

use core::sync::atomic::{AtomicU32, Ordering};

use crate::limits;

/// `FastView` 的 word 数（64 B / 4）。
pub const FAST_WORDS: usize = 16;
/// `CmdView` 的 word 数（24 B / 4）。
pub const CMD_WORDS: usize = 6;
/// `GainView` 的 word 数（32 B / 4）。
pub const GAIN_WORDS: usize = 8;

/// 下行快照视图：电流环 → 慢环，64 B 无填充。
///
/// 字段顺序与 `docs/software-architecture.md` §3.1 的表一致。`duty_norm` 是已夹紧后的
/// 实际值（不是指令值），便于事后区分算法错与饱和。
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct FastView {
    /// 采样时刻（TIM5，1 µs，64 bit 扩展值）。
    pub t_us: u64,
    /// 三相电流，安培。
    pub i_abc_amps: [f32; 3],
    /// dq 电流，安培。
    pub i_dq_amps: [f32; 2],
    /// 实际调制比（已夹紧），`2d - 1` 归一化到 `[-1, 1]`。
    pub duty_norm: [f32; 3],
    /// 电角度，弧度。
    pub theta_e_rad: f32,
    /// 母线电压，伏。
    pub vbus_v: f32,
    /// DWT 实测 ISR 周期数（I6）。
    pub isr_cycles: u32,
    /// 32 bit 状态字（`faults::StatusWord` 的原始值）。
    pub state: u32,
    /// 采样窗口余量，千分比（0 = 无余量）。
    pub margin_permille: u16,
    /// 队列/样本丢弃计数（饱和 u16）。
    pub dropped: u16,
    /// 帧序号低 32 位，LA 与帧对时用。
    pub seq_hint: u32,
}

impl FastView {
    /// 编码成 16 个 word，顺序 = 字节偏移 / 4。
    ///
    /// `f32` 一律走 `to_bits`（不借用 `transmute` 或裸指针，也不做舍入）；
    /// `margin_permille` 与 `dropped` 两个 u16 共用一个 word（低/高半字）。
    pub fn encode(&self) -> [u32; FAST_WORDS] {
        let [ia, ib, ic] = self.i_abc_amps;
        let [id, iq] = self.i_dq_amps;
        let [da, db, dc] = self.duty_norm;
        [
            self.t_us as u32,
            (self.t_us >> 32) as u32,
            ia.to_bits(),
            ib.to_bits(),
            ic.to_bits(),
            id.to_bits(),
            iq.to_bits(),
            da.to_bits(),
            db.to_bits(),
            dc.to_bits(),
            self.theta_e_rad.to_bits(),
            self.vbus_v.to_bits(),
            self.isr_cycles,
            self.state,
            u32::from(self.margin_permille) | (u32::from(self.dropped) << 16),
            self.seq_hint,
        ]
    }

    /// 从 16 个 word 还原，与 [`Self::encode`] 严格互逆（`f32` 位级复原，含 `-0.0`
    /// 与 `NaN` 的 payload）。
    pub fn decode(words: &[u32; FAST_WORDS]) -> Self {
        let [t_lo, t_hi, ia, ib, ic, id, iq, da, db, dc, theta, vbus, isr_cycles, state, margin_dropped, seq_hint] =
            *words;
        Self {
            t_us: (u64::from(t_hi) << 32) | u64::from(t_lo),
            i_abc_amps: [f32::from_bits(ia), f32::from_bits(ib), f32::from_bits(ic)],
            i_dq_amps: [f32::from_bits(id), f32::from_bits(iq)],
            duty_norm: [f32::from_bits(da), f32::from_bits(db), f32::from_bits(dc)],
            theta_e_rad: f32::from_bits(theta),
            vbus_v: f32::from_bits(vbus),
            isr_cycles,
            state,
            margin_permille: margin_dropped as u16,
            dropped: (margin_dropped >> 16) as u16,
            seq_hint,
        }
    }
}

/// 上行快照视图：慢环 → 电流环，24 B（6 word）。
///
/// 电流环只认 `mode` / `seq` / `ttl_ms` 与 dq 目标。`flags` 用于"停止或清故障后重新
/// 使能"这类一次性动作，必须在 `ttl_ms` 内生效，否则电流环丢帧并计数。
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct CmdView {
    /// 发布时刻（TIM5，1 µs）。
    pub t_us: u64,
    /// dq 电流目标，安培。
    pub i_dq_amps: [f32; 2],
    /// 目标模式编号（`Mode::as_u8`）。
    pub mode: u8,
    /// 主机/发布者帧序号低 8 位。
    pub seq: u8,
    /// 命令标志位（语义见 `icd::CMD_FLAG_*`）。
    pub flags: u8,
    /// 保留，必须为 0（layout 稳定性占位）。
    pub rsvd0: u8,
    /// 生存期，毫秒；0 = 用 `limits::CAN_TIMEOUT_MS`。
    pub ttl_ms: u16,
    /// 保留，必须为 0。
    pub rsvd1: u16,
}

impl CmdView {
    /// 编码成 6 个 word。word 顺序 = 字节偏移 / 4；`mode`/`seq`/`flags`/`rsvd0`
    /// 四个字节挤在同一 word（16..20），`ttl_ms`/`rsvd1` 同理（20..24）。
    pub fn encode(&self) -> [u32; CMD_WORDS] {
        let [id, iq] = self.i_dq_amps;
        [
            self.t_us as u32,
            (self.t_us >> 32) as u32,
            id.to_bits(),
            iq.to_bits(),
            u32::from(self.mode)
                | (u32::from(self.seq) << 8)
                | (u32::from(self.flags) << 16)
                | (u32::from(self.rsvd0) << 24),
            u32::from(self.ttl_ms) | (u32::from(self.rsvd1) << 16),
        ]
    }

    /// 从 6 个 word 还原。与 [`Self::encode`] 严格互逆。
    pub fn decode(words: &[u32; CMD_WORDS]) -> Self {
        let [t_lo, t_hi, id, iq, mode_seq_flags_rsvd, ttl_rsvd] = *words;
        Self {
            t_us: (u64::from(t_hi) << 32) | u64::from(t_lo),
            i_dq_amps: [f32::from_bits(id), f32::from_bits(iq)],
            mode: mode_seq_flags_rsvd as u8,
            seq: (mode_seq_flags_rsvd >> 8) as u8,
            flags: (mode_seq_flags_rsvd >> 16) as u8,
            rsvd0: (mode_seq_flags_rsvd >> 24) as u8,
            ttl_ms: ttl_rsvd as u16,
            rsvd1: (ttl_rsvd >> 16) as u16,
        }
    }
}

/// 增益快照视图：标定任务 → CAN / 遥测，32 B（8 word）。
///
/// `trust` / `step` 存枚举编号。`t_us` 是 `u32`（71.6 min 回绕），相对内部 `u64`
/// 有损：转换处必须显式截断，并保证快照存活时间远小于回绕周期。
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct GainView {
    /// 发布时刻（TIM5，1 µs，u32 截断）。
    pub t_us: u32,
    /// 相对增益 `[g_b, g_c]`（参考相 `g_a ≡ 1`，共模不可观测）。
    pub gain_bc: [f32; 2],
    /// 三相零位，安培。
    pub zero_amps: [f32; 3],
    /// 有效样本数 `N_eq`（饱和 3999）。
    pub n_eq: u16,
    /// `Trust` 编号（0 = Trusted）。
    pub trust: u8,
    /// `CalibStep` 编号（0–6）。
    pub step: u8,
    /// 最近一次 χ² 统计量。
    pub chi2: f32,
}

impl GainView {
    /// 编码成 8 个 word。word 顺序 = 字节偏移 / 4；`n_eq`/`trust`/`step`
    /// 三个字段挤在 24..28 的同一个 word（u16 + u8 + u8）。
    pub fn encode(&self) -> [u32; GAIN_WORDS] {
        let [gb, gc] = self.gain_bc;
        let [za, zb, zc] = self.zero_amps;
        [
            self.t_us,
            gb.to_bits(),
            gc.to_bits(),
            za.to_bits(),
            zb.to_bits(),
            zc.to_bits(),
            u32::from(self.n_eq) | (u32::from(self.trust) << 16) | (u32::from(self.step) << 24),
            self.chi2.to_bits(),
        ]
    }

    /// 从 8 个 word 还原。与 [`Self::encode`] 严格互逆。
    pub fn decode(words: &[u32; GAIN_WORDS]) -> Self {
        let [t_us, gb, gc, za, zb, zc, n_trust_step, chi2] = *words;
        Self {
            t_us,
            gain_bc: [f32::from_bits(gb), f32::from_bits(gc)],
            zero_amps: [f32::from_bits(za), f32::from_bits(zb), f32::from_bits(zc)],
            n_eq: n_trust_step as u16,
            trust: (n_trust_step >> 16) as u8,
            step: (n_trust_step >> 24) as u8,
            chi2: f32::from_bits(chi2),
        }
    }
}

// 有界读核心，三个快照共用。
//
// 一致性判据是 `s0 == s1` 且 `s0` 偶：`s0` 奇表示读开始于写者中途，直接放弃本次尝试；
// 两次 `Acquire` 读相同表示 payload 的 `Relaxed` 读全部落在同一拍内。返回尝试次数只为
// 测试证明"重试有界"，生产路径丢弃。
fn read_words<const N: usize>(
    seq: &AtomicU32,
    w: &[AtomicU32; N],
    max_attempts: u8,
) -> (Option<[u32; N]>, u8) {
    let mut attempts = 0u8;
    while attempts < max_attempts {
        attempts += 1;
        let s0 = seq.load(Ordering::Acquire);
        if s0 & 1 != 0 {
            continue;
        }
        let mut words = [0u32; N];
        for (dst, src) in words.iter_mut().zip(w.iter()) {
            *dst = src.load(Ordering::Relaxed);
        }
        let s1 = seq.load(Ordering::Acquire);
        if s0 == s1 {
            return (Some(words), attempts);
        }
    }
    (None, attempts)
}

/// 下行 seqlock：快环（prio 8）单写，慢环（prio 1–5）多读。
#[derive(Debug)]
pub struct FastSnapshot {
    seq: AtomicU32,
    w: [AtomicU32; FAST_WORDS],
}

impl FastSnapshot {
    /// 纯构造：全零、`seq` 偶（可读）。
    pub const fn empty() -> Self {
        Self {
            seq: AtomicU32::new(0),
            w: [const { AtomicU32::new(0) }; FAST_WORDS],
        }
    }

    /// 单写者发布：`seq` 置奇（`Release`）、写 payload（逐 word `Relaxed`）、再置偶
    /// （`Release`）。调用者必须确认自己是唯一写者（PRE-04）。
    pub fn write(&self, v: &FastView) {
        // 单写者 ⇒ 进入时 `seq` 必为偶，+1 即奇、+2 即偶，无需 CAS。
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
        for (dst, src) in self.w.iter().zip(v.encode().iter()) {
            dst.store(*src, Ordering::Relaxed);
        }
        self.seq.store(seq.wrapping_add(2), Ordering::Release);
    }

    /// 一次尝试；写者正在写时返回 `None`，调用方用上一拍值。
    ///
    /// 只适用于不会被 prio 8 写者抢占的读者。本快照的读者是 prio 1–5，会被抢占，
    /// 正常路径应使用 [`Self::read_retry`]。
    pub fn read_once(&self) -> Option<FastView> {
        read_words(&self.seq, &self.w, 1)
            .0
            .map(|words| FastView::decode(&words))
    }

    /// 有界重试（≤ [`limits::SNAPSHOT_RETRY_MAX`] 次），绝不阻塞。
    ///
    /// 写者是 prio 8，会在读者执行期间抢占并完成发布，重试到 `s1 == s0` 即可；用完
    /// 次数仍不收敛就返回 `None`，调用方用上一拍值。
    pub fn read_retry(&self) -> Option<FastView> {
        read_words(&self.seq, &self.w, limits::SNAPSHOT_RETRY_MAX)
            .0
            .map(|words| FastView::decode(&words))
    }
}

/// 上行 seqlock：慢环（prio 2–5）单写，快环（prio 8）只读。
#[derive(Debug)]
pub struct CmdSnapshot {
    seq: AtomicU32,
    w: [AtomicU32; CMD_WORDS],
}

impl CmdSnapshot {
    /// 纯构造：全零、`seq` 偶（可读）。
    pub const fn empty() -> Self {
        Self {
            seq: AtomicU32::new(0),
            w: [const { AtomicU32::new(0) }; CMD_WORDS],
        }
    }

    /// 单写者发布（`can` @5 或 `calib` @2，二者由状态机保证互斥）。
    pub fn write(&self, v: &CmdView) {
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
        for (dst, src) in self.w.iter().zip(v.encode().iter()) {
            dst.store(*src, Ordering::Relaxed);
        }
        self.seq.store(seq.wrapping_add(2), Ordering::Release);
    }

    /// 一次尝试，这是电流环唯一允许的读法。
    ///
    /// 读者是 prio 8，运行时写者（prio 2–5）全程冻结，`seq` 不可能在两次 load 之间
    /// 变化，一次判定就够；`s0` 为奇数说明写者被冻在发布中途，返回 `None` 用上一拍。
    pub fn read_once(&self) -> Option<CmdView> {
        read_words(&self.seq, &self.w, 1)
            .0
            .map(|words| CmdView::decode(&words))
    }

    /// 本方向的重试是死代码：读开始于奇数时写者被冻结、`seq` 保持奇数，重试任意次都
    /// 得到 `None`；读开始于偶数时 `read_once` 已经成功。保留它是为了三个快照的 API
    /// 形状一致，不允许在电流环里调用。
    pub fn read_retry(&self) -> Option<CmdView> {
        read_words(&self.seq, &self.w, limits::SNAPSHOT_RETRY_MAX)
            .0
            .map(|words| CmdView::decode(&words))
    }
}

/// 增益 seqlock：`calib`（prio 2）单写，`can`/遥测（prio 5）多读。
#[derive(Debug)]
pub struct GainSnapshot {
    seq: AtomicU32,
    w: [AtomicU32; GAIN_WORDS],
}

impl GainSnapshot {
    /// 纯构造：全零、`seq` 偶（可读）。
    pub const fn empty() -> Self {
        Self {
            seq: AtomicU32::new(0),
            w: [const { AtomicU32::new(0) }; GAIN_WORDS],
        }
    }

    /// 单写者发布（只在 C6 冻结或显式重启时）。
    pub fn write(&self, v: &GainView) {
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
        for (dst, src) in self.w.iter().zip(v.encode().iter()) {
            dst.store(*src, Ordering::Relaxed);
        }
        self.seq.store(seq.wrapping_add(2), Ordering::Release);
    }

    /// 一次尝试；写者可能在读期间抢占（prio 2 对 prio 5），失败返回 `None`。
    pub fn read_once(&self) -> Option<GainView> {
        read_words(&self.seq, &self.w, 1)
            .0
            .map(|words| GainView::decode(&words))
    }

    /// 有界重试（≤ [`limits::SNAPSHOT_RETRY_MAX`] 次）：读者 prio 5 会被写者
    /// prio 2 抢占，但有界，绝不阻塞。
    pub fn read_retry(&self) -> Option<GainView> {
        read_words(&self.seq, &self.w, limits::SNAPSHOT_RETRY_MAX)
            .0
            .map(|words| GainView::decode(&words))
    }
}

// 三个视图的字节布局与三个快照实例大小。
const _: () = assert!(core::mem::size_of::<FastView>() == 64);
const _: () = assert!(core::mem::offset_of!(FastView, t_us) == 0);
const _: () = assert!(core::mem::offset_of!(FastView, i_abc_amps) == 8);
const _: () = assert!(core::mem::offset_of!(FastView, i_dq_amps) == 20);
const _: () = assert!(core::mem::offset_of!(FastView, duty_norm) == 28);
const _: () = assert!(core::mem::offset_of!(FastView, theta_e_rad) == 40);
const _: () = assert!(core::mem::offset_of!(FastView, vbus_v) == 44);
const _: () = assert!(core::mem::offset_of!(FastView, isr_cycles) == 48);
const _: () = assert!(core::mem::offset_of!(FastView, state) == 52);
const _: () = assert!(core::mem::offset_of!(FastView, margin_permille) == 56);
const _: () = assert!(core::mem::offset_of!(FastView, dropped) == 58);
const _: () = assert!(core::mem::offset_of!(FastView, seq_hint) == 60);

const _: () = assert!(core::mem::size_of::<CmdView>() == 24);
const _: () = assert!(core::mem::offset_of!(CmdView, t_us) == 0);
const _: () = assert!(core::mem::offset_of!(CmdView, i_dq_amps) == 8);
const _: () = assert!(core::mem::offset_of!(CmdView, mode) == 16);
const _: () = assert!(core::mem::offset_of!(CmdView, seq) == 17);
const _: () = assert!(core::mem::offset_of!(CmdView, flags) == 18);
const _: () = assert!(core::mem::offset_of!(CmdView, rsvd0) == 19);
const _: () = assert!(core::mem::offset_of!(CmdView, ttl_ms) == 20);
const _: () = assert!(core::mem::offset_of!(CmdView, rsvd1) == 22);

const _: () = assert!(core::mem::size_of::<GainView>() == 32);
const _: () = assert!(core::mem::offset_of!(GainView, t_us) == 0);
const _: () = assert!(core::mem::offset_of!(GainView, gain_bc) == 4);
const _: () = assert!(core::mem::offset_of!(GainView, zero_amps) == 12);
const _: () = assert!(core::mem::offset_of!(GainView, n_eq) == 24);
const _: () = assert!(core::mem::offset_of!(GainView, trust) == 26);
const _: () = assert!(core::mem::offset_of!(GainView, step) == 27);
const _: () = assert!(core::mem::offset_of!(GainView, chi2) == 28);

const _: () = assert!(core::mem::size_of::<FastSnapshot>() == 4 + 4 * FAST_WORDS);
const _: () = assert!(core::mem::size_of::<CmdSnapshot>() == 4 + 4 * CMD_WORDS);
const _: () = assert!(core::mem::size_of::<GainSnapshot>() == 4 + 4 * GAIN_WORDS);
const _: () = assert!(limits::SNAPSHOT_RETRY_MAX == 4);

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;

    fn sample_fast() -> FastView {
        FastView {
            t_us: 0x1122_3344_5566_7788,
            // -0.0 专门用来验证编码是位级复原，而不是数值相等。
            i_abc_amps: [1.25, -0.0, -0.75],
            i_dq_amps: [0.125, -2.75],
            duty_norm: [-0.599_975, 0.0, 0.599_975],
            theta_e_rad: 1.5,
            vbus_v: 24.0,
            isr_cycles: 4_200,
            state: 0x0A00_1234,
            margin_permille: 250,
            dropped: 3,
            seq_hint: 0xDEAD_BEEF,
        }
    }

    fn sample_cmd() -> CmdView {
        CmdView {
            t_us: 0x0011_2233_4455_6677,
            i_dq_amps: [0.0, -0.5],
            mode: 3,
            seq: 0xA5,
            flags: 0x11,
            rsvd0: 0,
            ttl_ms: 20,
            rsvd1: 0,
        }
    }

    fn sample_gain() -> GainView {
        GainView {
            t_us: 0xDEAD_BEEF,
            gain_bc: [1.002, 0.998],
            zero_amps: [0.001, -0.002, 0.000_5],
            n_eq: 3_999,
            trust: 0b0011_1111,
            step: 6,
            chi2: 9.21,
        }
    }

    /// 三个类型各往返一次，逐字段（f32 位级）比对：`encode` 相等即所有字段相等。
    #[test]
    fn seqlock_reads_back_encoded_values() {
        let fast = FastSnapshot::empty();
        let want = sample_fast();
        fast.write(&want);
        let Some(got) = fast.read_retry() else {
            panic!("正式发布后 read_retry 必须成功")
        };
        assert_eq!(got.encode(), want.encode());
        assert_eq!(FastView::decode(&want.encode()).encode(), want.encode());

        let cmd = CmdSnapshot::empty();
        let want = sample_cmd();
        cmd.write(&want);
        let Some(got) = cmd.read_once() else {
            panic!("正式发布后 read_once 必须成功")
        };
        assert_eq!(got.encode(), want.encode());
        assert_eq!(CmdView::decode(&want.encode()).encode(), want.encode());

        let gain = GainSnapshot::empty();
        let want = sample_gain();
        gain.write(&want);
        let Some(got) = gain.read_retry() else {
            panic!("正式发布后 read_retry 必须成功")
        };
        assert_eq!(got.encode(), want.encode());
        assert_eq!(GainView::decode(&want.encode()).encode(), want.encode());
    }

    /// `seq` 为奇数 = 写者停在发布中途。一次判定的读法必须直接放弃。
    #[test]
    fn seqlock_odd_seq_returns_none() {
        let fast = FastSnapshot::empty();
        fast.write(&sample_fast());
        fast.seq.store(3, Ordering::Relaxed); // 手工构造 write 的中间态
        assert_eq!(fast.read_once(), None);
        assert_eq!(fast.read_retry(), None);

        let cmd = CmdSnapshot::empty();
        cmd.write(&sample_cmd());
        cmd.seq.store(1, Ordering::Relaxed);
        assert_eq!(cmd.read_once(), None);

        let gain = GainSnapshot::empty();
        gain.write(&sample_gain());
        gain.seq.store(9, Ordering::Relaxed);
        assert_eq!(gain.read_once(), None);
    }

    /// 模拟永不收敛：host 上顺序执行的测试无法在 s0/s1 之间改 `seq`，
    /// 所以用 `write` 的中间态（奇 `seq`）当"写者被永久冻结"，这是低优先级
    /// 读者会看到的真实形态。断言给出尝试次数上限，并且从头到尾只读不写。
    #[test]
    fn seqlock_retry_gives_up_after_n() {
        const N: u8 = 3;
        let snap = FastSnapshot::empty();
        snap.write(&sample_fast());
        let frozen = snap.seq.load(Ordering::Relaxed) + 1; // 奇：写者已暂停
        snap.seq.store(frozen, Ordering::Relaxed);

        let (out, attempts) = read_words(&snap.seq, &snap.w, N);
        assert!(out.is_none(), "永不收敛时必须返回 None");
        assert_eq!(attempts, N, "循环次数必须恰好是上限 N");
        assert!(attempts <= N);
        assert_eq!(snap.seq.load(Ordering::Relaxed), frozen);

        // 生产入口：上限来自 limits，永不超界，并且不阻塞（顺序执行即证明）。
        assert_eq!(snap.read_retry(), None);
        assert_eq!(snap.read_once(), None);
        let (out, attempts) = read_words(&snap.seq, &snap.w, limits::SNAPSHOT_RETRY_MAX);
        assert!(out.is_none());
        assert!(attempts <= limits::SNAPSHOT_RETRY_MAX);
    }

    /// 上行方向（prio 2–5 写、prio 8 读）：写者被冻结在中间态时，重试不可能把结果
    /// 救回来，`seq` 一直奇数，4 次与 1 次结论相同。
    #[test]
    fn cmd_snapshot_retry_can_never_succeed_at_prio8() {
        let snap = CmdSnapshot::empty();
        snap.write(&sample_cmd());
        snap.seq.store(1, Ordering::Relaxed); // prio 8 读者看到的就是这个状态

        assert_eq!(snap.read_once(), None);
        assert_eq!(snap.read_retry(), None);

        let (out, attempts) = read_words(&snap.seq, &snap.w, limits::SNAPSHOT_RETRY_MAX);
        assert!(out.is_none());
        assert_eq!(attempts, limits::SNAPSHOT_RETRY_MAX);
        assert_eq!(snap.seq.load(Ordering::Relaxed), 1, "读者不得改写 seq");
    }

    /// 诚实声明：host（x86-64、单线程、强序内存模型）造不出真正的内存序撕裂，本用例
    /// 只覆盖 odd/even 协议分支与"发布后完整可读"，不证明原子性。无撕裂保证依赖 M4
    /// 单核加 `Acquire` / `Release` 的论证（见模块文档），台架上用 LA 对时与
    /// `seq_hint` 才能观测。
    #[test]
    fn seqlock_does_not_tear_on_host() {
        let snap = FastSnapshot::empty();
        let v = sample_fast();

        snap.write(&v);
        assert_eq!(snap.seq.load(Ordering::Relaxed) % 2, 0, "发布完成 ⇒ seq 偶");
        let Some(got) = snap.read_retry() else {
            panic!("发布完成的快照必须可读")
        };
        assert_eq!(got.encode(), v.encode());

        // 连续发布两拍：每次读到的都是完整的一拍，而不是两次写的混合。
        let mut v2 = v;
        v2.t_us += 1;
        v2.i_abc_amps = [2.0, 2.0, 2.0];
        snap.write(&v2);
        let Some(got) = snap.read_once() else {
            panic!("无并发时 read_once 必须成功")
        };
        assert!(
            got.encode() == v.encode() || got.encode() == v2.encode(),
            "读到的必须是某一整拍，不能是混合"
        );
    }
}
