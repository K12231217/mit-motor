# mit-motor

单关节驱动器：STM32G474RBT6 + MP6540HA，低边比例电流采样，零和约束下的相对增益自校准与冻结/故障安全。

## 硬件

- MCU：STM32G474RBT6（LQFP64，128 KB Flash / 128 KB SRAM）
- 驱动：MP6540HA（`MP6540HGU-A-Z`，HS/LS 六电平输入，无 EN、无内部死区）
- 电机：WK4310H，14 对极；编码：双 MT6701（SPI1）
- 最新网表：`hardware/pcb/Netlist_Schematic14_1_2026-11-27.tel`（引脚以 `docs/hardware-requirements.md` §3.1 为准）

## 目录

```
software/
  contracts/     接口事实（零依赖、host 可测）
  algorithms/    算法层（gate/encoder/fsm/foc/svpwm/sense/pid/current/supervisor）
  bsp/           thumb 板级层（clock/pwm 已实现；adc/encoder/dwt 待实现）
  firmware/      RTIC + embassy 应用（当前 S0 bring-up）
  xtask/         唯一构建/门禁/台架入口
hardware/pcb/    网表、BOM、CubeMX
mechanical/      装配/CAD/BOM
docs/            SPEC、硬件/软件冷数据、archive
tools/           本地脚本
```

## 开发入口

```sh
cargo xtask check        # cargo check
cargo xtask rel          # release 构建（验收构型）
cargo xtask size         # Flash/RAM 与预算
cargo xtask test         # host workspace 单测
cargo xtask clippy       # -D warnings
cargo xtask arch         # I1/I2/I3/I7
cargo xtask hygiene      # H2/H4/H5/H6
cargo xtask flash        # probe-rs（芯片名在 software/firmware/.cargo/config.toml）
cargo xtask hil          # 编译+烧录+RTT，日志落 .scratch/hil/run.log
cargo xtask hil --boot-freeze  # 复位后 TP2/PB7 保持约 1 s，便于探头抓
cargo xtask rtt          # 只 attach 当前运行，不重新烧录
cargo xtask dac 2 4095   # 台架静态拉高 DAC2/TP2，便于万用表测
```

工具链由 `rust-toolchain.toml` 固定（`nightly-2025-11-05` 的 windows-gnu 主机；Linux/macOS 可用 `RUSTUP_TOOLCHAIN=stable`）。

## 文档

只有三个控制文件：

- `AGENTS.md` — RULES：物理/机制铁律、门禁、Router/Worker 边界。
- `docs/INDEX.md` — SPEC：一页事实、ADR、冷数据指针。
- `.scratch/handoff.md` — STATE：活跃 Thread、当前基线、HIL、待裁决。

冷数据：`docs/hardware-requirements.md`（引脚/采样/测试点）、`docs/software-architecture.md`（模块/门禁）、`docs/motor-wk4310h.md`（电机参数）。历史与研究资料在 `docs/archive/`，不参与冷启动。
