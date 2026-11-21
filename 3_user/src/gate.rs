//! 相对增益可信闸门：组窗隔离、四维结构门与两级 χ²。
//!
//! 三相共模增益不可观测，必须确认电流向量在转动且无 DC 主导；闸门是参数可辨识性的
//! 前提，不是可选的健康检查。每 `G = 200` 拍构成独立组窗，组内不写主累加器；组末按
//! 结构门 → 组内前后半窗 χ² → 跨组 χ² 判定，全过才并入并刷新生效增益。`pred` 只决定
//! 能否发布，不阻止合并（低幅值下 200 拍的 `pred` 必然超限）。
//!
//! 阈值与公式的唯一来源是 [`crate::policy`] 与 `accum`；事实表见 `docs/INDEX.md`
//!「门控定版」。

mod accum;
mod math;
#[cfg(test)]
mod tests;

pub use self::accum::GainAccum;

use self::accum::{chi2_between, cross_group_chi2, GroupWindow};
use self::math::{do2_from_half_width, powi_f32, sqrt_f32, ADC_MIDSCALE_CODE};
use crate::policy;
use contracts::types::RawSample3;
use contracts::Trust;

/// 闸门参数集合，全部取自 [`policy`]，禁止在别处另存副本。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateParams {
    /// 方向覆盖门限，见 [`policy::R_MAX`]。
    pub r_max: f32,
    /// 信息矩阵门限，见 [`policy::D_MIN`]。
    pub d_min: f32,
    /// DC 偏置门限，见 [`policy::Z_MAX`]。
    pub z_max: f32,
    /// 精度门降额系数，见 [`policy::KAPPA`]。
    pub kappa: f32,
    /// 精度目标，单位 pp，见 [`policy::TARGET_PP`]。
    pub target_pp: f32,
    /// χ²(2) 的 99 % 分位点，见 [`policy::CHI2_MAX`]。
    pub chi2_max: f32,
    /// 遗忘因子，见 [`policy::LAMBDA`]。
    pub lambda: f32,
    /// 组窗样本数 `G`，见 [`policy::GROUP_SAMPLES`]。
    pub group_samples: u32,
}

impl GateParams {
    /// 冻结参数集：数值只在 [`policy`] 定义一次（PRE-07）。
    pub const FROZEN: Self = Self {
        r_max: policy::R_MAX,
        d_min: policy::D_MIN,
        z_max: policy::Z_MAX,
        kappa: policy::KAPPA,
        target_pp: policy::TARGET_PP,
        chi2_max: policy::CHI2_MAX,
        lambda: policy::LAMBDA,
        group_samples: policy::GROUP_SAMPLES,
    };
}

/// 增益闸门状态机。单写者：只由 `calib` 任务推进。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainGate {
    /// 冻结参数集。
    pub params: GateParams,
    /// 通过 Commit Gate 的历史正规方程（λ 遗忘）。
    pub accum: GainAccum,
    // 当前生效的相对增益估计；`have_active` 为假时是名义值 `[1, 1]`，只作组内残差基准。
    active_theta: [f32; 2],
    // 是否已有至少一个通过 Commit Gate 的组窗。
    have_active: bool,
    // 当前组窗（无遗忘、与 `accum` 隔离）。
    group: GroupWindow,
    // 本组是否已出现单拍/累积残差异常。只标记；组末先跑结构门再决定诊断。
    change_suspect: bool,
    // 最近一次闸门结论，只由本模块写。
    trust: Trust,
    // 减零位用的逐相零位码（LSB），只在 `zero_valid` 为真时有效。
    zero_lsb: [f32; 3],
    // 零位标定的误差半宽 `h`，单位 LSB；`do2 = (2h)²/12`，跨窗保留。
    do_lsb_half_width: f32,
    // 零位是否已由 C0 实测写入；未置位时不拿默认零位静默标定（PRE-05）。
    zero_valid: bool,
    // 上一完整组窗的 `R²`、`z²` 与 `d`。
    group_r2: f32,
    group_z2: f32,
    group_d: f32,
    // 是否已有完整组窗。
    have_group: bool,
    // 上一完整组窗的精度门结论。
    pred_ok: bool,
    // 最近一次一致性 χ² 统计量（两层取最大），以及是否真的算过。
    chi2: f32,
    chi2_ok: bool,
}

