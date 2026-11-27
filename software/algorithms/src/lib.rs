//! 算法层：运行时算数、有状态、可脱离硬件单测的模块。
//!
//! 接口事实（线上帧布局、状态字位域、限值常量）只在 [`contracts`] 定义一次。
//!
//! - [`motor::service_gate`]：相对增益的四维结构门加 χ²，以及正规方程累加器。
//!   共模增益不可观测，闸门是参数可辨识性的前提（PRE-05）。
//! - [`motor::service_encoder`]：MT6701 24 bit 帧解析、CRC-6、状态位、全 0 护栏、
//!   跳变判据与去抖。`bsp` 只搬字节，判据是纯函数与纯状态机，host 可测。
//! - [`motor::chain_current`]：dq 电流环，PI + 交叉解耦 + 反电势前馈 + d 轴优先电压圆限幅。
//! - [`joint::service_supervisor`]：任务心跳与命令超时的独立监督，纯状态机，锁存后需显式清除。
//! - [`joint::service_fsm`]：状态 × 事件迁移矩阵。纯函数、无时钟；调用方只许调用 [`step`]，
//!   不许自己判断"能不能切"。未标定闭环、运行中切标定、无窗口清故障都在这里被拒。
//! - [`motor::chain_foc`]：Clarke / Park 与逆变换，以及无 libm 的正余弦。幅值不变约定。
//! - [`motor::chain_svpwm`]：αβ 调制量 → 三相占空比（几何扇区 + 中心对齐），移植自 oxifoc。
//! - [`motor::chain_sense`]：12 bit 原始码 → 安培，零位与相对增益来自标定档案。
//! - [`motor::chain_pid`]：电流环 PI，条件积分抗饱和，周期在构造时固定。
//!
//! 状态都在 `GainGate` 与 `EncoderMonitor` 里，无堆、`forbid(unsafe_code)`，禁止
//! `unwrap` / `expect` / 数组下标（见下方属性）。`no_std` 只在非测试构建启用：单测跑
//! std，固件目标是纯 `core`，因此 `sin` / `cos` / `sqrt` 一律不外调
//! （`motor::service_math` 与 `service_gate` 内的 `sqrt_f32`）。

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(
    clippy::doc_markdown,
    clippy::too_long_first_doc_paragraph,
    clippy::doc_lazy_continuation
)]

pub mod joint;
pub mod motor;

pub use crate::joint::service_fsm::step;
pub use crate::motor::chain_current::{CurrentLoop, MotorParams};
pub use crate::motor::chain_foc::{AlphaBeta, AlphaBetaVolts, Dq, SinCos};
pub use crate::motor::chain_pid::{Pi, PiOutput};
pub use crate::motor::chain_sense::SenseParams;
pub use crate::motor::chain_svpwm::{normalize_volts_with_inv, Sector};
pub use crate::motor::config_motor::WK4310H;
pub use crate::motor::service_encoder::{
    angle_rad, jump_limit_lsb, parse, AngleOutcome, EncoderMonitor, EncoderParams, Fault, Frame,
    FrameError, Health,
};
pub use crate::motor::service_gate::{GainGate, GateParams, GateReport};
pub use contracts::Trust;
