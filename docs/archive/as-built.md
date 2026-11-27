# as-built：当前树里到底有什么

> 状态：2026-11-27。判据是**文件存在且 `cargo xtask test` / `check` 覆盖**，不是文档承诺。
> 本表只做索引；事实细节仍以 `docs/INDEX.md` 与代码为准。

| 层/模块 | 状态 | 证据 / 备注 |
|---|---|---|
| `software/contracts/src/joint/*` + `software/contracts/src/motor/*` | **已实现** | host 单测；帧布局、状态字、seqlock、线协议换算 |
| `software/contracts/src/motor/config_hw.rs` + `software/contracts/src/joint/config_budget.rs` | **已实现** | 器件事实与预算；`const _` 断言 |
| `software/algorithms/src/motor/{service_gate,service_encoder,chain_foc,chain_svpwm,chain_sense,chain_pid,chain_current,config_motor}` + `software/algorithms/src/joint/{service_fsm,service_supervisor}` | **已实现** | host 单测；无 HIL 数据 |
| `software/algorithms/src/motor/config_policy.rs` | **已实现** | 策略阈值唯一来源 |
| `software/firmware/`（RTIC + embassy，单 bin `main`） | **S0 bring-up** | 编译 + `size` 通过；170 MHz 时钟读回/所有权/PWM safe-off/单次 ADC 采样已写，**未上板**，`PENDING_HIL` |
| `software/bsp/` | **部分实现（T-011/T-012）** | `service_clock`、`bus_pwm` 已实现并被 `main` 使用；引脚按 2026-11-27 网表（nSLEEP PC7 / nFAULT PC8 / 无 BKIN）；`adc`/`encoder`/`dwt` 与 `Board::init` 仍 `todo!()` |
| `software/system/` | **未开始** | 无 crate |
| `software/app/` | **未开始** | 无 crate |
| `Snapshot<T>` 泛型名 | **仅有契约** | 实际是 `FastSnapshot`/`CmdSnapshot`/`GainSnapshot` 三个具体类型 |
| `TraceFrame` / `trace.rs` / `usb.rs` | **计划** | 文件不存在；USB 已从板级需求砍掉 |
| `board.yaml` / `self_test` / `tools/replay` | **未开始** | `docs/INDEX.md` §4-5 |

## 口径

- “已实现”只代表 host 可测逻辑与契约，**不代表上板验证**。涉及电机出力、采样时序、保护动作的结论一律 `PENDING_HIL`。
- “模板”只代表能构建的固件骨架，不含本项目的控制任务。
- 任何新增文件先改本表，再改 `docs/software-architecture.md` 的文件树（H4 依赖后者）。
