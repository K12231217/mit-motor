//! CAN FD 线协议（ICD v1）：命令帧、反馈帧、慢帧的 `repr(C)` 布局。
//!
//! # 通用约定
//!
//! - 11-bit 标准帧：ID 低 7 位是节点号 `n`（1..=127，0 留给上位机），
//!   `CMD = 0x080 | n`、`FB = 0x100 | n`、`SLOW = 0x180 | n`。
//! - 数据段 2 Mbit/s；`CMD` 定长 16 B，`FB` / `SLOW` 定长 32 B（都是合法 FD DLC）。
//! - 全部小端（M4 原生顺序）。`rsvd` 必须写 0，接收侧见非 0 即整帧丢弃并计 `dropped`，
//!   这是版本探测防线，不做静默兼容。
//! - 物理量是定点整数加明确 LSB；标识符带单位后缀（PRE-06）。
//! - 没有"置信度 0–100 %"字段：置信度由 `d` / `z` / `pred_pp` / `trust` 表达。
//! - `g_b` / `g_c` 只是两个相对增益；共模增益不可观测，帧里不得出现"平均增益"。
//!
//! # 有损字段
//!
//! `isr_cycles: u16`、`t_us: u32`（FB）与 `t_commit_us: u32`（SLOW）相对内部
//! `u32` / `u64` 有损：`isr_cycles` 饱和到 `0xFFFF`；两个 32 bit 时间戳每 71.58 min
//! 回绕一次，由主机解绕，固件不做回绕合并。
//!
//! 内部 `u64` µs 与线上 `u32` µs 不得直接混用：打包只走 [`timestamp_to_wire_us`]，
//! 解包只走 [`timestamp_from_wire_us`]，截断点在源码里一眼可见。
//!
//! # 解码契约
//!
//! 每帧的 `decode` 校验版本号、全部 `rsvd*` 与枚举字段取值；任一不符返回
//! [`DecodeError::Rejected`] 并计 `dropped`，绝不 panic（PRE-05）。

use crate::limits;
use crate::types::Mode;

/// 布局版本（CMD/SLOW 帧的 `ver` 字段）。
pub const ICD_VERSION: u8 = 1;

/// 板地址下限（0 预留给上位机）。
pub const NODE_ADDR_MIN: u8 = 1;
/// 板地址上限（11-bit 标准帧，7 bit 地址）。
pub const NODE_ADDR_MAX: u8 = 127;

/// 命令帧 ID 基址（主机 → 板）。
pub const CAN_ID_CMD_BASE: u16 = 0x080;
/// 反馈帧 ID 基址（板 → 主机，1 kHz）。
pub const CAN_ID_FB_BASE: u16 = 0x100;
/// 慢帧 ID 基址（板 → 主机，50 Hz 或请求时）。
pub const CAN_ID_SLOW_BASE: u16 = 0x180;

/// 纯构造：命令帧 ID。
pub const fn cmd_id(node_addr: u8) -> u16 {
    CAN_ID_CMD_BASE | (node_addr as u16)
}

/// 纯构造：反馈帧 ID。
pub const fn fb_id(node_addr: u8) -> u16 {
    CAN_ID_FB_BASE | (node_addr as u16)
}

/// 纯构造：慢帧 ID。
pub const fn slow_id(node_addr: u8) -> u16 {
    CAN_ID_SLOW_BASE | (node_addr as u16)
}

// 缩放是协议事实，不是可调门限；门限见 `limits`。

/// 电流：1 mA/LSB。
pub const LSB_AMPS: f32 = 0.001;
/// 台架注入电流：10 mA/LSB。
pub const LSB_BENCH_AMPS: f32 = 0.01;
/// 电角度：2π/65536 rad/LSB（90° = 16384）。
pub const LSB_THETA_RAD: f32 = core::f32::consts::TAU / 65_536.0;
/// 转速：0.01 rad/s = 1 crad/s。
pub const LSB_VEL_RAD_S: f32 = 0.01;
/// 母线电压：10 mV/LSB。
pub const LSB_VBUS_V: f32 = 0.01;
/// 慢帧 `d`（det/tr²）：1e-5/LSB。
pub const LSB_D: f32 = 1e-5;
/// 慢帧 `z`：1e-4/LSB。
pub const LSB_Z: f32 = 1e-4;
/// 预测界：0.01 pp/LSB。
pub const LSB_PP: f32 = 0.01;
/// χ²：0.01/LSB。
pub const LSB_CHI2: f32 = 0.01;
/// `σ̂_e`：0.01 LSB/LSB。
pub const LSB_SIGMA_E: f32 = 0.01;
/// 温度：0.1 °C/LSB。
pub const LSB_TEMP_C: f32 = 0.1;
/// Q1.14 的 1.0。
pub const GAIN_Q14_ONE: u16 = 16_384;

