//! SPI1 + MT6701 原始帧读取。
//!
//! 本模块只搬三个字节；CRC、状态位、跳变判据在 `user::encoder`（ADR-08：SPI1 只由
//! 20 kHz 任务持有）。

/// SPI1 与主编码器片选的所有者。
pub struct Encoder;

impl Encoder {
    /// 读主编码器一帧（MSB 先出）。
    pub fn read_raw(&mut self) -> [u8; 3] {
        todo!()
    }
}
