//! 20 kHz 预/后触发录波环与 RTT 导出。
//!
//! 2048 槽覆盖环形：未触发时持续覆盖最旧帧；触发后写满 1536 帧才锁存。
//! 保留窗口 = 触发前 512 帧 + 触发后 1536 帧 = 2048 帧：
//!
//! - `stride = 1`：20 kHz，512×50 µs = 25.6 ms 预触发，1536×50 µs = 76.8 ms
//!   后触发，总 102.4 ms。
//! - `stride = 10`：每 10 个 PWM 周期采 1 帧，2 kHz；同样的 512/1536 帧
//!   对应 256/768 ms，总 1.024 s。
//!
//! 并发模型：20 kHz 单写者；`push` 不取锁、不打印、不重试。触发由标定任务写
//! `trigger_head`/`triggered`；写满后 `locked` 以 Release 发布，导出任务先
//! Acquire `locked` 再读槽。所有槽都是 `AtomicU32`，不使用 `UnsafeCell`。
//!
//! 触发前不足 512 帧时，预触发窗口自然缩短；`summary` 返回实际 `pre` 数，不假装
//! 有 25.6 ms。`arm()` 只能在锁存后调用，重新开始下一轮。

use core::mem::size_of;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// 环形槽数（2 的幂）。
pub const TRACE_FRAMES: usize = 2048;
/// 触发前保留帧数：512 × 50 µs = 25.6 ms（stride=1）。
pub const PRE_FRAMES: usize = 512;
/// 触发后记录帧数：1536 × 50 µs = 76.8 ms（stride=1）。
pub const POST_FRAMES: usize = 1536;
/// 默认采样步长：每个 PWM 周期一帧。
pub const DEFAULT_STRIDE: u32 = 1;

/// 帧标志：冻结期间采集。
pub const TRACE_FLAG_FROZEN: u8 = 1 << 0;
/// 帧标志：HIL 自检帧，不是真实电流。
pub const TRACE_FLAG_SELFTEST: u8 = 1 << 7;

const _: () = assert!(TRACE_FRAMES.is_power_of_two());
const _: () = assert!(PRE_FRAMES + POST_FRAMES == TRACE_FRAMES);

/// 一拍录波：原始 ADC 码 + 残差 + 标志。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TraceFrame {
    /// 20 kHz 拍号。
    pub k: u32,
    /// A 相原始码（12 bit，0…4095）。
    pub ia_raw: u16,
    /// B 相原始码。
    pub ib_raw: u16,
    /// C 相原始码。
    pub ic_raw: u16,
    /// 残差，LSB（有符号）。
    pub r_lsb: i16,
    /// [`TRACE_FLAG_FROZEN`] / [`TRACE_FLAG_SELFTEST`] 位。
    pub flags: u8,
    /// 保留，写 0。
    pub _rsvd: u8,
}

const _: () = assert!(size_of::<TraceFrame>() == 16);

/// 锁存后的窗口摘要。
#[derive(Clone, Copy)]
pub struct Summary {
    /// 实际保留的预触发帧数（触发前不足 512 时更小）。
    pub pre: u32,
    /// 实际保留的后触发帧数。
    pub post: u32,
    /// 窗口总帧数。
    pub len: u32,
    /// 环形累计写入帧数。
    pub total: u32,
    /// 是否已锁存。
    pub locked: bool,
    /// 是否已触发。
    pub triggered: bool,
    /// 当前采样步长。
    pub stride: u32,
}

/// 16 B 的原子槽；四字写入后由 `head.store(Release)` 发布。
struct TraceSlot {
    w0: AtomicU32,
    w1: AtomicU32,
    w2: AtomicU32,
    w3: AtomicU32,
}

impl TraceSlot {
    const fn new() -> Self {
        Self {
            w0: AtomicU32::new(0),
            w1: AtomicU32::new(0),
            w2: AtomicU32::new(0),
            w3: AtomicU32::new(0),
        }
    }

    fn write(&self, f: &TraceFrame) {
        self.w0.store(f.k, Ordering::Relaxed);
        self.w1.store(
            u32::from(f.ia_raw) | (u32::from(f.ib_raw) << 16),
            Ordering::Relaxed,
        );
        self.w2.store(
            u32::from(f.ic_raw) | (u32::from(f.r_lsb as u16) << 16),
            Ordering::Relaxed,
        );
        self.w3.store(u32::from(f.flags), Ordering::Relaxed);
    }

    fn read(&self) -> TraceFrame {
        let w1 = self.w1.load(Ordering::Relaxed);
        let w2 = self.w2.load(Ordering::Relaxed);
        TraceFrame {
            k: self.w0.load(Ordering::Relaxed),
            ia_raw: w1 as u16,
            ib_raw: (w1 >> 16) as u16,
            ic_raw: w2 as u16,
            r_lsb: (w2 >> 16) as u16 as i16,
            flags: self.w3.load(Ordering::Relaxed) as u8,
            _rsvd: 0,
        }
    }
}

const _: () = assert!(size_of::<TraceSlot>() == 16);

