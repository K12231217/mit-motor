//! 物理量、定点数与模式枚举（PRE-06：量纲进标识符）。
//!
//! 只放板内外用同一句话描述的量：占空比、原始样本、相电流、dq 目标、模式与标定步骤。
//! 带策略的换算（夹紧、四舍五入、死区补偿）不在这里实现。

use crate::limits;

/// Q1.15 满量程（1.0 的定点表示）。
pub const Q15_FULL_SCALE: f32 = 32_768.0;

/// 有符号 Q1.15 定点数：`raw / 32768` 落在 `[-1.0, 0.99997]`。
///
/// 用于调制量 `m = 2d - 1`，`d` 是高边占空比；`d = D_MAX = 0.60` 时 `m = 0.2`，
/// 所以合法原始码限制在 `±0.2 · 32768`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct Q15(i16);

impl Q15 {
    /// `0.0`（= 50 % 占空比，零线电压）。
    pub const ZERO: Self = Self(0);
    /// 最大可表示值（`32767 / 32768`）。
    pub const MAX: Self = Self(i16::MAX);
    /// 最小可表示值（`-1.0`）。
    pub const MIN: Self = Self(i16::MIN);

    /// 纯构造：不做夹紧、不做舍入。
    pub const fn from_raw(raw: i16) -> Self {
        Self(raw)
    }

    /// 纯取值：原始码。
    pub const fn raw(self) -> i16 {
        self.0
    }

    /// 纯换算：定点 → 比值，除以 `2^15`，无分支。
    pub fn to_ratio(self) -> f32 {
        f32::from(self.0) / Q15_FULL_SCALE
    }

    /// 比值 → 定点：四舍五入（half away from zero）、饱和到 `[MIN, MAX]`、
    /// `NaN → ZERO`（`NaN` 没有可表示的定点值，按"无指令"处理）。
    ///
    /// 饱和边界是定点类型自身的 `[-1.0, 32767/32768]`，不是 [`MODULATION_MAX`]：
    /// 超限指令必须原样落到这里，[`ClampedModulation::try_new`] 才能事后判为超限；若在这里
    /// 按 `D_MAX` 悄悄夹紧，判据会永远为真，算法错误就变成静默饱和。
    ///
    /// `+1.0` 对应 `32768`，超出 `i16`，因此正饱和值是 [`Self::MAX`]（`32767`）；
    /// `-1.0` 恰好是 [`Self::MIN`]。
    pub fn from_ratio(ratio: f32) -> Self {
        if ratio.is_nan() {
            return Self::ZERO;
        }
        // no_std：`f32::round` 在 std 才有，这里用"加/减半 LSB 后截断"实现四舍五入
        // （远离 0）。|scaled| ≤ ~1e38 时加 0.5 不会丢整数位：结果只可能是 ±32k 量级，
        // 远小于 f32 的 2^24 精确整数上限。
        let scaled = ratio * Q15_FULL_SCALE;
        let rounded = if scaled.is_sign_negative() {
            scaled - 0.5
        } else {
            scaled + 0.5
        };
        // ±inf 与超量程的有限值都在这里饱和。
        if rounded >= f32::from(i16::MAX) {
            Self::MAX
        } else if rounded <= f32::from(i16::MIN) {
            Self::MIN
        } else {
            Self::from_raw(rounded as i16)
        }
    }
}

/// 三相调制量上限：`d ≤ D_MAX ⇒ |m| ≤ 2 · D_MAX - 1`。
pub const MODULATION_MAX: f32 = 2.0 * limits::hw::D_MAX - 1.0;

/// [`MODULATION_MAX`] 的定点表示（四舍五入到最近的码），派生值，不引入新门限。
///
/// `D_MAX = 0.60 ⇒ m = 0.2 ⇒ 0.2 × 32768 = 6553.6 → 6554`。它是
/// [`ClampedModulation`] 的量化边界：`Q15::from_ratio(MODULATION_MAX)` 恰好落在
/// 这一码上，所以"恰好等于 `D_MAX`"不会因为最后一个 LSB 的舍入被判超限，再大一个码
/// 就算超限。
pub const MODULATION_MAX_RAW: i16 = (MODULATION_MAX * Q15_FULL_SCALE + 0.5) as i16;

