//! 空间矢量调制：αβ 调制量 → 三相占空比。
//!
//! 移植自 `okhsunrog/oxifoc@3d48332` 的 `oxifoc-core/src/foc/svpwm.rs`
//! （Apache-2.0，注释自述源自 VESC 的 `foc_svm`）：几何法判扇区、算两个有效矢量
//! 的作用时间、中心对齐拆分零矢量，最后夹到比较寄存器范围。
//!
//! 与源码的唯一结构差异：扇区用 `enum` 表达，`match` 因此穷尽，不出现
//! `unreachable!` 的隐式 panic 分支（MECH-04）。
//!
//! 归一化约定沿用源码：`m = 1.5 · v / vbus`，即 [`normalize_volts`]；
//! 调制度幅值不超过 `√3/2` 才是线性区。
//!
//! 输出边界只有一处：比较值先用 [`hw::PWM_ARR`] 算，再夹到 `ARR·D_MAX`，最后换成
//! [`ClampedModulation`]。调用方拿不到 `max_duty`，也就无法绕过低边采样窗口约束。

use contracts::motor::config_hw as hw;
use contracts::motor::protocol_types::{ClampedModulation, Q15};

use crate::motor::service_math::INV_SQRT3;

/// 6 个扇区，编号与源码的整数 1–6 一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sector {
    /// 0°–60°。
    S1,
    /// 60°–120°。
    S2,
    /// 120°–180°。
    S3,
    /// 180°–240°。
    S4,
    /// 240°–300°。
    S5,
    /// 300°–360°。
    S6,
}

/// 用几何法判扇区（源 `get_sector`）。αβ 是调制量，不是伏特。
pub fn get_sector(alpha: f32, beta: f32) -> Sector {
    let one_by_sqrt3 = INV_SQRT3;
    let sector = if beta >= 0.0 {
        if alpha >= 0.0 {
            if one_by_sqrt3 * beta > alpha {
                2
            } else {
                1
            }
        } else if -one_by_sqrt3 * beta > alpha {
            3
        } else {
            2
        }
    } else if alpha >= 0.0 {
        if -one_by_sqrt3 * beta > alpha {
            5
        } else {
            6
        }
    } else if one_by_sqrt3 * beta > alpha {
        4
    } else {
        5
    };

    match sector {
        2 => Sector::S2,
        3 => Sector::S3,
        4 => Sector::S4,
        5 => Sector::S5,
        6 => Sector::S6,
        _ => Sector::S1,
    }
}

/// `2/√3`：SVPWM 作用时间的常数乘子，编译期折叠。
const TWO_BY_SQRT3: f32 = 2.0 * INV_SQRT3;

/// αβ 调制量 → 三相调制量（源 `space_vector_pwm`）。
///
/// 返回顺序 A/B/C。比较值以 [`hw::PWM_ARR`] 为满量程计算，再夹到 `ARR·D_MAX`；
/// 最后的 `f32 → Q15` 转换由 [`ClampedModulation`] 唯一完成。
#[inline]
pub fn space_vector_pwm(alpha: f32, beta: f32) -> [ClampedModulation; 3] {
    let sector = get_sector(alpha, beta);
    let pwm_full = i32::from(hw::PWM_ARR);
    let max_duty = (f32::from(hw::PWM_ARR) * hw::D_MAX) as i32;
    let pwm_full_f = pwm_full as f32;
    let one_by_sqrt3 = INV_SQRT3;

    let (ta, tb, tc) = match sector {
        Sector::S1 => {
            let t1 = ((alpha - one_by_sqrt3 * beta) * pwm_full_f) as i32;
            let t2 = (TWO_BY_SQRT3 * beta * pwm_full_f) as i32;
            let ta = (pwm_full + t1 + t2) / 2;
            (ta, ta - t1, ta - t1 - t2)
        }
        Sector::S2 => {
            let t2 = ((alpha + one_by_sqrt3 * beta) * pwm_full_f) as i32;
            let t3 = ((-alpha + one_by_sqrt3 * beta) * pwm_full_f) as i32;
            let tb = (pwm_full + t2 + t3) / 2;
            (tb - t3, tb, tb - t3 - t2)
        }
        Sector::S3 => {
            let t3 = (TWO_BY_SQRT3 * beta * pwm_full_f) as i32;
            let t4 = ((-alpha - one_by_sqrt3 * beta) * pwm_full_f) as i32;
            let tb = (pwm_full + t3 + t4) / 2;
            let tc = tb - t3;
            (tc - t4, tb, tc)
        }
        Sector::S4 => {
            let t4 = ((-alpha + one_by_sqrt3 * beta) * pwm_full_f) as i32;
            let t5 = (-TWO_BY_SQRT3 * beta * pwm_full_f) as i32;
            let tc = (pwm_full + t4 + t5) / 2;
            let tb = tc - t5;
            (tb - t4, tb, tc)
        }
        Sector::S5 => {
            let t5 = ((-alpha - one_by_sqrt3 * beta) * pwm_full_f) as i32;
            let t6 = ((alpha - one_by_sqrt3 * beta) * pwm_full_f) as i32;
            let tc = (pwm_full + t5 + t6) / 2;
            let ta = tc - t5;
            (ta, ta - t6, tc)
        }
        Sector::S6 => {
            let t6 = (-TWO_BY_SQRT3 * beta * pwm_full_f) as i32;
            let t1 = ((alpha + one_by_sqrt3 * beta) * pwm_full_f) as i32;
            let ta = (pwm_full + t6 + t1) / 2;
            let tc = ta - t1;
            (ta, tc - t6, tc)
        }
    };

    let full = pwm_full as f32;
    [ta, tb, tc].map(|duty| {
        let clamped = duty.clamp(0, max_duty);
        let ratio = clamped as f32 / full;
        ClampedModulation::new_clamped(Q15::from_ratio(2.0 * ratio - 1.0))
    })
}

/// 伏特 → 调制量：`m = 1.5 · v / vbus`（源文档的 `voltage_normalize`）。
///
/// 母线电压必须为正且有限，否则返回 `None`，由调用方进故障而不是让 `inf`/`NaN`
/// 流进比较寄存器。该归一化在初始化时确定一次（MECH-03）。
pub fn normalize_volts(alpha_v: f32, beta_v: f32, vbus_v: f32) -> Option<(f32, f32)> {
    if vbus_v <= 0.0 || !vbus_v.is_finite() {
        return None;
    }
    normalize_volts_with_inv(alpha_v, beta_v, 1.0 / vbus_v)
}

/// 已缓存 `1/vbus` 时的归一化入口：20 kHz 环只做乘法，除法留在 1 kHz 慢任务。
#[inline]
pub fn normalize_volts_with_inv(alpha_v: f32, beta_v: f32, inv_vbus: f32) -> Option<(f32, f32)> {
    if inv_vbus <= 0.0 || !inv_vbus.is_finite() {
        return None;
    }
    if !alpha_v.is_finite() || !beta_v.is_finite() {
        return None;
    }
    let scale = 1.5 * inv_vbus;
    Some((alpha_v * scale, beta_v * scale))
}

#[cfg(test)]
mod tests;