/// 32 bit 时间戳的回绕周期，单位 µs（4 294 967 295 µs ≈ 71.58 min）。
pub const TIMESTAMP_WRAP_US: u64 = 4_294_967_295;

/// 内部 `u64` µs → 线上 32 bit µs，有损截断，打包帧时只调用这个函数。
pub const fn timestamp_to_wire_us(t_us: u64) -> u32 {
    t_us as u32
}

/// 线上 32 bit µs → 内部 `u64`（零扩展）。回绕合并由主机负责。
pub const fn timestamp_from_wire_us(t_us: u32) -> u64 {
    t_us as u64
}

/// 线协议解码失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DecodeError {
    /// 版本号不符 / `rsvd*` 非 0 / 枚举越界 / 字段非法：整帧丢弃并计 `dropped`。
    Rejected = 0,
}

/// 命令标志位：使能。
pub const CMD_FLAG_ENABLE: u8 = 1 << 0;
/// 命令标志位：清故障（还需无故障持续 T）。
pub const CMD_FLAG_CLEAR_FAULT: u8 = 1 << 1;
/// 命令标志位：提交标定。
pub const CMD_FLAG_COMMIT_CALIB: u8 = 1 << 2;
/// 命令标志位：请求慢帧。
pub const CMD_FLAG_REQUEST_SLOW: u8 = 1 << 3;
/// 命令标志位：台架注入。
pub const CMD_FLAG_BENCH_INJECT: u8 = 1 << 4;

/// 命令标志位保留掩码：b5..b7 必须为 0（版本探测防线，非 0 即整帧丢弃）。
pub const CMD_FLAGS_RESERVED_MASK: u8 = 0xE0;

/// `ttl_ms = 0`：用默认 [`limits::budget::CAN_TIMEOUT_MS`]。
pub const TTL_MS_DEFAULT: u16 = 0;
/// `ttl_ms = 0xFFFF`：不超时（只在台架/标定维护时用）。
pub const TTL_MS_NONE: u16 = 0xFFFF;

/// 纯取值：实际生效的 TTL（毫秒）。
pub const fn effective_ttl_ms(ttl_ms: u16) -> u16 {
    match ttl_ms {
        TTL_MS_DEFAULT => limits::budget::CAN_TIMEOUT_MS,
        other => other,
    }
}

/// 命令帧（16 B，主机 → 板）。
///
/// `mode` 沿用 HR §6 编号：1 待机 / 2 标定 / 3 运行 / 4 台架；0（自检）与 5（故障）
/// 不可命令。
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct CmdFrame {
    /// 目标模式编号（`Mode::as_u8`；只接受 `is_commandable()` 为真的值）。
    pub mode: u8,
    /// 标志位（`CMD_FLAG_*`），b5..7 保留 = 0。
    pub flags: u8,
    /// 命令生存期，毫秒；0 = 默认 20 ms，0xFFFF = 不超时。
    pub ttl_ms: u16,
    /// d 轴电流参考，毫安（1 mA/LSB，±32.7 A）。
    pub i_d_ref_ma: i16,
    /// q 轴电流参考，毫安（1 mA/LSB）。
    pub i_q_ref_ma: i16,
    /// 主机帧计数，回绕。
    pub seq: u16,
    /// 台架相选择：0=A / 1=B / 2=C / 0xFF=非台架。
    pub bench_phase: u8,
    /// 台架注入电流，10 mA/LSB（由主机限幅，不用 `i_q_ref` 兼职）。
    pub bench_amps_10ma: i8,
    /// 布局版本，当前 = [`ICD_VERSION`]。
    pub ver: u8,
    /// 保留，必须为 0。
    pub rsvd: [u8; 3],
}

/// 反馈帧（32 B，板 → 主机，1 kHz）。
///
/// 状态字 bit24–31 携带帧序号低 8 位，供 LA 与帧对时。
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct FbFrame {
    /// 32 bit 状态字（`faults::StatusWord`）。
    pub state: u32,
    /// 样本时间戳（TIM5，1 µs，u32）；71.58 min 回绕一次，由主机解绕。
    /// 打包只走 [`timestamp_to_wire_us`]。
    pub t_us: u32,
    /// ISR 周期数（DWT，饱和 0xFFFF）。
    pub isr_cycles: u16,
    /// VREFINT 原始码：`VDDA = 3.0 V × CAL / code`。
    pub vrefint: u16,
    /// 三相电流，毫安（1 mA/LSB）。
    pub i_abc_ma: [i16; 3],
    /// dq 电流，毫安。
    pub i_dq_ma: [i16; 2],
    /// 电角度，2π/65536 rad/LSB。
    pub theta_e: u16,
    /// 转速，0.01 rad/s/LSB。
    pub vel_crad_s: i16,
    /// 母线电压，10 mV/LSB。
    pub vbus_10mv: u16,
    /// 采样窗口余量，千分比；0 = 无余量。
    pub margin_permille: u8,
    /// 队列/样本丢弃计数，饱和 u8。
    pub dropped: u8,
    /// 保留，必须为 0。
    pub rsvd: u16,
}