impl GainGate {
    /// 纯构造：参数冻结、累加器清零、结论置为"激励不足"（还没证据）。
    ///
    /// 零位不确定度取 [`policy::DO_LSB_HALF_WIDTH`]，不是 0：零位标定永远有残余误差，
    /// `pred` 第二项必须把它算进去。
    pub const fn new(params: GateParams) -> Self {
        Self {
            params,
            accum: GainAccum {
                do2: do2_from_half_width(policy::DO_LSB_HALF_WIDTH),
                ..GainAccum::empty()
            },
            active_theta: [1.0, 1.0],
            have_active: false,
            group: GroupWindow::empty(),
            change_suspect: false,
            trust: Trust::UntrustedExcitation,
            zero_lsb: [ADC_MIDSCALE_CODE; 3],
            do_lsb_half_width: policy::DO_LSB_HALF_WIDTH,
            zero_valid: false,
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

    /// 设置减零位用的逐相零位码（C0 测量结果，LSB），并置零位有效位。
    pub fn set_zero_lsb(&mut self, zero_lsb: [f32; 3]) {
        self.zero_lsb = zero_lsb;
        self.zero_valid = true;
    }

    /// 设置零位标定的误差半宽 `h`（单位 LSB），刷新 `δo² = (2h)²/12`。
    ///
    /// 非有限或负的输入按 0 处理，不允许 `NaN` 把 `pred` 变成 `None`。
    pub fn set_zero_uncertainty_lsb(&mut self, half_width_lsb: f32) {
        let h = if half_width_lsb.is_finite() && half_width_lsb > 0.0 {
            half_width_lsb
        } else {
            0.0
        };
        self.do_lsb_half_width = h;
        self.accum.do2 = do2_from_half_width(h);
    }

    /// 当前零位误差半宽，单位 LSB。
    pub const fn zero_uncertainty_lsb(&self) -> f32 {
        self.do_lsb_half_width
    }

    /// 推进一拍。`valid` 来自 ADC 窗口判据。
    ///
    /// 未写入零位时不累加，返回 `None` 并置 `UntrustedExcitation`（PRE-05）。
    /// `valid == false` 时冻结：不累加、不遗忘、不改 `trust`，返回 `None`。有效样本只进
    /// 当前组窗；组未满返回上一份已冻结增益，组满后按 Commit Gate 判定。
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

        self.group
            .push(s_lsb, self.active_theta, self.params.group_samples);

        // 组内突变检测只在"已有生效增益"后才有意义；σ_res² 取主累加器的残差方差。
        if self.have_active {
            if let Some(sigma2) = self.accum.sigma_res2_lsb2() {
                let k = policy::RESIDUAL_K_SIGMA;
                if self.group.last_change_sq > k * k * sigma2 {
                    self.change_suspect = true;
                }
                let n = self.group.n;
                if n >= policy::GROUP_SSE_MIN_SAMPLES {
                    let nf = n as f32;
                    // 减均值后的残差平方和：常量 DC 由 z 门负责，这里只看起伏。
                    let centered =
                        self.group.change_sse - self.group.change_sum * self.group.change_sum / nf;
                    let allowed = (nf + k * sqrt_f32(2.0 * nf)) * sigma2;
                    if centered > allowed {
                        self.change_suspect = true;
                    }
                }
            }
        }

        if self.params.group_samples == 0 || self.group.n < self.params.group_samples {
            return self.frozen_gain();
        }

        self.commit_group()
    }

    // 已冻结并可发布的增益：只有最近一个组窗通过全部 Commit Gate 才发布。
    fn frozen_gain(&self) -> Option<(f32, f32)> {
        if self.trust.is_trusted() && self.have_active {
            let [g_b, g_c] = self.active_theta;
            Some((g_b, g_c))
        } else {
            None
        }
    }

    // 丢弃当前组窗，历史累加器与 `active_theta` 不动。
    fn discard_group(&mut self) {
        self.group = GroupWindow::empty();
        self.change_suspect = false;
    }

    // 组窗末端：结构门 → 组内突变嫌疑 → 前后半窗 χ² → 跨组 χ² → 并入主累加器。
    // `pred` 只决定能否发布。
    fn commit_group(&mut self) -> Option<(f32, f32)> {
        let group = self.group;
        let params = self.params;

        let r2 = group.accum.direction_r2();
        let z2 = group.accum.bias_z2();
        let d = group.accum.info_d();
        self.group_r2 = r2;
        self.group_z2 = z2;
        self.group_d = d;
        self.have_group = true;

        let structural_ok = r2 <= params.r_max * params.r_max
            && d >= params.d_min
            && z2 <= params.z_max * params.z_max;
        if !structural_ok {
            // 结构门先定诊断：DC 偏置报 Bias，秩亏/方向单一报 Excitation。
            self.trust = if r2 > params.r_max * params.r_max || d < params.d_min {
                Trust::UntrustedExcitation
            } else {
                Trust::UntrustedBias
            };
            self.chi2 = 0.0;
            self.chi2_ok = false;
            self.discard_group();
            return None;
        }

        let Some((t0, t1)) = group.accum.solve() else {
            self.trust = Trust::UntrustedExcitation;
            self.chi2 = 0.0;
            self.chi2_ok = false;
            self.discard_group();
            return None;
        };

        // 候选主累加器：只做临时副本，未通过前不写回 `self.accum`。
        let decay = powi_f32(params.lambda, params.group_samples);
        let mut candidate = self.accum;
        candidate.absorb_group(&group.accum, decay);
        candidate.theta = [t0, t1];

        let pred = candidate.pred_pp();
        self.pred_ok = match pred {
            Some(value) => value <= params.kappa * params.target_pp,
            None => false,
        };

        if self.change_suspect {
            self.trust = Trust::UntrustedChange;
            self.chi2 = 0.0;
            self.chi2_ok = false;
            self.discard_group();
            return None;
        }

        let sigma2 = match self.accum.sigma_res2_lsb2() {
            Some(value) => value,
            None => match group.accum.sigma_res2_lsb2() {
                Some(value) => value,
                None => {
                    self.trust = Trust::UntrustedExcitation;
                    self.chi2 = 0.0;
                    self.chi2_ok = false;
                    self.discard_group();
                    return None;
                }
            },
        };

        // 组内前后半窗一致性：抓组中突变。
        let Some(mut chi2) = chi2_between(&group.half_a, &group.half_b, sigma2) else {
            self.trust = Trust::UntrustedChange;
            self.chi2 = 0.0;
            self.chi2_ok = false;
            self.discard_group();
            return None;
        };

        // 跨组一致性：抓组首第一拍突变。
        if self.have_active {
            let Some(cross) =
                cross_group_chi2(&self.accum, &group.accum, self.active_theta, sigma2)
            else {
                self.trust = Trust::UntrustedChange;
                self.chi2 = 0.0;
                self.chi2_ok = false;
                self.discard_group();
                return None;
            };
            if cross > chi2 {
                chi2 = cross;
            }
        }

        self.chi2 = chi2;
        if chi2 > params.chi2_max {
            self.trust = Trust::UntrustedChange;
            self.chi2_ok = false;
            self.discard_group();
            return None;
        }
        self.chi2_ok = true;

        // 发布值取合并后窗口的解（统计效率高于单个 200 拍组），组解只作跨组检验输入。
        let (active_b, active_c) = match candidate.solve() {
            Some(value) => value,
            None => {
                self.trust = Trust::UntrustedExcitation;
                self.chi2 = 0.0;
                self.chi2_ok = false;
                self.discard_group();
                return None;
            }
        };
        candidate.theta = [active_b, active_c];
        self.accum = candidate;
        self.active_theta = [active_b, active_c];
        self.have_active = true;
        self.discard_group();

        if self.pred_ok {
            self.trust = Trust::Trusted;
            Some((active_b, active_c))
        } else {
            // 结构/一致性通过但精度不足：数据可信，继续积累但拒绝发布。
            self.trust = Trust::UntrustedAmplitude;
            None
        }
    }

    /// 当前预测精度界，单位 pp；样本不足或 `s_rms` 为 0 时 `None`。
    ///
    /// `pred = 100·[√3·σ̂_ch/(s_rms·√N_eq) + (σ̂_ch² + δo²)/s_rms²]`，其中 `σ̂_ch`
    /// 是单通道残差标准差，`s_rms` 是三相均方根；`δo²` 见
    /// [`Self::set_zero_uncertainty_lsb`]。
    pub fn pred_pp(&self) -> Option<f32> {
        self.accum.pred_pp()
    }

    /// 清空（换模式或显式重启标定）。零位也置无效，必须重新测。
    pub fn reset(&mut self) {
        let half_width = self.do_lsb_half_width;
        self.accum.reset();
        self.set_zero_uncertainty_lsb(half_width);
        self.trust = Trust::UntrustedExcitation;
        self.zero_lsb = [ADC_MIDSCALE_CODE; 3];
        self.zero_valid = false;
        self.active_theta = [1.0, 1.0];
        self.have_active = false;
        self.group = GroupWindow::empty();
        self.change_suspect = false;
        self.group_r2 = 0.0;
        self.group_z2 = 0.0;
        self.group_d = 0.0;
        self.have_group = false;
        self.pred_ok = false;
        self.chi2 = 0.0;
        self.chi2_ok = false;
    }
}
