//! 无 libm 的确定性数值工具。
//!
//! host 与 G474 必须逐位一致，因此不用 `std`/`libm` 的 `sqrt`。只放被多个模块共用、
//! 且语义唯一的原语；模块私有的判据参数仍留在各自文件。

/// √3，Clarke 变换与电压圆限幅共用。
pub(crate) const SQRT3: f32 = 1.732_050_8;

/// 确定性平方根：牛顿迭代 4 次，初值取指数折半位模式。
///
/// `x ≤ 0`（含 `NaN`）返回 0，`+inf` 原样返回。调用方仍须先判输入是否有限；
/// 这里的 `NaN → 0` 只服务数值原语，不是控制路径的故障兜底。
pub(crate) fn sqrt_f32(x: f32) -> f32 {
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
