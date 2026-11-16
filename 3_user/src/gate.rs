//! 相对增益可信闸门（四维结构门 + χ²），以及相对增益正规方程累加器。
//!
//! # 为什么需要闸门
//!
//! 三路采样链的**共模**增益不可观测：三相同比例放大时 `i_a + i_b + i_c = 0`
//! 仍然成立，最小二乘残差对它完全盲。相对增益（`g_b/g_a`、`g_c/g_a`）只有
//! 在电流向量"动起来"、且没有 DC 主导时才能被辨识出来。因此闸门不是
//! 可选的健康度检查，而是**参数可辨识性的前提**（PRE-05：无静默继续）。
//!
//! # 四维门与本模块的对应关系
//!
//! | 维度 | 量 | 阈值（limits.rs） | 抓什么 |
//! |---|---|---|---|
//! | 方向覆盖 | `R² = |Σi_αβ|² / (n·Σ|i_αβ|²)` | `R_MAX²` | 激励方向单一（常数向量 ⇒ `R²=1`）|
//! | 信息矩阵 | `d = det(S)/tr²(S)`（减零位后） | `D_MIN` | 无激励 / 回归量秩亏 |
//! | DC 主导 | `z² = 3·A0²/(G·A1)` | `Z_MAX²` | 整段电流偏一侧，把偏置学成增益 |
//! | 精度降额 | `pred ≤ KAPPA·target_pp` | `KAPPA` | `pred` 是 RMS 界，p99/pred ≈ 2.3 |
//!
//! 一致性门 `CHI2_MAX` 只在通过上面四条之后才算——否则 χ² 的分布假设不成立。
//!
//! # 预测界（写进实现时的公式，不要改形状）
//!
//! ```text
//! pred(N_eq) = 100 · [ √3·σ̂_e / (s_rms·√N_eq) + (σ̂_e² + δo²) / s_rms² ]   [pp]
//! N_eq = (1 + λ)·Σw,  λ = 1 - 1/2000,  Σw ≤ 2000 ⇒ N_eq ≤ 3999
//! ```
//!
//! 全部量都在**减零位后的 LSB（码）域**：`σ̂_e`、`s_rms`、`δo` 同量纲，比值
//! 无量纲，`pred` 直接出 pp。闸门内部不做码 → 安培换算（PRE-06）。
//!
//! 幅值参考点（是**上限**，不参与计算；σ̂_ch = 1.5 LSB 单通道、N_eq ≈ 3998 实测）：
//! 0.14 A → 0.19 pp（按 RMS）/ 0.32 pp（按峰值）；0.55 A → 0.028 pp（RMS）/
//! 0.044 pp（峰值）。冻结记录要求 ≤0.5 pp @ 0.14 A、≤0.05 pp @ 0.55 A，
//! 两种幅值口径都满足。设备侧固定接受 `pred ≤ KAPPA·TARGET_PP = 0.45 pp`；
//! 主机可以拿慢帧里的 `pred_pp` 按自己的目标离线重判（例如台架要 0.05 pp 时）。

use contracts::limits;
use contracts::types::RawSample3;
use contracts::Trust;

/// √3（幅值不变 Clarke 变换用）。
const SQRT3: f32 = 1.732_050_8;

/// 2×2 行列式近零判据。`S` 的元素是码²·权重（量级 1e5 以上），
/// 小于它的 `det` 已没有数值意义。
const DET_EPS: f32 = 1e-12;

/// 12 bit ADC 标称中点（`VREF/2`，HR-203 的 1.65 V）。实际零位由 C0 测量后
/// 经 [`GainGate::set_zero_lsb`] 覆盖；这里只给 host 测试与冷启动一个确定值。
const ADC_MIDSCALE_CODE: f32 = 2_048.0;

/// 闸门参数集合。全部取自 [`crate::limits`]，禁止在别处另存副本。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateParams {
    /// 方向覆盖门限（FROZEN 0.995）。见模块文档：残差对共模增益盲，必须确认向量在转。
    pub r_max: f32,
    /// 信息矩阵门限（FROZEN 0.01），作用在**减零位后**的回归量上。
    pub d_min: f32,
    /// DC 偏置门限（FROZEN 0.02）：残差对增益盲，`z` 专抓"三相同时偏"的窗口。
    pub z_max: f32,
    /// 精度门降额（FROZEN 0.9）：`pred` 是 RMS 界，实测 p99/pred ≈ 2.3。
    pub kappa: f32,
    /// 精度目标（FROZEN 0.5 pp）：设备侧接受判据，不是从幅值推出来的。
    /// 做成字段是为了让测试能改它，验证 `kappa` 降额真的在起作用。
    pub target_pp: f32,
    /// χ²(2) 99 % 分位点（FROZEN 9.21），只在 r/d/z 全过后计算。
    pub chi2_max: f32,
    /// 遗忘因子（FROZEN 0.9995）。
    pub lambda: f32,
    /// 组窗样本数 `G`（FROZEN 200 = 10 ms @ 20 kHz）。
    pub group_samples: u32,
}

impl GateParams {
    /// 冻结参数集。数值只在 `limits.rs` 定义一次（PRE-07）。
    pub const FROZEN: Self = Self {
        r_max: limits::R_MAX,
        d_min: limits::D_MIN,
        z_max: limits::Z_MAX,
        kappa: limits::KAPPA,
        target_pp: limits::TARGET_PP,
        chi2_max: limits::CHI2_MAX,
        lambda: limits::LAMBDA,
        group_samples: limits::GROUP_SAMPLES,
    };
}

