# 参考实现清单（草稿 v0.1）

> 用途：把每个子系统落到具体文件，标明「能翻译 / 只能参考 / 必须自己设计」，作为编码工作的输入清单。
> 来源：GitHub 仓库调研（50 个，见 `~/research/repos.jsonl` 与批次文件）；文献结论见 [research-findings.md](research-findings.md)，结构约束见 [architecture.md](architecture.md)。

## 1. 结论先说

- **关节级没有 Rust 实现**：CAN FD + MIT 协议 + 多圈绝对角 + 电流标定冻结，这四块在 Rust 生态里找不到成品。
- **组件级并不少**：FOC 数学、电流采样抽象、角度传感器驱动、CAN/USB 任务骨架、上位机 + ICD，Rust 侧都有可用代码。
- 其他语言里，ODrive / VESC / SimpleFOC / moteus 覆盖了我们需要的全部子系统，但**只有纯算法可以直接翻译**，其余是「借配置参数与结构」。

## 2. Rust 侧现状

| 项目 | 运行时 | 分层 | 可直接借鉴的部分 |
|---|---|---|---|
| `gulitsky/reactance` | RTIC 1.1 + `stm32g4` PAC（**裸寄存器，无 HAL**）| 单 crate + `dsp/` | G4 上跑通整机 FOC 的实证；`dsp/src/transforms.rs` 的变换实现 |
| `qff233/FOC` | embassy（executor + usb + sync）| `crates/foc`（硬件无关）+ `crates/stm32-g474` | **骨架最接近我们**：`foc/src/current_sensor.rs`、`angle_sensor/`（含 mt6818）、`driver/interface.rs`、`pid.rs`、`lowpass.rs`；平台侧 `task/{foc,can_comm,usb_comm}.rs` |
| `okhsunrog/oxifoc` | embassy，core 内 embassy 依赖全部 optional（`runtime` feature，host 可用 tokio）| `oxifoc-core` + `oxifoc-bridge` | `foc/current_sense.rs`、`foc/current_offset.rs`、`foc/detection/`、ICD 与交付策略 |
| `calebfletcher/foc` | 无运行时（纯库）+ 独立 GUI | `src/`（`park_clarke.rs`、`pid.rs`、`pwm.rs`）+ `gui/{gui,icd,firmware}` | **「算法库 + ICD + GUI」三件套的先例**，正好对上我们的 USB 上位机需求 |
| `trouverun/rust-stm32g4` | 平台层在独立 crate | `field-oriented/{commission,control,estimation}` + `bootloader` | 电角度辨识、HFI、弱磁、估计器仲裁 |

共同形状：**纯算法 crate 与平台 crate 分离**。三家独立项目都这么做，我们照做。

## 3. 子系统映射

| 子系统 | 参考实现（文件）| 规模 | 改写性质 | 优先级 |
|---|---|---|---|---|
| 采样触发链 TIM1→ADC→DMA | ODrive `Firmware/MotorControl/low_level.cpp`（15 KB）| 中 | **参考配置**：触发源、采样时刻、DMA 布局按 G491 重写 | P0 |
| 低边三电阻采样 | SimpleFOC `src/current_sense/LowsideCurrentSense.cpp` | 中 | **可翻译**：零偏计算、多采样点、增益换算 | P0 |
| 电流零点/增益标定 | ODrive `MotorControl/motor.cpp`（26 KB）的 current-offset 段 | 小 | **可翻译** 算法，流程需重设计（三 ADC 同步、零和约束）| P0 |
| FOC 数学 | ODrive `foc.cpp`（6.9 KB）、VESC `motor/foc_math.c`；**Rust 已有** `qff233`、`calebfletcher` | 小 | **直接用 Rust 实现**，不要自己推公式 | P0 |
| 编码器 PLL / 偏置 | ODrive `encoder.cpp`（31 KB）、moteus `fw/absolute_encoder_validator.h` | 中 | 结构借鉴；游标绝对角为新增 | P1 |
| 运行状态机 | ODrive `axis.cpp`（23 KB）的 AxisState、moteus `fw/bldc_servo_position.cc` | 中 | **结构借鉴**，语义按我们的 LATENT/ACTIVE/FAULT 重写 | P1 |
| 控制器与前馈 | ODrive `controller.cpp`（18 KB）| 中 | 可翻译（位置/速度/电流串级 + 前馈）| P1 |
| CAN 帧与协议 | ODrive `communication/can/`、`interface_can.hpp`；moteus `lib/` | 小 | **帧格式参考**，实现重写 | P1 |
| 保护策略 | VESC `motor/mcpwm_foc.c` 保护段、SimpleFOC `src/common/base_classes/FOCMotor.cpp` | 小 | **阈值与策略参考**，实现重写 | P1 |
| 配置持久化 | ODrive `nvm_config.hpp` | 小 | 重写（flash + CRC + 版本号）| P2 |
| 上位机 | ODrive `odrivetool`、VESC `bldc-tool`、moteus `tview` | — | 只借 UI 思路与 ICD 概念 | P2 |

