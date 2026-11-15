# mit-motor firmware — Rust + RTIC template

Template for the graduation-design joint driver
(STM32G474RBT6 + MP6540HA, WK4310, dual MT6701, CAN FD).
Migrated from the RTIC `embassy-stm32g4` example
(rtic-rs/rtic, master @ `832307d8`, `examples/embassy-stm32g4`).

## Target

- MCU: **STM32G474RBT6** (LQFP64) — Cortex-M4F @ up to 170 MHz,
  128 KB Flash, 128 KB SRAM (see `memory.x`). Same package and pinout as
  the RC/RE parts, so a Flash upgrade is a `memory.x` edit, not a board
  change. Its 29 MCU signals all keep their PCB14 functions — but the V2
  board moves six of them (analog front end, six PWM, CAN, UART, BKIN,
  ADC4): the pin map is `../docs/hardware-requirements.md` §3, not the
  PCB14 netlist.
- Driver: MPS MP6540HA three-phase bridge (not part of this template yet)
- Motor: WK4310 frameless, 14 pole pairs, 8:1 planetary
- Encoders: 2× MT6701 (not wired yet)
- Bus: CAN FD (not wired yet)

## What is in this template

- `src/bin/blinky.rs` — RTIC app, systick monotonic, LED blink task.
  Proves the RTIC + embassy bring-up works; nothing motor-specific yet.
- `Cargo.toml` — rtic 2.4.0, rtic-monotonics 3.0.0, rtic-sync 1.6.0,
  embassy-stm32 0.1.0 (`stm32g474rb` feature), defmt logging.
- `memory.x` — hand-written, verified against the datasheet (see below).
- `.cargo/config.toml` — target `thumbv7em-none-eabihf`, probe-rs runner.
- `build.rs` — link args (`-Tlink.x`, `-Tdefmt.x`), unchanged from the example.

## Build (verified 2026-11-15)

```sh
rustup toolchain install stable
rustup target add thumbv7em-none-eabihf
cargo build            # debug: ~53 % of the 128 KB part (see the table)
cargo build --release  # the acceptance profile
```

On Windows the **msvc** host toolchain needs VS Build Tools: without
`link.exe` + `msvcrt.lib` the host build scripts do not link (Coreutils'
`link.exe` from Git Bash is not it). A `*-pc-windows-gnu` host toolchain
with MinGW works with no extra setup — the thumb target itself is linked by
the bundled `rust-lld` either way:

```sh
cargo +<gnu-or-msvc>-toolchain build --release
```

Section sizes (only blinky + defmt), two independent measurements:

| toolchain | profile | .text | .rodata | total | of 128 KiB Flash |
|---|---|---|---|---|---|
| rustc 1.99.1, `stm32g474re` | debug | 52,120 | 12,512 | 64,632 B | 49 % |
| rustc 1.99.1, `stm32g474re` | release | 13,884 | 2,196 | 16,080 B | 12 % |
| rustc 1.93.0-nightly gnu, `stm32g474rb` | debug | 55,268 | 13,464 | 68,732 B | 52 % |
| rustc 1.93.0-nightly gnu, `stm32g474rb` | release | 14,632 | 2,552 | 17,184 B | 13 % |

The ~6 % difference tracks the toolchain, not the chip feature: `stm32g474rb`
is the same peripheral set as `stm32g474re` (metapac's chip dirs differ only
in FLASH size) and the same hand-written `memory.x`, and both builds link
inside the 128 KB region. Raw `llvm-objcopy -O binary` images: 69,320 B debug
(53 %), 17,768 B release (14 %); `.data` 112 B + `.bss` 480 B.

The template is small; the full application is not. At this template's
3.9x debug/release ratio a full app (release estimate 40–50 KB) lands at
110–175 KB in a debug build, i.e. **over the part**. HIL work therefore has
to run release builds, or with `opt-level` raised for debug: the budget rule
is in `../docs/hardware-requirements.md` §1.1.

Flash (not run yet — no G474RBT6 board exists; PCB14 carries a G491RET6):

```sh
cargo run --release   # probe-rs run --chip STM32G474RB --connect-under-reset
```

## Changes vs the RTIC example

1. **Chip feature**: `stm32g431cb` → `stm32g474rb` (via `stm32g491re`;
   embassy-stm32 0.1.0 has all of them). The control MCU was locked to
   G474RBT6 on 2026-11-15.
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
   authoritative. G474RB as built: FLASH 128 K @ 0x08000000;
   SRAM1 80 K + SRAM2 32 K contiguous at 0x20000000; CCM 16 K
   @ 0x10000000 (no DMA access, so never put ADC/SPI/USB buffers there).
   Do not re-enable `memory-x` alongside this file.
6. **LED pin is PB2.** The example's PC6 is unrouted on PCB14 and there is
   no MCU-controlled LED on that board; PB2 is the pin proposed in
   `../docs/hardware-requirements.md` §3 (HR-505).

## Where the motor-control code goes next (not in this template)

- **TIM2 is taken.** `embassy-stm32`'s `time-driver-any` feature (enabled
  in `Cargo.toml`) resolves to the first available singleton, which is
  TIM2 on this chip (`embassy-stm32 0.1.0`, `build.rs`: `any` → `TIM2`),
  and `embassy_stm32::init` keeps its update interrupt. Do not plan TIM2
  for PWM or a tick; the µs timebase in
  `../docs/software-architecture.md` uses TIM5 (32-bit) for that reason.
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

- `probe-rs` chip name `STM32G474RB` in `.cargo/config.toml` is checked
  against `probe-rs chip list` (probe-rs 0.31.0 lists it); nothing has been
  flashed (no G474RBT6 board exists yet).
- No hardware was attached: flashing, RTT output and the LED blink have
  not been run on a real board.
- The template has no tests yet (`cargo test` for a `no_std` target
  needs a host-test harness or on-target runner).