/// 单写者预/后触发环形。
pub struct TraceRecorder {
    slots: [TraceSlot; TRACE_FRAMES],
    /// 累计采样帧数（回绕 59.6 h @20 kHz）。
    head: AtomicU32,
    /// 触发瞬间的 `head`，决定窗口起点。
    trigger_head: AtomicU32,
    /// 触发后已写帧数。
    post: AtomicU32,
    /// 本轮 `push` 调用计数（每个 PWM 周期 +1）。
    period: AtomicU32,
    /// 采样步长。
    stride: AtomicU32,
    triggered: AtomicBool,
    locked: AtomicBool,
}

impl TraceRecorder {
    /// 静态构造。用于静态实例时传 0，让整个环落在 `.bss`（零初始化），
    /// 避免 32 KiB 槽被复制到 `.data`/Flash；`push` 把 0 当 1。
    pub const fn new(stride: u32) -> Self {
        Self {
            slots: [const { TraceSlot::new() }; TRACE_FRAMES],
            head: AtomicU32::new(0),
            trigger_head: AtomicU32::new(0),
            post: AtomicU32::new(0),
            period: AtomicU32::new(0),
            stride: AtomicU32::new(stride),
            triggered: AtomicBool::new(false),
            locked: AtomicBool::new(false),
        }
    }

    /// 设置采样步长；建议在 `arm()` 前配置，运行中切换会改变窗口时间含义。
    pub fn set_stride(&self, stride: u32) {
        self.stride.store(stride.max(1), Ordering::Relaxed);
    }

    /// 20 kHz 每拍调用；内部按 `stride` 降采样。未触发时覆盖最旧帧。
    pub fn push(&self, frame: TraceFrame) {
        if self.locked.load(Ordering::Acquire) {
            return;
        }

        let period = self.period.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let stride = self.stride.load(Ordering::Relaxed).max(1);
        if !period.is_multiple_of(stride) {
            return;
        }

        let head = self.head.load(Ordering::Relaxed);
        let idx = (head as usize) & (TRACE_FRAMES - 1);
        self.slots[idx].write(&frame);
        self.head.store(head.wrapping_add(1), Ordering::Release);

        if self.triggered.load(Ordering::Acquire) {
            let post = self.post.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
            if post >= POST_FRAMES as u32 {
                self.locked.store(true, Ordering::Release);
            }
        }
    }

    /// 触发冻结；返回 false 表示已触发或已锁存。
    pub fn trigger(&self) -> bool {
        if self.locked.load(Ordering::Acquire) || self.triggered.load(Ordering::Acquire) {
            return false;
        }
        let head = self.head.load(Ordering::Acquire);
        // 先发布窗口起点，再发布触发；写者看到 triggered 时一定看到 trigger_head。
        self.trigger_head.store(head, Ordering::Release);
        self.triggered.store(true, Ordering::Release);
        true
    }

    /// 锁存后重新布防；未锁存返回 false，避免与写者竞争。
    pub fn arm(&self) -> bool {
        if !self.locked.load(Ordering::Acquire) {
            return false;
        }
        self.head.store(0, Ordering::Relaxed);
        self.trigger_head.store(0, Ordering::Relaxed);
        self.post.store(0, Ordering::Relaxed);
        self.period.store(0, Ordering::Relaxed);
        self.triggered.store(false, Ordering::Relaxed);
        self.locked.store(false, Ordering::Release);
        true
    }

    /// 窗口/锁存状态快照；供 HIL 与导出任务判断。
    pub fn summary(&self) -> Summary {
        let trigger = self.trigger_head.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        let end = head.min(trigger.wrapping_add(POST_FRAMES as u32));
        let start = trigger.saturating_sub(PRE_FRAMES as u32);
        Summary {
            pre: trigger.saturating_sub(start),
            post: end.saturating_sub(trigger),
            len: end.saturating_sub(start),
            total: head,
            locked: self.locked.load(Ordering::Acquire),
            triggered: self.triggered.load(Ordering::Acquire),
            stride: self.stride.load(Ordering::Relaxed).max(1),
        }
    }

    /// 导出前 `limit` 帧；`limit = usize::MAX` 导出完整窗口。只在锁存后调用。
    pub fn dump(&self, limit: usize) {
        let mut n = 0;
        self.for_each_window(|f| {
            if n < limit {
                defmt::info!(
                    "DATA,{=u32},{=u16},{=u16},{=u16},{=i16},{=u8}",
                    f.k,
                    f.ia_raw,
                    f.ib_raw,
                    f.ic_raw,
                    f.r_lsb,
                    f.flags
                );
                n += 1;
            }
        });
    }

    fn for_each_window(&self, mut f: impl FnMut(TraceFrame)) {
        if !self.locked.load(Ordering::Acquire) {
            return;
        }
        let trigger = self.trigger_head.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        let end = head.min(trigger.wrapping_add(POST_FRAMES as u32));
        let start = trigger.saturating_sub(PRE_FRAMES as u32);
        let mut seq = start;
        while seq < end {
            let idx = (seq as usize) & (TRACE_FRAMES - 1);
            f(self.slots[idx].read());
            seq = seq.wrapping_add(1);
        }
    }
}

/// 全工程唯一的录波环；20 kHz 写者、标定触发者、导出者各司其职。
pub static TRACE: TraceRecorder = TraceRecorder::new(0);
