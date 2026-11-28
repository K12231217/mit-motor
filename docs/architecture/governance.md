# 架构降熵治理 — 轮次日志与清单

> 流程：`/arch-entropy-governance`。本文件是冷数据（不是控制文件）；控制文件仍是
> `AGENTS.md`(RULES) / `docs/INDEX.md`(SPEC) / `.scratch/handoff.md`(STATE)。
> 所有条目以 `文件:行` 取证；追不到的一律标 `UNVERIFIED`。
>
> 轮次 0 = 只读建图（本文件）。轮次 1 及以后在此追加，格式见文末模板。

## 轮次 0：建图（只读）

- 日期：2026-11-28
- 范围：`D:/STM32_LQY/Rust/mit-motor` 全仓
- 树状态：`main` @ `5b4485f`；工作区干净（仅本文件新增）
- 门禁现状：`cargo xtask test` PASS（host 136 个 `#[test]`，`grep -c '#\[test\]'`），
  `cargo xtask check` PASS。`bsp` / `firmware` 无 host 测试（`grep '#\[test\]' software/bsp/src` 为空）。

### 1. 架构地图

| crate / 层 | 路径 | 职责 | 公开接口（入口） | 所有者 | 状态 |
|---|---|---|---|---|---|
| `contracts` / `lib` | `software/contracts/src` | 接口事实唯一来源：线协议、状态字、快照、设备常量 | `lib.rs` re-export；`joint::*`、`motor::*` | `lib` | 全实现，44 个 host 测试 |
| `algorithms` / `lib` | `software/algorithms/src` | 纯算法、可 host 测；依赖仅 `contracts` | `joint::{service_fsm,service_supervisor}`、`motor::{chain_*,service_*}` | 见 §3 | 全实现，92 个 host 测试 |
| `bsp` / `bsp` | `software/bsp/src` | 唯一 `unsafe` 层；外设配置与所有权 | `Board`、`motor::{bus_pwm,bus_adc,bus_encoder,service_clock,service_dwt,service_timebase,service_trace}` | 见 §3 | clock/pwm/adc/dwt/timebase/trace 已实现；`bus_encoder` 与 `Board::init` 为 `todo!()` |
| `firmware` / `app` | `software/firmware/src` | S0/电流环 bring-up 装配（独立 workspace，单 bin） | `bin/main.rs`（RTIC app）、`bin/trace/mod.rs`（录波环） | `bringup`/`current`/`monitor` | 已 HIL（T-015）；**不是最终 `app` 层** |
| `xtask` / 工具 | `software/xtask/src` | 门禁唯一入口（check/test/fmt/clippy/rel/size/arch/doc/hygiene/tier） | `main.rs` 子命令分发 | host 工具 | 无测试；`arch` 只实现 I1/I2/I3/I7 |

未建：`software/system`、`software/app`（`docs/software-architecture.md` §2 目标位）。按
`AGENTS.md` §2，`system`/`app` 未建时不得把相关检查报成 PASS。

### 2. 依赖图

```
contracts ──┬──> algorithms ──┐
            └──> bsp ─────────┤
                               ├──> firmware (bin, RTIC + embassy-stm32)
xtask (host, 无 crate 依赖，shell 调 cargo/llvm)   rtic, rtic-sync, rtic-monotonics
```

| 源 → 目标 | 类型 | 循环 | 允许性 | 证据 |
|---|---|---|---|---|
| `algorithms → contracts` | 编译 | 否 | 允许（I1） | `software/algorithms/Cargo.toml:15` |
| `bsp → contracts` | 编译 | 否 | 允许（I1 未检查 bsp） | `software/bsp/Cargo.toml:19` |
| `firmware → {bsp, algorithms, contracts}` | 编译 | 否 | 允许（装配层） | `software/firmware/Cargo.toml:60-77` |
| `firmware → embassy-stm32::pac`（直接改 RCC/GPIOB/TIM1 SR） | 调用 | 否 | **偏离**：app 层直接碰寄存器 | `software/firmware/src/bin/main.rs:30,79-84,294` |

- I1 只对 `algorithms` 跑 `cargo tree`（`software/xtask/src/checks.rs:74-118`），`bsp` 的依赖方向无检查。
- 根 workspace 只含 `contracts`+`algorithms`（`Cargo.toml:16-19`），`cargo test --workspace` 不编译 `bsp`/`firmware`。

### 3. 状态所有权地图

| 状态 | 声明写者（注册表） | 实际写者 | 读者 | 同步 | 证据 |
|---|---|---|---|---|---|
| TIM1 寄存器 / `nSLEEP` / `nFAULT` | `current @8` | **`bringup @1`** | `monitor @1`（`state()` 只读） | RTIC `Local` 独占所有权 | 声明 `docs/software-architecture.md:66`；实际 `main.rs:79-80,274` |
| ADC1/2/3 注入组 + OPAMP | `current @8` | `current @8` | 同左 | RTIC `Local` | 一致（`main.rs:62,291`） |
| `GainGate` | `calib @2` | **`current @8`** | 同左 | RTIC `Local` | 声明 `docs/software-architecture.md:71`；实际 `main.rs:62,296` |
| `TraceRecorder`（32 KiB 环） | `current @8` | `current @8` 写、`monitor @1` 导出 | 两任务 | `pub static` + 全 `AtomicU32`，单写者 | `trace/mod.rs:253`、`main.rs:311,360` |
| `ISR_COUNT` / `ISR_CYCLES` / `ADC_ERRORS` | 未注册 | `current @8` | `monitor @1` | `static AtomicU32` | `main.rs:46-48,293-294,347` |
| `DWT` / `Timebase` | `current @8` / `init` | `current @8` / `init` | 同左 | 独占 / defmt 时间戳 | 一致 |
| `Snapshot` / `Cmd`/`Event` 队列 | `lib` | 无运行期写者（app 未建） | 无 | 类型级 seqlock/队列 | `contracts/src/joint/protocol_snapshot.rs`、`protocol_events.rs`；**无集成证据** |

> 结论：注册表 owner 列有 3 处与代码不符（TIM1、GainGate、编码器边界），且
> H6 只校验 owner 非空（`hygiene.rs:348-356`），不校验真实性。
> **部分解决（轮次 8）**：state 列与 `todo!()` 已机器对齐；owner 是否是"唯一实际写者"仍需静态分析，未解决。

