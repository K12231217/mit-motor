//! 全工程唯一数值来源（PRE-07）。
//!
//! 门限、容量、占空比上限、超时与计数阈值只在这里出现一次，其它 crate 只引名字。
//! 每个常量写明取值依据：物理窗口、分辨率或实测统计。

/// 电流环频率，单位 Hz。由 ADC 注入组完成事件驱动，不是软件定时器。
pub const CURRENT_LOOP_HZ: u32 = 20_000;

/// ISR 周期，单位 µs（1 / 20 kHz）。
pub const ISR_PERIOD_US: u32 = 50;

/// ISR 时间预算，单位 µs。
///
/// 预算含快照发布（< 0.5 µs）；DWT 实测值进遥测。
pub const ISR_BUDGET_US: u32 = 25;

/// 高边占空比上限。
///
/// 超过它，半周期（25 µs）内留给低边采样的窗口不足 5 µs。
pub const D_MAX: f32 = 0.80;

/// 采样窗口下限，单位 ns。
///
/// 前提是中心对齐 20 kHz 的半周期 25 µs（ARR = 4250 @ 42.5 MHz）。低边开通到 ADC
/// 取完样需要：MP6540HA 低边建立 2.0 µs + 采样 92.5 cyc（2.176 µs）+ 转换 12.5 cyc
/// （0.294 µs）= 4.47 µs，再留 0.5 µs 余量取 5.0 µs。反推占空比上限
/// `1 - 5.0/25 = 0.80`，与 [`D_MAX`] 一致（见文件末尾的编译期断言）。
pub const SAMPLE_WINDOW_MIN_NS: u16 = 5_000;

/// 电流采样对（ADC1/2 与 ADC3/4）的固定偏斜上限，单位 ns（E2）。
pub const ADC_PAIR_SKEW_MAX_NS: u16 = 50;

/// 方向覆盖门限：`R = |mean(unit current vector)|`，低于它判激励不足。
///
/// 残差对共模增益完全盲：三相同比例放大时 `i_a + i_b + i_c` 仍为 0，回归只能看到
/// 相对增益，因此必须确认电流向量确实在转。
pub const R_MAX: f32 = 0.995;

/// 无激励门，作用在减零位后的回归量上：`d = det(S) / tr²(S)`。
///
/// 低于它是信息矩阵秩亏，参数不可辨识；电流停在一条轴上也能"收敛"。
pub const D_MIN: f32 = 0.01;

/// DC 偏置门：`z = |A0| · √3 / √(G · A1)`。
///
/// 其中 `A0 = Σ(s_a + s_b + s_c)`、`A1 = Σ(s_a² + s_b² + s_c²)`、`G = 200`（10 ms
/// 组窗）。整段电流偏一侧时，偏置会被学成增益，`z` 抓的就是这种窗口。
pub const Z_MAX: f32 = 0.02;

/// 精度门降额系数，判据是 `pred ≤ KAPPA · target`。
///
/// `pred` 是 RMS 界，实测 `p99 / pred ≈ 2.3`，不降额会把 p99 超差放过去。
pub const KAPPA: f32 = 0.9;

/// 跨工况一致性门：χ²(2) 的 99 % 分位点。
///
/// 只在通过 `R` / `d` / `z` 三个结构门的样本组上计算，否则分布假设不成立。
pub const CHI2_MAX: f32 = 9.21;

/// 遗忘因子 `λ = 1 - 1/2000`。
pub const LAMBDA: f32 = 0.9995;

/// 组窗样本数 `G`：10 ms @ 20 kHz，z 门每 200 个样本重算一次。
pub const GROUP_SAMPLES: u32 = 200;

/// 标定窗有效样本权重和 `Σw`：100 ms @ 20 kHz。
pub const WINDOW_SAMPLES: u32 = 2_000;

/// 有效样本数上限 `N_eq = (1 + λ) · Σw = 3999`。
pub const N_EQ_MAX: u32 = 3_999;

/// 预测界的 p99 与 `pred` 之比（实测），只用于解释 [`KAPPA`]，不参与判定。
pub const PRED_P99_OVER_PRED: f32 = 2.3;

/// 设备侧精度目标，单位 pp：`pred ≤ KAPPA · TARGET_PP` 才允许冻结相对增益。
///
/// 这是接受判据，不是从电流幅值推出来的：0.5 % 增益误差对应 0.5 % 的电流/力矩刻度
/// 误差，低于机械与编码器误差预算，也远低于 E4 要演示的 -25 % 失配量级。主机可以拿
/// 慢帧上报的 `pred_pp` 按自己的目标离线重判。
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
/// 取值 8（8 × 50 µs = 400 µs）尚无 HIL 依据，阻塞于两个统计：E2（ADC 对间偏斜
/// 补偿后的残余）与 E3（占空比 90 % 时窗口不足 3 µs 的拍数占比）联合给出的无效样本
/// 比例。定值方法：取冷启动与最坏占空比下该比例的 p99.9，换算成连续拍数后向上取整，
/// 并留一次组窗（`G = 200`）的回退余量。
pub const INVALID_SAMPLE_LIMIT: u16 = 8;

/// 标定 dispatcher 周期，单位 ms（与组窗 `G` 对齐）。
pub const CALIB_TICK_MS: u32 = 10;

/// 通信看门狗超时，单位 ms；也作为 CMD 帧 `ttl_ms = 0` 的默认值。
pub const CAN_TIMEOUT_MS: u16 = 20;

/// CAN FD 数据段波特率，单位 bit/s。
pub const CAN_DATA_BITRATE_BPS: u32 = 2_000_000;

