//! 闸门内部的数值常量与判据辅助。
//!
//! host 与 G474 必须逐位一致；通用的 `sqrt_f32` 已上移到 `crate::motor::service_math`，这里只保留
//! 闸门专用量。

// 2×2 行列式近零判据：`S` 的元素是码²·权重（量级 1e5 以上），更小的 `det` 没有数值意义。
pub(super) const DET_EPS: f32 = 1e-12;

// 零位误差按 `[-h, +h]` 均匀分布时的方差：`(2h)²/12`。
pub(super) const fn do2_from_half_width(half_width_lsb: f32) -> f32 {
    let full_width = 2.0 * half_width_lsb;
    full_width * full_width / 12.0
}

// 整数幂（平方-乘），`λ^G` 组间遗忘用。无 libm：`powf` 在 `core` 不可用。
pub(super) fn powi_f32(base: f32, mut exp: u32) -> f32 {
    let mut acc = 1.0;
    let mut b = base;
    while exp > 0 {
        if exp & 1 == 1 {
            acc *= b;
        }
        b *= b;
        exp >>= 1;
    }
    acc
}
