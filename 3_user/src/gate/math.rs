//! 闸门内部的无 libm 数值工具。
//!
//! host 与 G474 必须逐位一致，因此不用 `std`/`libm` 的 `sqrt`/`powf`。这些函数只服务
//! `gate` 的判定，不对外导出。

// √3，幅值不变 Clarke 变换用。
pub(super) const SQRT3: f32 = 1.732_050_8;

// 2×2 行列式近零判据：`S` 的元素是码²·权重（量级 1e5 以上），更小的 `det` 没有数值意义。
pub(super) const DET_EPS: f32 = 1e-12;

// 12 bit ADC 标称中点（`VREF/2`，HR-203 的 1.65 V）。C0 测量后由
// `GainGate::set_zero_lsb` 覆盖。
pub(super) const ADC_MIDSCALE_CODE: f32 = 2_048.0;

// 零位误差按 `[-h, +h]` 均匀分布时的方差：`(2h)²/12`。
pub(super) const fn do2_from_half_width(half_width_lsb: f32) -> f32 {
    let full_width = 2.0 * half_width_lsb;
    full_width * full_width / 12.0
}

// 确定性平方根：牛顿迭代 4 次，初值取指数折半位模式。
// `x ≤ 0`（含 `NaN`）返回 0，`+inf` 原样返回。
pub(super) fn sqrt_f32(x: f32) -> f32 {
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
