# 不二过台账（历史）

> 从 `AGENTS.md` §4 迁出：这里是历史教训，不是每次开工必须读的红线。
> 现行红线与门禁命令见 [`../AGENTS.md`](../AGENTS.md)；新增条目仍先过"没它会失效吗？能合并吗？6 个月后还用吗？"。

| # | 现象 | 根因类别 | 拦截动作 |
|---|---|---|---|
| 1 | 把 `MP6540HGU-A-Z` 读成 PWM+EN 的 H 版 | 流程缺口：靠型号规则猜，未查一手手册 | 料号/引脚/量程只能来自 `board.yaml` 或手册**原文行**；文档结论必须带来源 |
| 2 | 手写 Flash 数字互相矛盾（`hardware-requirements.md` §9 的 64,632/16,080 与实测镜像 17,768 不同，来自另一次工具链） | 流程缺口 | PRE-09 |
| 3 | `stm32-metapac` 的 G4 SRAM 数字错（`g474rb/re` 给 96K） | 工具缺陷 | `software/firmware/memory.x` 手写，**禁止**开 `memory-x` |
| 4 | TIM2 被 embassy `time-driver-any` 占用 | 工具局限 | µs 时基用 TIM5；新增定时器前先查 embassy `build.rs` 的 any→TIM 映射 |
| 5 | debug 构型已占 52 % Flash，全量应用会超 128 K | 真违规：预算未前置 | HIL 出数一律 release；确需 RTT 时 `[profile.dev] opt-level = 2` |
| 6 | RTIC 模板的 25 MHz monotonic 与 G4 默认 HSI16 不符 | 工具局限 | 改时钟树必须同步 `Mono::start(...)` 频率 |
| 7 | `σ̂_e` 的 √3 被双计：预测式里已有显式 √3，实现又用残差 σ（=√3·σ_ch）再乘一次，`pred` 第一项虚高 3 倍 | 真违规：同量纲两种口径混用 | 噪声口径必须写明"单通道 vs 残差方差"：`σ̂_ch=√(sse/(3(weight−2)))` 是单通道，残差方差 = 3σ_ch²，公式里的 √3 只乘一次 |
| 8 | 把 `d = det S/tr²S` 当跨工况阈值（离线实测反序：arc 工况 `d=0.250` 误差 100 pp；单方向+变幅 `d=2.5e-7` 误差 1.40 pp；旋转 `d=4.1e-9` 误差 0.32 pp。实验脚本未入仓，数字来自 2026-11-16 数值实验记录） | 真违规：门限只在单一工况族内标定 | 任何门限必须给"跨工况单调性"证据；`d` 只作**同工况内**的无激励/秩亏门（`D_MIN=0.01`），不作跨工况精度判据 |
| 9 | `#[repr(C, packed)]` + `forbid(unsafe_code)` + `deny(indexing_slicing)` 三者叠加：取字段要 `read_unaligned`（unsafe），按字节索引又被 clippy 禁，实现无路可走 | 设计封死：约束组合自相矛盾 | 线协议优先**自然对齐布局**（字段顺序/填充自己排，`size_of`+`offset_of!` 断言守形状），`packed` 只留给真正省字节且无解包路径的帧 |
| 10 | 契约层混入策略参数：算法超参（`R_MAX`/`KAPPA`/`TARGET_PP`）与容错常数曾和 `D_MAX`、队列深度并列在一个 `limits.rs`，`contracts` 成了"调参表"（2026-11-16 已拆分） | 真违规：层次边界侵蚀 | `contracts` 只放器件事实与电气互锁断言（`motor/config_hw.rs`、`joint/config_budget.rs`）；策略参数进 `software/algorithms/src/motor/config_policy.rs`。review 时见 `software/contracts/src/motor/` 或 `joint/` 出现可调超参即打回 |
| 11 | 字段有初值、却没有设置者：`GainAccum::do2` 曾有默认值 `0.0` 且构造后无写入路径，`pred` 第二项 `(σ̂_e² + δo²)` 退化成 `σ̂_e²`——零位残余不确定度从未生效 | 真违规：不确定度字段两侧未闭合 | 任何"不确定度 / 协方差"类字段必须两侧闭合：有默认值 + 有设置路径 + 有测试证明它非零时行为改变（`do2 > 0` ⇒ `pred` 必须更大）。现行闭合点：`gate.rs` 构造取 `do2_from_half_width(DO_LSB_HALF_WIDTH)`、`set_zero_uncertainty_lsb` 重算、`pred` 第二项使用 |
| 12 | 容错常数写成裸整数却与物理量耦合：原 `ENC_MAX_MISS = 5` 折算 `(5+1) × 45 LSB × 14 极对 × 360 / 16384 ≈ 83°` 电角，"5 帧"无法解释、改转速时也不知道要不要改 | 真违规：时间窗被截成拍数 | 凡是"能容忍多久 / 多久算丢多久"的常数必须由时间窗或物理界限派生，并配编译期断言；现行做法见 `software/algorithms/src/motor/config_policy.rs`：`ENC_MAX_MISS=1` 由 `THETA_E_MAX_DEG=30` 与 `MAX_BLIND_US` 反解，文件末尾两条 `const _` 守混叠底线与电角度底线 |
| 13 | `Trusted = 0` 让全零快照/状态字被误读成可信 | 真违规：默认值即有效值 | 线协议枚举必须有 `Unknown = 0`；空快照测试 `empty_gain_snapshot_is_not_trusted` 守住 |
| 14 | `space_vector_pwm` 由调用方传 `max_duty`，可绕过 `D_MAX` | 设计缺口：输出边界不在唯一处 | 输出函数内部从 `hw::PWM_ARR`/`hw::D_MAX` 算上限并返回 `ClampedModulation`；调用方不再持有满量程 |
