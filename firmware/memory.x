/* STM32G474RBT6 memory layout (LQFP64, 128 KB Flash, dual-bank).
 *
 * The final part for this project (decided 2026-11-15). Its pin-identical
 * siblings RC/RE differ only in Flash density, so a future upgrade is a
 * two-line edit here plus the `stm32g474rb` chip feature in Cargo.toml.
 *
 * FLASH budget for this season: 64 KB (bank 1) @ 0x08000000.
 *   RBT6 is 128 KB total as two 64 KB banks when DBANK=1 (the option byte
 *   setting chosen for this board, so a future bootloader / log archive can
 *   own bank 2 and a page erase in one bank never stalls an execution from
 *   the other). Only bank 1 is linked and written this season.
 *
 * CONSERVATIVE VALUES — DO NOT TREAT AS DATASHEET FACTS:
 *   DBANK=1 vs DBANK=0 and the exact SRAM1/SRAM2/CCM split still need a
 *   first-hand read of DS12288 (STM32G474xB/xC/xE) and RM0440 chapter 3.
 *   Until then every region below is a SUBSET of the smallest plausible
 *   answer, so the image links under any factory configuration:
 *
 *     FLASH       64K @ 0x08000000   (bank 1 of 128K, DBANK=1 assumed)
 *     FLASH_BANK2 64K @ 0x08040000   (declared, never written this season)
 *     RAM         96K @ 0x20000000   (SRAM1 80K + SRAM2 16K; SRAM2 is 16K or
 *                                     32K depending on the part config, so
 *                                     assume 16K = 96K, not the 112K the old
 *                                     note claimed)
 *     CCMRAM      32K @ 0x10000000   (CCM is 32K on G474xB/xC/xE per the
 *                                     reference manual; core-only, NOT
 *                                     reachable by any DMA controller, so
 *                                     never put ADC/SPI/USB DMA buffers here)
 *
 * cortex-m-rt's link.x has no .ccmram output section, so CCMRAM below is a
 * region declaration only: actually using it needs a section added to the
 * linker script (AN4296), not just #[link_section].
 *
 * NOTE: do NOT re-enable embassy-stm32's `memory-x` feature. stm32-metapac
 * 15.0.0 understates G4 SRAM (32K for stm32g491re, 96K for
 * stm32g474rb/re), so the generator is not authoritative. Keep this file
 * hand-written and replace the conservative numbers with datasheet ones
 * once DS12288/RM0440 have been read first-hand.
 *
 * Flash budget: the bare template is 17,768 B in release; against the 64K
 * bank-1 region that is 27.1 % (it read 12 % when this file claimed 128K).
 * See `cargo xtask size`, which parses the FLASH length below.
 */
MEMORY
{
    FLASH       : ORIGIN = 0x08000000, LENGTH = 64K
    FLASH_BANK2 (rwx) : ORIGIN = 0x08040000, LENGTH = 64K
    RAM         : ORIGIN = 0x20000000, LENGTH = 96K
    CCMRAM      : ORIGIN = 0x10000000, LENGTH = 32K
}