### 4. 重复实现清单

| 语义 | 实现 A | 实现 B | 差异 | 保留 | 证据 |
|---|---|---|---|---|---|
| SPI1 唯一所有者 | `bsp.motor.bus_encoder` "SPI1/主编码器唯一所有者；只搬字节" | `lib.motor.service_encoder` "SPI1 唯一所有者；帧判据纯函数" | 两行同时声称"唯一所有者"，物理上互斥 | 按层拆分：bsp 拥有总线字节搬运，algorithms 拥有帧判据与状态 | `docs/software-architecture.md:65,71` |
| 整机装配 | `bsp::Board::init()`（`todo!()`） | `main.rs` 手工按序装配 clock→trace→pwm→adc | 两条装配路径，只有一条可运行；`Board` 无人调用 | 删除未用的 `Board`/`init`，或让 main 改用它（二选一，见 TASKS） | `bsp/src/lib.rs:24-41`、`main.rs:100-260` |
| 器件数值事实 | `contracts/src/motor/config_hw.rs`（唯一真源） | `docs/INDEX.md` §1 复写 170 MHz / ARR=4250 / 500 ns / 0.2712 V·A⁻¹ / 2.25 mA/LSB | 违反 PRE-07"文档只引编号不复写数值"；改常量不会让文档编译失败 | 文档只留指针 | `docs/INDEX.md:13,15,16` |
| "0 A = 2048" DAC 零点 | `service_trace.rs:20`（`DAC_MID_CODE = 0x0800`） | 注释复写：`service_trace.rs:6,109`、`main.rs:43`、`trace/mod.rs` 隐含 | 无代码重复，仅注释三处 | 可接受，低危 | 同左 |
| 失败表达 | `Adc::init/take_sample → Option` | `Pwm::wake/enable → Result<(), PwmFault>`；`Board::init → 不可失败签名`；main 里 `loop { spin }` 故障码 | 三套语义并存 | 统一为"构造点 `Result`，热路径 `Option`/内联判定" | `bus_adc.rs:76,120`、`bus_pwm.rs:193,219`、`main.rs:118-123` |
| seqlock 发布序列 | `FastSnapshot::write` | `CmdSnapshot::write` / `GainSnapshot::write` | 三份逐字相同的 `seq` 置奇→payload→置偶；只有 decode 目标不同 | 私有 `Snapshot<V, N>` 一份 + 三个薄包装（轮次 6 已合并） | 轮次 5 前 `protocol_snapshot.rs` 三处 `write` |

证伪记录（怀疑但**未**成立）：`AMPS_PER_LSB` 是 `config_hw::SOX_AMPS_PER_LSB` 的再导出
（`bus_adc.rs:15,26`），非第二份数值；`sqrt_f32` 只有一个实现（`service_math.rs:19`），
`service_gate/math.rs` 只留常量与 `powi`。

未用 `pub const` 审计（轮次 11）：`config_motor.rs` 的 `NOMINAL_*`/`STALL_*`、`config_policy.rs` 的
`AMP_FLOOR_*`/`CALIB_TICK_MS`、`protocol_icd.rs` 的 `LSB_*`/`NODE_ADDR_MIN`/`TTL_MS_NONE` 在当前树
没有调用方，但它们是**铭牌与线协议事实**（PRE-07 的唯一来源），不是重复实现，不删。
唯一删掉的是把 4 bit 编码器字段说成四个独立布尔位的 `ENC1_OK_BIT` 等常量。

### 5. 技术方案并存

| 领域 | 并存方案 | 范围 | 收敛结论 |
|---|---|---|---|
| 运行时 | RTIC 2.4 + embassy-stm32 0.1 混合（ADR-02） | 全局唯一 | 保持；单点风险是 embassy PAC 与 G474 编码不一致（已两次踩坑，见 handoff §3） |
| 时钟 | `service_clock::config()` 里的 PLL 参数 vs `config_budget::SYS_CLK_HZ` | 配置点 / 常量 | 已用 `const _: () = assert!` 连接（`service_clock.rs:17`），可接受 |
| `no_std` 策略 | `contracts` 恒 `no_std`；`algorithms` 仅非 test 启用；`bsp`/`firmware` 恒 `no_std` | 按"能否 host 测"分 | 两类理由成立，保持 |
| 跨优先级通道 | 宪法只认 `Snapshot` + 有界队列；实际还有 `TRACE` 静态原子环 + 3 个 `AtomicU32` 诊断计数 | 诊断 vs 控制 | 需在 PRE-04 明确"诊断通道"例外，或纳入快照（见 TASKS R4） |
| crate 命名 | 现行 `algorithms`/`contracts`；注释仍引用旧名 `user::` | 8 处注释 | 全部改掉，并加 grep 门禁（见 R1） |
| 失败上报 | `defmt::error!` + 死循环故障码（`TP2/PB7` 常高） vs 计划中的 fault dump/状态字 | 现为探针 | 迁移到 `app/joint/service_fault.rs` 时才统一；当前不得报"已验证"以外的结论 |

### 6. 规范偏离清单

