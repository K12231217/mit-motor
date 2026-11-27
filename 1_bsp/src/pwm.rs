//! TIM1 三相互补 PWM、BDTR 死区、BKIN 与 MOE。
//!
//! 输出只接受 [`Duty`]：调制量在构造点已按 `D_MAX` 夹紧（PRE-08）。六路全低是
//! `Duty::Coast`，不能用零调制量冒充。

use contracts::Duty;

/// TIM1 与六路栅极输入的所有者。
pub struct Pwm;

impl Pwm {
    /// 设置三相输出；`Duty::Coast` 等价于六路全低。
    pub fn set_duty(&mut self, _duty: Duty) {
        todo!()
    }

    /// 六路全低，三相高阻。
    pub fn coast(&mut self) {
        todo!()
    }

    /// 置 MOE；只允许在自检通过或故障清除后调用。
    pub fn enable(&mut self) {
        todo!()
    }

    /// 清 MOE；BKIN 触发后 `AOE = 0` 保证不会自动恢复。
    pub fn disable(&mut self) {
        todo!()
    }
}