/// 相对增益正规方程累加器。
///
/// 未知量固定为 `x = [g_b, g_c]`（参考相 `g_a ≡ 1`）。**共模增益不可观测**，
/// 因此这里没有"平均增益"字段，线协议里也不允许出现（见 `icd.rs` 的慢帧）。
///
/// 模型：`y = φᵀθ`，`φ = [s_b, s_c]`，`y = -s_a`，`θ = [γ_b, γ_c]`。
/// 所有量都是"减零位后"的回归量，单位是 **ADC LSB（码）**，不是安培。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainAccum {
    /// 信息矩阵 `S = Σ w·φφᵀ`，按行主序压成 `[s00, s01, s10, s11]`。
    /// `d = det(S)/tr²(S)` 从这里算；`D_MIN` 门控就是看它有没有秩亏。
    pub s: [f32; 4],
    /// 正规方程右端 `b = Σ w·φ·y`。
    pub b: [f32; 2],
    /// `A0 = Σ(s_a + s_b + s_c)`：z 门的分子。对**共模**偏移敏感，对增益不敏感。
    pub a0: f32,
    /// `A1 = Σ(s_a² + s_b² + s_c²)`：z 门的分母（乘组窗 G 后开方）。
    pub a1: f32,
    /// 组窗内已累加样本数 `G`（每 `group_samples` 清零重算 z）。
    pub groups: u32,
    /// 权重和 `Σw`，被遗忘因子限制在 `WINDOW_SAMPLES`。
    pub weight: f32,
    /// 残差平方和 `Σe²`，供 `σ̂_e`（`pred` 第一项）。
    pub sse: f32,
    /// `δo²`：零位残余不确定度，供 `pred` 第二项（单位 LSB²）。
    pub do2: f32,
    /// `Σi_α`（Clarke 幅值不变版：`i_α = s_a`）。
    pub rsum: [f32; 2],
    /// `Σ(i_α² + i_β²)`，方向覆盖门的归一化分母。
    pub ri2: f32,
    /// 方向覆盖门的样本数（与 `groups` 分开：组窗清空后仍要能算 R）。
    pub rn: u32,
    /// 当前相对增益估计 `[γ_b, γ_c]`，每组窗末由 [`Self::solve`] 刷新；
    /// `push_sample` 用它算残差 `e`。初值 `[1, 1]`（未标定）。
    pub theta: [f32; 2],
}

impl GainAccum {
    /// 纯构造：全零，相对增益按名义值 1。
    pub const fn empty() -> Self {
        Self {
            s: [0.0; 4],
            b: [0.0; 2],
            a0: 0.0,
            a1: 0.0,
            groups: 0,
            weight: 0.0,
            sse: 0.0,
            do2: 0.0,
            rsum: [0.0; 2],
            ri2: 0.0,
            rn: 0,
            theta: [1.0, 1.0],
        }
    }

    /// 累加一个样本（已减零位，单位 LSB 码）。`weight` 是本拍样本权重 `w`。
    ///
    /// 信息形式（λ 遗忘）：
    /// `S ← λS + w·φφᵀ`、`b ← λb + w·φ·y`、`weight ← λ·weight + w`（饱和到
    /// `WINDOW_SAMPLES`）。`a0/a1/ri2/rsum/rn/groups` 是**组窗内不清零**的
    /// 累加量，由 [`GainGate::step`] 每 `G` 个样本取走并清零。
    ///
    /// `sse` 用**当前** θ（每组窗末更新一次）算 `e = y - φᵀθ`，再
    /// `sse ← λ·sse + e²`。这是近似：θ 在组窗内不更新，实测 `σ̂_e` 偏差
    /// 约 +2.8 %（良态），不足以影响 0.45 pp 的判据。
    pub fn push_sample(&mut self, s_lsb: [f32; 3], weight: f32) {
        if !weight.is_finite() || weight <= 0.0 {
            return;
        }
        let lambda = limits::LAMBDA;
        let [sa, sb, sc] = s_lsb;

        let [s00, s01, s10, s11] = self.s;
        self.s = [
            lambda * s00 + weight * sb * sb,
            lambda * s01 + weight * sb * sc,
            lambda * s10 + weight * sc * sb,
            lambda * s11 + weight * sc * sc,
        ];

        let y = -sa;
        let [b0, b1] = self.b;
        self.b = [lambda * b0 + weight * sb * y, lambda * b1 + weight * sc * y];

        let next_weight = lambda * self.weight + weight;
        let weight_cap = limits::WINDOW_SAMPLES as f32;
        self.weight = if next_weight >= weight_cap {
            weight_cap
        } else {
            next_weight
        };

        self.a0 += sa + sb + sc;
        self.a1 += sa * sa + sb * sb + sc * sc;
        self.groups = self.groups.saturating_add(1);

        // 方向覆盖：Clarke 幅值不变简化版。
        let i_alpha = sa;
        let i_beta = (sa + 2.0 * sb) / SQRT3;
        let [r0, r1] = self.rsum;
        self.rsum = [r0 + i_alpha, r1 + i_beta];
        self.ri2 += i_alpha * i_alpha + i_beta * i_beta;
        self.rn = self.rn.saturating_add(1);

        let [t0, t1] = self.theta;
        let residual = y - (sb * t0 + sc * t1);
        self.sse = lambda * self.sse + residual * residual;
    }

    /// 解 2×2 正规方程，返回 `(g_b, g_c)`；`det(S)` 不足或非有限时 `None`。
    pub fn solve(&self) -> Option<(f32, f32)> {
        let [s00, s01, s10, s11] = self.s;
        let det = s00 * s11 - s01 * s10;
        if !det.is_finite() || det.abs() < DET_EPS {
            return None;
        }
        let [b0, b1] = self.b;
        let x0 = (s11 * b0 - s01 * b1) / det;
        let x1 = (s00 * b1 - s10 * b0) / det;
        if x0.is_finite() && x1.is_finite() {
            Some((x0, x1))
        } else {
            None
        }
    }

    /// 清空累加器（换工况/换模式/提交后）。
    pub fn reset(&mut self) {
        *self = Self::empty();
    }

    /// 有效样本数 `N_eq = floor((1+λ)Σw)`，饱和到 `N_EQ_MAX = 3999`。
    pub fn n_eq(&self) -> u32 {
        if !self.weight.is_finite() || self.weight <= 0.0 {
            return 0;
        }
        let n = (1.0 + limits::LAMBDA) * self.weight;
        if !n.is_finite() || n <= 0.0 {
            0
        } else {
            (n as u32).min(limits::N_EQ_MAX)
        }
    }

    /// 组窗结束：清掉 z/R 的组内累加量（`groups` 也归零）。
    fn clear_group(&mut self) {
        self.a0 = 0.0;
        self.a1 = 0.0;
        self.ri2 = 0.0;
        self.rsum = [0.0; 2];
        self.rn = 0;
        self.groups = 0;
    }

    /// 方向覆盖平方 `R²`；无效时返回 0（随后会被 `D_MIN` 门拦下）。
    fn direction_r2(&self) -> f32 {
        let [r0, r1] = self.rsum;
        let n = self.rn as f32;
        if n <= 0.0 || self.ri2 <= 0.0 || !self.ri2.is_finite() {
            return 0.0;
        }
        let r2 = (r0 * r0 + r1 * r1) / (n * self.ri2);
        if r2.is_finite() {
            r2
        } else {
            0.0
        }
    }

