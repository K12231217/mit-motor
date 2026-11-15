/* STM32G474RBT6 memory layout (LQFP64, 128 KB Flash).
 *
 * The final part for this project (decided 2026-11-15). Its pin-identical
 * siblings RC/RE differ only in Flash density, so a future upgrade is a
 * two-line edit here plus the `stm32g474rb` chip feature in Cargo.toml.
 *
 * FLASH : 128 Kbytes @ 0x08000000   (RB = 128K; RC = 256K; RE = 512K)
 * SRAM  : 128 Kbytes total
 *         - SRAM1  80K @ 0x20000000  (first 32K with hardware parity)
 *         - SRAM2  32K @ 0x20014000  (retained in Stop/Standby)
 *         - CCM    16K @ 0x10000000  (Cortex-M4 core only, NOT reachable
 *                                     by any DMA controller - never put
 *                                     ADC/SPI/USB DMA buffers here)
 *         SRAM1+SRAM2 are contiguous, so RAM is declared as 112K.
 *         cortex-m-rt's link.x has no .ccmram output section, so CCMRAM
 *         below is a region declaration only: actually using it needs a
 *         section added to the linker script (AN4296), not just
 *         #[link_section].
 *
 * NOTE: do NOT re-enable embassy-stm32's `memory-x` feature. stm32-metapac
 * 15.0.0 understates G4 SRAM (32K for stm32g491re, 96K for
 * stm32g474rb/re), so the generator is not authoritative. Keep this file
 * hand-written and check it against the datasheet (DS12288,
 * STM32G474xB/xC/xE) if the part number changes.
 *
 * Flash budget: the bare template is 64,632 B in debug (49 % of the 128K
 * part) and 16,080 B in release (12 %). That 3.9x ratio is why the full
 * application must be budgeted on release builds; see
 * ../docs/hardware-requirements.md §1.1.
 */
MEMORY
{
    FLASH  : ORIGIN = 0x08000000, LENGTH = 128K
    RAM    : ORIGIN = 0x20000000, LENGTH = 112K
    CCMRAM : ORIGIN = 0x10000000, LENGTH = 16K
}
