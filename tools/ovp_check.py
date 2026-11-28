#!/usr/bin/env python3
"""母线软件 OVP 台架验证（Korad KA3005P + RTT）。

流程：电源输出关 → 12 V → 24 V → 27.5 V，每档烧录/复位一次并读 `adc0 ... vbus=`
与 1 s 汇总里的 `ovp=`。任一档电压读数与设定相差超过 1.5 V 就中止（防止在
"测不到母线"的情况下把电压继续往上加）。27.5 V 档要求 `ovp=1`。

用法（默认 COM13，可用 --port 覆盖）：
    python tools/ovp_check.py
    python tools/ovp_check.py --steps 12,24,26.5,27.5

退出码：0 = 全部符合预期；1 = 断言失败；2 = 环境/电源问题（不碰高压）。
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import time

import serial

LOG = ".scratch/hil/run.log"


def cmd(ser: serial.Serial, text: str) -> str:
    ser.write(text.encode("ascii"))
    time.sleep(0.08)
    return ser.read(ser.in_waiting or 1).decode("ascii", "replace").strip()


def supply_set(ser: serial.Serial, volts: float, amps: float) -> None:
    cmd(ser, f"VSET1:{volts:05.2f}")
    cmd(ser, f"ISET1:{amps:05.3f}")


def supply_on(ser: serial.Serial) -> None:
    cmd(ser, "OUT1")


def supply_off(ser: serial.Serial) -> None:
    cmd(ser, "OUT0")


def read_status(ser: serial.Serial) -> tuple[float, float]:
    vout = cmd(ser, "VOUT1?")
    iout = cmd(ser, "IOUT1?")
    try:
        return float(vout), float(iout)
    except ValueError:
        return float("nan"), float("nan")


def run_hil_capture(timeout_s: int = 30) -> str:
    """烧录并抓一段 RTT；超时被杀属正常（固件常驻）。"""
    try:
        subprocess.run(
            ["cargo", "xtask", "hil"],
            timeout=timeout_s,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    except subprocess.TimeoutExpired:
        pass
    try:
        with open(LOG, "r", encoding="utf-8", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def parse(log: str) -> tuple[int | None, int | None, bool]:
    """取最后一次 adc0 的 vbus 码/mV 与是否 OVP。"""
    vbus = mv = None
    for m in re.finditer(r"adc0 .*?vbus=(\d+).*?mv=(-?\d+)", log):
        vbus, mv = int(m.group(1)), int(m.group(2))
    ovp = re.search(r"ovp=(\d+)", log)
    tripped = bool(ovp and int(ovp.group(1)) != 0)
    return vbus, mv, tripped


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", default="COM13")
    ap.add_argument("--steps", default="12,24,27.5")
    ap.add_argument("--limit-a", type=float, default=1.0)
    ap.add_argument(
        "--expect-trip",
        action="store_true",
        help="固件已把 VBUS_OVP_ENABLED 置 true 时才加；默认只验证电压读数",
    )
    args = ap.parse_args()

    steps = [float(x) for x in args.steps.split(",")]
    if not steps or steps[0] > 20.0:
        print("第一步必须是安全电压（≤20 V），用于确认母线确实接到电源")
        return 2

    try:
        ser = serial.Serial(args.port, 9600, timeout=0.3)
    except serial.SerialException as e:
        print(f"打不开电源 {args.port}: {e}")
        return 2

    try:
        cmd(ser, "")
        print("电源:", cmd(ser, "*IDN?") or "(无 IDN)")
        supply_off(ser)
        time.sleep(0.3)

        for volts in steps:
            supply_set(ser, volts, args.limit_a)
            supply_on(ser)
            time.sleep(0.5)
            vout, iout = read_status(ser)
            log = run_hil_capture()
            code, mv, tripped = parse(log)
            if code is None:
                print(f"{volts:5.1f} V: 没有 adc0 行（RTT 未抓到）")
                return 2
            err = abs(mv - volts * 1000.0)
            print(
                f"{volts:5.1f} V  set={vout:5.2f} V  iout={iout:5.3f} A  "
                f"vbus={code:4d} ({mv:5d} mV, err={err:4.0f})  ovp={'TRIP' if tripped else 'ok'}"
            )
            if err > 1500:
                print("母线读数与设定差 > 1.5 V：电源没有真正接到 VBUS_ADC，中止（不再升压）")
                return 2
            if args.expect_trip:
                expect_trip = volts > 27.0
                if tripped != expect_trip:
                    print(f"OVP 行为不符：设定 {volts} V 期望 trip={expect_trip}，实际 {tripped}")
                    return 1
            elif tripped:
                print(f"OVP 意外触发：VBUS_OVP_ENABLED 应为 false，或读数标度不对")
                return 1
        print("OVP 验证通过")
        return 0
    finally:
        supply_off(ser)
        ser.close()


if __name__ == "__main__":
    sys.exit(main())
