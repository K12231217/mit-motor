/* STM32G474RBT6 memory layout (LQFP64, 128 KB Flash, dual-bank).
 *
 * The final part for this project (decided 2026-11-15). Its pin-identical
 * siblings RC/RE differ only in Flash density, so a future upgrade is a
 * two-line edit here plus the `stm32g474rb` chip feature in Cargo.toml.
 *
 * FLASH budget for this season: 64 KB (bank 1) @ 0x08000000.
 *
 * DUAL BANK IS CONFIRMED (2026-11-16). RBT6 is 128 KB as two 64 KB banks when
 * DBANK=1, and in dual-bank mode the two banks are NOT contiguous: bank 2
 * starts at 0x0804_0000, so 0x0801_0000-0x0803_FFFF is an address hole.
 * Outside-bank 1 code linked as "128K contiguous" fails to download (below).
 * Sources:
 * - DS12288 `docs/DS_stm32g474rb/Functional_overview.md` §3.4: "Single or dual
 *   bank operating modes", "Read-while-write (RWW) in dual bank mode ... read
 *   from one bank while an erase or program is performed on the other".
 * - RM0440 Table 7 "512/256/128 KB dual bank organization" (single bank =
 *   Table 8), cited for this exact part by ST staff:
 *   community.st.com/.../stm32g474rb-can-t-debug-program-over-64-kb-159628
 *   ("128K with two flash banks of 64K but NOT contiguous", "the upper 64K
 *   are after 0x08040000").
 * - Same thread, user log: STM32CubeProgrammer refuses one segment crossing
 *   64 KB ("Operation exceeds memory limits"), device reports 2 KB pages.
 *
 * Consequence: code and rodata stay in bank 1, so a page erase/program in
 * bank 2 never stalls instruction fetch from bank 1. That is what makes the
 * A/B calibration archive (two slots in bank 2) safe while the 20 kHz loop
 * runs. Never link bank 2 as a continuation of bank 1.
 *
 * SRAM (DS12288 §3.5, exact, no longer conservative):
 *     SRAM1        80K @ 0x20000000
 *     SRAM2        16K @ 0x20014000
 *     CCM SRAM     32K @ 0x10000000   (CPU via I/D-code bus) and aliased at
 *                  0x20018000 so DMA1/DMA2 can reach it through SBUS --
 *                  "CCM is not DMA-reachable" holds only for the 0x1000_0000
 *                  address. All three blocks are 0 wait state at full clock.
 * This file links only SRAM1 + SRAM2 (96K) as `RAM`: the 0x2000_0000 window is
 * contiguous up to 0x2002_0000 through the alias, so widening to 128K is a
 * separate change (stack placement + link.x sections) and is not done here.
 *
 * cortex-m-rt's link.x has no .ccmram output section, so CCMRAM below is a
 * region declaration only: actually using it needs a section added to the
 * linker script (AN4296), not just #[link_section].
 *
 * NOTE: do NOT re-enable embassy-stm32's `memory-x` feature. stm32-metapac
 * 15.0.0 understates G4 SRAM (32K for stm32g491re, 96K for stm32g474rb/re),
 * so the generator is not authoritative. Keep this file hand-written.
 *
 * Flash budget: the bare template is 17,768 B in release; against the 64K
 * bank-1 region that is 27.1 %. See `cargo xtask size`, which parses the
 * FLASH length below.
 */

MEMORY
{
    FLASH       : ORIGIN = 0x08000000, LENGTH = 64K
    FLASH_BANK2 (rwx) : ORIGIN = 0x08040000, LENGTH = 64K
    RAM         : ORIGIN = 0x20000000, LENGTH = 96K
    CCMRAM      : ORIGIN = 0x10000000, LENGTH = 32K
}