/// `trust` 位：四维门与 χ² 全过。
pub const SLOW_TRUST_TRUSTED: u8 = 1 << 0;
/// `trust` 位：激励充分（R/D 门）。
pub const SLOW_TRUST_EXCITATION_OK: u8 = 1 << 1;
/// `trust` 位：精度达标（`pred ≤ KAPPA·target`）。
pub const SLOW_TRUST_AMPLITUDE_OK: u8 = 1 << 2;
/// `trust` 位：无 DC 偏置（z 门）。
pub const SLOW_TRUST_BIAS_OK: u8 = 1 << 3;
/// `trust` 位：跨工况一致（χ² 门）。
pub const SLOW_TRUST_CHANGE_OK: u8 = 1 << 4;
/// `trust` 位：已提交/已冻结。
pub const SLOW_TRUST_COMMITTED: u8 = 1 << 5;

/// `trust` 保留掩码：b6..b7 必须为 0（版本探测防线）。
pub const SLOW_TRUST_RESERVED_MASK: u8 = 0xC0;

/// 慢帧（32 B，板 → 主机，50 Hz 或请求时）。
///
/// 布局是线协议：自然对齐、无填充，不是 `packed`；`packed` 会让字段引用在
/// `forbid(unsafe_code)` 与 `deny(indexing_slicing)` 下无法安全访问。全部 `rsvd*`
/// 必须为 0，`decode` 见非 0 即 [`DecodeError::Rejected`]。
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct SlowFrame {
    /// 参考相增益：0=A / 1=B / 2=C（参考相增益 ≡ 1.0）。
    pub g_ref_phase: u8,
    /// 闸门位（`SLOW_TRUST_*`），与状态字 bit7–10 互补表达。
    pub trust: u8,
    /// 布局版本，当前 = [`ICD_VERSION`]。
    pub ver: u8,
    /// 保留，必须为 0。
    pub rsvd0: u8,
    /// B 相相对增益，Q1.14（1.0 = 16384）。
    pub g_b_q14: u16,
    /// C 相相对增益，Q1.14。
    pub g_c_q14: u16,
    /// `d = det(S)/tr²(S)`，1e-5/LSB（0.02 = 200，上限 0.25 = 2500）。
    pub d_1e5: u16,
    /// `z`，1e-4/LSB（0.02 = 200，饱和 0xFFFF）。
    pub z_1e4: u16,
    /// 预测精度界，0.01 pp/LSB（0.05 pp = 5）。
    pub pred_pp_centi: u16,
    /// χ² 统计量，0.01/LSB（9.21 = 921）。
    pub chi2_centi: u16,
    /// 受试样本数，饱和 u16。
    pub n_samples: u16,
    /// `σ̂_e`，0.01 LSB/LSB（2.6 LSB = 260）。
    pub sigma_e_centi: u16,
    /// NTC 温度，0.1 °C/LSB。
    pub temp_dc: i16,
    /// 保留，必须为 0（占住偏移 22，使 `t_commit_us` 落在自然的 4 字节位置）。
    pub rsvd2: u16,
    /// 提交时刻（TIM5，1 µs，u32）；0 = 未提交。
    ///
    /// 71.58 min 回绕一次，由主机解绕；打包只走 [`timestamp_to_wire_us`]。
    pub t_commit_us: u32,
    /// 保留，必须为 0（以后扩字段；非 0 即整帧丢弃）。
    pub rsvd1: [u8; 4],
}

impl CmdFrame {
    /// 编码成 16 B 小端线格式。与 [`Self::decode`] 严格互逆（`rsvd` 原样写出，
    /// 调用方负责保持为 0）。
    pub fn encode(&self) -> [u8; 16] {
        let [ttl_lo, ttl_hi] = self.ttl_ms.to_le_bytes();
        let [d_lo, d_hi] = self.i_d_ref_ma.to_le_bytes();
        let [q_lo, q_hi] = self.i_q_ref_ma.to_le_bytes();
        let [seq_lo, seq_hi] = self.seq.to_le_bytes();
        let [bench_amps] = self.bench_amps_10ma.to_le_bytes();
        let [r0, r1, r2] = self.rsvd;
        [
            self.mode,
            self.flags,
            ttl_lo,
            ttl_hi,
            d_lo,
            d_hi,
            q_lo,
            q_hi,
            seq_lo,
            seq_hi,
            self.bench_phase,
            bench_amps,
            self.ver,
            r0,
            r1,
            r2,
        ]
    }

