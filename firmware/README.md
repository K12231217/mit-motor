# mit-motor firmware — Rust + RTIC template

Template for the graduation-design joint driver
(STM32G491 + MP6540HA, WK4310, dual MT6701, CAN FD).
Migrated from the RTIC `embassy-stm32g4` example
(rtic-rs/rtic, master @ `832307d8`, `examples/embassy-stm32g4`).

## Target

- MCU: **STM32G491RET6** — Cortex-M4F @ up to 170 MHz, 512 KB Flash,
  112 KB SRAM (see `memory.x`)
- Driver: MPS MP6540HA three-phase bridge (not part of this template yet)
- Motor: WK4310 frameless, 14 pole pairs, 8:1 planetary
- Encoders: 2× MT6701 (not wired yet)
- Bus: CAN FD (not wired yet)

## What is in this template

- `src/bin/blinky.rs` — RTIC app, systick monotonic, LED blink task.
  Proves the RTIC + embassy bring-up works; nothing motor-specific yet.
- `Cargo.toml` — rtic 2.4.0, rtic-monotonics 3.0.0, rtic-sync 1.6.0,
  embassy-stm32 0.1.0 (`stm32g491re` feature), defmt logging.
- `memory.x` — hand-written, verified against the datasheet (see below).
- `.cargo/config.toml` — target `thumbv7em-none-eabihf`, probe-rs runner.
- `build.rs` — link args (`-Tlink.x`, `-Tdefmt.x`), unchanged from the example.

## Build (verified 2026-11-11)

```sh
rustup toolchain install stable
rustup target add thumbv7em-none-eabihf
cargo build        # debug; cargo build --release for release
```

Verified on 2026-11-11 with rustc/cargo 1.99.1 (stable):
`cargo build` succeeds in ~90 s (first build, crates.io fetch included).
Debug binary `target/thumbv7em-none-eabihf/debug/blinky`:
`.text` 51,376 B + `.rodata` 12,264 B → ~62.6 KB of 512 KB Flash;
`.data` 112 B + `.bss` 480 B → ~0.6 KB of 112 KB RAM.
Rebuild after the `memory.x` fix: 11.9 s, same sizes.

Flash (not run yet — no hardware attached during this migration):

```sh
cargo run --release   # probe-rs run --chip STM32G491RETx --connect-under-reset
```

## Changes vs the RTIC example

1. **Chip feature**: `stm32g431cb` → `stm32g491re` (embassy-stm32 0.1.0
   already has this feature; version unchanged).
2. **Target**: `thumbv7m-none-eabi` → `thumbv7em-none-eabihf`
   (Cortex-M4F with FPU).
3. **Systick frequency**: `25_000_000` → `16_000_000`. The example's
   25 MHz does not match the default clock config: embassy's G4
   default is HSI 16 MHz with no PLL (`embassy-stm32 0.1.0`,
   `src/rcc/g4.rs`: `HSI_FREQ = 16_000_000`, `mux: HSI`, `pll: None`).
   With 25 MHz, every `Mono::delay()` would have run 25/16× too long.
   If the clock config changes later (e.g. PLL to 170 MHz), update
   this number in `src/bin/blinky.rs` to match HCLK.
4. **Dependencies**: the example used path dependencies on the RTIC
   workspace (`../../rtic` …); replaced by crates.io dependencies at
   the *same* versions (rtic 2.4.0, rtic-monotonics 3.0.0,
   rtic-sync 1.6.0), so the template builds standalone.
5. **`memory.x` is hand-written, and embassy's `memory-x` feature is
   off — deliberately.** The auto-generated memory.x from
   stm32-metapac 15.0.0 lists RAM = 32K for `stm32g491re`, but the
   reference manual (RM0440 Rev 9, §2.4 "Embedded SRAM", category 4
   devices) gives 112K total:
   SRAM1 80K @ 0x20000000 (first 32K with hardware parity) +
   SRAM2 16K @ 0x20014000 + CCM SRAM 16K @ 0x10000000 (aliased at
   0x20018000, contiguous after SRAM1+SRAM2). FLASH 512K @ 0x08000000.
   The 32K value would have made the link fail spuriously once the
   firmware used more than 32K of RAM. Do not re-enable `memory-x`
   alongside this file.
6. **LED pin PC6 is a placeholder** carried over from the example.
   Replace it with the real LED pin from the board schematic.

## Where the motor-control code goes next (not in this template)

- **PWM**: TIM1 (advanced timer, complementary outputs + dead time
  for the MP6540HA HS/LS inputs) — new `src/bin/` or a module under
  `src/`, driven from a high-priority RTIC task.
- **Current sampling**: ADC1/2/3 (three independent ADCs — this is
  why the design moved to G491) + DMA, reading the MP6540HA SOx
  outputs; per-phase gain/offset calibration tables live with the
  sampling code.
- **Current loop / FOC**: a periodic RTIC task at the PWM frequency
  (e.g. 20 kHz); keep it short — Clarke/Park, PI, inverse Park,
  SVPWM — and push logging/telemetry to a lower-priority task.
- **Encoders**: MT6701 via SPI (or SSI/ABI as wired on the board);
  angle → electrical angle with the 14 pole pairs.
- **CAN FD**: FDCAN1, 2 Mbps, MIT-protocol frames; a lower-priority
  RTIC task or interrupt-bound handler.
- **TIM2 is taken**: embassy's time driver (`time-driver-any`
  feature) claims TIM2 — and its update interrupt — at
  `embassy_stm32::init` time, so don't plan TIM2 for PWM or
  anything else.
- **Clock**: if the loop needs the full 170 MHz, configure the PLL in
  `embassy_stm32::init(...)` and update the systick frequency (item 3
  above) to the new HCLK.

## Known unverified items

- `probe-rs` chip name `STM32G491RETx` in `.cargo/config.toml` is
  verified (2026-11-11) against probe-rs's own target definitions
  (`probe-rs/targets/STM32G4_Series.yaml`: `STM32G491RE` lists
  `STM32G491RETx` as a package variant). First flash on real
  hardware is still untested (see below).
- No hardware was attached during this migration: flashing, RTT
  output, and the LED blink have not been run on a real board.
- The template has no tests yet (`cargo test` for a `no_std` target
  needs a host-test harness or on-target runner).