| 规则 | 期望 | 实际 | 证据 | 可否机器强制 |
|---|---|---|---|---|
| PRE-07 数值唯一来源 | 文档只引编号 | INDEX §1 复写 5 个常量 | `docs/INDEX.md:13,15,16` | 可（`doc_check` 未落地） |
| PRE-04 两种交叉通道 | 仅 Snapshot/队列 | `TRACE` 静态环 + 控制台计数 | `trace/mod.rs:253`、`main.rs:46-48` | **已解决（轮次 7）**：ADR-16 把只读诊断通道写进 PRE-04 |
| MECH-01 驱动不设全局 static | 无 `static` 驱动 | `pub static TRACE`（数据环，非驱动对象） | `trace/mod.rs:253` | 可（区分驱动/数据） |
| PRE-05 配置期断言 | 器件事实用 `const _: () = assert!` | `deadtime_dtg()` 在运行期调用点执行 `assert!` | `bus_pwm.rs:291-294`，调用 `:149` | 可（`const DTG` 化） |
| MECH-05 注册表 owner 真实 | owner = 唯一实际写者 | TIM1→`bringup`、GainGate→`current`、编码器双声明 | `docs/software-architecture.md:66,71,65` vs `main.rs:79,291` | 难（需静态分析）；当前 H6 只查非空 |
| H6 文件覆盖 | 每个源码文件恰好被一个模块覆盖 | `/mod.rs` 与 `/src/bin/` 一律豁免，且可匹配**任意** ID | `hygiene.rs:393-405`（`is_module_tree_file` 短路返回 true） | 可（收紧规则） |
| 不变量清单 | 文档与实际一致 | 代码实现 I1/I2/I3/I4/I7；旧文档无 I7 | `checks.rs`、`docs/software-architecture.md` §1 | **已解决（轮次 2）**：§1 补齐 I7，I5/I6 标未落地 |
| 模块/命名 | `algorithms`/`contracts` | 注释引用旧 crate `user::` 共 8 处 | `bus_encoder.rs:3`、`protocol_fsm.rs:3,10,49`、`service_supervisor.rs:5`、`config_policy.rs:230`、`config_hw.rs:6,106` | 可（grep 门禁） |
| 文档状态 | 只写当前事实 | `bsp/src/lib.rs:9` 仍称 `bus_adc`/`service_dwt` 是 `todo!()`（已实现） | 同左 | **已解决（轮次 4/8）**：该行已改；注册表 `state` 与 `todo!()` 由 H6 交叉校验 |

### 7. Phase 1：12 域分级

| 域 | 结论 | 依据 |
|---|---|---|
| UI / 设计系统 | N/A | 无 UI |
| 领域模型 | 合规 | `contracts` 单点定义物理量/定点/帧；编译期 `size_of`/`offset_of` 断言 |
| API 契约 | 合规 | 线协议唯一在 `protocol_icd/wire`；`ICD_VERSION` 常量 + 测试 |
| 状态与数据流 | 偏离 | 注册表 owner 3 处不符；诊断通道未入宪法 |
| 数据访问与缓存 | 部分缺失 | `CalibRecord`/flash 接口未定（INDEX §4 待裁决）；缓存无实现 |
| 配置 | 合规 | `config_hw`/`config_budget`/`config_policy` 三层分工 + 编译期自洽断言 |
| 错误处理与重试 | 偏离 | 三套失败语义（§4）；运行期 `assert!` |
| 日志与可观测性 | 偏离 | 无统一 `system/log.rs`；RTT 直出 + 静态计数器；计划位未建 |
| 权限与认证 | N/A | 单机固件 |
| 命名与模块边界 | 偏离 | `user::` 残留；H6 对 `mod.rs`/`bin` 失效；app 直碰 PAC |
| 公共基础设施 | 合规 | `service_math` 单点原语；`xtask` 单入口 |
| 测试与工具链 | 偏离 | bsp/firmware 零 host 测试（允许，但 HIL 是唯一证据）；I4–I6 无检查；`system`/`app`/`replay` 未落地却被文档引用（AGENTS §2 已禁止） |

### 8. 待执行（按总复杂度排序，详见下一轮）

| ID | 主题 | 级别 | 方案 | 默认选择 |
|---|---|---|---|---|
| R1 | 文档/注册表/命名纠偏（8 处 `user::`、3 处 owner、SPI1 双声明、lib.rs 陈旧状态） | A（免 HIL） | (a) 改文档随代码 + 补 grep 门禁；(b) 改代码随文档（Tier B） | (a) |
| R2 | PRE-07 文档数值收敛 | A | (a) INDEX §1 换指针；(b) 落地 `doc_check` 比对常量；(c) 记为例外 | (a)+(c) |
| R3 | `Board::init` vs `main.rs` 双装配 | B（Tier B） | (a) 删 `Board`/`init` 脚手架；(b) 实现并让 main 用它 | (a)（当前无人调用） |
| R4 | 诊断通道入宪法 | A | (a) PRE-04 增"诊断通道"例外并写 ADR；(b) 改为 `Snapshot` | (a) |
| R5 | H6 覆盖收紧（`mod.rs`/`bin` 不豁免） | A | (a) 要求 `bin/*/mod.rs` 也有注册项；(b) 只豁免 `src/**/mod.rs`，`bin` 例外单独列 | (a) |
| R6 | HR 两套引脚表并存（§3 V2 历史 vs §3.1 最新网表） | A | (a) §3 表移入 `docs/archive/`，正文只留 §3.1；(b) 保留双表并加"冲突行"标注 | (a) |
| R7 | I5/I6 落地 | B | (a) 队列有运行期调用方后再加检查与 `dropped` 计数；(b) 先写 `isr_cycles` 预算表 + 上限检查 | (b) 先做 I6 预算 |
| R8 | `deadtime_dtg` 运行期 `assert!` → 编译期常量 | B（Tier B） | (a) `const DEADTIME_DTG: u8 = deadtime_dtg();`；(b) 保持现状 | (a) |
| R9 | `main.rs` 5 处 `trace.freeze(); defmt::error!; loop spin` 故障停机副本 | B（Tier B） | (a) 提取 `halt_with_fault(code)` 单点；(b) 留在 `init` 内联 | (a) |
| R10 | 文档级引脚 lint（`P[A-D]\d+` 只许出现在 HR §3.1） | A | (a) 加 hygiene 规则 + 白名单；(b) 不加，靠 PRE-10 人工检查 | (b) 先评估误报率 |
| R11 | firmware/bsp 注释级纠偏（`TRACE_FLAG_SELFTEST` 悬空 link、`bus_pwm.rs:101` 的 PB10 措辞） | B（Tier B，注释级） | (a) 与 R8/R9 同批做，一次 HIL 签收；(b) 单独做并标 `PENDING_HIL` | (a) |
| R12 | `bsp` 的 fmt/clippy 覆盖 | A | (a) 以 `CARGO_BUILD_TARGET=thumbv7em-none-eabihf --manifest-path software/bsp/Cargo.toml` 跑；(b) 让 firmware 的 `-p bsp` 生效需先并入同一 workspace（会破坏独立 workspace 约定） | (a) 评估构建副作用 |
| R13 | `tier` 对注释级 bsp/firmware 改动也要求 HIL（路径粒度） | A | (a) 行级判断"只改注释则跳过"；(b) 接受摩擦，人决定是否开 hil-pending 分支 | (b) 不用启发式 |
| R14 | `LSB_*` 等线协议常量未被编码函数引用 | A | (a) 编码/解码引用 `1.0/LSB_*`；(b) 保持内联，在常量上注明"文档值" | (b) 不碰 wire 语义 |