**反面教材**：VESC 的 `motor/mcpwm_foc.c` 有 **176 KB**，把 FOC、采样、观测器、保护全部塞进一个文件。我们按 `motor-core` 的模块边界切，不复制这种形状。

## 4. 改写性质的定义

- **可翻译**：输入输出是纯数值，没有 RTOS 假设。翻成 f32、写 host 测试向量，逐条对齐输出。
- **参考配置**：代码里有效的是寄存器配置值和时序约束（触发源、采样窗口、死区、DMA 模式），不是控制流。
- **结构借鉴**：借状态划分、模块边界、错误分类；实现全部重写。
- **自己设计**：没有现成答案（见 §6）。

**禁止逐行翻译。** ODrive 在 FreeRTOS、VESC 在 ChibiOS，它们的 C 代码大量依赖「当前中断优先级下可以做什么」的隐含假设（ADC 中断里直接浮点运算、DMA 半传输中断里读半缓冲）。这些假设搬进 RTIC 全部失效。

## 5. 关键模块契约（草案）

```rust
// motor-core::sense
pub struct RawSample { pub code: [u16; 3], pub vref_mv: u16 }
pub struct PhaseCurrent { pub amps: [f32; 3], pub valid: bool, pub residual_ma: f32 }
pub fn to_amps(raw: &RawSample, cal: &CalibActive) -> PhaseCurrent;

// motor-core::calib
pub enum CalibState { Idle, Collecting, Converged, Frozen, LatchedFault }
pub struct Estimator; // RLS on sum(i)=0
impl Estimator {
    pub fn push(&mut self, cur: &PhaseCurrent, ctx: &SampleCtx) -> Option<CalibLatent>;
    pub fn state(&self) -> CalibState;
    pub fn freeze(&mut self, reason: FreezeReason);
}

// motor-core::foc
pub struct DqTarget { pub id: f32, pub iq: f32 }
pub fn current_loop(i_abc: &[f32; 3], theta_e: f32, target: DqTarget, pi: &mut Pi) -> Duty;

// motor-icd
pub enum Frame { Command(Cmd), Feedback(Feedback), Trace(TraceChunk) }
```

契约写进 `motor-icd` 之后，`motor-core` 的测试只依赖这些结构体，与芯片无关。

## 6. 没有现成实现、必须自己设计的四块

1. **CAN FD + MIT 协议**的关节接口（帧布局、超时、主权切换）。
2. **多圈绝对角**：30/31 齿游标解算 + 上电找零 + 与 FOC 用角的关系。
3. **标定的可信状态机**：何时更新、何时冻结、何种残差算故障——这是毕设主线。
4. **G491 + MP6540H 的具体时序**：EN 与 PWM 的语义差、BKIN 刹车/停机的区别、三 ADC 同步触发。

## 7. 待补

- 论文侧已完成 41 篇归纳，见 [research-findings.md](research-findings.md)；P2（集成驱动片上采样）与部分 2025–2026 方法因 OpenAlex 配额停在 41/60，可换 Crossref/arXiv 补齐。
- `g0_gimbal_motor`（唯一使用 MP6540 的仓库）尚未逐文件读，需补采样与 PWM 配置对照。
- `trouverun/rust-stm32g4` 的平台层运行时待确认。