/// 保证满足 `|raw| <= MODULATION_MAX_RAW` 的合法调制量。
///
/// 字段私有：模块外无法绕过检查拼出一个越界的调制量。取值为 Q1.15 的 `m = 2d - 1`，
/// 因此 `D_MAX` 约束在类型层面成立，而不是靠调用方记得事后调用判据（PRE-08）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct ClampedModulation(Q15);

impl ClampedModulation {
    /// 零调制量（三路 50 %）。
    pub const ZERO: Self = Self(Q15::ZERO);

    /// 受检构造：超限直接返回 `Err`，由边界入口决定进 Fault 还是饱和。
    ///
    /// 返回原始 `Q15` 而不是丢值，便于调用方把拒绝的码写进遥测。
    pub const fn try_new(q: Q15) -> Result<Self, Q15> {
        let raw = q.raw();
        let limit = MODULATION_MAX_RAW;
        if raw >= -limit && raw <= limit {
            Ok(Self(q))
        } else {
            Err(q)
        }
    }

    /// 显式饱和构造：夹紧到 `[-MODULATION_MAX_RAW, +MODULATION_MAX_RAW]`。
    ///
    /// 只给"算法给出小幅越界、策略就是饱和"的入口用；需要区分故障时用
    /// [`Self::try_new`]。
    pub const fn new_clamped(q: Q15) -> Self {
        let max = MODULATION_MAX_RAW;
        let raw = q.raw();
        let clamped = if raw > max {
            max
        } else if raw < -max {
            -max
        } else {
            raw
        };
        Self(Q15::from_raw(clamped))
    }

    /// 纯取值：把调制量交回定点域。
    pub const fn get(self) -> Q15 {
        self.0
    }
}

/// 三相输出指令。
///
/// - `Pwm`：每相存调制量 `m = 2d - 1`（Q1.15），`m = 0` 是该相 50 % 占空比。
///   字段是 [`ClampedModulation`]，越界占空比在构造处就无法表达。
/// - `Coast`：六路栅极全低。MP6540HA 没有 EN 引脚，六路全低即三相 Hi-Z，与
///   `m = 0`（每相 50 %，三相同电位）不是同一件事，不允许用零调制量冒充。
///
/// 上电默认 `Coast`；任何故障锁存后必须回到 `Coast` 而不是 `Pwm{0,0,0}`（PRE-08）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duty {
    /// 六路全低 = Hi-Z（安全态）。
    Coast,
    /// 三相调制量：类型系统保证均满足 `D_MAX` 低边采样窗口约束。
    Pwm {
        a: ClampedModulation,
        b: ClampedModulation,
        c: ClampedModulation,
    },
}

impl Duty {
    /// 纯构造：安全态。
    pub const fn coast() -> Self {
        Self::Coast
    }

    /// 纯构造：零调制量（三路 50 %）。不是 `Coast`。
    pub const fn zero_modulation() -> Self {
        Self::Pwm {
            a: ClampedModulation::ZERO,
            b: ClampedModulation::ZERO,
            c: ClampedModulation::ZERO,
        }
    }
}

/// 一拍三相 ADC 原始码（12 bit 右对齐，未减零位、未乘增益）。
///
/// 字段名带 `_code` / `_us`，这里绝不出现安培（PRE-06）。
///
/// `tick` 是 PWM 周期单调递增的拍号。三路 ADC 注入组必须由同一拍触发，任意一路的
/// `tick` 与当前控制拍不一致即视为跨拍拼凑，调用方不得把它当同一拍样本使用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct RawSample3 {
    /// PWM 周期单调递增拍号（用于校验三路 ADC 注入组同步归属同一拍，以及监督看门狗）。
    pub tick: u32,
    /// 采样时刻（TIM5，1 µs）。
    pub t_us: u32,
    /// 三路原始码，顺序固定 A/B/C。
    pub code: [u16; 3],
}

