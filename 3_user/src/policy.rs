//! 算法策略参数。
//!
//! 这里的每个数都是**可调的判定依据或设计选择**（门限、降额、窗口长度、分频），
//! 不是器件事实。器件事实与电气硬约束在 `contracts::limits::hw`，跨 crate 的系统
//! 预算在 `contracts::limits::budget`。把门限改大改小不会让硬件不成立，只会改变
//! 判定松紧，因此它们属于算法层，`contracts` 不持有。
//!
//! 允许引用 `contracts::limits::hw` / `budget` 派生出量纲正确的新常量（如
//! [`MAX_BLIND_US`] → [`ENC_MAX_MISS`]）；不允许把 `contracts` 的数值抄一份。

use contracts::limits::budget;
use contracts::limits::hw;

/// 方向覆盖门限：`R = |mean(unit current vector)|`，低于它判激励不足。
///
/// 依据：残差对共模增益完全盲（三相同比例放大时 `i_a + i_b + i_c` 仍为 0，回归只能
/// 看到相对增益），必须确认电流向量确实在转。
pub const R_MAX: f32 = 0.995;

/// 无激励门，作用在减零位后的回归量上：`d = det(S) / tr²(S)`。
///
/// 依据：低于它是信息矩阵秩亏，参数不可辨识；电流停在一条轴上也能"收敛"。
pub const D_MIN: f32 = 0.01;

/// DC 偏置门：`z = |A0| · √3 / √(G · A1)`。
///
/// 其中 `A0 = Σ(s_a + s_b + s_c)`、`A1 = Σ(s_a² + s_b² + s_c²)`、`G = 200`（10 ms
/// 组窗）。整段电流偏一侧时，偏置会被学成增益，`z` 抓的就是这种窗口。
pub const Z_MAX: f32 = 0.02;

/// 精度门降额系数，判据是 `pred ≤ KAPPA · target`。
///
/// 依据：`pred` 是 RMS 界，实测 `p99 / pred ≈ 2.3`（见
/// [`PRED_P99_OVER_PRED`]），不降额会把 p99 超差放过去。0.9 是留出该比例的工程取值。
pub const KAPPA: f32 = 0.9;

/// 跨工况一致性门：χ²(2) 的 99 % 分位点。
///
/// 依据：假设检验的自由度固定为 2（两个相对增益），只在通过 `R` / `d` / `z` 三个
/// 结构门的样本组上计算，否则分布假设不成立。
pub const CHI2_MAX: f32 = 9.21;

/// 遗忘因子 `λ = 1 - 1/2000`。
///
/// 依据：标定窗 100 ms @ 20 kHz，选择让窗尾权重衰到 1/e 附近；做成可调是为了在
/// 变频工况下重调记忆长度。
pub const LAMBDA: f32 = 0.9995;

/// 组窗样本数 `G`：10 ms @ 20 kHz，z 门每 200 个样本重算一次。
pub const GROUP_SAMPLES: u32 = 200;

/// 组内突变检测的 σ 倍数：单拍残差门限 `|r| > K·σ_res`，组内累积残差门限
/// `Σr² > (n + K·√(2n))·σ_res²`。
///
/// 依据：`σ_res²` 取上一已通过组窗的残差方差（三相零和违背）。`K = 8` 对单拍
/// 等效 64σ²，高于三路均匀噪声叠加的最坏幅值（约 36σ²），不会把本底噪声判成突变；
/// 25 % 相对增益阶跃在满量程样本上约 85σ，单拍即越界。累积式在 `n = 200` 时给出
/// `360σ_res²`（均值 `200σ_res²` + 8σ 尾），2 % 阶跃（约 `4360σ_res²`）足以触发。
pub const RESIDUAL_K_SIGMA: f32 = 8.0;

/// 组内累积残差判据的最少拍数。
///
/// 依据：小样本的 χ² 尾部分布过重（n = 1 时 `n + K√(2n)` 只有约 12σ²，而单拍
/// 最坏可达 36σ²），必须等样本足够多才启用累积判据。取 20 拍（1 ms）后
/// `χ²(n)/n` 的波动已降到约 `√(2/n) ≈ 0.32`。
pub const GROUP_SSE_MIN_SAMPLES: u32 = 20;

/// 标定窗有效样本权重和 `Σw`：100 ms @ 20 kHz。
pub const WINDOW_SAMPLES: u32 = 2_000;

