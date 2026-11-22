//! 电流环 PI。
//!
//! 增益语义与结构移植自 `calebfletcher/foc@1db0260` 的 `src/pid.rs`
//! （Apache-2.0）：`u = kp·e + ∫ki·e·dt`，积分分量单独维护，`update` 接受
//! `(setpoint, measurement, dt)`。
//!
//! 本地扩展：源码不做输出限幅。母线电压有限，积分必须抗饱和，否则恢复要等几百拍。
//! 这里加 `limit_v` 与条件积分——饱和且误差继续往同向推时冻结积分，反向误差立即回调。

/// 一拍 PI 结果；`proportional_v` 供外层反算积分器时复用，避免再算一次 `kp·e`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PiOutput {
    /// 总输出电压，伏特（已按 `limit_v` 夹紧）。
    pub voltage_v: f32,
    /// 比例项 `kp·e`，伏特。
    pub proportional_v: f32,
}

/// 带输出限幅与条件积分的 PI。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pi {
    kp_v_per_a: f32,
    ki_v_per_a_s: f32,
    integral_v: f32,
    limit_v: f32,
}

impl Pi {
    /// 构造。`limit_v` 是输出电压的对称上限（通常取母线电压的一半）。
    pub const fn new(kp_v_per_a: f32, ki_v_per_a_s: f32, limit_v: f32) -> Self {
        Self {
            kp_v_per_a,
            ki_v_per_a_s,
            integral_v: 0.0,
            limit_v,
        }
    }

    /// 清零积分项。切模式、清故障、重新使能前必须调用。
    pub fn reset(&mut self) {
        self.integral_v = 0.0;
    }

    /// 当前积分项，单位 伏特（遥测与测试用）。
    pub fn integral_v(&self) -> f32 {
        self.integral_v
    }

    /// 直接预置积分项，单位伏特（带速接管用）。
    ///
    /// 非有限值按 0 处理；这是积分器的唯一写入原语，`reset()` 与反算都走它。
    #[inline]
    pub fn preload_v(&mut self, integral_v: f32) {
        self.integral_v = if integral_v.is_finite() {
            integral_v
        } else {
            0.0
        };
    }

    /// 按最终输出反算积分项：`integral = output − kp·error`。
    ///
    /// 外层做电压圆限幅后调用，保证积分器与最终输出一致。
    pub fn back_calculate_integral(&mut self, output_v: f32, error_a: f32) {
        self.back_calculate_integral_from_p(output_v, self.kp_v_per_a * error_a);
    }

    /// 已持有比例项时的反算入口：`integral = output − proportional`。
    #[inline]
    pub fn back_calculate_integral_from_p(&mut self, output_v: f32, proportional_v: f32) {
        self.preload_v(output_v - proportional_v);
    }

    /// 一拍更新：`u = kp·(r − y) + ∫ki·(r − y)·dt`，输出夹在 `±limit_v`。
    pub fn update(&mut self, setpoint_a: f32, measurement_a: f32, dt_s: f32) -> f32 {
        self.update_with_p(setpoint_a, measurement_a, dt_s)
            .voltage_v
    }

    /// 与 [`Self::update`] 相同，但同时返回比例项，供外层反算积分器复用。
    #[inline]
    pub fn update_with_p(
        &mut self,
        setpoint_a: f32,
        measurement_a: f32,
        dt_s: f32,
    ) -> PiOutput {
        let error_amps = setpoint_a - measurement_a;
        let p_v = self.kp_v_per_a * error_amps;
        let step_v = self.ki_v_per_a_s * error_amps * dt_s;
        let candidate_v = self.integral_v + step_v;
        let out_v = p_v + candidate_v;

        let voltage_v = if out_v > self.limit_v {
            if step_v <= 0.0 {
                self.integral_v = candidate_v;
            }
            self.limit_v
        } else if out_v < -self.limit_v {
            if step_v >= 0.0 {
                self.integral_v = candidate_v;
            }
            -self.limit_v
        } else {
            self.integral_v = candidate_v;
            out_v
        };

        PiOutput {
            voltage_v,
            proportional_v: p_v,
        }
    }
}

#[cfg(test)]
mod tests;
