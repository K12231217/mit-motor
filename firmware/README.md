# mit-motor firmware — Rust + RTIC template

Template for the graduation-design joint driver
(STM32G474RET6 + MP6540HA, WK4310, dual MT6701, CAN FD).
Migrated from the RTIC `embassy-stm32g4` example
(rtic-rs/rtic, master @ `832307d8`, `examples/embassy-stm32g4`).

## Target

- MCU: **STM32G474RET6** (LQFP64) — Cortex-M4F @ up to 170 MHz,
  512 KB Flash, 128 KB SRAM (see `memory.x`). Same package as the
  PCB14 `G491RET6`, and its 29 MCU signals all keep their functions —
  but the V2 board moves six of them (analog front end, six PWM,
  CAN, UART, BKIN, ADC4): the pin map is
  `../docs/hardware-requirements.md` §3, not the PCB14 netlist.
- Driver: MPS MP6540HA three-phase bridge (not part of this template yet)
- Motor: WK4310 frameless, 14 pole pairs, 8:1 planetary
- Encoders: 2× MT6701 (not wired yet)
- Bus: CAN FD (not wired yet)

## What is in this template

- `src/bin/blinky.rs` — RTIC app, systick monotonic, LED blink task.
  Proves the RTIC + embassy bring-up works; nothing motor-specific yet.
- `Cargo.toml` — rtic 2.4.0, rtic-monotonics 3.0.0, rtic-sync 1.6.0,
  embassy-stm32 0.1.0 (`stm32g474re` feature), defmt logging.
- `memory.x` — hand-written, verified against the datasheet (see below).
- `.cargo/config.toml` — target `thumbv7em-none-eabihf`, probe-rs runner.
- `build.rs` — link args (`-Tlink.x`, `-Tdefmt.x`), unchanged from the example.

## Build (verified 2026-11-15)

```sh
rustup toolchain install stable
rustup target add thumbv7em-none-eabihf
cargo build            # debug (fits 512 KB, but still slower/larger)
cargo build --release  # the acceptance profile
```

Section sizes for the ported target (`stm32g474re`, only blinky + defmt):

| profile | .text | .rodata | total | of 512 KiB Flash |
|---|---|---|---|---|
| debug | 52,120 | 12,512 | 64,632 B | 12 % |
| release | 13,884 | 2,196 | 16,080 B | 3 % |

`.data` 112 B + `.bss` 480 B. The same template on the 128 KB RBT6 already
used 49 % of Flash in debug — that is the whole reason the part is RET6.

Flash (not run yet — no G474RET6 board exists; PCB14 carries a G491RET6):

```sh
cargo run --release   # probe-rs run --chip STM32G474RE --connect-under-reset
```

## Changes vs the RTIC example

1. **Chip feature**: `stm32g431cb` → `stm32g474re` (via `stm32g491re`;
   embassy-stm32 0.1.0 has all three). The control MCU was re-picked from
   G491RET6 to G474RET6 on 2026-11-15.
2. **Target**: `thumbv7m-none-eabi` → `thumbv7em-none-eabihf`
   (Cortex-M4F with FPU).
3. **Systick frequency**: `25_000_000` → `16_000_000`. The example's
   25 MHz does not match the default clock config: embassy's G4
   default is HSI 16 MHz with no PLL (`embassy-stm32 0.1.0`,
   `src/rcc/g4.rs`: `HSI_FREQ = 16_000_000`, `mux: HSI`, `pll: None`).
   With 25 MHz, every `Mono::delay()` would have run 25/16× too long.
   When the PLL is configured for 170 MHz, update this number in
   `src/bin/blinky.rs` to match HCLK.
4. **Dependencies**: the example used path dependencies on the RTIC
   workspace (`../../rtic` …); replaced by crates.io dependencies at
   the *same* versions (rtic 2.4.0, rtic-monotonics 3.0.0,
   rtic-sync 1.6.0), so the template builds standalone.
5. **`memory.x` is hand-written, and embassy's `memory-x` feature is
   off — deliberately.** stm32-metapac 15.0.0 understates G4 SRAM
   (32 K for `stm32g491re`, 96 K for `stm32g474rb/re`), so it is not
   authoritative. G474RE as built: FLASH 512 K @ 0x08000000;
   SRAM1 80 K + SRAM2 32 K contiguous at 0x20000000; CCM 16 K
   @ 0x10000000 (no DMA access, so never put ADC/SPI/USB buffers there).
   Do not re-enable `memory-x` alongside this file.
6. **LED pin is PB2.** The example's PC6 is unrouted on PCB14 and there is
   no MCU-controlled LED on that board; PB2 is the pin proposed in
   `../docs/hardware-requirements.md` §3 (HR-505).

## Where the motor-control code goes next (not in this template)

- **PWM**: TIM1 (advanced timer). MP6540HA takes six level inputs (HSx/LSx)
  and has **no internal dead time**, so the dead time must come from TIM1
  BDTR and the MCU drives CH1/2/3 + CH1N/2N/3N — driven from a
  high-priority RTIC task.
- **Current sampling**: ADC1/2/3 — three separate ADC units, which is why
  the controller must be a 3-ADC part — synchronous trigger from TIM1,
  injected group, reading the MP6540HA SOx outputs; per-phase gain/offset
  calibration tables live with the sampling code.
- **Current loop / FOC**: a periodic RTIC task at the PWM frequency
  (20 kHz); keep it short — Clarke/Park, PI, inverse Park, SVPWM
  (CORDIC for the trig) — and push logging/telemetry to a lower-priority
  task.
- **Encoders**: MT6701 via SPI1 (SCK/MISO shared, one CS each);
  angle → electrical angle with the 14 pole pairs.
- **CAN FD**: FDCAN2 (PB12/PB13), 2 Mbps data phase, MIT-protocol frames.
- **Clock**: 170 MHz needs HSE 16 MHz → PLL M=4/N=85/P=2 plus boost mode;
  ADC clock 42.5 MHz (PLLP/4); USB FS must use HSI48 + CRS because 340 MHz
  VCO cannot be divided to 48 MHz.

## Known unverified items

- `probe-rs` chip name `STM32G474RE` in `.cargo/config.toml` is checked
  against `probe-rs chip list`; nothing has been flashed (no G474RET6
  board exists yet).
- No hardware was attached: flashing, RTT output and the LED blink have
  not been run on a real board.
- The template has no tests yet (`cargo test` for a `no_std` target
  needs a host-test harness or on-target runner).