/// 有效样本数上限 `N_eq = (1 + λ) · Σw = 3999`。
///
/// 由 [`WINDOW_SAMPLES`] 与 [`LAMBDA`] 派生；策略上不允许 `N_eq` 超过它（再多也不
/// 增加独立信息）。
pub const N_EQ_MAX: u32 = 3_999;

/// 预测界的 p99 与 `pred` 之比（实测），只用于解释 [`KAPPA`]，不参与判定。
pub const PRED_P99_OVER_PRED: f32 = 2.3;

/// 设备侧精度目标，单位 pp：`pred ≤ KAPPA · TARGET_PP` 才允许冻结相对增益。
///
/// 依据：0.5 % 增益误差对应 0.5 % 的电流/力矩刻度误差，低于机械与编码器误差预算，
/// 也远低于 E4 要演示的 -25 % 失配量级。这是接受判据，不是从电流幅值推出来的；主机
/// 可以拿慢帧上报的 `pred_pp` 按自己的目标离线重判。
pub const TARGET_PP: f32 = 0.5;

/// 幅值下限参考点 1 的电流，单位 A。
pub const AMP_FLOOR_LOW_AMPS: f32 = 0.14;

/// 0.14 A 处 `pred` 的降额后目标，单位 pp。
pub const AMP_FLOOR_LOW_PP: f32 = 0.5;

/// 幅值下限参考点 2 的电流，单位 A。
pub const AMP_FLOOR_HIGH_AMPS: f32 = 0.55;

/// 0.55 A 处 `pred` 的降额后目标，单位 pp。
pub const AMP_FLOOR_HIGH_PP: f32 = 0.05;

/// 连续无效样本计数阈值：超过即判窗口不可信，增益门回退到上一份快照。
///
/// 依据：8 × 50 µs = 400 µs，尚无 HIL 依据，阻塞于两个统计：E2（ADC 对间偏斜补偿
/// 后的残余）与 E3（占空比 90 % 时窗口不足 3 µs 的拍数占比）联合给出的无效样本
/// 比例。定值方法：取冷启动与最坏占空比下该比例的 p99.9，换算成连续拍数后向上取
/// 整，并留一次组窗（`G = 200`）的回退余量。
pub const INVALID_SAMPLE_LIMIT: u16 = 8;

/// 标定 dispatcher 周期，单位 ms（与组窗 `G` 对齐）。
pub const CALIB_TICK_MS: u32 = 10;

/// 编码器 SPI 时钟，单位 Hz（PCLK/16 = 10.625 MHz）。
///
/// 策略值：器件上限是 `hw::ENC_SPI_MAX_HZ`（15.625 MHz），这里取分频选择。10.625 MHz
/// 下 24 bit 帧约 2.26 µs，留出 ISR 预算。
pub const ENC_SPI_HZ: u32 = 10_625_000;

/// 关节机械转速上限，单位 rpm（当前按 3000 降额使用）。
///
/// 依据：关节机械极限（减速比、联轴器与轴承的许用转速），不是 MT6701 自身的
/// 55 000 rpm 上限；跳变判据只用它把一个电流环周期内可能发生的真实转角换算成 LSB。
pub const ENC_MECH_RPM_MAX: f32 = 3_000.0;

/// 电机极对数。
///
/// `[OPEN]`：WK4310 为 14 对极，待结构/铭牌确认；只用于把机械计数换算成电角度
/// 误差（见文件末尾的盲推断言），不参与控制。
pub const POLE_PAIRS: u32 = 14;

/// 允许的最坏电角度误差，单位度。超过它，位置环会把错误电角当成真实角度。
///
/// 策略值：30° 电角是 FOC 转矩可用性的工程底线（cos 30° ≈ 0.87），再大就不是
/// "稍微多走了一点"，而是必须停机。
pub const THETA_E_MAX_DEG: u32 = 30;