状态：R1 完成（轮次 1）；R2/R5 完成（轮次 1/2）；R6 完成（轮次 3）；文档真实性完成（轮次 4）；工具链门禁完成（轮次 5/9）；seqlock 归一完成（轮次 6）；R4 完成（轮次 7，ADR-16/17）；注册表 state 门禁完成（轮次 8）；R12 完成（轮次 9）；架构文档 §4–§10 纠偏完成（轮次 10）；状态字表示归一完成（轮次 11）；检查器自测完成（轮次 12）；内存事实分层完成（轮次 13）；R3 hold（§9-1）；R7/R9–R11、R13/R14 待做。

### 9. 待确认（人）

1. R3：是否现在删除未使用的 `Board`/`init`？`main.rs` 正在使用中，删 `Board` 不影响它。
   **本轮：用户选 `hold`，不动。**
2. R5：`firmware/src/bin/trace/mod.rs`（254 行，含 32 KiB 环）应登记为 `app.*.service_trace` 还是等 `system/trace.rs` 落地再迁移。
   **本轮：用户选 `narrow`，已登记 `app.motor.service_trace` 并把整片前缀豁免改成显式 `BIN_FILES` 白名单。**
3. R1 的 owner 修正方向：以"代码现状"为准（bringup 持有 PWM、current 持有 GainGate），还是以"目标架构"为准（current 持有 PWM、calib 持有 GainGate）并排期迁移。
   **未决。** 本轮只做了无争议的事实纠偏（旧 crate 名、SPI1 双声明、陈旧状态行），owner 行未动。

---

## 轮次 1：命名与注册表事实纠偏

- 熵源：§6 第 1/5/6/8/9 行，§4 第 1 行
- 调用方核实：`grep -rn 'user::' software --include=*.rs` = 8 处 / 5 文件；`grep -rn 'service_encoder\|bus_encoder' docs/software-architecture.md` = 2 行冲突
- 选定方案：文档随代码（1 行事实替换），不做代码迁移
  - `bus_encoder.rs:3` → `algorithms::motor::service_encoder`
  - `config_hw.rs:6,106` → `algorithms::motor::config_policy[::ENC_SPI_HZ]`
  - `protocol_fsm.rs:3,10,49` → `algorithms::joint::service_fsm[::step]`
  - `service_supervisor.rs:5`、`config_policy.rs:230` → `crate::joint::service_fsm`
  - 注册表：SPI1 拆成"bsp 拥有总线 / algorithms 拥有判据"；新增 `app.motor.service_trace`
  - `bsp/src/lib.rs:7-9` 状态行改为当前事实
- 放弃方案：把 PWM/GainGate 写者改成注册表声明（Tier B，需 HIL，且与 `main.rs` 现有任务划分冲突）
- 迁移完成：8/8 注释；2/2 注册表 contract；1 个新注册项
- 删除的旧实现：无（只改措辞与检查规则）
- 防回归约束：
  - H5 新增 `user::` 命中即 FAIL（`software/xtask/src/hygiene.rs:146-153`）
  - H6 的 `/src/bin/` 前缀豁免改为显式 `BIN_FILES` 白名单（`hygiene.rs:398-412`）；新增 bin 子模块必须先登记
- 验证：`cargo xtask hygiene` PASS（H2/H4/H5/H6）；`cargo xtask doc` PASS；`cargo xtask test` PASS
- 未决项：R1 owner 方向（§9-3）；R2 文档数值收敛；R4 诊断通道

---

## 轮次 2：不变量从"文档声称"变成"命令可失败"

- 熵源：§6「不变量清单」行、「PRE-07 数值唯一来源」行；§7「测试与工具链」行
- 调用方核实：
  - I4 的检查写在 `docs/software-architecture.md:15`，指向 `software/app/src/tasks/current.rs`；该路径不存在（`ls software/app` 为空），等于不变量零覆盖。
  - I7 只在 §9 出现一行（`docs/software-architecture.md:386`），§1 表里没有它，AGENTS §2 仍写"arch 只实现 I1/I2/I3"。
  - `software/xtask/src/checks.rs:78` 注释仍写旧 crate 名 `user`。
  - INDEX §1 复写 5 个常量 + 4 组引脚 + 4 个 HIL 数字（PRE-07/PRE-10 双违反）。
- 选定方案：
  - `check_isr_purity`（I4）：从 `#[task(binds = TIM1_UP…)]` 之后按花括号配对截出 `fn current` 函数体，查 `defmt`/`.await`/`.lock(`/`unwrap(`/`expect(`；锚点找不到即 FAIL。
  - §1 表补 I7，I5/I6 显式标 `未落地`，I1–I4/I7 写明是哪条命令。
  - INDEX §1 的数值/引脚/HIL 数字全部换指针（`HSE_HZ`、`SYS_CLK_HZ`、`PWM_ARR`、`SOX_AMPS_PER_LSB`、HR §3.1、handoff §3）。
- 放弃方案：
  - (a) 给 `bsp` 加 `cargo tree -p bsp`：`software/bsp` 是 excluded 独立 workspace，`cargo tree --manifest-path` 会生成 `software/bsp/Cargo.lock` 这一新文件，副作用大于收益。
  - (b) 给 INDEX 数字加 `doc_check` 比对：需要第二份"文档数值 ↔ 常量"映射表，把重复从文档搬到工具，Deletion Test 不通过。
- 行为基线：改前 `cargo xtask arch` PASS（I1/I2/I3/I7）；改后 PASS（I1/I2/I3/I4/I7），I4 报 `main.rs:292–344`。
- 迁移完成：I4 指向真实 ISR；I7 进 §1；INDEX 5 行换指针；AGENTS §2 命令描述与"未落地"清单同步。
- 删除的旧实现：INDEX §1 的常量副本（170 MHz/ARR/0.2712 V·A⁻¹/2.25 mA·LSB⁻¹）、引脚副本、HIL 数字副本。
- 防回归约束：`check_isr_purity` + `isr_body` + `strip_strings`（`checks.rs`）；ISR 改名会让 I4 FAIL 而不是静默跳过。
- 反证：临时在 ISR 首行插入 `defmt::info!("falsify")` → I4 FAIL 并定位 `main.rs:293`；还原后 `git diff` 为空。
- 验证：`arch`/`fmt-check`/`clippy`/`test`/`hygiene`/`doc` 全 PASS（命令原文见提交回执）。
- 未决项：I5/I6 仍未落地；HR §3（V2 历史表）与 §3.1（最新网表）两套引脚表并存；`Board::init`；`deadtime_dtg` 运行期 assert。