/// 反馈帧速率，单位 Hz（1 kHz，E7 要求无丢帧）。
pub const TELEMETRY_HZ: u32 = 1_000;

/// 慢帧速率，单位 Hz（50 Hz，状态变化或请求时提前发）。
pub const SLOW_FRAME_HZ: u32 = 50;

/// 命令队列深度（2 的幂）。
pub const CMD_Q: usize = 16;

/// 事件队列深度（2 的幂）。
pub const EVT_Q: usize = 32;

/// 黑匣子 trace 队列深度（2 的幂）。
pub const TRC_Q: usize = 8;

/// 日志环条目数：12 B/条 × 512 = 6 KB 的 SRAM2 保留区。
pub const LOG_Q: usize = 512;

/// seqlock 读重试上限；超过就用上一拍值，绝不阻塞。
pub const SNAPSHOT_RETRY_MAX: u8 = 4;

/// SPI 时钟，单位 Hz（10.625 MHz）。
pub const ENC_SPI_HZ: u32 = 10_625_000;

/// 帧长，单位 bit（24）。
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

/// 关节机械转速上限，单位 rpm（当前按 3000 降额使用）。
///
/// 依据是关节机械极限（减速比、联轴器与轴承的许用转速），不是 MT6701 自身的
/// 55 000 rpm 上限；跳变判据只用它把一个电流环周期内可能发生的真实转角换算成 LSB。
pub const ENC_MECH_RPM_MAX: f32 = 3_000.0;

/// 全 0 帧护栏：连续多少帧全 0、且此前接受角非 0 时判硬失败。
///
/// `0x000000` 本身是合法帧（θ=0、Mg=0、CRC=0），无法与 MISO 恒低区分；靠近回绕处
/// 的前几帧全 0 还能通过跳变判据，只能靠这个计数抓。
pub const ENC_ZERO_FRAME_LIMIT: u8 = 8;

/// 磁场异常（`Mg[1:0] != 0`）的连续帧门限。
///
/// 单帧异常不拒帧，只上报健康度由上层降额；连续达到门限才升级为硬失败。
pub const ENC_MAG_WARN_LIMIT: u8 = 20;

/// 语义硬失败锁存所需的连续帧数。
///
/// `Mg[3]`、全 0 护栏、跳变、磁场升级连续 2 帧才锁存。CRC 丢帧只计入
/// [`ENC_MAX_MISS`] 窗口，不占这个计数：丢帧是传输问题，不是传感器坏了的证据。
pub const ENC_FAIL_LATCH_FRAMES: u8 = 2;

/// 丢失帧窗口上限：连续未接受的帧超过它就直接锁存。
///
/// 窗口内跳变界按丢失拍数线性放宽为 `(missed + 1) · B`；`B` 是一个周期的最大位移，
/// 丢了几拍就按几拍算，否则一次 CRC 抖动后的恢复帧会被误判为跳变。超过窗口则角度
/// 不是"稍微多走了一点"，而是完全不知道，必须停机。
pub const ENC_MAX_MISS: u8 = 5;

/// 跳变判据在单周期转角预算上附加的余量，单位 LSB。
pub const ENC_JUMP_MARGIN_LSB: u8 = 4;

// 编译期自检：常量本身不许自相矛盾。
const _: () = assert!(D_MAX > 0.0);
const _: () = assert!(D_MAX < 1.0);

// `D_MAX` 的千分位镜像：const 上下文里 f32 不能转整数，一致性检查必须走整数算术
// （避免 f32 抽奖）。
const D_MAX_PERMILLE: u32 = 800;

// `D_MAX` 与采样窗口不许各说各话：半周期 25 µs、占空比千分位 800 时，
// `25_000 - 25_000 × 800 / 1000` 必须等于窗口值。
const _: () = assert!(25_000 - 25_000 * D_MAX_PERMILLE / 1000 == SAMPLE_WINDOW_MIN_NS as u32);
// 镜像常量自己也得钉在 `D_MAX` 上：`[0.800, 0.801)` 内才算同一数值。
const _: () = assert!(
    D_MAX >= D_MAX_PERMILLE as f32 / 1000.0 && D_MAX < (D_MAX_PERMILLE + 1) as f32 / 1000.0
);
const _: () = assert!(R_MAX < 1.0);
const _: () = assert!(D_MIN > 0.0);
const _: () = assert!(Z_MAX > 0.0);
const _: () = assert!(KAPPA < 1.0);
const _: () = assert!(LAMBDA < 1.0);
const _: () = assert!(CHI2_MAX > 0.0);
const _: () = assert!(GROUP_SAMPLES > 0);
const _: () = assert!(WINDOW_SAMPLES > GROUP_SAMPLES);
const _: () = assert!(CMD_Q.is_power_of_two());
const _: () = assert!(EVT_Q.is_power_of_two());
const _: () = assert!(TRC_Q.is_power_of_two());
const _: () = assert!(SNAPSHOT_RETRY_MAX > 0);
const _: () = assert!(ENC_FRAME_BITS == ENC_ANGLE_BITS + ENC_MAG_BITS + ENC_CRC_BITS);
const _: () = assert!(ENC_MECH_RPM_MAX > 0.0);
const _: () = assert!(ENC_ZERO_FRAME_LIMIT > ENC_FAIL_LATCH_FRAMES);
const _: () = assert!(ENC_MAG_WARN_LIMIT > ENC_FAIL_LATCH_FRAMES);
const _: () = assert!(ENC_MAX_MISS > 0);