    /// 解码并校验版本、保留字节、保留标志位、模式与台架相选择。
    ///
    /// # Errors
    ///
    /// 下列任一情况返回 [`DecodeError::Rejected`]：
    ///
    /// - `ver != ICD_VERSION`，或 `rsvd != [0; 3]`；
    /// - `flags` 与 [`CMD_FLAGS_RESERVED_MASK`] 非零；
    /// - `mode` 不是 `Mode::is_commandable()` 的取值（0、5 及未知编号）；
    /// - `bench_phase` 不在 `{0, 1, 2, 0xFF}` 内。
    ///
    /// 解析路径不做假设：字段按 `offset_of!` 断言的偏移逐字节组装，不 panic。
    pub fn decode(raw: &[u8; 16]) -> Result<Self, DecodeError> {
        let [mode, flags, ttl_lo, ttl_hi, d_lo, d_hi, q_lo, q_hi, seq_lo, seq_hi, bench_phase, bench_amps_10ma, ver, r0, r1, r2] =
            *raw;

        if ver != ICD_VERSION || [r0, r1, r2] != [0; 3] {
            return Err(DecodeError::Rejected);
        }
        if flags & CMD_FLAGS_RESERVED_MASK != 0 {
            return Err(DecodeError::Rejected);
        }
        if !matches!(bench_phase, 0 | 1 | 2 | 0xFF) {
            return Err(DecodeError::Rejected);
        }
        match Mode::from_u8(mode) {
            Some(m) if m.is_commandable() => {}
            _ => return Err(DecodeError::Rejected),
        }

        Ok(Self {
            mode,
            flags,
            ttl_ms: u16::from_le_bytes([ttl_lo, ttl_hi]),
            i_d_ref_ma: i16::from_le_bytes([d_lo, d_hi]),
            i_q_ref_ma: i16::from_le_bytes([q_lo, q_hi]),
            seq: u16::from_le_bytes([seq_lo, seq_hi]),
            bench_phase,
            bench_amps_10ma: i8::from_le_bytes([bench_amps_10ma]),
            ver,
            rsvd: [r0, r1, r2],
        })
    }
}

impl FbFrame {
    /// 编码成 32 B 小端线格式。与 [`Self::decode`] 严格互逆。
    pub fn encode(&self) -> [u8; 32] {
        let [s0, s1, s2, s3] = self.state.to_le_bytes();
        let [t0, t1, t2, t3] = self.t_us.to_le_bytes();
        let [isr_lo, isr_hi] = self.isr_cycles.to_le_bytes();
        let [vref_lo, vref_hi] = self.vrefint.to_le_bytes();
        let [a, b, c] = self.i_abc_ma;
        let [ia_lo, ia_hi] = a.to_le_bytes();
        let [ib_lo, ib_hi] = b.to_le_bytes();
        let [ic_lo, ic_hi] = c.to_le_bytes();
        let [d, q] = self.i_dq_ma;
        let [id_lo, id_hi] = d.to_le_bytes();
        let [iq_lo, iq_hi] = q.to_le_bytes();
        let [th_lo, th_hi] = self.theta_e.to_le_bytes();
        let [vel_lo, vel_hi] = self.vel_crad_s.to_le_bytes();
        let [vb_lo, vb_hi] = self.vbus_10mv.to_le_bytes();
        let [r0, r1] = self.rsvd.to_le_bytes();
        [
            s0,
            s1,
            s2,
            s3,
            t0,
            t1,
            t2,
            t3,
            isr_lo,
            isr_hi,
            vref_lo,
            vref_hi,
            ia_lo,
            ia_hi,
            ib_lo,
            ib_hi,
            ic_lo,
            ic_hi,
            id_lo,
            id_hi,
            iq_lo,
            iq_hi,
            th_lo,
            th_hi,
            vel_lo,
            vel_hi,
            vb_lo,
            vb_hi,
            self.margin_permille,
            self.dropped,
            r0,
            r1,
        ]
    }

    /// 解码并校验保留字节。
    ///
    /// # Errors
    ///
    /// `rsvd != 0` 时返回 [`DecodeError::Rejected`]。其余字段是位模式，无非法值，长度由
    /// `[u8; 32]` 保证。不做时间戳回绕合并，也不校验状态字位域（由
    /// `faults::StatusWord` 的访问器按需解释）。
    pub fn decode(raw: &[u8; 32]) -> Result<Self, DecodeError> {
        let [s0, s1, s2, s3, t0, t1, t2, t3, isr_lo, isr_hi, vref_lo, vref_hi, ia_lo, ia_hi, ib_lo, ib_hi, ic_lo, ic_hi, id_lo, id_hi, iq_lo, iq_hi, th_lo, th_hi, vel_lo, vel_hi, vb_lo, vb_hi, margin_permille, dropped, r0, r1] =
            *raw;

        let rsvd = u16::from_le_bytes([r0, r1]);
        if rsvd != 0 {
            return Err(DecodeError::Rejected);
        }

        Ok(Self {
            state: u32::from_le_bytes([s0, s1, s2, s3]),
            t_us: u32::from_le_bytes([t0, t1, t2, t3]),
            isr_cycles: u16::from_le_bytes([isr_lo, isr_hi]),
            vrefint: u16::from_le_bytes([vref_lo, vref_hi]),
            i_abc_ma: [
                i16::from_le_bytes([ia_lo, ia_hi]),
                i16::from_le_bytes([ib_lo, ib_hi]),
                i16::from_le_bytes([ic_lo, ic_hi]),
            ],
            i_dq_ma: [
                i16::from_le_bytes([id_lo, id_hi]),
                i16::from_le_bytes([iq_lo, iq_hi]),
            ],
            theta_e: u16::from_le_bytes([th_lo, th_hi]),
            vel_crad_s: i16::from_le_bytes([vel_lo, vel_hi]),
            vbus_10mv: u16::from_le_bytes([vb_lo, vb_hi]),
            margin_permille,
            dropped,
            rsvd,
        })
    }
}

