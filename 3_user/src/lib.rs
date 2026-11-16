//! # `user` —— 算法层
//!
//! 这一层放**会在运行时算数、有状态、能脱离硬件单元测试**的东西；接口事实
//! （线上帧布局、状态字位域、限值常量）仍然只在 [`contracts`] 里定义一次。
//!
//! 当前有两块：
//!
//! - 增益闸门 [`gate`]：四维结构门（方向覆盖 R、信息矩阵 D、DC 主导 z、精度降额
//!   pred）+ χ²，以及相对增益的正规方程累加器。它是**参数可辨识性的前提**（共模
//!   增益不可观测），不是可选健康度检查。
//! - MT6701 接受机 [`encoder`]：24 bit 帧解析、CRC-6、状态位、全 0 护栏、跳变判据
//!   与去抖。`1_bsp` 只搬字节，判据是纯函数 / 纯状态机，host 可测。
//!
//! # 纪律
//!
//! - PRE-01 无堆：没有 `alloc`；所有状态都在 `GainGate` / `EncoderMonitor` 里。
//! - PRE-03 不安全代码：禁止。
//! - PRE-05 无静默继续：禁止 `unwrap/expect` 与数组下标（clippy deny）。
//! - PRE-07 数值唯一来源：阈值全部来自 [`contracts::limits`]，本层不另存副本。
//!
//! `no_std` 只在非测试构建里启用：单元测试跑在 host 的 std 上（有 libtest），
//! 固件目标 `thumbv7em-none-eabihf` 上则是纯 `core`——因此 `sin/cos/sqrt` 一律
//! 不外调（见 [`gate`] 里的私有 `sqrt_f32`）。

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]

pub mod encoder;
pub mod gate;

pub use crate::encoder::{
    angle_rad, jump_limit_lsb, parse, AngleOutcome, EncoderMonitor, EncoderParams, Fault, Frame,
    FrameError, Health,
};
pub use crate::gate::{GainAccum, GainGate, GateParams};
pub use contracts::Trust;