---

## 轮次 3：引脚真源收敛为一张表（HR §3.1）

- 熵源：§6「PRE-10 数值/契约唯一真源」行；§8 R6
- 调用方核实：
  - `docs/hardware-requirements.md` §3（V2 表 24 行）+ §3.1（覆盖表 13 行）对同一功能给出不同答案：nSLEEP（PC5 vs PC7）、nFAULT（PB10+PC9 vs PC8）、相电流（PA1/PA7/PB0 vs PB0/PA7/PA3）、编码器片选（PB7/PB9 vs PC12/PC10）。
  - INDEX §4 与 `.scratch/handoff.md` §4 各有一份 `SOA→PB0` 副本（PRE-10 违反）。
- 一手材料：`hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel` 的 `$NETS` 段（U1 脚 ↔ 网络）+ `docs/DS_stm32g474rb/Pinouts_and_pin_description.md` Table 12 的 LQFP64 列（脚号 → PXn / 复用 / 附加功能）。
- 选定方案：§3 = 出处与指针；§3.1 = 唯一完整表（17 行，含"未连接"与"待确认"）；V2 表逐字移入 `docs/archive/pin-map-v2.md`，与 §3.1 冲突的行作废。
- 放弃方案：(a) 保留双表 + 冲突标注 —— 读者仍要逐行判断哪条有效；(b) 只写脚本从网表生成表 —— 脚号→PXn 仍需 DS 解析器，本轮人工逐行复核成本更低（脚本化记为 R10 的延伸）。
- 新发现（一手，未改代码）：`ADC12_IN6` 网络只有 U1.11 一个节点，无分压/热敏；LQFP64 脚 11 = PC3，附加功能 `ADC12_IN9`/`OPAMP5_VINP`。NTC 既未接、通道名也不存在 → 写进 HR §8-9 与 INDEX §4，标待确认。
- 行为基线：纯文档；门禁输出不变。
- 迁移完成：HR §3 双表合并；INDEX §4 引脚副本改指针；handoff §4 引脚副本改指针。
- 删除的旧实现：HR §3 的 V2 表；§3.1 的"覆盖上表"语义（已无被覆盖对象）。
- 防回归约束：未加机器检查（`P[A-D]\d+` 在代码注释里大量出现，误报高，见 R10）。
- 验证：`test`/`check`/`hygiene`/`doc` 全 PASS（提交回执）。
- 未决项：NTC 通道（HR §8-9）；R10 引脚 lint 误报率；`Board::init`。

---

## 轮次 4：文档只写当前树（architecture / instrumentation）

- 熵源：§6「文档状态」行；§6「PRE-10 引脚唯一真源」行
- 调用方核实：
  - `docs/software-architecture.md:5` 把落地状态指向 `archive/as-built.md`，而该文件仍写 `adc`/`dwt` 是 `todo!()`（`archive/as-built.md:13`）。
  - §2 的 bsp 状态行停在 T-012（漏 adc/dwt/timebase/trace）；`bus_encoder` 写成"SPI1 双 CS + MT6701 读帧"，实际是 `todo!()` 骨架（`bsp/src/motor/bus_encoder.rs:9`）。
  - §2 的 app crate 路径写成 `src/app/joint/...`，与四轴投影 `<crate>/src/<domain>/<role>.rs` 不符（7 行）。
  - `docs/instrumentation.md` §1/§7 复制 8 处引脚；§4.1 声称 `flags` bit7=`SELFTEST`，而代码只有 `TRACE_FLAG_FROZEN`（`trace/mod.rs:47`），`TRACE_FLAG_SELFTEST` 只存在于同一行的 doc 注释里。
- 选定方案：文档只写"当前树 + 信号名"；脚位一律回 HR §3.1；flags 只写代码里存在的位；历史 `0x81` 标注为旧自检。
- 放弃方案：
  - (a) 顺带修 `trace/mod.rs` 的悬空 intra-doc link —— firmware 属 Tier B，注释级改动也要 HIL 证据，留到 R11 与 R8/R9 同批；
  - (b) 写脚本比对文档 flags 位与代码 —— 单点字符串，维护成本高于收益。
- 行为基线：纯文档；门禁输出不变。
- 迁移完成：architecture §2 的 bsp/system/app/firmware 四块状态、app 路径、旧 crate 名；instrumentation §1/§4.1/§7 与排障段落的引脚/flag 表述。
- 删除的旧实现：`as-built.md` 状态指针；instrumentation 的 8 处引脚副本与 `SELFTEST` 声明。
- 防回归约束：无（自然语言里的状态/引脚无法低成本机器校验；PRE-10 仍是人工 grep，见 R10）。
- 验证：`test`/`check`/`hygiene`/`doc` PASS（提交回执）。
- 未决项：`trace/mod.rs:50` 悬空 link（R11）；`bus_pwm.rs:101` 的 PB10 措辞（R11）。

---

## 轮次 5：门禁工具自己也过门禁

- 熵源：§4「重复实现清单」新增项；§7「测试与工具链」行
- 调用方核实：
  - `collect_rs` 在 `checks.rs:183` 与 `hygiene.rs:501` 函数体逐字相同；路径显示 `.replace('\\\\', "/")` 在 checks 出现 2 次、hygiene 封装 1 次。
  - 在 `software/xtask` 直接跑 `cargo fmt --all -- --check` 有 1 处 diff（`checks.rs` 的 `isr_body`），`cargo clippy --all-targets -- -D warnings` 有 2 个错误（`question_mark`、`useless_format`）。
  - 根因：`cargo xtask fmt-check` / `clippy` 只跑 firmware 与根 workspace，`software/xtask` 是独立 workspace，无人检查。