    /// DC 偏置平方 `z² = 3·A0²/(G·A1)`；无激励时返回 0。
    fn bias_z2(&self) -> f32 {
        let g = self.groups as f32;
        if g <= 0.0 || self.a1 <= 0.0 || !self.a1.is_finite() {
            return 0.0;
        }
        let z2 = 3.0 * self.a0 * self.a0 / (g * self.a1);
        if z2.is_finite() {
            z2
        } else {
            0.0
        }
    }

    /// 信息矩阵规整度 `d = det(S)/tr²(S)`。
    fn info_d(&self) -> f32 {
        let [s00, s01, s10, s11] = self.s;
        let det = s00 * s11 - s01 * s10;
        let tr = s00 + s11;
        if tr <= 0.0 || !tr.is_finite() || !det.is_finite() {
            return 0.0;
        }
        let d = det / (tr * tr);
        if d.is_finite() {
            d.max(0.0)
        } else {
            0.0
        }
    }

    /// 残差标准差 `σ̂_ch`（**单通道** LSB）。
    ///
    /// 残差是三相零和违背，三路独立噪声合成后方差为 `3σ_ch²`；因此
    /// `σ̂_ch = √(sse / (3·(weight-2)))`。`pred` 的第一项再乘显式 `√3`
    /// 还原到残差口径，避免把 √3 计两次。
    fn sigma_ch(&self) -> Option<f32> {
        let dof = self.weight - 2.0;
        let denom = dof * 3.0;
        if dof <= 0.0 || denom <= 0.0 || !denom.is_finite() || !self.sse.is_finite() {
            return None;
        }
        Some(sqrt_f32(self.sse / denom))
    }

    /// 残差方差 `σ_res² = sse/(weight-2) = 3σ_ch²`。
    /// 只给 χ² 用：`θ` 的协方差是 `σ_res²·S⁻¹`，不是单通道方差。
    fn sigma_res2(&self) -> Option<f32> {
        let dof = self.weight - 2.0;
        if dof <= 0.0 || !dof.is_finite() || !self.sse.is_finite() {
            return None;
        }
        let v = self.sse / dof;
        if v > 0.0 && v.is_finite() {
            Some(v)
        } else {
            None
        }
    }

    /// 三相**等权均方根** `s_rms = √(A1/(3G))`（LSB，按相）。
    ///
    /// 这是 z 门的分母口径，也是 `pred` 唯一使用的幅值口径：
    /// 旋转平衡时等于相电流 RMS（幅值/√2），单相激励时为幅值/√3——
    /// **不要**把它读成"电流幅值/√2"。
    fn s_rms_lsb(&self) -> Option<f32> {
        let g = self.groups as f32;
        if g <= 0.0 || !self.a1.is_finite() {
            return None;
        }
        let ms = self.a1 / (3.0 * g);
        if ms <= 0.0 || !ms.is_finite() {
            return None;
        }
        Some(sqrt_f32(ms))
    }
}

/// 半组窗（χ² 用）：普通求和，不带 λ 遗忘。
#[derive(Clone, Copy, Debug, PartialEq)]
struct HalfWindow {
    s: [f32; 4],
    b: [f32; 2],
    weight: f32,
    n: u32,
}

impl HalfWindow {
    const fn empty() -> Self {
        Self {
            s: [0.0; 4],
            b: [0.0; 2],
            weight: 0.0,
            n: 0,
        }
    }

    fn push(&mut self, s_lsb: [f32; 3], weight: f32) {
        let [sa, sb, sc] = s_lsb;
        let [s00, s01, s10, s11] = self.s;
        self.s = [
            s00 + weight * sb * sb,
            s01 + weight * sb * sc,
            s10 + weight * sc * sb,
            s11 + weight * sc * sc,
        ];
        let y = -sa;
        let [b0, b1] = self.b;
        self.b = [b0 + weight * sb * y, b1 + weight * sc * y];
        self.weight += weight;
        self.n = self.n.saturating_add(1);
    }

    fn solve(&self) -> Option<(f32, f32)> {
        let [s00, s01, s10, s11] = self.s;
        let det = s00 * s11 - s01 * s10;
        if !det.is_finite() || det.abs() < DET_EPS {
            return None;
        }
        let [b0, b1] = self.b;
        let x0 = (s11 * b0 - s01 * b1) / det;
        let x1 = (s00 * b1 - s10 * b0) / det;
        if x0.is_finite() && x1.is_finite() {
            Some((x0, x1))
        } else {
            None
        }
    }

    /// `S⁻¹` 的四个元素（行主序）。协方差 = `S⁻¹·σ̂_e²`。
    fn inv(&self) -> Option<[f32; 4]> {
        let [s00, s01, s10, s11] = self.s;
        let det = s00 * s11 - s01 * s10;
        if !det.is_finite() || det.abs() < DET_EPS {
            return None;
        }
        let inv = [s11 / det, -s01 / det, -s10 / det, s00 / det];
        if inv.iter().all(|v| v.is_finite()) {
            Some(inv)
        } else {
            None
        }
    }
}

/// 增益闸门状态机。单写者：只由 `calib` 任务推进。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainGate {
    /// 冻结参数集。
    pub params: GateParams,
    /// 正规方程累加器。
    pub accum: GainAccum,
    /// 最近一次闸门结论（只由本模块写）。
    trust: Trust,
    /// 减零位用的**逐相**零位码（LSB）。只有 `zero_valid == true` 时有效；
    /// 构造后必须由 C0 实测经 [`GainGate::set_zero_lsb`] 写入。
    zero_lsb: [f32; 3],
    /// 零位是否已由 C0 实测写入。未置位时 [`GainGate::step`] 直接返回
    /// `None` 并置 `UntrustedExcitation`（PRE-05：不拿默认零位静默标定）。
    zero_valid: bool,
    /// 组窗前半（χ² 的 θ1）。
    half_a: HalfWindow,
    /// 组窗后半（χ² 的 θ2）。
    half_b: HalfWindow,
    /// 上一完整组窗的方向覆盖 `R²`（R 门用它，组窗中途不重算）。
    group_r2: f32,
    /// 上一完整组窗的 `z²`。
    group_z2: f32,
    /// 上一完整组窗结束时的 `d`。
    group_d: f32,
    /// 是否已有完整组窗。
    have_group: bool,
    /// 上一完整组窗的精度门结论。
    pred_ok: bool,
    /// 最近一次 χ² 统计量。
    chi2: f32,
    /// χ² 是否真的算过（四维全过才置位）。
    chi2_ok: bool,
}

