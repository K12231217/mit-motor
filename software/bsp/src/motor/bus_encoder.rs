//! SPI1 + MT6701 原始帧读取（软件片选）。
//!
//! 本模块只搬三个字节；CRC、状态位、跳变判据在
//! `algorithms::motor::service_encoder`（ADR-08：SPI1 只由 20 kHz 任务持有）。
//!
//! # 时序（MT6701 Rev.1.8 §7.8.1 / Figure-24）
//!
//! - DO 在上升沿更新、应在下降沿采样 ⇒ `CPOL=0`+`CPHA=1`（SPI Mode 1）。
//!   **实测（2026-11-28，两片各 8 帧）**：Mode 1 CRC 8/8，Mode 0/2/3 全 0；
//!   手册 Figure-24 写"无传输时 CLK 为高"，与本板实测不一致，以实测为准。
//! - CSN 下降沿到第一个 CLK 上升沿 ≥ 100 ns；`TCLK ≥ 64 ns`（即 ≤ 15.625 MHz）。
//! - 最后一拍下降沿到 CSN 拉高 ≥ `0.5·TCLK`。
//! - 24 bit MSB 先出：`D[13:0] + Mg[3:0] + CRC[5:0]`，布局见
//!   `service_encoder::frame`。
//!
//! 两片 MT6701 共用 SCK/MISO、各有独立 CS；CS 空闲高（板上有 10 kΩ 上拉，HR-108）。

use embassy_stm32::dma::NoDma;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::peripherals::{PB3, PB4, PC10, PC12, SPI1};
use embassy_stm32::spi::{Config, Spi, MODE_1};
use embassy_stm32::time::Hertz;

/// 两片 MT6701 的逻辑编号；物理上共用 SCK/MISO，各有独立 CS。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderId {
    /// 板载主编码器（`ENC1_CS`）。
    E1,
    /// 外置/第二编码器（`ENC2_CS`）。
    E2,
}

impl EncoderId {
    /// 日志与遥测用的编号（1/2），不是数组下标。
    pub const fn number(self) -> u8 {
        match self {
            Self::E1 => 1,
            Self::E2 => 2,
        }
    }
}

/// SPI1 与两片 MT6701 的引脚集合。
pub struct EncoderPins {
    /// SPI1 外设。
    pub spi: SPI1,
    /// `SPI1_SCK`。
    pub sck: PB3,
    /// `SPI1_MISO`（两片共用）。
    pub miso: PB4,
    /// 主编码器片选。
    pub cs1: PC12,
    /// 第二编码器片选。
    pub cs2: PC10,
}

/// SPI1 总线的唯一所有者：持有外设与两片片选，不解释帧内容。
pub struct Encoder {
    spi: Spi<'static, SPI1, NoDma, NoDma>,
    cs1: Output<'static, PC12>,
    cs2: Output<'static, PC10>,
}

impl Encoder {
    /// 构造：SPI1 只读、Mode 1、MSB 先出；两片 CS 空闲为高。
    ///
    /// `spi_hz` 由调用方传入（策略值在 `config_policy::ENC_SPI_HZ`，bsp 不复制）。
    pub fn new(pins: EncoderPins, spi_hz: u32) -> Self {
        // `Config` 是 `#[non_exhaustive]`，只能先取默认值再改字段。
        let mut config = Config::default();
        config.mode = MODE_1;
        config.frequency = Hertz(spi_hz);
        Self {
            spi: Spi::new_rxonly(pins.spi, pins.sck, pins.miso, NoDma, NoDma, config),
            cs1: Output::new(pins.cs1, Level::High, Speed::Medium),
            cs2: Output::new(pins.cs2, Level::High, Speed::Medium),
        }
    }

    /// 读一帧（3 字节，MSB 先出）。
    ///
    /// SPI 外设报错（模式故障等）时返回 `None`，由调用方计丢帧；不重试、不 panic。
    pub fn read_raw(&mut self, id: EncoderId) -> Option<[u8; 3]> {
        match id {
            EncoderId::E1 => read_with(&mut self.spi, &mut self.cs1),
            EncoderId::E2 => read_with(&mut self.spi, &mut self.cs2),
        }
    }
}

/// 拉低片选 → 等 TH → 读 3 字节 → 拉高；两片编码器共用这一段时序。
fn read_with(
    spi: &mut Spi<'static, SPI1, NoDma, NoDma>,
    cs: &mut Output<'static, impl embassy_stm32::gpio::Pin>,
) -> Option<[u8; 3]> {
    cs.set_low();
    // TH ≥ 100 ns：`asm::delay` 每圈约 3 周期，60 圈约 1 µs @170 MHz。
    cortex_m::asm::delay(60);
    let mut raw = [0u8; 3];
    let ok = spi.blocking_read(&mut raw).is_ok();
    cs.set_high();
    ok.then_some(raw)
}