/// 连续丢帧期间允许的盲推时长，单位 µs。
///
/// 由两条物理底线反解（见文件末尾断言）：
///
/// - 混叠底线：`(M+1)·B < ENC_COUNTS_PER_REV/(2·POLE_PAIRS)`，M 是允许连续丢失的
///   帧数。`M = 12` 时 `13·45 = 585 < 585.14`，这是混叠单独允许的上限。
/// - 电角度底线：`(M+1)·B·POLE_PAIRS·360 ≤ THETA_E_MAX_DEG·ENC_COUNTS_PER_REV`。
///   代入 `B = 45`、`POLE_PAIRS = 14`、`THETA_E_MAX_DEG = 30`：
///   `(M+1)·45·14·360 = (M+1)·226_800 ≤ 491_520` ⇒ `M+1 ≤ 2.17` ⇒ `M ≤ 1`。
///
/// 电角度底线是瓶颈。取 `M = 1` 时最小整数窗口是 `MAX_BLIND_US = 50`
/// （`50 / ISR_PERIOD_US = 1`）；`MAX_BLIND_US = 100` 对应 `M = 2`，最坏电角
/// `3·45·14·360/16384 = 34.7° > 30°`，不成立。若强行保留 100 µs，必须把
/// `B` 压到 32 以下（`ENC_MECH_RPM_MAX ≤ ≈2050 rpm`），那会把已按机械极限降额的
/// 3000 rpm 再砍三分之一，代价远大于收益。
///
/// 代价：一次 CRC 抖动后只容许 1 帧盲推，第二帧连续丢失即锁存。这是安全侧的
/// 有意选择——帧率 20 kHz、MT6701 的 CRC 残余错误率远低于连续两帧。
pub const MAX_BLIND_US: u32 = 50;

/// 丢失帧窗口上限：连续未接受的帧超过它就直接锁存。
///
/// 由 [`MAX_BLIND_US`] 与 ISR 周期派生（整数除法，不留余数）：`50 / 50 = 1`。
/// 窗口内跳变界按丢失拍数线性放宽为 `(missed + 1) · B`；超过窗口则角度不是
/// "稍微多走了一点"，而是完全不知道，必须停机。
pub const ENC_MAX_MISS: u8 = (MAX_BLIND_US / budget::ISR_PERIOD_US) as u8;

/// 单周期最大真实转角预算 `B = jump_limit_lsb(ENC_MECH_RPM_MAX, ISR_PERIOD_US)`，
/// 单位 LSB。
///
/// 手算（`jump_limit_lsb` 含 f32，在 const 上下文不可调用）：
/// `3000 rpm / 60 = 50 rev/s`，`× 16384 count/rev = 819_200 count/s`，
/// `× 50 µs = 40.96 count/拍` ⇒ 向上取整 41，再加 [`ENC_JUMP_MARGIN_LSB`] = 45。
/// 测试 `jump_limit_matches_policy_mirror` 断言本常量与函数结果相等，
/// 防止两处漂移。
pub const B_MAX_LSB: u16 = 45;

/// 全 0 帧护栏：连续多少帧全 0、且此前接受角非 0 时判硬失败。
///
/// 依据：`0x000000` 本身是合法帧（θ=0、Mg=0、CRC=0），无法与 MISO 恒低区分；靠近
/// 回绕处的前几帧全 0 还能通过跳变判据，只能靠这个计数抓。8 帧 = 400 µs。
pub const ENC_ZERO_FRAME_LIMIT: u8 = 8;

/// 磁场异常（`Mg[1:0] != 0`）的连续帧门限。
///
/// 依据：单帧异常不拒帧，只上报健康度由上层降额；连续达到门限才升级为硬失败。
/// 20 帧 = 1 ms。
pub const ENC_MAG_WARN_LIMIT: u8 = 20;

/// 语义硬失败锁存所需的连续帧数。
///
/// `Mg[3]`、全 0 护栏、跳变、磁场升级连续 2 帧才锁存。CRC 丢帧只计入
/// [`ENC_MAX_MISS`] 窗口，不占这个计数：丢帧是传输问题，不是传感器坏了的证据。
pub const ENC_FAIL_LATCH_FRAMES: u8 = 2;

/// 跳变判据在单周期转角预算上附加的余量，单位 LSB。
///
/// 依据：覆盖量化与两帧之间的采样相位抖动；4 LSB ≈ 0.088° 机械角。
pub const ENC_JUMP_MARGIN_LSB: u8 = 4;

/// 故障清除前要求的无故障持续时长，单位 `µs`。
///
/// 依据：HR 的安全阶梯要求"故障锁存，清除需显式命令 + 无故障持续 T"，未给 T。取
/// 0.5 s 的两条理由：比通信看门狗（`budget::CAN_TIMEOUT_MS = 20 ms`）长 25 倍，
/// 通信抖动不会被当成"已经稳定"；又短于人工对命令生效的耐心（超过 1 s 会以为命令
/// 丢了）。判据在 `user::fsm`，本常量只给时长。
pub const FAULT_CLEAR_HOLD_US: u32 = 500_000;