impl GainGate {
    /// 纯构造：参数冻结、累加器清零、结论置为"激励不足"（还没证据）。
    pub const fn new(params: GateParams) -> Self {
        Self {
            params,
            accum: GainAccum::empty(),
            trust: Trust::UntrustedExcitation,
            zero_lsb: [ADC_MIDSCALE_CODE; 3],
            zero_valid: false,
            half_a: HalfWindow::empty(),
            half_b: HalfWindow::empty(),
            group_r2: 0.0,
            group_z2: 0.0,
            group_d: 0.0,
            have_group: false,
            pred_ok: false,
            chi2: 0.0,
            chi2_ok: false,
        }
    }

    /// 纯取值：最近一次闸门结论。
    pub const fn trust(&self) -> Trust {
        self.trust
    }

    /// 设置减零位用的**逐相**零位码（C0 测量结果，LSB），并置零位有效位。
    /// 未调用本函数前 [`Self::step`] 不会累加任何样本。
    pub fn set_zero_lsb(&mut self, zero_lsb: [f32; 3]) {
        self.zero_lsb = zero_lsb;
        self.zero_valid = true;
    }

    /// 推进一拍。`valid` 来自 ADC 窗口判据（三路对齐 + 窗口余量）。
    ///
    /// 前置：`set_zero_lsb` 已写入逐相零位。未写入时本函数直接返回 `None`
    /// 并置 `UntrustedExcitation`——绝不拿 2048 中点冒充实测零位（PRE-05）。
    ///
    /// `valid == false` 时**冻结**：不累加、不遗忘、不改 `trust`，直接返回
    /// `None`（用上一份冻结增益）。有效样本先累加；到组窗末端再按
    /// `R² → d → z² → pred → χ²` 顺序判定并锁存结论。返回 `Some((g_b, g_c))`
    /// 仅当本拍锁存为 `Trusted` 且 `N_eq ≥ WINDOW_SAMPLES`。
    pub fn step(&mut self, raw: RawSample3, valid: bool) -> Option<(f32, f32)> {
        if !self.zero_valid {
            self.trust = Trust::UntrustedExcitation;
            return None;
        }
        if !valid {
            return None;
        }

        let [ca, cb, cc] = raw.code;
        let [za, zb, zc] = self.zero_lsb;
        let s_lsb = [f32::from(ca) - za, f32::from(cb) - zb, f32::from(cc) - zc];

        self.accum.push_sample(s_lsb, 1.0);

        let half = self.params.group_samples / 2;
        if self.accum.groups <= half {
            self.half_a.push(s_lsb, 1.0);
        } else {
            self.half_b.push(s_lsb, 1.0);
        }

        if self.params.group_samples > 0 && self.accum.groups >= self.params.group_samples {
            let r2 = self.accum.direction_r2();
            let z2 = self.accum.bias_z2();
            let d = self.accum.info_d();
            self.group_r2 = r2;
            self.group_z2 = z2;
            self.group_d = d;
            self.have_group = true;

            let pred = self.pred_pp();
            self.pred_ok = match pred {
                Some(value) => value <= self.params.kappa * self.params.target_pp,
                None => false,
            };

            let structural_ok = r2 <= self.params.r_max * self.params.r_max
                && d >= self.params.d_min
                && z2 <= self.params.z_max * self.params.z_max;

            if structural_ok && self.pred_ok {
                match self.consistency_chi2() {
                    Some(stat) => {
                        self.chi2 = stat;
                        self.chi2_ok = true;
                    }
                    None => {
                        self.chi2 = 0.0;
                        self.chi2_ok = false;
                    }
                }
            } else {
                // 四维没过就不算 χ²：分布假设不成立，也不允许报 UntrustedChange。
                self.chi2 = 0.0;
                self.chi2_ok = false;
            }

            // 上一组窗的 θ 用于下一组窗的残差。
            if let Some((t0, t1)) = self.accum.solve() {
                self.accum.theta = [t0, t1];
            }

            self.accum.clear_group();
            self.half_a = HalfWindow::empty();
            self.half_b = HalfWindow::empty();
        }

        self.trust = if !self.have_group
            || self.group_r2 > self.params.r_max * self.params.r_max
            || self.group_d < self.params.d_min
        {
            Trust::UntrustedExcitation
        } else if self.group_z2 > self.params.z_max * self.params.z_max {
            Trust::UntrustedBias
        } else if !self.pred_ok {
            Trust::UntrustedAmplitude
        } else if !self.chi2_ok || self.chi2 > self.params.chi2_max {
            Trust::UntrustedChange
        } else {
            Trust::Trusted
        };

        if self.trust.is_trusted() && self.accum.n_eq() >= limits::WINDOW_SAMPLES {
            self.accum.solve()
        } else {
            None
        }
    }

    /// 当前预测精度界（pp）。`N_eq` 不足或 `s_rms` 为 0 时 `None`。
    ///
    /// `pred = 100·[√3·σ̂_ch/(s_rms·√N_eq) + (σ̂_ch²+δo²)/s_rms²]`。
    ///
    /// - `σ̂_ch = √(sse / (3(weight-2)))`：**单通道**噪声，来自三相零和残差
    ///   （残差方差 = 3σ_ch²，故除以 3 再开方）。第一项的显式 `√3` 把
    ///   单通道 σ 还原成残差口径，只乘一次。
    /// - `s_rms = √(a1/(3G))`：**三相等权均方根**，不是"电流幅值/√2"。
    ///   平衡旋转时它等于相 RMS（幅值/√2）；单相激励时等于幅值/√3。
    /// - `δo²` 是零位残余不确定度，LSB²。
    pub fn pred_pp(&self) -> Option<f32> {
        let n_eq = self.accum.n_eq();
        if n_eq == 0 {
            return None;
        }
        let sigma_ch = self.accum.sigma_ch()?;
        let s_rms = self.accum.s_rms_lsb()?;
        let n = n_eq as f32;
        let term1 = SQRT3 * sigma_ch / (s_rms * sqrt_f32(n));
        let term2 = (sigma_ch * sigma_ch + self.accum.do2) / (s_rms * s_rms);
        let pred = 100.0 * (term1 + term2);
        if pred.is_finite() {
            Some(pred)
        } else {
            None
        }
    }

