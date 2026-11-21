//! 器件事实与电气硬约束。
//!
//! 只放两类数：手册/需求文档直接给出的量，以及由它们**唯一推出**的时序约束。改任何
//! 一条都会让电路或时序物理上不成立，所以每条都带来源，文件末尾用编译期断言把互相
//! 矛盾钉成编译错误。门限、降额与容量预算不在这里：预算见 [`super::budget`]，策略见
//! `user::policy`。

use super::budget::ISR_PERIOD_US;

/// 采样窗口下限，单位 ns。
///
/// 推导（HR-202 + HR-303，不是手册直接给的值）：MP6540HA 低边开通到 `SOx` 稳定
/// 2.0 µs（architecture.md §2.3，前端 τ ≈ 110 ns 不是瓶颈）+ ADC 采样 92.5 cyc
/// @ 42.5 MHz = 2.176 µs + 转换 12.5 cyc = 0.294 µs = 4.47 µs，再留 0.5 µs 余量
/// ⇒ 5.0 µs。
pub const SAMPLE_WINDOW_MIN_NS: u32 = 5_000;

/// PWM 半周期，单位 ns（中心对齐 20 kHz，HR-302：ARR = 4250 @ 42.5 MHz）。
///
/// 中心对齐时一个 50 µs 周期有两次低边共导窗口，各占半个周期。
pub const PWM_HALF_PERIOD_NS: u32 = ISR_PERIOD_US * 1_000 / 2;

/// 高边占空比上限。
///
/// 由 [`SAMPLE_WINDOW_MIN_NS`] 反推：`1 - 5.0/25 = 0.80`（半周期 25 µs）。HR-305 的
/// 3 µs / 90 % 是同一约束的下限版本，这里取更严的值。超过它，半周期内留给低边采样
/// 的窗口不足 5 µs。
pub const D_MAX: f32 = 0.80;

/// 电流采样对（ADC1/2 与 ADC3/4）的固定偏斜上限，单位 ns。
///
/// **目标值，不是器件保证**：E2 要求同一相的两路采样时刻对齐到 50 ns 内，靠固件
/// 同时触发与走线等长保证；实测值进遥测（HR-201/HR-202）。
pub const ADC_PAIR_SKEW_MAX_NS: u16 = 50;

/// ADC 参考电压，单位 V（VREF+ 接 3V3 轨，TL431 的 2.5 V 只喂比较器）。
pub const VREF_V: f32 = 3.3;

/// 12 bit ADC 的满量程码数（`2^12`）。
pub const ADC_FULL_SCALE_CODE: f32 = 4_096.0;

/// 12 bit ADC 的标称中点码（`VREF/2`）。
///
/// C0 实测零位后由标定档案覆盖；这里是未标定时唯一可用的复位值。
pub const ADC_MIDSCALE_CODE: f32 = 2_048.0;

/// `SOx` 的电流传输比，单位 安培⁻¹（典型 1/9200，范围 1/10500…1/7800）。
///
/// 依据：MP6540HGU-A-Z 数据手册；`architecture.md` §2.2 记录了料号与引脚事实。
pub const SOX_RATIO_PER_AMP: f32 = 1.0 / 9_200.0;

/// `SOx` 节点的戴维南等效负载，单位 Ω。
///
/// 4.99 kΩ/4.99 kΩ 分压偏置的并联值（HR-203），静态偏置 `VREF/2`。
pub const SOX_LOAD_OHMS: f32 = 2_495.0;

/// 电流采样标度，单位 V/A：`SOX_RATIO_PER_AMP × SOX_LOAD_OHMS ≈ 0.2712`。
pub const SOX_VOLTS_PER_AMP: f32 = SOX_RATIO_PER_AMP * SOX_LOAD_OHMS;

/// 每 LSB 对应的相电流，单位 A/LSB：`VREF_V / ADC_FULL_SCALE_CODE / SOX_VOLTS_PER_AMP`。
///
/// 数值约 2.97 mA/LSB（HR-203）。工作区按 ±5 A 设计，±6.08 A 只是电气满量程。
pub const SOX_AMPS_PER_LSB: f32 = VREF_V / ADC_FULL_SCALE_CODE / SOX_VOLTS_PER_AMP;

/// 帧长，单位 bit（24）。
///
/// MT6701 数据手册：24 bit 绝对角输出帧 = 14 bit 角度 + 4 bit 状态 + 6 bit CRC。
pub const ENC_FRAME_BITS: u8 = 24;

/// 角度位宽，单位 bit（14）。
pub const ENC_ANGLE_BITS: u8 = 14;

/// 磁场强度位宽，单位 bit（4）。
pub const ENC_MAG_BITS: u8 = 4;

/// CRC 位宽，单位 bit（6）。
pub const ENC_CRC_BITS: u8 = 6;

/// CRC-6 生成多项式 `x^6 + x + 1`（含最高位，`0b1000011`）。
pub const ENC_CRC_POLY: u8 = 0x43;

/// 每圈角度计数：`2^14 = 16384`，即 16384 计数 = 2π。
pub const ENC_COUNTS_PER_REV: u16 = 1 << ENC_ANGLE_BITS;