/// 三相电流，单位安培；归算后，含零位与相对增益。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct PhaseCurrent {
    /// A 相电流，安培。
    pub a_amps: f32,
    /// B 相电流，安培。
    pub b_amps: f32,
    /// C 相电流，安培。
    pub c_amps: f32,
}

/// dq 电流目标，单位安培。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct DqTarget {
    /// d 轴电流目标，安培（表贴电机，本工程恒为 0）。
    pub d_amps: f32,
    /// q 轴电流目标，安培。
    pub q_amps: f32,
}

/// 电机本体参数，单位进字段名（PRE-06）。
///
/// 本期不做在线辨识：数值由离线 LCR 表/示波器实测后，经标定或应用层配置传入。
/// 类型放这里只为跨 crate 传递；取值策略不在这里。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct MotorParams {
    /// 定子相电阻，欧姆。
    pub r_ohm: f32,
    /// d 轴电感，亨利。
    pub ld_h: f32,
    /// q 轴电感，亨利。
    pub lq_h: f32,
    /// 永磁磁链，韦伯。
    pub flux_wb: f32,
}

impl MotorParams {
    /// 是否可用于电流环：全部有限，且 `R > 0`、`Ld > 0`、`Lq > 0`、`ψ ≥ 0`。
    pub fn is_valid(&self) -> bool {
        self.r_ohm.is_finite()
            && self.ld_h.is_finite()
            && self.lq_h.is_finite()
            && self.flux_wb.is_finite()
            && self.r_ohm > 0.0
            && self.ld_h > 0.0
            && self.lq_h > 0.0
            && self.flux_wb >= 0.0
    }
}

/// dq 电压指令，单位伏特；电流环输出、逆 Park 之前的一站。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct DqVolts {
    /// d 轴电压，伏特。
    pub d_v: f32,
    /// q 轴电压，伏特。
    pub q_v: f32,
}

impl DqVolts {
    /// 零电压指令（安全兜底）。
    pub const ZERO: Self = Self {
        d_v: 0.0,
        q_v: 0.0,
    };
}

/// 运行模式。
///
/// 编号必须与 `docs/hardware-requirements.md` §6 的状态字 bit0–3 一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    /// 0 自检（S0–S5）。不可被 CAN 命令进入。
    SelfTest = 0,
    /// 1 待机（六路全低）。
    Standby = 1,
    /// 2 标定（C0–C6）。
    Calibrate = 2,
    /// 3 运行（20 kHz 电流环）。
    Run = 3,
    /// 4 台架（单相低边常通注入）。
    Bench = 4,
    /// 5 故障（锁存，只可由显式命令加无故障持续 T 清除）。
    Fault = 5,
}

impl Mode {
    /// 纯取值：状态字 bit0–3 的编号。
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// 纯构造：编号 → 模式；0–5 之外返回 `None`。
    pub const fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::SelfTest),
            1 => Some(Self::Standby),
            2 => Some(Self::Calibrate),
            3 => Some(Self::Run),
            4 => Some(Self::Bench),
            5 => Some(Self::Fault),
            _ => None,
        }
    }

    /// 纯判据：CAN 命令能否要求进入该模式。自检与故障只能由固件自身进入。
    pub const fn is_commandable(self) -> bool {
        matches!(
            self,
            Self::Standby | Self::Calibrate | Self::Run | Self::Bench
        )
    }
}

/// 标定步骤 C0–C6。顺序即执行顺序，编号进慢帧与日志。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CalibStep {
    /// C0 零位测量：六路低、无电流，记录三相零位。
    C0Zero = 0,
    /// C1 极性/相序：正反注入确认符号约定。
    C1Polarity = 1,
    /// C2 方向覆盖：`R ≥ R_MAX`。
    C2Direction = 2,
    /// C3 相对增益 RLS 累积（`D ≥ D_MIN` 门控）。
    C3GainAccum = 3,
    /// C4 偏置门：`z ≤ Z_MAX`。
    C4Bias = 4,
    /// C5 一致性加精度降额：χ² 与 `pred ≤ KAPPA · target`。
    C5Consistency = 5,
    /// C6 冻结：发布 Gain 快照；档案落盘布局见 `contracts::limits::hw` 的双槽定义。
    C6Freeze = 6,
}

