//! 电机控制链与电机侧服务/配置。

pub mod chain_current;
pub mod chain_foc;
pub mod chain_pid;
pub mod chain_sense;
pub mod chain_svpwm;
pub mod config_motor;
pub mod config_policy;
pub mod service_encoder;
pub mod service_gate;
pub(crate) mod service_math;