/// MT6701 SPI 时钟上限，单位 Hz：`t_CLK ≥ 64 ns` ⇒ 15.625 MHz。
///
/// 这是器件侧上限。实际分频选择（PCLK/16 = 10.625 MHz）是策略，见
/// `user::policy::ENC_SPI_HZ`。
pub const ENC_SPI_MAX_HZ: u32 = 1_000_000_000 / 64;

/// Flash bank 2 起始地址，单位 字节。
///
/// 双 bank 模式下两个 bank **不连续**：bank 1 是 `0x0800_0000…0x0800_FFFF`，bank 2 起于
/// `0x0804_0000`，中间 `0x0801_0000…0x0803_FFFF` 是地址空洞。依据：RM0440 Table 7
/// （512/256/128 KB dual bank organization，单 bank 见 Table 8）与 DS12288 §3.4 的
/// `Read-while-write` 特性；本文件的镜像见 `firmware/memory.x`。
pub const FLASH_BANK2_BASE: u32 = 0x0804_0000;

/// Flash bank 2 容量，单位 字节（128 KB 器件每 bank 64 KB）。
pub const FLASH_BANK2_BYTES: u32 = 64 * 1024;

/// Flash 页大小，单位 字节（G474 全系 2 KB/页）。
pub const FLASH_PAGE_BYTES: u32 = 2 * 1024;

/// 标定档案单槽占用的页数，单位 页。
///
/// 槽内只放一条定长记录（`board_id` + 版本 + 零位 + 相对增益 + 编码器零偏 + `OVP` +
/// CRC），剩余空间留给同槽的第二份副本，供写中断电时回退。
pub const CALIB_SLOT_PAGES: u32 = 2;

/// 标定档案单槽容量，单位 字节。
pub const CALIB_SLOT_BYTES: u32 = CALIB_SLOT_PAGES * FLASH_PAGE_BYTES;

/// 标定档案 A 槽起始地址，单位 字节。
pub const CALIB_SLOT_A_BASE: u32 = FLASH_BANK2_BASE;

/// 标定档案 B 槽起始地址，单位 字节。
pub const CALIB_SLOT_B_BASE: u32 = FLASH_BANK2_BASE + CALIB_SLOT_BYTES;

// 编译期自检：这些数不许自相矛盾。
const _: () = assert!(D_MAX > 0.0);
const _: () = assert!(D_MAX < 1.0);
const _: () = assert!(ADC_PAIR_SKEW_MAX_NS > 0);

// 采样标度钉在 HR-203 的两个特征值上（0.2712 V/A、2.97 mA/LSB），留 ±1 % 容差：
// 改分压或改参考电压必须同时改这里，否则编译失败。
const _: () = assert!(SOX_VOLTS_PER_AMP > 0.268 && SOX_VOLTS_PER_AMP < 0.274);
const _: () = assert!(SOX_AMPS_PER_LSB > 0.002_94 && SOX_AMPS_PER_LSB < 0.003_00);
const _: () = assert!(ADC_MIDSCALE_CODE * 2.0 == ADC_FULL_SCALE_CODE);

// `D_MAX` 的千分位镜像：const 上下文里 f32 不能转整数，一致性检查必须走整数算术
// （避免 f32 抽奖）。
const D_MAX_PERMILLE: u32 = 800;

// 硬约束：半周期减去高边占用后必须仍容得下采样窗口。
// `25_000 × (1000 - 800) = 5_000_000 ≥ 1000 × 5_000`。
const _: () = assert!(PWM_HALF_PERIOD_NS * (1000 - D_MAX_PERMILLE) >= 1000 * SAMPLE_WINDOW_MIN_NS);

// 镜像常量自己也得钉在 `D_MAX` 上：`[0.800, 0.801)` 内才算同一数值。
const _: () = assert!(
    D_MAX >= D_MAX_PERMILLE as f32 / 1000.0 && D_MAX < (D_MAX_PERMILLE + 1) as f32 / 1000.0
);
const _: () = assert!(PWM_HALF_PERIOD_NS * 2 == ISR_PERIOD_US * 1_000);

const _: () = assert!(ENC_FRAME_BITS == ENC_ANGLE_BITS + ENC_MAG_BITS + ENC_CRC_BITS);
const _: () = assert!(ENC_COUNTS_PER_REV == 1 << ENC_ANGLE_BITS);
const _: () = assert!(ENC_CRC_POLY >> ENC_CRC_BITS == 1);
const _: () = assert!(ENC_SPI_MAX_HZ == 15_625_000);

// bank 2 的页几何：整 bank、槽、页三者互相整除，两个槽都落在 bank 2 内且不重叠。
const _: () = assert!(FLASH_BANK2_BASE.is_multiple_of(FLASH_PAGE_BYTES));
const _: () = assert!(FLASH_BANK2_BYTES.is_multiple_of(FLASH_PAGE_BYTES));
const _: () = assert!(CALIB_SLOT_BYTES.is_multiple_of(FLASH_PAGE_BYTES));
const _: () = assert!(2 * CALIB_SLOT_BYTES <= FLASH_BANK2_BYTES);
const _: () = assert!(CALIB_SLOT_B_BASE == CALIB_SLOT_A_BASE + CALIB_SLOT_BYTES);
