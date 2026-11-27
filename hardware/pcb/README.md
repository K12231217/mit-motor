# hardware/pcb — 硬件资料

| 文件 | 说明 |
|---|---|
| `Netlist_Schematic14_1_2026-11-27.tel` | **最新网表（G474RBT6，Schematic14_1，2026-11-27）**；引脚/功能以此为准。 |
| `Netlist_PCB14_2026-11-11.tel` | PCB14 / G491RET6 旧网表；只作旧板参考。 |
| `Desktop.ioc` / `Desktop.txt` / `Desktop.pdf` | STM32G474RBTx CubeMX 6.18 工程（2026-11-17）；nSLEEP/nFAULT/OPAMP1/VBUS 是旧版，冲突时以最新网表为准。 |
| `BOM_wzy_ver_mit_motor_PCB14_1_2026-11-17.xlsx` | PCB14 BOM。 |

- 引脚唯一真源：`../docs/hardware-requirements.md` §3.1。
- HIL 探针/测试点：同文件 §4/§5。
- 旧板 TP 编号与最新板不同（PCB14 的 TP5/6/7 是 ADC 侧）；不要混用。