impl SlowFrame {
    /// 编码成 32 B 小端线格式。与 [`Self::decode`] 严格互逆（保留字段原样写出，
    /// 调用方负责保持为 0）。
    pub fn encode(&self) -> [u8; 32] {
        let [gb_lo, gb_hi] = self.g_b_q14.to_le_bytes();
        let [gc_lo, gc_hi] = self.g_c_q14.to_le_bytes();
        let [d_lo, d_hi] = self.d_1e5.to_le_bytes();
        let [z_lo, z_hi] = self.z_1e4.to_le_bytes();
        let [pred_lo, pred_hi] = self.pred_pp_centi.to_le_bytes();
        let [chi_lo, chi_hi] = self.chi2_centi.to_le_bytes();
        let [n_lo, n_hi] = self.n_samples.to_le_bytes();
        let [sg_lo, sg_hi] = self.sigma_e_centi.to_le_bytes();
        let [tmp_lo, tmp_hi] = self.temp_dc.to_le_bytes();
        let [r2_lo, r2_hi] = self.rsvd2.to_le_bytes();
        let [t0, t1, t2, t3] = self.t_commit_us.to_le_bytes();
        let [r0, r1, r2, r3] = self.rsvd1;
        [
            self.g_ref_phase,
            self.trust,
            self.ver,
            self.rsvd0,
            gb_lo,
            gb_hi,
            gc_lo,
            gc_hi,
            d_lo,
            d_hi,
            z_lo,
            z_hi,
            pred_lo,
            pred_hi,
            chi_lo,
            chi_hi,
            n_lo,
            n_hi,
            sg_lo,
            sg_hi,
            tmp_lo,
            tmp_hi,
            r2_lo,
            r2_hi,
            t0,
            t1,
            t2,
            t3,
            r0,
            r1,
            r2,
            r3,
        ]
    }

    /// 解码并校验版本、保留字段、`trust` 保留位与参考相。
    ///
    /// # Errors
    ///
    /// 下列任一情况返回 [`DecodeError::Rejected`]，整帧丢弃并计 `dropped`：
    ///
    /// - `ver != ICD_VERSION`、`rsvd0 != 0`、`rsvd1 != [0; 4]` 或 `rsvd2 != 0`；
    /// - `trust` 与 [`SLOW_TRUST_RESERVED_MASK`] 非零；
    /// - `g_ref_phase > 2`。
    pub fn decode(raw: &[u8; 32]) -> Result<Self, DecodeError> {
        let [g_ref_phase, trust, ver, rsvd0, gb_lo, gb_hi, gc_lo, gc_hi, d_lo, d_hi, z_lo, z_hi, pred_lo, pred_hi, chi_lo, chi_hi, n_lo, n_hi, sg_lo, sg_hi, tmp_lo, tmp_hi, r2_lo, r2_hi, t0, t1, t2, t3, r1_0, r1_1, r1_2, r1_3] =
            *raw;

        if ver != ICD_VERSION
            || rsvd0 != 0
            || trust & SLOW_TRUST_RESERVED_MASK != 0
            || g_ref_phase > 2
            || [r1_0, r1_1, r1_2, r1_3] != [0; 4]
            || u16::from_le_bytes([r2_lo, r2_hi]) != 0
        {
            return Err(DecodeError::Rejected);
        }

        Ok(Self {
            g_ref_phase,
            trust,
            ver,
            rsvd0,
            g_b_q14: u16::from_le_bytes([gb_lo, gb_hi]),
            g_c_q14: u16::from_le_bytes([gc_lo, gc_hi]),
            d_1e5: u16::from_le_bytes([d_lo, d_hi]),
            z_1e4: u16::from_le_bytes([z_lo, z_hi]),
            pred_pp_centi: u16::from_le_bytes([pred_lo, pred_hi]),
            chi2_centi: u16::from_le_bytes([chi_lo, chi_hi]),
            n_samples: u16::from_le_bytes([n_lo, n_hi]),
            sigma_e_centi: u16::from_le_bytes([sg_lo, sg_hi]),
            temp_dc: i16::from_le_bytes([tmp_lo, tmp_hi]),
            rsvd2: u16::from_le_bytes([r2_lo, r2_hi]),
            t_commit_us: u32::from_le_bytes([t0, t1, t2, t3]),
            rsvd1: [r1_0, r1_1, r1_2, r1_3],
        })
    }
}

