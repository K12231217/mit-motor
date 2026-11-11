/* STM32G491RET6 memory layout, verified against the reference manual
 * (RM0440 Rev 9, section 2.4 "Embedded SRAM", category 4 devices):
 *   FLASH : 512 Kbytes @ 0x08000000
 *   SRAM  : 112 Kbytes total, contiguous at 0x20000000:
 *           - SRAM1  80K @ 0x20000000 (first 32K with hardware parity)
 *           - SRAM2  16K @ 0x20014000 (retained in Stop/Standby)
 *           - CCM    16K @ 0x10000000, aliased at 0x20018000 so all bus
 *             masters see it contiguously after SRAM1+SRAM2
 *
 * NOTE: do NOT re-enable embassy-stm32's `memory-x` feature. The
 * auto-generated memory.x from stm32-metapac 15.0.0 lists RAM as only
 * 32K for stm32g491re, which understates the real 112K and would make
 * the link fail once the firmware uses more than 32K of RAM. This
 * hand-written file is the correction; keep it in sync with the
 * datasheet if the chip changes.
 */
MEMORY
{
    FLASH : ORIGIN = 0x08000000, LENGTH = 512K
    RAM   : ORIGIN = 0x20000000, LENGTH = 112K
}