    /// 清空（换模式/显式重启标定）。零位也必须重新测（置无效），
    /// 防止旧零位跨工况继续使用。
    pub fn reset(&mut self) {
        self.accum.reset();
        self.trust = Trust::UntrustedExcitation;
        self.zero_lsb = [ADC_MIDSCALE_CODE; 3];
        self.zero_valid = false;
        self.half_a = HalfWindow::empty();
        self.half_b = HalfWindow::empty();
        self.group_r2 = 0.0;
        self.group_z2 = 0.0;
        self.group_d = 0.0;
        self.have_group = false;
        self.pred_ok = false;
        self.chi2 = 0.0;
        self.chi2_ok = false;
    }

    /// χ²：前后两半组窗的 θ 差与各自协方差。
    ///
    /// `(θ1-θ2)ᵀ (Σ1+Σ2)⁻¹ (θ1-θ2)`，`Σi = Si⁻¹·σ_res²`，
    /// `σ_res² = sse/(weight-2) = 3σ_ch²`（θ 的协方差用残差方差，不是单通道）。
    fn consistency_chi2(&self) -> Option<f32> {
        let sigma2 = self.accum.sigma_res2()?;
        if sigma2 <= 0.0 || !sigma2.is_finite() {
            return None;
        }
        let (t1a, t1b) = self.half_a.solve()?;
        let (t2a, t2b) = self.half_b.solve()?;
        let inv_a = self.half_a.inv()?;
        let inv_b = self.half_b.inv()?;

        let [a00, a01, a10, a11] = inv_a;
        let [b00, b01, b10, b11] = inv_b;
        let m00 = a00 + b00;
        let m01 = a01 + b01;
        let m10 = a10 + b10;
        let m11 = a11 + b11;
        let det = m00 * m11 - m01 * m10;
        let scale = (m00 * m11).abs() + (m01 * m10).abs();
        if !det.is_finite() || scale <= 0.0 || !scale.is_finite() || det.abs() < 1e-12 * scale {
            return None;
        }

        let d0 = t1a - t2a;
        let d1 = t1b - t2b;
        let q0 = (m11 * d0 - m01 * d1) / det;
        let q1 = (m00 * d1 - m10 * d0) / det;
        let stat = (d0 * q0 + d1 * q1) / sigma2;
        if stat.is_finite() {
            Some(stat)
        } else {
            None
        }
    }
}

/// 确定性平方根：牛顿迭代 4 次，初值用指数折半位模式。
///
/// 无 libm、无表、无分支依赖；`f32` 的位模式在 host 与 target 上一致，
/// 因此 host 测试与 G474 上得到逐位相同结果。`x ≤ 0`（含 NaN）返回 0。
fn sqrt_f32(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    if x == f32::INFINITY {
        return x;
    }
    let seed = f32::from_bits((x.to_bits() >> 1) + 0x1fbd_1df5);
    let mut z = seed;
    let mut i = 0;
    while i < 4 {
        z = 0.5 * (z + x / z);
        i += 1;
    }
    z
}

// ---------------------------------------------------------------------------
// 编译期断言
// ---------------------------------------------------------------------------

// `size_of::<Trust>() == 1` 的断言随 `Trust` 一起搬到了 `contracts::trust`。
const _: () = assert!(limits::N_EQ_MAX == 3_999);
const _: () = assert!(limits::GROUP_SAMPLES == 200);