// 三帧的 size 与逐字段偏移。
const _: () = assert!(cfg!(target_endian = "little"));
const _: () = assert!(ICD_VERSION == 1);
const _: () = assert!(CAN_ID_CMD_BASE | (NODE_ADDR_MAX as u16) == 0x0FF);
const _: () = assert!(CAN_ID_FB_BASE == CAN_ID_CMD_BASE + 0x080);
const _: () = assert!(CAN_ID_SLOW_BASE == CAN_ID_FB_BASE + 0x080);
const _: () = assert!(TTL_MS_DEFAULT == 0);
const _: () = assert!(GAIN_Q14_ONE == 16_384);

const _: () = assert!(core::mem::size_of::<CmdFrame>() == 16);
const _: () = assert!(core::mem::offset_of!(CmdFrame, mode) == 0);
const _: () = assert!(core::mem::offset_of!(CmdFrame, flags) == 1);
const _: () = assert!(core::mem::offset_of!(CmdFrame, ttl_ms) == 2);
const _: () = assert!(core::mem::offset_of!(CmdFrame, i_d_ref_ma) == 4);
const _: () = assert!(core::mem::offset_of!(CmdFrame, i_q_ref_ma) == 6);
const _: () = assert!(core::mem::offset_of!(CmdFrame, seq) == 8);
const _: () = assert!(core::mem::offset_of!(CmdFrame, bench_phase) == 10);
const _: () = assert!(core::mem::offset_of!(CmdFrame, bench_amps_10ma) == 11);
const _: () = assert!(core::mem::offset_of!(CmdFrame, ver) == 12);
const _: () = assert!(core::mem::offset_of!(CmdFrame, rsvd) == 13);

const _: () = assert!(core::mem::size_of::<FbFrame>() == 32);
const _: () = assert!(core::mem::offset_of!(FbFrame, state) == 0);
const _: () = assert!(core::mem::offset_of!(FbFrame, t_us) == 4);
const _: () = assert!(core::mem::offset_of!(FbFrame, isr_cycles) == 8);
const _: () = assert!(core::mem::offset_of!(FbFrame, vrefint) == 10);
const _: () = assert!(core::mem::offset_of!(FbFrame, i_abc_ma) == 12);
const _: () = assert!(core::mem::offset_of!(FbFrame, i_dq_ma) == 18);
const _: () = assert!(core::mem::offset_of!(FbFrame, theta_e) == 22);
const _: () = assert!(core::mem::offset_of!(FbFrame, vel_crad_s) == 24);
const _: () = assert!(core::mem::offset_of!(FbFrame, vbus_10mv) == 26);
const _: () = assert!(core::mem::offset_of!(FbFrame, margin_permille) == 28);
const _: () = assert!(core::mem::offset_of!(FbFrame, dropped) == 29);
const _: () = assert!(core::mem::offset_of!(FbFrame, rsvd) == 30);

const _: () = assert!(core::mem::size_of::<SlowFrame>() == 32);
const _: () = assert!(core::mem::align_of::<SlowFrame>() == 4);
const _: () = assert!(core::mem::offset_of!(SlowFrame, g_ref_phase) == 0);
const _: () = assert!(core::mem::offset_of!(SlowFrame, trust) == 1);
const _: () = assert!(core::mem::offset_of!(SlowFrame, ver) == 2);
const _: () = assert!(core::mem::offset_of!(SlowFrame, rsvd0) == 3);
const _: () = assert!(core::mem::offset_of!(SlowFrame, g_b_q14) == 4);
const _: () = assert!(core::mem::offset_of!(SlowFrame, g_c_q14) == 6);
const _: () = assert!(core::mem::offset_of!(SlowFrame, d_1e5) == 8);
const _: () = assert!(core::mem::offset_of!(SlowFrame, z_1e4) == 10);
const _: () = assert!(core::mem::offset_of!(SlowFrame, pred_pp_centi) == 12);
const _: () = assert!(core::mem::offset_of!(SlowFrame, chi2_centi) == 14);
const _: () = assert!(core::mem::offset_of!(SlowFrame, n_samples) == 16);
const _: () = assert!(core::mem::offset_of!(SlowFrame, sigma_e_centi) == 18);
const _: () = assert!(core::mem::offset_of!(SlowFrame, temp_dc) == 20);
const _: () = assert!(core::mem::offset_of!(SlowFrame, rsvd2) == 22);
const _: () = assert!(core::mem::offset_of!(SlowFrame, t_commit_us) == 24);
const _: () = assert!(core::mem::offset_of!(SlowFrame, rsvd1) == 28);

