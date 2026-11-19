//! 契约数值的两个来源：[`hw`] 器件事实、[`budget`] 系统预算。
//!
//! 这里只放"改了会让硬件或时序不成立"的量，以及跨 crate 必须一致的预算。策略选择
//! （门限、降额、盲推窗口、SPI 分频）在 `user::policy`，两处不许互相复制：`contracts`
//! 不知道算法，也不许依赖 `user`。

pub mod budget;
pub mod hw;