// ---------------------------------------------------------------------------
// host 测试支撑：固定种子 LCG + 三相旋转发生器（不引入 rand）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_support {
    use super::{GainGate, GateParams, RawSample3, ADC_MIDSCALE_CODE};

    /// 测试用闸门：显式写入标称逐相零位（模拟 C0 已完成）。
    pub fn gate() -> GainGate {
        let mut gate = GainGate::new(GateParams::FROZEN);
        gate.set_zero_lsb([ADC_MIDSCALE_CODE; 3]);
        gate
    }

    /// 每 64 拍转一圈（2π/64）。
    pub const FULL_STEP_COS: f32 = 0.995_184_7;
    pub const FULL_STEP_SIN: f32 = 0.098_017_14;
    /// 每拍 0.5°（±10° 弧用）。
    pub const ARC_STEP_COS: f32 = 0.999_961_9;
    pub const ARC_STEP_SIN: f32 = 0.008_726_536;

    /// 线性同余发生器（Numerical Recipes 常量），确定性、无依赖。
    pub struct Lcg {
        state: u32,
    }

    impl Lcg {
        pub fn new(seed: u32) -> Self {
            Self { state: seed | 1 }
        }

        fn next_u32(&mut self) -> u32 {
            self.state = self
                .state
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            self.state
        }

        fn uniform(&mut self) -> f32 {
            let top = self.next_u32() >> 8;
            (top as f32) / 16_777_216.0
        }

        /// 近似零均值、标准差 `sigma` 的噪声（4 个均匀分布之和）。
        pub fn noise(&mut self, sigma: f32) -> f32 {
            let mut acc = 0.0;
            let mut i = 0;
            while i < 4 {
                acc += self.uniform();
                i += 1;
            }
            (acc - 2.0) * sigma * 1.732_050_8
        }
    }

    /// 单位复数旋转器：不需要 libm 的 sin/cos。
    #[derive(Clone, Copy)]
    pub struct Rot {
        x: f32,
        y: f32,
    }

    impl Rot {
        pub fn new() -> Self {
            Self { x: 1.0, y: 0.0 }
        }

        pub fn step(&mut self, cos_step: f32, sin_step: f32) {
            let x = self.x * cos_step - self.y * sin_step;
            let y = self.x * sin_step + self.y * cos_step;
            self.x = x;
            self.y = y;
        }

        pub fn step_back(&mut self, cos_step: f32, sin_step: f32) {
            let x = self.x * cos_step + self.y * sin_step;
            let y = -self.x * sin_step + self.y * cos_step;
            self.x = x;
            self.y = y;
        }

        /// 幅值不变 Clarke 逆变换：相峰值 `amp`，三相和恒为 0。
        pub fn iabc(&self, amp: f32) -> [f32; 3] {
            let alpha = amp * self.x;
            let beta = amp * self.y;
            [
                alpha,
                -0.5 * alpha + 0.866_025_4 * beta,
                -0.5 * alpha - 0.866_025_4 * beta,
            ]
        }
    }

    /// 注入相对增益（`s_b /= gamma_b`）、共模 DC 与噪声。
    pub fn measured(abc: [f32; 3], gamma_b: f32, dc: f32, sigma: f32, rng: &mut Lcg) -> [f32; 3] {
        let [a, b, c] = abc;
        [
            a + dc + rng.noise(sigma),
            b / gamma_b + dc + rng.noise(sigma),
            c + dc + rng.noise(sigma),
        ]
    }

    /// LSB 码（含标称中点）→ 原始样本。
    pub fn raw(s_lsb: [f32; 3]) -> RawSample3 {
        let [a, b, c] = s_lsb;
        RawSample3 {
            t_us: 0,
            code: [to_code(a), to_code(b), to_code(c)],
        }
    }

    pub fn push(gate: &mut GainGate, s_lsb: [f32; 3]) -> Option<(f32, f32)> {
        gate.step(raw(s_lsb), true)
    }

    fn to_code(x: f32) -> u16 {
        let v = x + ADC_MIDSCALE_CODE;
        if v <= 0.0 || v.is_nan() {
            0
        } else if v >= 65_535.0 {
            65_535
        } else {
            (v + 0.5) as u16
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{
        gate, measured, push, raw, Lcg, Rot, ARC_STEP_COS, ARC_STEP_SIN, FULL_STEP_COS,
        FULL_STEP_SIN,
    };
    use super::{sqrt_f32, GainAccum, GainGate, GateParams};
    use contracts::{limits, Trust};

    /// 1 A 相峰值对应的 LSB 码（2.97 mA/LSB，HR-203）：只用于构造测试数据。
    const LSB_PER_AMP: f32 = 1.0 / 0.002_97;

    fn frozen_gate() -> GainGate {
        gate()
    }

    #[test]
    fn sqrt_matches_known_values() {
        assert!((sqrt_f32(0.0) - 0.0).abs() < 1e-12);
        assert!((sqrt_f32(4.0) - 2.0).abs() < 1e-6);
        assert!((sqrt_f32(1e-6) - 1e-3).abs() < 1e-8);
    }

    #[test]
    fn rotating_1a_recovers_injected_gain() {
        let mut gate = frozen_gate();
        let mut rng = Lcg::new(0x1234_5678);
        let mut rot = Rot::new();
        let mut last = None;
        let mut saw_trusted = false;

        for _ in 0..3_000 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(1.0 * LSB_PER_AMP);
            let s = measured(abc, 1.02, 0.0, 1.5, &mut rng);
            if let Some(g) = push(&mut gate, s) {
                last = Some(g);
            }
            if gate.trust().is_trusted() {
                saw_trusted = true;
            }
        }

        let (gb, gc) = match last {
            Some(v) => v,
            None => panic!("gate never produced a gain estimate"),
        };
        assert!(saw_trusted, "never saw Trusted");
        assert!((gb - 1.02).abs() < 1e-3, "gamma_b = {gb}");
        assert!((gc - 1.0).abs() < 1e-3, "gamma_c = {gc}");
        assert_eq!(gate.trust(), Trust::Trusted);
    }

    #[test]
    fn single_direction_is_rejected_by_bias_gate() {
        // 全窗同一 AC 方向 + 共模 DC：R/d 过，z 必须拒（DC 会被学成增益）。
        let mut gate = frozen_gate();
        let mut rng = Lcg::new(11);
        let mut rot = Rot::new();
        for _ in 0..400 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(94.0);
            let s = measured(abc, 1.01, 5.0, 0.5, &mut rng);
            let _ = push(&mut gate, s);
        }
        assert_eq!(gate.trust(), Trust::UntrustedBias);
    }

    #[test]
    fn narrow_arc_with_dc_is_rejected() {
        let mut gate = frozen_gate();
        let mut rng = Lcg::new(21);
        let mut rot = Rot::new();
        let mut forward = true;
        for k in 0..400 {
            if k % 20 == 0 {
                forward = !forward;
            }
            if forward {
                rot.step(ARC_STEP_COS, ARC_STEP_SIN);
            } else {
                rot.step_back(ARC_STEP_COS, ARC_STEP_SIN);
            }
            let abc = rot.iabc(94.0);
            let s = measured(abc, 1.0, 30.0, 0.5, &mut rng);
            let _ = push(&mut gate, s);
        }
        assert!(!gate.trust().is_trusted());
    }

    #[test]
    fn low_amplitude_is_rejected_by_precision_gate() {
        // 0.1 A 峰值（≈33.7 LSB）+ 3.5 LSB 噪声：pred > 0.45 pp。
        let mut gate = frozen_gate();
        let mut rng = Lcg::new(31);
        let mut rot = Rot::new();
        for _ in 0..400 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(0.1 * LSB_PER_AMP);
            let s = measured(abc, 1.0, 0.0, 3.5, &mut rng);
            let _ = push(&mut gate, s);
        }
        assert_eq!(gate.trust(), Trust::UntrustedAmplitude);
    }

    #[test]
    fn pure_noise_is_rejected_by_excitation_gate() {
        // 秩 1 噪声：方向单一，S 秩亏，d 必须拒。
        let mut gate = frozen_gate();
        let mut rng = Lcg::new(41);
        for _ in 0..400 {
            let n = rng.noise(4.0);
            let _ = push(&mut gate, [n, -0.5 * n, -0.5 * n]);
        }
        assert_eq!(gate.trust(), Trust::UntrustedExcitation);
    }

    #[test]
    fn two_percent_step_trips_change_gate_within_10ms() {
        let mut gate = frozen_gate();
        let mut rng = Lcg::new(51);
        let mut rot = Rot::new();

        for _ in 0..1_200 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(1.0 * LSB_PER_AMP);
            let s = measured(abc, 1.0, 0.0, 0.5, &mut rng);
            let _ = push(&mut gate, s);
        }
        assert_eq!(gate.trust(), Trust::Trusted);

        let mut saw_change = false;
        for k in 0..200 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let gamma_b = if k < 50 { 1.0 } else { 1.02 };
            let abc = rot.iabc(1.0 * LSB_PER_AMP);
            let s = measured(abc, gamma_b, 0.0, 0.5, &mut rng);
            let _ = push(&mut gate, s);
            if gate.trust() == Trust::UntrustedChange {
                saw_change = true;
                break;
            }
        }
        assert!(saw_change, "2 % step not caught within 200 samples (10 ms)");
    }

    #[test]
    fn kappa_derating_rejects_marginal_case() {
        // 0.14 A 峰值：pred ≈ 0.475 pp，落在 (KAPPA·0.5, 0.5)。
        let mut gate = frozen_gate();
        let mut rng = Lcg::new(61);
        let mut rot = Rot::new();
        let amp = 0.14 * LSB_PER_AMP;
        for _ in 0..1_999 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(amp);
            let s = measured(abc, 1.0, 0.0, 1.75, &mut rng);
            let _ = push(&mut gate, s);
        }

        let pred = match gate.pred_pp() {
            Some(v) => v,
            None => panic!("pred_pp is None"),
        };
        assert!(
            pred > GateParams::FROZEN.kappa * GateParams::FROZEN.target_pp && pred < 0.5,
            "pred = {pred} pp not in the marginal band"
        );

        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let abc = rot.iabc(amp);
        let s = measured(abc, 1.0, 0.0, 1.75, &mut rng);
        let _ = push(&mut gate, s);
        assert_eq!(gate.trust(), Trust::UntrustedAmplitude);

        // 同一份数据，把目标放宽到 2.0 pp：降额后 1.8 pp 应通过。
        gate.params.target_pp = 2.0;
        let mut saw_trusted = false;
        for _ in 0..200 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(amp);
            let s = measured(abc, 1.0, 0.0, 1.75, &mut rng);
            let _ = push(&mut gate, s);
            if gate.trust().is_trusted() {
                saw_trusted = true;
            }
        }
        assert!(
            saw_trusted,
            "relaxed target did not let the marginal case pass"
        );
    }

    #[test]
    fn invalid_sample_does_not_decay() {
        let mut gate = frozen_gate();
        let mut rng = Lcg::new(71);
        let mut rot = Rot::new();
        for _ in 0..300 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(200.0);
            let s = measured(abc, 1.0, 0.0, 1.0, &mut rng);
            let _ = push(&mut gate, s);
        }
        let weight = gate.accum.weight;
        let sse = gate.accum.sse;
        let groups = gate.accum.groups;
        let n_eq = gate.accum.n_eq();

        for _ in 0..32 {
            assert!(gate.step(raw([10.0, -5.0, -5.0]), false).is_none());
        }

        assert_eq!(gate.accum.weight, weight);
        assert_eq!(gate.accum.sse, sse);
        assert_eq!(gate.accum.groups, groups);
        assert_eq!(gate.accum.n_eq(), n_eq);
    }

    #[test]
    fn s_rms_is_equal_weight_three_phase() {
        // 平衡旋转：三相等权均方根 = 相峰值/√2。
        let amp = 100.0;
        let mut acc = GainAccum::empty();
        let mut rot = Rot::new();
        for _ in 0..64 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let [a, b, c] = rot.iabc(amp);
            acc.push_sample([a, b, c], 1.0);
        }
        let s_rot = match acc.s_rms_lsb() {
            Some(v) => v,
            None => panic!("s_rms is None"),
        };
        let sqrt2 = core::f32::consts::SQRT_2;
        assert!((s_rot - amp / sqrt2).abs() < 1e-2, "s_rot = {s_rot}");

        // 单相激励：= 幅值/√3，明确不是幅值/√2。
        let mut acc_one = GainAccum::empty();
        for _ in 0..64 {
            acc_one.push_sample([amp, 0.0, 0.0], 1.0);
        }
        let s_one = match acc_one.s_rms_lsb() {
            Some(v) => v,
            None => panic!("s_rms is None"),
        };
        assert!((s_one - amp / super::SQRT3).abs() < 1e-3, "s_one = {s_one}");
        assert!((s_one - amp / sqrt2).abs() > 5.0, "s_one read as peak/√2");
    }

    #[test]
    fn step_returns_none_until_zero_is_set() {
        let mut gate = GainGate::new(GateParams::FROZEN);
        let s = [100.0, -50.0, -50.0];
        assert!(gate.step(raw(s), true).is_none());
        assert_eq!(gate.trust(), Trust::UntrustedExcitation);
        assert_eq!(gate.accum.weight, 0.0);

        gate.set_zero_lsb([2_048.0, 2_044.0, 2_051.0]);
        let _ = gate.step(raw(s), true);
        assert!(gate.accum.weight > 0.0);
    }

    #[test]
    fn n_eq_saturates_at_3999() {
        // Σw = 2000（单拍权重 2000 等价于 2000 个等权样本之和）。
        let mut accum = GainAccum::empty();
        accum.push_sample([100.0, -50.0, -50.0], 2000.0);
        assert_eq!(accum.weight, 2000.0);
        assert_eq!(accum.n_eq(), 3_999);
        accum.push_sample([100.0, -50.0, -50.0], 1.0);
        assert_eq!(accum.weight, 2000.0);
        assert_eq!(accum.n_eq(), 3_999);
    }

    // -----------------------------------------------------------------------
    // 以下 10 条原在 `contracts/src/lib.rs` 的契约测试里；估计器搬到本层后
    // 跟着一起搬（它们断言的是算法行为，不是线协议事实）。
    // -----------------------------------------------------------------------

    #[test]
    fn gate_rejects_dc_biased_window() {
        // 结论：z = |A0|·√3/√(G·A1) > Z_MAX = 0.02 时必须 UntrustedBias。
        let mut gate = gate();
        let mut rng = Lcg::new(11);
        let mut rot = Rot::new();
        for _ in 0..400 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(94.0);
            let s = measured(abc, 1.01, 5.0, 0.5, &mut rng);
            let _ = push(&mut gate, s);
        }
        assert_eq!(gate.trust(), Trust::UntrustedBias);
    }

    #[test]
    fn gate_rejects_collinear_current_excitation() {
        // 结论：d = det(S)/tr²(S) < D_MIN = 0.01（减零位后回归量）时必须 UntrustedExcitation。
        let mut gate = gate();
        let mut rng = Lcg::new(41);
        for _ in 0..400 {
            let n = rng.noise(4.0);
            let _ = push(&mut gate, [n, -0.5 * n, -0.5 * n]);
        }
        assert_eq!(gate.trust(), Trust::UntrustedExcitation);
    }

    #[test]
    fn gate_rejects_single_direction_rotation() {
        // 结论：R 门抓"整段同一方向"：R² = 1 > R_MAX² 时必须 UntrustedExcitation。
        let mut gate = gate();
        let mut rng = Lcg::new(43);
        for _ in 0..400 {
            let s = measured([94.0, -47.0, -47.0], 1.0, 0.0, 0.5, &mut rng);
            let _ = push(&mut gate, s);
        }
        assert_eq!(gate.trust(), Trust::UntrustedExcitation);
    }

    #[test]
    fn gate_accepts_balanced_synthetic_rotation() {
        // 结论：三相平衡旋转且无 DC 偏置时四门 + χ² 全过，返回 Trusted 与 (g_b, g_c)。
        let mut gate = gate();
        let mut rng = Lcg::new(0x1234_5678);
        let mut rot = Rot::new();
        let mut last = None;
        for _ in 0..3_000 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(1.0 * LSB_PER_AMP);
            let s = measured(abc, 1.0, 0.0, 1.5, &mut rng);
            if let Some(g) = push(&mut gate, s) {
                last = Some(g);
            }
        }
        let (gb, gc) = match last {
            Some(v) => v,
            None => panic!("no gain estimate"),
        };
        assert!((gb - 1.0).abs() < 0.01, "gamma_b = {gb}");
        assert!((gc - 1.0).abs() < 0.01, "gamma_c = {gc}");
        assert_eq!(gate.trust(), Trust::Trusted);
    }

    #[test]
    fn gate_derates_prediction_by_kappa() {
        // 结论：判定用 pred ≤ KAPPA·target，KAPPA = 0.9（p99/pred ≈ 2.3 的降额）。
        let mut gate = gate();
        let mut rng = Lcg::new(61);
        let mut rot = Rot::new();
        let amp = 0.14 * LSB_PER_AMP;
        for _ in 0..1_999 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(amp);
            let s = measured(abc, 1.0, 0.0, 1.75, &mut rng);
            let _ = push(&mut gate, s);
        }
        let pred = match gate.pred_pp() {
            Some(v) => v,
            None => panic!("pred_pp is None"),
        };
        assert!(pred > 0.45 && pred < 0.5, "pred = {pred} pp not marginal");

        rot.step(FULL_STEP_COS, FULL_STEP_SIN);
        let abc = rot.iabc(amp);
        let s = measured(abc, 1.0, 0.0, 1.75, &mut rng);
        let _ = push(&mut gate, s);
        assert_eq!(gate.trust(), Trust::UntrustedAmplitude);

        // 放宽目标后同一数据必须能过，证明降额真的在起作用。
        gate.params.target_pp = 2.0;
        let mut saw_trusted = false;
        for _ in 0..200 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let abc = rot.iabc(amp);
            let s = measured(abc, 1.0, 0.0, 1.75, &mut rng);
            let _ = push(&mut gate, s);
            if gate.trust().is_trusted() {
                saw_trusted = true;
            }
        }
        assert!(saw_trusted, "relaxed target did not pass");
    }

    #[test]
    fn pred_pp_meets_0p5_pp_at_0p14_a() {
        // 结论：0.14 A RMS 旋转工况 pred ≤ 0.5 pp（幅值下限参考点 1）。
        // σ_ch = 1.5 LSB（单通道；残差 σ≈2.6），N_eq 在 2000 拍后约 2527。
        let mut gate = gate();
        let mut rng = Lcg::new(101);
        let mut rot = Rot::new();
        let peak = core::f32::consts::SQRT_2 * 0.14 * LSB_PER_AMP;
        let sigma_ch = 1.5;
        for _ in 0..2_000 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let [a, b, c] = rot.iabc(peak);
            gate.accum.push_sample(
                [
                    a + rng.noise(sigma_ch),
                    b + rng.noise(sigma_ch),
                    c + rng.noise(sigma_ch),
                ],
                1.0,
            );
        }
        let pred = match gate.pred_pp() {
            Some(v) => v,
            None => panic!("pred_pp is None"),
        };
        assert!(pred <= 0.5, "pred = {pred} pp > 0.5 pp");
    }

    #[test]
    fn pred_pp_meets_0p05_pp_at_0p55_a() {
        // 结论：0.55 A RMS 旋转工况 pred ≤ 0.05 pp（幅值下限参考点 2）。
        let mut gate = gate();
        let mut rng = Lcg::new(103);
        let mut rot = Rot::new();
        let peak = core::f32::consts::SQRT_2 * 0.55 * LSB_PER_AMP;
        let sigma_ch = 1.5;
        for _ in 0..2_000 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let [a, b, c] = rot.iabc(peak);
            gate.accum.push_sample(
                [
                    a + rng.noise(sigma_ch),
                    b + rng.noise(sigma_ch),
                    c + rng.noise(sigma_ch),
                ],
                1.0,
            );
        }
        let pred = match gate.pred_pp() {
            Some(v) => v,
            None => panic!("pred_pp is None"),
        };
        assert!(pred <= 0.05, "pred = {pred} pp > 0.05 pp");
    }

    #[test]
    fn chi2_gate_only_runs_after_structural_gates() {
        // 结论：CHI2_MAX = 9.21 只在 r/d/z 全过的样本组上计算，否则不得报 UntrustedChange。
        let mut gate = gate();
        let mut rng = Lcg::new(105);
        let mut rot = Rot::new();
        for _ in 0..600 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let s = measured(rot.iabc(337.0), 1.0, 0.0, 0.5, &mut rng);
            let _ = push(&mut gate, s);
        }
        // 同时注入共模 DC（z 门失败）与 2 % 增益阶跃（本会触发 χ²）。
        for _ in 0..400 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let s = measured(rot.iabc(337.0), 1.02, 20.0, 0.5, &mut rng);
            let _ = push(&mut gate, s);
        }
        assert_eq!(gate.trust(), Trust::UntrustedBias);
    }

    #[test]
    fn n_eq_saturates_at_3999_for_2000_weighted_samples() {
        // 结论：λ = 0.9995，Σw = 2000 ⇒ N_eq = (1+λ)·Σw = 3999，且饱和。
        let mut accum = GainAccum::empty();
        accum.push_sample([100.0, -50.0, -50.0], 2000.0);
        assert_eq!(accum.weight, 2000.0);
        assert_eq!(accum.n_eq(), 3_999);
        accum.push_sample([100.0, -50.0, -50.0], 1.0);
        assert_eq!(accum.weight, 2000.0);
        assert_eq!(accum.n_eq(), 3_999);
    }

    #[test]
    fn invalid_samples_above_limit_force_previous_gain() {
        // 结论：连续无效样本 > INVALID_SAMPLE_LIMIT 时必须回退到上一份冻结增益。
        let mut gate = gate();
        let mut rng = Lcg::new(0x1234_5678);
        let mut rot = Rot::new();
        for _ in 0..3_000 {
            rot.step(FULL_STEP_COS, FULL_STEP_SIN);
            let s = measured(rot.iabc(1.0 * LSB_PER_AMP), 1.0, 0.0, 1.5, &mut rng);
            let _ = push(&mut gate, s);
        }
        let weight = gate.accum.weight;
        let n_invalid = u32::from(limits::INVALID_SAMPLE_LIMIT) + 1;
        for _ in 0..n_invalid {
            assert!(gate.step(raw([0.0, 0.0, 0.0]), false).is_none());
        }
        assert_eq!(gate.accum.weight, weight);
    }
}