- 选定方案：新建 `software/xtask/src/fs.rs`（`collect_rs` + `rel`），删掉两份副本；`fmt`/`fmt-check` 增加根 workspace 与 xtask 两跳；`clippy` 增加 xtask 一跳；修掉既有 2 个 lint 与格式偏差。
- 放弃方案：把 `bsp` 一起纳入 clippy —— thumb crate 需要目标配置，`--manifest-path software/bsp` 会另生 lock 并需 `CARGO_BUILD_TARGET`，本轮风险大于收益（记 R12）。
- 行为基线：合并前后 `arch`（I1/I2/I3/I4/I7）、`hygiene`（H2/H4/H5/H6）、`test`（136）输出一致。
- 迁移完成：`checks.rs`/`hygiene.rs` 的走查与路径显示；`main.rs` 三处命令分发与 help。
- 删除的旧实现：`checks.rs:collect_rs`、`hygiene.rs:{collect_rs, display}`。
- 防回归约束：`fmt-check`/`clippy` 现在覆盖 `software/xtask` 本身；工具里的格式/lint 退化会直接 FAIL。
- 验证：`fmt-check`/`clippy`/`arch`/`hygiene`/`test`/`doc` 全 PASS（提交回执）。
- 未决项：`bsp` 的 fmt/clippy 覆盖（R12）；门禁命令仍无自测（`xtask` 零测试）。

---

## 轮次 6：seqlock 协议只实现一次

- 熵源：§4「seqlock 发布序列」行；§3.1 文档与代码不一致
- 调用方核实：
  - `protocol_snapshot.rs` 里 `FastSnapshot::write`、`CmdSnapshot::write`、`GainSnapshot::write` 函数体逐字相同；三个 `empty()` 同形；`read_once/read_retry` 只差 `decode` 目标（读核心 `read_words` 早已共享）。
  - 公开调用方：当前为零（`system`/`app` 未建），改动只影响 contracts 自己的测试。
  - `docs/software-architecture.md` §3.1 写"两个方向各一个 `Snapshot<T>`"，实际是三个视图（上行拆 Cmd/Gain 保单写者）。
- 选定方案：私有 `Snapshot<V, const N: usize>` + `ViewWords<N>` trait；公开类型收敛为三个薄包装，各自保留"谁能重试"的 API 约束；补三条 `size_of` 断言钉死槽大小。
- 放弃方案：把 `CmdSnapshot` 直接别名成 `Snapshot<CmdView, CMD_WORDS>` —— 会让上行方向意外获得 `read_retry`，与"该方向重试必然拿到同一个 `None`"的既有论证冲突。
- 行为基线：改前 `cargo test -p contracts` 44 PASS；改后仍 44 PASS，golden words（`golden_words_pin_each_view_field`）未变 ⇒ 线格式与 word 布局不变。
- 迁移完成：三个快照类型、6 个 seqlock 测试用例的字段访问（`snap.0.seq`）。
- 删除的旧实现：两份 `write` 副本、两份 `empty` 副本、三处 `read_words` 调用样板。
- 防回归约束：`size_of::<{Fast,Cmd,Gain}Snapshot>() == 4 + 4·N` 三条编译期断言；44 个 host 测试（含 odd-seq/重试上限/不撕裂）。
- 验证：`test`/`fmt-check`/`clippy`/`doc`/`arch`/`hygiene` 全 PASS（提交回执）。
- 未决项：无（`Snapshot` 的 M4 内存序论证仍只在注释里，需 LA 实测才能说"已验证"）。

---

## 轮次 7：宪法追上代码（ADR-16 诊断通道 / ADR-17 seqlock 槽）

- 熵源：§5「跨优先级通道」行（诊断通道未入宪法）；§6「PRE-04」行；§3.1 文档声称"不用 `const N`"而轮次 6 已用
- 调用方核实：
  - `AGENTS.md:26` PRE-04 只认 Snapshot/队列，而 `trace/mod.rs` 的 2048×16 B 静态环与 `main.rs` 三个 `AtomicU32` 一直在用第三条路。
  - `docs/software-architecture.md:257` 原写"**不用 `const N` 泛型参数**——避免同一份代码在不同 N 上被单态化多次"，与轮次 6 的实现相反。
  - 快照当前无生产调用方（`grep -rn "FastSnapshot\|CmdSnapshot\|GainSnapshot" software` 只命中 contracts 自身与 re-export），因此单态化产物为零。
  - `AGENTS.md:24` PRE-02 的检查路径仍指向不存在的 `software/app/src/tasks/current.rs`。
- 选定方案：ADR-16 把诊断通道写进 PRE-04（四条条件 + 现有实例）；ADR-17 把 `Snapshot<V, N>` 的取舍写清楚，并把"单态化成本"交给 `cargo xtask size` 在 `app` 落地后裁决；PRE-02 的检查改指 I4 的真实锚点。
- 放弃方案：(a) 把 TRACE 环改造成 `Snapshot` —— 读者要历史窗口，不是最新值，语义不对；(b) 为单态化提前改成"非泛型超集 + 定长切片" —— 当前无调用方，属于为假想成本付复杂度。
- 行为基线：纯规则/文档；`test`/`arch`/`hygiene` 输出不变。
- 迁移完成：PRE-04 例外、PRE-02 检查路径、架构 §3.4、§3.1 的 `const N` 决策、ADR 索引 16/17。
- 删除的旧实现：architecture §3.1 的"不用 const N"结论（被 ADR-17 取代）；`protocol_snapshot.rs` 里重复的三条 size 断言。
- 防回归约束：无新增机器检查；诊断通道仍靠注册表 + 人工复核（可机器化的只有"静态环必须有 owner"，已在 H6）。
- 验证：`test`/`fmt-check`/`clippy`/`doc`/`arch`/`hygiene` 全 PASS（提交回执）。
- 未决项：`ISR_COUNT/ISR_CYCLES/ADC_ERRORS` 没有独立模块条目（寄生在 `app.motor.task_bringup`），是否升为 `app.motor.service_diag` 待 `app` 落地。

---

## 轮次 8：注册表的 state 列与 `todo!()` 机器对齐

- 熵源：§6「MECH-05 注册表 owner 真实」行、「文档状态」行；§7「测试与工具链」
- 调用方核实：
  - `grep -rn "todo!()" software --include=*.rs` 命中 2 个真实桩：`bsp/src/lib.rs:39`（`Board::init`）、`bsp/src/motor/bus_encoder.rs:12`（`read_raw`）。
  - 注册表 34 行原本只有 id/path/owner/contract 四列，H6 只校验 owner 非空，**无法回答"这个模块是不是桩"**；`docs/software-architecture.md:5` 又刚把状态指针从 `as-built.md` 移走，状态一度只剩 handoff 散文。
