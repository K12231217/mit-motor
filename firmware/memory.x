/* STM32G474RET6 memory layout (LQFP64, 512 KB Flash).
 *
 * FLASH : 512 Kbytes @ 0x08000000   (RE = 512K; RC = 256K, RB = 128K)
 * SRAM  : 128 Kbytes total
 *         - SRAM1  80K @ 0x20000000  (first 32K with hardware parity)
 *         - SRAM2  32K @ 0x20014000  (retained in Stop/Standby)
 *         - CCM    16K @ 0x10000000  (Cortex-M4 core only, NOT reachable
 *                                     by any DMA controller - never put
 *                                     ADC/SPI/USB DMA buffers here)
 *         SRAM1+SRAM2 are contiguous, so RAM is declared as 112K.
 *         CCM is declared separately and is unused unless a section is
 *         explicitly placed there.
 *
 * NOTE: do NOT re-enable embassy-stm32's `memory-x` feature. stm32-metapac
 * 15.0.0 understates G4 SRAM (32K for stm32g491re, 96K for
 * stm32g474rb/re), so the generator is not authoritative. Keep this file
 * hand-written and check it against the datasheet (DS12288,
 * STM32G474xB/xC/xE) if the part number changes.
 */
MEMORY
{
    FLASH  : ORIGIN = 0x08000000, LENGTH = 128K
    RAM    : ORIGIN = 0x20000000, LENGTH = 112K
    CCMRAM : ORIGIN = 0x10000000, LENGTH = 16K
}
