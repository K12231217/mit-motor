//! 算法层：运行时算数、有状态、可脱离硬件单测的模块。
//!
//! 接口事实（线上帧布局、状态字位域、限值常量）只在 [`contracts`] 定义一次。
//!
//! - [`gate`]：相对增益的四维结构门加 χ²，以及正规方程累加器。共模增益不可
//!   观测，闸门是参数可辨识性的前提（PRE-05）。
//! - [`encoder`]：MT6701 24 bit 帧解析、CRC-6、状态位、全 0 护栏、跳变判据与
//!   去抖。`1_bsp` 只搬字节，判据是纯函数与纯状态机，host 可测。
//! - [`fsm`]：状态 × 事件迁移矩阵。纯函数、无时钟；调用方只许调用 [`step`]，不许自己
//!   判断"能不能切"。未标定闭环、运行中切标定、无窗口清故障都在这里被拒。
//!
//! 状态都在 `GainGate` 与 `EncoderMonitor` 里，无堆、`forbid(unsafe_code)`，禁止
//! `unwrap` / `expect` / 数组下标（见下方属性）。`no_std` 只在非测试构建启用：单测跑
//! std，固件目标是纯 `core`，因此 `sin` / `cos` / `sqrt` 一律不外调（[`gate`] 内有
//! 私有的 `sqrt_f32`）。

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

pub mod encoder;
pub mod fsm;
pub mod gate;
pub mod policy;

pub use crate::encoder::{
    angle_rad, jump_limit_lsb, parse, AngleOutcome, EncoderMonitor, EncoderParams, Fault, Frame,
    FrameError, Health,
};
pub use crate::fsm::step;
pub use crate::gate::{GainAccum, GainGate, GateParams};
pub use contracts::Trust;
