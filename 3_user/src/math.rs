//! 无 libm 的确定性数值工具。
//!
//! host 与 G474 必须逐位一致，因此不用 `std`/`libm` 的 `sqrt`。只放被多个模块共用、
//! 且语义唯一的原语；模块私有的判据参数仍留在各自文件。

/// √3，Clarke 变换与电压圆限幅共用。
pub(crate) const SQRT3: f32 = 1.732_050_8;

/// 1/√3：热路径只做乘法，不做除法。
pub(crate) const INV_SQRT3: f32 = 0.577_350_26;

/// 确定性平方根：逆平方根牛顿迭代 3 次，再乘回 `x`。
///
/// `x ≤ 0`（含 `NaN`）返回 0，`+inf` 原样返回。初值用位模式魔数，迭代式
/// `y ← y·(1.5 − 0.5·x·y²)` 全程只有乘法，没有除法；3 次后相对误差约 1e-10。
/// 调用方仍须先判输入是否有限；这里的 `NaN → 0` 只服务数值原语，不是控制路径的
/// 故障兜底。
pub(crate) fn sqrt_f32(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    if x == f32::INFINITY {
        return x;
    }

    let half_x = 0.5 * x;
    let mut inv_sqrt = f32::from_bits(0x5f37_5a86 - (x.to_bits() >> 1));
    inv_sqrt *= 1.5 - half_x * inv_sqrt * inv_sqrt;
    inv_sqrt *= 1.5 - half_x * inv_sqrt * inv_sqrt;
    inv_sqrt *= 1.5 - half_x * inv_sqrt * inv_sqrt;
    x * inv_sqrt
}

#[cfg(test)]
mod tests {
    use super::sqrt_f32;

    #[test]
    fn sqrt_matches_std_over_the_control_range() {
        let mut x = 1e-6f32;
        while x < 1e3 {
            let expected = x.sqrt();
            let actual = sqrt_f32(x);
            let relative = (actual - expected).abs() / expected;
            assert!(relative < 1e-5, "x={x} actual={actual} expected={expected}");
            x *= 1.7;
        }
    }

    #[test]
    fn sqrt_handles_zero_and_non_finite_inputs() {
        assert_eq!(sqrt_f32(0.0), 0.0);
        assert_eq!(sqrt_f32(-1.0), 0.0);
        assert_eq!(sqrt_f32(f32::NAN), 0.0);
        assert_eq!(sqrt_f32(f32::INFINITY), f32::INFINITY);
    }
}