/// 零位标定的误差半宽，单位 LSB。
///
/// C0 测量零位后残余误差按 `[-h, +h]` 的均匀分布处理（`h = 0.5` LSB 即"半个码"，
/// 覆盖 ADC 量化与取整）。方差 `δo² = (2h)²/12 = 1/12 ≈ 0.0833 LSB²`，见
/// [`DO2_LSB2`]。
pub const DO_LSB_HALF_WIDTH: f32 = 0.5;

/// 零位残余不确定度 `δo²`，单位 LSB²：`(2·DO_LSB_HALF_WIDTH)²/12 = 1/12`。
///
/// 这是 `pred` 第二项的分子组成部分。零位残余必须非零：取 0 等于宣称零位无误差，
/// 预测界会系统性偏乐观。有效性由 `gate` 的 `do2` 测试守住（`do2 > 0` ⇒ `pred` 更大）。
pub const DO2_LSB2: f32 = (2.0 * DO_LSB_HALF_WIDTH) * (2.0 * DO_LSB_HALF_WIDTH) / 12.0;

// 策略参数自身不许自相矛盾（门限的取值范围，不含物理推导）。
const _: () = assert!(R_MAX < 1.0);
const _: () = assert!(D_MIN > 0.0);
const _: () = assert!(Z_MAX > 0.0);
const _: () = assert!(KAPPA < 1.0);
const _: () = assert!(LAMBDA < 1.0);
const _: () = assert!(CHI2_MAX > 0.0);
const _: () = assert!(GROUP_SAMPLES > 0);
const _: () = assert!(RESIDUAL_K_SIGMA > 0.0);
const _: () = assert!(GROUP_SSE_MIN_SAMPLES > 0 && GROUP_SSE_MIN_SAMPLES < GROUP_SAMPLES);
const _: () = assert!(WINDOW_SAMPLES > GROUP_SAMPLES);
const _: () = assert!(N_EQ_MAX >= WINDOW_SAMPLES);
const _: () = assert!(ENC_MECH_RPM_MAX > 0.0);
const _: () = assert!(POLE_PAIRS > 0);
const _: () = assert!(THETA_E_MAX_DEG > 0 && THETA_E_MAX_DEG <= 90);
const _: () = assert!(ENC_SPI_HZ > 0 && ENC_SPI_HZ <= hw::ENC_SPI_MAX_HZ);
const _: () = assert!(ENC_ZERO_FRAME_LIMIT > ENC_FAIL_LATCH_FRAMES);
const _: () = assert!(ENC_MAG_WARN_LIMIT > ENC_FAIL_LATCH_FRAMES);
const _: () = assert!(ENC_MAX_MISS > 0);
const _: () = assert!(DO_LSB_HALF_WIDTH > 0.0);

// 清故障窗口必须长于通信看门狗：否则"通信刚恢复"就能被当成"已经稳定"，
// 20 ms 的丢帧抖动会直接换来一次清故障。
const _: () = assert!(FAULT_CLEAR_HOLD_US > budget::CAN_TIMEOUT_MS as u32 * 1_000);

// 盲推窗口的两条物理底线（整数算术，避免 f32 在 const 里的不确定舍入）。
//
// 1) 混叠底线：恢复帧的跳变界必须小于半个电周期对应的机械计数，
//    否则 `(M+1)·B` 本身就能跨过一个电周期，恢复帧无法区分。
const _: () = assert!(
    ((ENC_MAX_MISS as u32 + 1) * B_MAX_LSB as u32)
        < (hw::ENC_COUNTS_PER_REV as u32) / (2 * POLE_PAIRS)
);
// 2) 电角度底线：盲推期间真实可能走过的电角必须 ≤ THETA_E_MAX_DEG。
//    当前 `(1+1)·45·14·360 = 453_600 ≤ 30·16384 = 491_520`（最坏 27.69° elec）。
const _: () = assert!(
    ((ENC_MAX_MISS as u32 + 1) * B_MAX_LSB as u32 * POLE_PAIRS * 360)
        <= THETA_E_MAX_DEG * hw::ENC_COUNTS_PER_REV as u32
);
