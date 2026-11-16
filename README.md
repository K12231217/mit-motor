# 基于 STM32G491 与 MP6540HA 的关节驱动器设计及三相电流可信自校准

本科毕业设计。做一个关节驱动器，核心是把三相电流采准：低边比例电流采样 + 零和约束下的相对增益自校准，用外部电流基准验证，校准冻结与故障安全处理。

## 硬件

- MCU：STM32G474RBT6（LQFP64，128K Flash / 128K SRAM；5 个 ADC 中引出 3 个用于相电流；同封装的 RC/RE 是仅差 Flash 的升级路径）
- 驱动：MPS MP6540HA 三相集成驱动（5.5–50 V，内部低边 SOx 比例电流输出）
- 电机：WK4310 无框电机，14 对极
- 减速：单级行星 8:1
- 编码：双 MT6701（电机端 + 辅助端，30/31 齿游标）
- 机械：YHorizon-JM 开源关节结构（仅采用机械设计）

## 研究主线（方案 V1.1，2026-11-08 定稿）

1. **驱动系统**：G491 + MP6540HA 驱动器，FOC / MIT 控制。
2. **自校准**：低边比例电流采样模型 + 零和约束下的相对增益自校准，多方向、多幅值激励，用外部电流基准验证。
3. **校准冻结与故障安全**：测量异常时先检查再更新、冻结再诊断；确认与安全相关的传感器故障锁存停机，不用预测电流/角度继续运行。

## 边界（不做什么）

- 不做绝对共模增益恢复（零和只能辨识相对增益，绝对电流和力矩仍需外部基准）
- 不做全故障覆盖、热估计、精确动态力矩测量
- 不做双编码器故障检测、停机电流路径辨识（非主线内容）

## 研究方法

按 ScientistTwo 式的研究闭环做（见 issue #1）：提假设 → 做实验 → 消融对比 → 评审。循环是方法，全自动不是目标：硬件在环实验、物理真实性确认、论文评审都要人来做。

## 开发入口

所有构建/检查/烧录动作只有一处实现：`cargo xtask <cmd>`（`xtask/src/main.rs`）。
CLion、VS Code、CI 都调它，IDE 配置里不写逻辑。

```sh
cargo xtask check        # cargo check
cargo xtask rel          # release 构建（验收构型）
cargo xtask size         # 打印 .text/.rodata/.data/.bss + 镜像大小与 memory.x 预算占比
cargo xtask clippy       # -D warnings
cargo xtask test         # cargo test --workspace（根 workspace 的 host 单测）
cargo xtask arch         # 架构不变量 I1/I2/I3（纯 Rust）
cargo xtask fmt-check    # CI 用
cargo xtask flash        # probe-rs runner（芯片名在 firmware/.cargo/config.toml）
cargo xtask rtt          # probe-rs attach，读 defmt/RTT
```

工具链由 `rust-toolchain.toml` 固定（`nightly-2025-11-05` 的 **windows-gnu** 主机工具链，
因为本机没有 VS Build Tools；Linux/macOS 上用 `RUSTUP_TOOLCHAIN=stable` 覆盖）。

## 状态

- 2026-11-08：方案 V1.1 定稿
- 2026-11-15：主控定稿 **G474RBT6**（LQFP64，128K Flash / 128K SRAM，与 PCB14 的 G491RET6 同封装、引脚功能逐脚兼容；RC/RE 同脚位仅差 Flash，是零改板升级路径）；驱动确认为 MP6540HA（`MP6540HGU-A-Z`，HS/LS 六输入）。V2 板需求与验收见 [docs/hardware-requirements.md](docs/hardware-requirements.md)，对外包的任务书见 [docs/hardware-taskbook.md](docs/hardware-taskbook.md)，任务书错误清单见 [docs/taskbook-review.md](docs/taskbook-review.md)，交给 agent 开发前还缺什么见 [docs/agent-readiness.md](docs/agent-readiness.md)，固件文件架构（五 crate）见 [docs/software-architecture.md](docs/software-architecture.md)
- 进行中：第一部分——文献与缺口分析（issue #1）