// 时间戳边界：32 bit 回绕 = 71.58 min；两个方向只允许经这两个函数
const _: () = assert!(TIMESTAMP_WRAP_US == (u32::MAX as u64));
const _: () = assert!(TIMESTAMP_WRAP_US / 60_000_000 == 71);
const _: () = assert!(timestamp_to_wire_us(TIMESTAMP_WRAP_US) == u32::MAX);
const _: () = assert!(timestamp_from_wire_us(u32::MAX) == TIMESTAMP_WRAP_US);
const _: () = assert!(core::mem::size_of::<DecodeError>() == 1);
const _: () = assert!(CMD_FLAG_BENCH_INJECT & CMD_FLAGS_RESERVED_MASK == 0);
const _: () = assert!(SLOW_TRUST_COMMITTED & SLOW_TRUST_RESERVED_MASK == 0);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Mode;

    fn sample_cmd() -> CmdFrame {
        CmdFrame {
            mode: Mode::Run.as_u8(),
            flags: CMD_FLAG_ENABLE | CMD_FLAG_REQUEST_SLOW,
            ttl_ms: 20,
            i_d_ref_ma: 0,
            i_q_ref_ma: -1_234,
            seq: 0xBEEF,
            bench_phase: 0xFF,
            bench_amps_10ma: -5,
            ver: ICD_VERSION,
            rsvd: [0; 3],
        }
    }

    fn sample_fb() -> FbFrame {
        FbFrame {
            state: 0x0A00_1234,
            t_us: 0xDEAD_BEEF,
            isr_cycles: 4_123,
            vrefint: 1_500,
            i_abc_ma: [1_000, -2_000, 3_000],
            i_dq_ma: [-700, 1_400],
            theta_e: 0xABCD,
            vel_crad_s: -12_345,
            vbus_10mv: 1_200,
            margin_permille: 250,
            dropped: 7,
            rsvd: 0,
        }
    }

    fn sample_slow() -> SlowFrame {
        SlowFrame {
            g_ref_phase: 1,
            trust: SLOW_TRUST_TRUSTED
                | SLOW_TRUST_EXCITATION_OK
                | SLOW_TRUST_AMPLITUDE_OK
                | SLOW_TRUST_BIAS_OK
                | SLOW_TRUST_CHANGE_OK
                | SLOW_TRUST_COMMITTED,
            ver: ICD_VERSION,
            rsvd0: 0,
            g_b_q14: GAIN_Q14_ONE + 37,
            g_c_q14: GAIN_Q14_ONE - 21,
            d_1e5: 200,
            z_1e4: 123,
            pred_pp_centi: 5,
            chi2_centi: 921,
            n_samples: 2_000,
            sigma_e_centi: 260,
            temp_dc: -125,
            rsvd2: 0,
            t_commit_us: 0xDEAD_BEEF,
            rsvd1: [0; 4],
        }
    }

    /// 测试专用：把 `raw` 的某字节改成 `value`（越界直接失败，不静默）。
    fn poke<const N: usize>(mut raw: [u8; N], at: usize, value: u8) -> [u8; N] {
        let Some(slot) = raw.get_mut(at) else {
            panic!("offset {at} out of range for {N} bytes")
        };
        *slot = value;
        raw
    }

    #[test]
    fn cmd_frame_roundtrips() {
        let frame = sample_cmd();
        assert_eq!(CmdFrame::decode(&frame.encode()), Ok(frame));
    }

    #[test]
    fn cmd_frame_accepts_every_commandable_mode() {
        for mode in [Mode::Standby, Mode::Calibrate, Mode::Run, Mode::Bench] {
            let mut frame = sample_cmd();
            frame.mode = mode.as_u8();
            assert_eq!(CmdFrame::decode(&frame.encode()), Ok(frame), "{mode:?}");
        }
    }

    #[test]
    fn cmd_frame_accepts_every_bench_phase() {
        for phase in [0u8, 1, 2, 0xFF] {
            let mut frame = sample_cmd();
            frame.bench_phase = phase;
            assert_eq!(
                CmdFrame::decode(&frame.encode()),
                Ok(frame),
                "phase {phase}"
            );
        }
    }

    #[test]
    fn cmd_decode_rejects_wrong_version() {
        let bad = poke(
            sample_cmd().encode(),
            core::mem::offset_of!(CmdFrame, ver),
            ICD_VERSION + 1,
        );
        assert_eq!(CmdFrame::decode(&bad), Err(DecodeError::Rejected));
    }

    #[test]
    fn cmd_decode_rejects_nonzero_reserved_bytes() {
        let raw = sample_cmd().encode();
        let base = core::mem::offset_of!(CmdFrame, rsvd);
        for i in 0..3 {
            let bad = poke(raw, base + i, 1);
            assert_eq!(
                CmdFrame::decode(&bad),
                Err(DecodeError::Rejected),
                "rsvd byte {i}"
            );
        }
    }

    #[test]
    fn cmd_decode_rejects_reserved_flag_bits() {
        for bit in 5..8 {
            let bad = poke(
                sample_cmd().encode(),
                core::mem::offset_of!(CmdFrame, flags),
                1 << bit,
            );
            assert_eq!(
                CmdFrame::decode(&bad),
                Err(DecodeError::Rejected),
                "flag bit {bit}"
            );
        }
    }

    #[test]
    fn cmd_decode_rejects_non_commandable_mode() {
        for raw_mode in [Mode::SelfTest.as_u8(), Mode::Fault.as_u8(), 6, 0xFF] {
            let bad = poke(
                sample_cmd().encode(),
                core::mem::offset_of!(CmdFrame, mode),
                raw_mode,
            );
            assert_eq!(
                CmdFrame::decode(&bad),
                Err(DecodeError::Rejected),
                "mode {raw_mode}"
            );
        }
    }

    #[test]
    fn cmd_decode_rejects_out_of_range_bench_phase() {
        for phase in [3u8, 0xFE] {
            let bad = poke(
                sample_cmd().encode(),
                core::mem::offset_of!(CmdFrame, bench_phase),
                phase,
            );
            assert_eq!(
                CmdFrame::decode(&bad),
                Err(DecodeError::Rejected),
                "phase {phase}"
            );
        }
    }

    #[test]
    fn fb_frame_roundtrips() {
        let frame = sample_fb();
        assert_eq!(FbFrame::decode(&frame.encode()), Ok(frame));
    }

    #[test]
    fn fb_decode_rejects_nonzero_reserved_bytes() {
        let raw = sample_fb().encode();
        let base = core::mem::offset_of!(FbFrame, rsvd);
        for i in 0..2 {
            let bad = poke(raw, base + i, 0x01);
            assert_eq!(
                FbFrame::decode(&bad),
                Err(DecodeError::Rejected),
                "rsvd byte {i}"
            );
        }
    }

    #[test]
    fn slow_frame_roundtrips() {
        let frame = sample_slow();
        assert_eq!(SlowFrame::decode(&frame.encode()), Ok(frame));
    }

    #[test]
    fn slow_frame_accepts_every_reference_phase() {
        for phase in 0u8..=2 {
            let mut frame = sample_slow();
            frame.g_ref_phase = phase;
            assert_eq!(
                SlowFrame::decode(&frame.encode()),
                Ok(frame),
                "phase {phase}"
            );
        }
    }

    #[test]
    fn slow_decode_rejects_wrong_version() {
        let bad = poke(
            sample_slow().encode(),
            core::mem::offset_of!(SlowFrame, ver),
            ICD_VERSION + 1,
        );
        assert_eq!(SlowFrame::decode(&bad), Err(DecodeError::Rejected));
    }

    #[test]
    fn slow_decode_rejects_nonzero_reserved_fields() {
        let raw = sample_slow().encode();
        let rsvd0 = core::mem::offset_of!(SlowFrame, rsvd0);
        let rsvd1 = core::mem::offset_of!(SlowFrame, rsvd1);
        let rsvd2 = core::mem::offset_of!(SlowFrame, rsvd2);

        assert_eq!(
            SlowFrame::decode(&poke(raw, rsvd0, 1)),
            Err(DecodeError::Rejected),
            "rsvd0"
        );
        for i in 0..4 {
            assert_eq!(
                SlowFrame::decode(&poke(raw, rsvd1 + i, 1)),
                Err(DecodeError::Rejected),
                "rsvd1 byte {i}"
            );
        }
        for i in 0..2 {
            assert_eq!(
                SlowFrame::decode(&poke(raw, rsvd2 + i, 1)),
                Err(DecodeError::Rejected),
                "rsvd2 byte {i}"
            );
        }
    }

    #[test]
    fn slow_decode_rejects_reserved_trust_bits() {
        for bit in [6, 7] {
            let bad = poke(
                sample_slow().encode(),
                core::mem::offset_of!(SlowFrame, trust),
                1 << bit,
            );
            assert_eq!(
                SlowFrame::decode(&bad),
                Err(DecodeError::Rejected),
                "trust bit {bit}"
            );
        }
    }

    #[test]
    fn slow_decode_rejects_unknown_reference_phase() {
        for phase in [3u8, 0xFF] {
            let bad = poke(
                sample_slow().encode(),
                core::mem::offset_of!(SlowFrame, g_ref_phase),
                phase,
            );
            assert_eq!(
                SlowFrame::decode(&bad),
                Err(DecodeError::Rejected),
                "phase {phase}"
            );
        }
    }
}
