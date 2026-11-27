//! 电机侧板级驱动：时钟、PWM、ADC、编码器、DWT。

pub mod bus_adc;
pub mod bus_encoder;
pub mod bus_pwm;
pub mod service_clock;
pub mod service_dwt;
pub mod service_timebase;
pub mod service_trace;
