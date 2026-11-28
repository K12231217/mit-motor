//! SPI1 + MT6701 原始帧读取。
//!
//! 本模块只搬三个字节；CRC、状态位、跳变判据在
//! `algorithms::motor::service_encoder`（ADR-08：SPI1 只由 20 kHz 任务持有）。

/// SPI1 总线的唯一所有者：持有外设与片选，不解释帧内容。
pub struct Encoder;

impl Encoder {
    /// 读主编码器一帧（MSB 先出）。
    pub fn read_raw(&mut self) -> [u8; 3] {
        todo!()
    }
}