- 选定方案：注册表加 `state` 列（`impl`/`scaffold`），H6 与文件内容交叉校验：标 `impl` 却有 `todo!()`、或标 `scaffold` 却没有，都 FAIL；文档注释里提到 `todo!()` 不算。
- 放弃方案：(a) 把 `todo!()` 当作 TODO 标记让 H2 检查 —— 与仓库"空壳脚手架 + Worker 填函数体"的流程冲突，脚手架本来就是合法状态；(b) 另建 `STATUS.md` —— 控制文件已限定三个，状态属于模块属性，放注册表最省。
- 行为基线：改前 `cargo xtask hygiene` PASS（34 模块 / 42 文件）；改后 PASS（34 模块 / 42 文件，scaffold 2）。
- 迁移完成：注册表 34 行加 state；两个 scaffold 行；H6 解析（5 列）与 `todo_state` 扫描；AGENTS 命令描述。
- 删除的旧实现：无（新增约束）。
- 防回归约束：H6 的 state↔`todo!()` 交叉校验；新增模块必须同时声明 state。
- 反证：把 `bsp.motor.bus_encoder` 临时改成 `impl` → H6 FAIL 并点名该文件；还原后 PASS。
- 验证：`fmt-check`/`clippy`/`test`/`doc`/`arch`/`hygiene` 全 PASS（提交回执）。
- 未决项：owner 是否等于"唯一实际写者"仍无静态检查；`ISR_*` 计数器与 TRACE 的 state 由其宿主模块代表。

---

## 轮次 9：`bsp` 也进 fmt/clippy（四个 workspace 全过门禁）

- 熵源：§7「测试与工具链」行；R12
- 调用方核实：`software/bsp` 是独立 thumb workspace，自带 `.cargo/config.toml`（target=thumbv7em-none-eabihf）与 `.gitignore`（忽略 `target/`、`Cargo.lock`）；`cargo xtask clippy` 此前只跑 firmware / 根 / xtask，bsp 的 8 个源文件无人检查。
- 可行性实测：`cd software/bsp && cargo clippy -- -D warnings` PASS（不另生锁文件，`.gitignore` 已覆盖）；`cargo fmt --manifest-path software/bsp/Cargo.toml --all -- --check` 也 PASS。`--all-targets` 会拉 host 测试装置并 `can't find crate for test`，故 bsp 只查 lib。
- 选定方案：`bsp_cargo` 一跳，接在 `fmt`/`fmt-check`/`clippy` 末尾。
- 放弃方案：把 bsp 并入 firmware workspace —— 会破坏"独立 workspace 才能保证 `cargo build` 用 thumb 目标"的既有约定（`Cargo.toml` 顶部注释）。
- 行为基线：改前 `fmt-check`/`clippy` PASS（三个 workspace）；改后仍 PASS（四个）。
- 迁移完成：`main.rs` 命令分发、help、`BSP` 常量。
- 删除的旧实现：无（补覆盖）。
- 防回归约束：bsp 的格式与 lint 退化现在会让 `cargo xtask fmt-check` / `clippy` 直接失败。
- 验证：`fmt-check`/`clippy`/`test`/`doc`/`arch`/`hygiene` 全 PASS（提交回执）。
- 未决项：`bsp` 仍无 host 测试（no_std + 寄存器，属 HIL 域）；`cargo xtask check`/`build` 仍只覆盖 firmware。

---

## 轮次 10：架构文档 §4–§10 只写当前树（去掉第二份门禁清单）

- 熵源：§6「不变量清单」行残留、§7「测试与工具链」；文档自述"v0.1 只定契约不落实现"
- 调用方核实：
  - §9 用 9 条 shell 命令复写 `checks.rs`，其中 `cargo tree -p system`（无此 crate）、`software/app/src/tasks/current.rs`（不存在）、`llvm-nm target/.../app`（文件名不对）三条无法执行；I7 的实现函数名从未出现。
  - §4 表把 `FDCAN2 / USART3 / USB FS` 列为在用外设；HR §3.1 的最新网表是 FDCAN1、USART3 本季不启用、本板无 USB。
  - §6 任务表写 `TIM1_BRK`，而本板无 BKIN（HR §3.1）；`current` 绑定写"ADC DMA 完成"，现实现是 TIM1_UP。
  - §7 写 `.uninit.trace` 1024 帧/16 KB，与已裁决的 2048 帧/32 KiB 冲突；`.uninit.log/.magic`、BKPR 无代码却未标计划。
  - §10 三条"未定项"全部已关闭（注入组、embassy 初始化、内联 `tests.rs`）。
  - 实测 unsafe：`bsp` 只有 `service_dwt.rs:34` 一个块，标题"两处 unsafe"不成立。
- 选定方案：§9 改成"不变量 → 实现函数 → 扫描范围"的指针表，并明确不复写命令行；§4/§6/§7 按 HR §3.1 与已裁决项纠正；§10 改为指针；标题升 v0.3 并删掉"不落实现"。
- 放弃方案：给 §9 的 9 条命令加"已废弃"标注 —— 保留了第二份清单，迟早再次漂移。
- 行为基线：纯文档；`arch`/`hygiene`/`test`/`doc` 输出不变。
- 迁移完成：§4、§6、§7、§9、§10、文档头。
- 删除的旧实现：§9 的 9 条 shell 复写、§10 的三条已关闭未定项、§7 的 1024 帧方案。
- 防回归约束：无新增机器检查；§9 的指针表由人工对照 `checks.rs`（可机器化的部分已在 `arch` 本身）。
- 验证：`test`/`fmt-check`/`clippy`/`doc`/`arch`/`hygiene` 全 PASS（提交回执）。
- 未决项：§5 的 `system` 边界（crate 未建）仍按计划保留；§6 的 `idle 不可达` 等 RTIC 细节待 `app` 落地复核。

---

## 轮次 11：删掉状态字的第二套表示，并做一次"未用 pub const"审计