impl CalibStep {
    /// 纯取值：慢帧的 `CalibStep` 编号。
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// 纯构造：编号 → 步骤；0–6 之外返回 `None`。
    pub const fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::C0Zero),
            1 => Some(Self::C1Polarity),
            2 => Some(Self::C2Direction),
            3 => Some(Self::C3GainAccum),
            4 => Some(Self::C4Bias),
            5 => Some(Self::C5Consistency),
            6 => Some(Self::C6Freeze),
            _ => None,
        }
    }

    /// 下一有效步骤；C6 之后无步骤（`None`）。
    ///
    /// 迁移表只有顺序 C0→C1→…→C6→`None`。某一步判据失败时回退到哪里、是否锁存属于
    /// 标定状态机，不在契约层表达。
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::C0Zero => Some(Self::C1Polarity),
            Self::C1Polarity => Some(Self::C2Direction),
            Self::C2Direction => Some(Self::C3GainAccum),
            Self::C3GainAccum => Some(Self::C4Bias),
            Self::C4Bias => Some(Self::C5Consistency),
            Self::C5Consistency => Some(Self::C6Freeze),
            Self::C6Freeze => None,
        }
    }
}

// 尺寸变化必须显式暴露：这些类型会进 SRAM 快照与队列。
const _: () = assert!(core::mem::size_of::<Q15>() == 2);
const _: () = assert!(core::mem::size_of::<ClampedModulation>() == 2);
const _: () = assert!(core::mem::size_of::<RawSample3>() == 16);
const _: () = assert!(core::mem::offset_of!(RawSample3, tick) == 0);
const _: () = assert!(core::mem::offset_of!(RawSample3, t_us) == 4);
const _: () = assert!(core::mem::offset_of!(RawSample3, code) == 8);
const _: () = assert!(core::mem::size_of::<PhaseCurrent>() == 12);
const _: () = assert!(core::mem::size_of::<DqTarget>() == 8);
const _: () = assert!(core::mem::offset_of!(DqTarget, d_amps) == 0);
const _: () = assert!(core::mem::offset_of!(DqTarget, q_amps) == 4);
const _: () = assert!(core::mem::size_of::<MotorParams>() == 16);
const _: () = assert!(core::mem::offset_of!(MotorParams, r_ohm) == 0);
const _: () = assert!(core::mem::offset_of!(MotorParams, ld_h) == 4);
const _: () = assert!(core::mem::offset_of!(MotorParams, lq_h) == 8);
const _: () = assert!(core::mem::offset_of!(MotorParams, flux_wb) == 12);
const _: () = assert!(core::mem::size_of::<DqVolts>() == 8);
const _: () = assert!(core::mem::offset_of!(DqVolts, d_v) == 0);
const _: () = assert!(core::mem::offset_of!(DqVolts, q_v) == 4);
const _: () = assert!(core::mem::size_of::<Mode>() == 1);
const _: () = assert!(core::mem::size_of::<CalibStep>() == 1);
const _: () = assert!(MODULATION_MAX < 1.0);
const _: () = assert!(MODULATION_MAX_RAW > 0);
const _: () = assert!(MODULATION_MAX_RAW < i16::MAX);

#[cfg(test)]
mod tests {
    use super::*;

    /// 定点域的四舍五入：半个 LSB 远离 0，整 LSB 精确。
    #[test]
    fn q15_from_ratio_rounds_half_away_from_zero() {
        let half_lsb = 0.5 / Q15_FULL_SCALE;
        assert_eq!(Q15::from_ratio(half_lsb).raw(), 1);
        assert_eq!(Q15::from_ratio(-half_lsb).raw(), -1);
        assert_eq!(Q15::from_ratio(1.0 / Q15_FULL_SCALE).raw(), 1);
        assert_eq!(Q15::from_ratio(-1.0 / Q15_FULL_SCALE).raw(), -1);
        assert_eq!(Q15::from_ratio(0.4 / Q15_FULL_SCALE).raw(), 0);
    }

    #[test]
    fn q15_from_ratio_saturates_and_maps_nan_to_zero() {
        assert_eq!(Q15::from_ratio(1.0), Q15::MAX);
        assert_eq!(Q15::from_ratio(2.0), Q15::MAX);
        assert_eq!(Q15::from_ratio(f32::INFINITY), Q15::MAX);
        assert_eq!(Q15::from_ratio(-1.0), Q15::MIN);
        assert_eq!(Q15::from_ratio(-2.0), Q15::MIN);
        assert_eq!(Q15::from_ratio(f32::NEG_INFINITY), Q15::MIN);
        assert_eq!(Q15::from_ratio(f32::NAN), Q15::ZERO);
        assert_eq!(Q15::from_ratio(0.0), Q15::ZERO);
    }

    /// 每个 `i16` 原始码 → 比值 → 原始码必须回来（除以/乘以 `2^15` 在 f32 里精确）。
    #[test]
    fn q15_ratio_roundtrip_is_lossless() {
        for raw in [i16::MIN, -19_661, -1, 0, 1, 19_661, i16::MAX] {
            let q = Q15::from_raw(raw);
            assert_eq!(Q15::from_ratio(q.to_ratio()).raw(), raw, "raw = {raw}");
        }
    }

    /// 边界：恰好 `D_MAX`（量化码 6554）合法，任一方向多一个 LSB 即构造失败。
    #[test]
    fn clamped_modulation_boundary_matches_d_max() {
        // 0.2 × 32768 = 6553.6 → 四舍五入 6554。
        assert_eq!(MODULATION_MAX_RAW, 6_554);
        let max = Q15::from_ratio(MODULATION_MAX);
        assert_eq!(max.raw(), MODULATION_MAX_RAW);
        assert_eq!(
            ClampedModulation::try_new(max),
            Ok(ClampedModulation::new_clamped(max))
        );

        for raw in [0, 1, -1, MODULATION_MAX_RAW, -MODULATION_MAX_RAW] {
            let q = Q15::from_raw(raw);
            assert!(ClampedModulation::try_new(q).is_ok(), "raw = {raw}");
        }
        for raw in [
            MODULATION_MAX_RAW + 1,
            -MODULATION_MAX_RAW - 1,
            i16::MIN,
            i16::MAX,
        ] {
            let q = Q15::from_raw(raw);
            assert_eq!(ClampedModulation::try_new(q), Err(q), "raw = {raw}");
        }
    }

    /// 显式饱和只夹到 `D_MAX`，不夹到定点满量程。
    #[test]
    fn clamped_modulation_saturates_at_d_max() {
        let saturated = ClampedModulation::new_clamped(Q15::from_raw(i16::MAX));
        assert_eq!(saturated.get().raw(), MODULATION_MAX_RAW);
        let saturated = ClampedModulation::new_clamped(Q15::from_raw(i16::MIN));
        assert_eq!(saturated.get().raw(), -MODULATION_MAX_RAW);
        assert_eq!(
            ClampedModulation::new_clamped(Q15::from_raw(123))
                .get()
                .raw(),
            123
        );
    }

    /// `Coast` 与零调制量语义不同，但都不携带越界码。
    #[test]
    fn duty_coast_stays_distinct_from_zero_modulation() {
        assert_ne!(Duty::Coast, Duty::zero_modulation());
        match Duty::zero_modulation() {
            Duty::Pwm { a, b, c } => {
                assert_eq!(a.get(), Q15::ZERO);
                assert_eq!(b.get(), Q15::ZERO);
                assert_eq!(c.get(), Q15::ZERO);
            }
            Duty::Coast => panic!("zero_modulation 不得为 Coast"),
        }
    }

    /// 从 C0 走到 C6 必须恰好七步、编号连续，且 C6 之后与越界输入都是 None。
    #[test]
    fn calib_step_next_walks_all_seven_steps_once() {
        let mut step = CalibStep::from_u8(0);
        let mut seen = 0u8;
        while let Some(current) = step {
            assert_eq!(current.as_u8(), seen);
            seen += 1;
            step = current.next();
        }
        assert_eq!(seen, 7);
        assert_eq!(CalibStep::from_u8(7), None);
        assert_eq!(CalibStep::from_u8(0xFF), None);
    }
}