- 熵源：§4 证伪记录扩展；PRE-07（同一语义只有一种表示）
- 调用方核实：
  - `grep -rn "ENC1_OK_BIT\|ENC1_FAULT_BIT\|ENC2_OK_BIT\|ENC2_FAULT_BIT"` 全仓只命中定义行，零调用。
  - 这四位与 `ENCODER_SHIFT`/`ENCODER_MASK`/`StatusWord::encoder_bits()` 描述同一 4 bit 字段，且语义冲突（字段是**一个码**，不是四个布尔位）。
  - 批量扫描 `pub const` 的零引用项：另外 22 个（`NOMINAL_*`、`LSB_*`、`AMP_FLOOR_*` 等）是铭牌与线协议事实，保留。
  - `algorithms/src/lib.rs:20` 仍说 `sqrt_f32` 在 `service_gate` 内（实际已上移 `service_math`）；`contracts/src/lib.rs:15` 仍说跨优先级"只有"两种通道（ADR-16 已有例外）。
- 选定方案：删四个 `ENC*_BIT`，在 `ENCODER_MASK` 上写明"4 bit 码，不要加位别名"；两处 crate 根文档改为当前事实。
- 放弃方案：保留四位并加编译期断言把它们与 `ENCODER_SHIFT` 对齐 —— 等于给错误表示发合法性证书。
- 行为基线：纯常量/文档删除；`test`（44+92）与 golden words 不变。
- 迁移完成：`protocol_faults.rs`、`algorithms/src/lib.rs`、`contracts/src/lib.rs`。
- 删除的旧实现：`ENC1_OK_BIT`、`ENC1_FAULT_BIT`、`ENC2_OK_BIT`、`ENC2_FAULT_BIT`。
- 防回归约束：无新增机器检查（`ENCODER_MASK` 的注释是唯一防线）；删除后无引用即可编译期证明无调用方。
- 验证：`test`/`fmt-check`/`clippy`/`doc`/`arch`/`hygiene` 全 PASS（提交回执）。
- 未决项：`LSB_*` 等未用常量是否应被编码函数引用（当前函数内联了同等比例），改动会触碰 wire 语义，另开任务（R14）。

---

## 轮次 12：门禁检查器自测（I4 锚点 / H6 格式与 state）

- 熵源：§7「测试与工具链」行（`xtask` 零测试）
- 调用方核实：
  - `isr_body` 是手写花括号配对 + 字符串剥离；`parse_module_registry` 是手写 5 列解析；`todo_state` 用手写行首判定。三者任一放宽，正确的代码仍 PASS、错误的代码也 PASS。
  - `cargo xtask test` 此前只跑根 workspace，`software/xtask` 的测试即使存在也不会被执行（与轮次 5 的 fmt/clippy 漏洞同源）。
- 选定方案：`checks.rs` 加 3 例（锚点缺失、未闭合、字符串里的花括号、`monitor` 必须在区间外）；`hygiene.rs` 加 2 例（4 列必须报错、`//! todo!()` 不算桩而裸 `todo!()` 算）；`cargo xtask test` 追加 xtask 一跳。
- 放弃方案：为一个 60 行的刮擦器引入 `syn` —— 依赖体积与编译时间远超收益，且 ISR 体扫描只服务于一条 lint。
- 行为基线：`cargo xtask test` 从 136 条变 141 条（+5）；根 workspace 条数不变。
- 迁移完成：`main.rs` 的 `test` 分发与 help；两个检查模块的 `#[cfg(test)]`。
- 删除的旧实现：无（补覆盖）。
- 防回归约束：检查器行为回归会让 `cargo xtask test` FAIL；CI 已调用该命令。
- 验证：`test`（141）/`fmt-check`/`clippy`/`doc`/`arch`/`hygiene` 全 PASS（提交回执）。
- 未决项：`check_isr_purity` 的禁用词表（`defmt/.await/.lock(/unwrap(/expect(`）仍是文本匹配，宏展开后的形态不在覆盖内。

---

## 轮次 13：把"128 KB 器件"与"96 K/64 K 链接子集"分开写

- 熵源：§5「数据访问/配置」附近；PRE-09（数字只由工具产生）
- 调用方核实：
  - `docs/INDEX.md:10` 只写"128 KB Flash / 128 KB SRAM"，而 `software/firmware/memory.x` 明确只链接 bank 1（64 KB）与 SRAM1+SRAM2（96 KB），并记录了 dual-bank 地址洞（0x0801_0000–0x0803_FFFF）与"按 128K 连续链接会下载失败"的实测。
  - 两处都是事实，但只说器件容量会让下一位 agent 把 `memory.x` "修正"成 128K 连续，正好踩已知的下载失败。
  - `.scratch/handoff.md` §2 未提本轮治理产出，下一个会话只能靠翻 commit 才知道门禁覆盖面变了。
  - CI 注释仍写 `H2/H4/H5`，漏了 H6。
- 选定方案：INDEX 主控行补一句"链接只用 bank1 64 KB + SRAM 96 KB，理由见 memory.x"；handoff §2 增加治理指针；CI 注释补 H6。
- 放弃方案：把 128 KB 从 INDEX 删掉只留链接子集 —— 器件容量是选型事实，删了会让"换 RE 料号"的判断失去依据。
- 行为基线：纯文档/注释；门禁输出不变。
- 迁移完成：`docs/INDEX.md` §1、`.scratch/handoff.md` §2、`.github/workflows/ci.yml` 注释。
- 删除的旧实现：无。
- 防回归约束：无（数值与链接预算的权威在 `memory.x` 与 `cargo xtask size`）。
- 验证：`test`/`fmt-check`/`clippy`/`doc`/`arch`/`hygiene` 全 PASS（提交回执）。
- 未决项：`cargo xtask size` 的预算检查仍是"打印 vs memory.x 长度"，没有把"不得跨 bank"写成断言。

---

## 轮次记录模板（下一轮起复制）

```md
## 轮次 N：<主题>
- 熵源：<本文件清单条目>
- 调用方核实：<数量、路径>
- 选定方案 / 放弃方案：
- 行为基线：<测试或观测证据>
- 迁移完成：<调用方清单>
- 删除的旧实现：<路径>
- 防回归约束：<lint / 类型 / 契约测试 / xtask 检查>
- 验证：PASS/FAIL + 命令与输出摘要
- 未决项：
```
