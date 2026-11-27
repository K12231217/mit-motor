#!/usr/bin/env python3
"""mit-motor MCU 工具：编译固件 + 经 J-Link/OpenOCD 识别与烧录芯片。

只用 Python 标准库。`python tools/mcu.py -h` 看用法。

为什么不用 `cargo xtask flash`：那条路走 `software/firmware/.cargo/config.toml` 里的
probe-rs runner，而 probe-rs 不支持 SEGGER J-Link；本机唯一可用通路是
OpenOCD 0.12 的 jlink 驱动（需 JLinkARM.dll）。

2026-11-27 实测的工具有限性：
- jlink 驱动不认 `adapter speed`，日志恒为 `clock speed 2000 kHz`；
- OpenOCD 在运行期加载 JLinkARM.dll，本脚本自动把 SEGGER 安装目录前置到 PATH；
- `flash banks` 报的是 G4 家族上限（0x50000），实际容量看 `flash probe` 的
  `flash size = ... KiB`，G474RB 为 128 KiB 双 bank。
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FW = ROOT / "software" / "firmware"
TARGET_DIR = FW / "target" / "thumbv7em-none-eabihf"
ELF_STEM = "main"

# 工程级探针配置（与 openocd/jlink 技能同一份格式，见 skill/config.example.json）。
PROJECT_CFG = ROOT / ".embeddedskills" / "config.json"
IFACE_CFG = "interface/jlink.cfg"
TARGET_CFG = "target/stm32g4x.cfg"
DEFAULT_SPEED = 4000
DEV_ID_G47X = 0x469


def die(msg: str) -> "None":
    print(f"error: {msg}", file=sys.stderr)
    raise SystemExit(1)


def find_openocd(explicit: str | None) -> str:
    for cand in (explicit, os.environ.get("MIT_MOTOR_OPENOCD"), os.environ.get("OPENOCD")):
        if not cand:
            continue
        p = Path(cand)
        if p.is_dir():
            for name in ("openocd.exe", "openocd"):
                if (p / name).is_file():
                    return str(p / name)
            die(f"目录里没有 openocd 可执行文件: {cand}")
        if p.is_file() or shutil.which(cand):
            return cand
        die(f"指定的 openocd 不存在: {cand}")
    found = shutil.which("openocd")
    if found:
        return found
    guesses = [
        Path.home() / "scoop/apps/openocd/current/bin/openocd.exe",
        Path("C:/Program Files/OpenOCD/bin/openocd.exe"),
        Path("/usr/bin/openocd"),
        Path("/usr/local/bin/openocd"),
    ]
    for g in guesses:
        if g.is_file():
            return str(g)
    die("找不到 openocd；用 --openocd 指定，或设置 MIT_MOTOR_OPENOCD")


def find_jlink_dir(explicit: str | None) -> str | None:
    for cand in (explicit, os.environ.get("MIT_MOTOR_JLINK_DIR"), os.environ.get("JLINK_DIR")):
        if cand:
            p = Path(cand)
            if (p / "JLinkARM.dll").is_file():
                return str(p)
            die(f"指定的 J-Link 目录里没有 JLinkARM.dll: {cand}")
    for pattern in (
        "C:/Program Files/SEGGER/JLink*",
        "C:/Program Files (x86)/SEGGER/JLink*",
        "/opt/SEGGER/JLink*",
    ):
        hits = sorted(
            (Path(p) for p in glob.glob(pattern) if (Path(p) / "JLinkARM.dll").is_file()),
            key=lambda p: p.name,
            reverse=True,
        )
        if hits:
            return str(hits[0])
    return None


def openocd_env(jlink_dir: str | None) -> dict:
    env = os.environ.copy()
    if jlink_dir:
        env["PATH"] = jlink_dir + os.pathsep + env.get("PATH", "")
    return env


def load_settings() -> dict:
    """工程级 openocd 配置；缺省回落到本仓默认的 J-Link + STM32G4。"""
    settings = {
        "board": "",
        "interface": IFACE_CFG,
        "target": TARGET_CFG,
        "transport": "swd",
        "adapter_speed": DEFAULT_SPEED,
    }
    if PROJECT_CFG.is_file():
        try:
            data = json.loads(PROJECT_CFG.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            print(f"warn : {PROJECT_CFG} 读取失败({error})，用默认值", file=sys.stderr)
            data = {}
        openocd = data.get("openocd") or {}
        for key in ("board", "interface", "target", "transport"):
            value = openocd.get(key)
            if value:
                settings[key] = str(value)
        if openocd.get("adapter_speed"):
            settings["adapter_speed"] = int(openocd["adapter_speed"])
    return settings


def run(argv: list[str], *, env: dict | None = None, cwd: Path | None = None,
        timeout: float = 120.0, echo: bool = False) -> tuple[int, str]:
    """跑一条命令，边跑边回显（echo=True），同时收集完整输出供解析。"""
    try:
        proc = subprocess.Popen(
            argv, cwd=str(cwd) if cwd else None, env=env,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, errors="replace", bufsize=1,
        )
    except FileNotFoundError:
        die(f"命令不存在: {argv[0]}")

    timed_out = threading.Event()

    def _kill() -> None:
        timed_out.set()
        proc.kill()

    watchdog = threading.Timer(timeout, _kill)
    watchdog.start()
    chunks: list[str] = []
    try:
        assert proc.stdout is not None
        for line in proc.stdout:
            chunks.append(line)
            if echo:
                sys.stdout.write(line)
                sys.stdout.flush()
        code = proc.wait()
    finally:
        watchdog.cancel()
    if timed_out.is_set():
        die(f"命令超时({timeout:.0f}s): {' '.join(argv)}")
    return code, "".join(chunks)


def openocd_argv(openocd: str, settings: dict, speed: int,
                 commands: list[str]) -> list[str]:
    argv = [openocd]
    if settings["board"]:
        # board/*.cfg 自带 interface+target+transport，再选 transport 会报
        # "Can't change session's transport"。
        argv += ["-f", settings["board"]]
        argv += ["-c", f"adapter speed {speed}"]
    else:
        # transport 必须在 target cfg 之前选定：target cfg 会触发 jtag 默认自动选择。
        argv += ["-f", settings["interface"]]
        argv += ["-c", f"transport select {settings['transport']}; adapter speed {speed}"]
        argv += ["-f", settings["target"]]
    for cmd in commands:
        argv += ["-c", cmd]
    return argv


def openocd_call(openocd: str, jlink_dir: str | None, settings: dict, speed: int,
                 commands: list[str], timeout: float = 60.0,
                 echo: bool = False) -> str:
    argv = openocd_argv(openocd, settings, speed, commands)
    code, out = run(argv, env=openocd_env(jlink_dir), timeout=timeout, echo=echo)
    if code != 0:
        die(f"openocd 退出码 {code}")
    return out


def settings_banner(settings: dict, speed: int) -> str:
    target = settings["board"] or f"{settings['interface']} + {settings['target']}"
    return f"probe: {target}  transport={settings['transport']}  speed={speed} kHz"


def probe(openocd: str, jlink_dir: str | None, settings: dict, speed: int) -> dict:
    out = openocd_call(openocd, jlink_dir, settings, speed,
                       ["init", "reset halt", "flash probe 0", "exit"])
    if "cannot read IDR" in out or "all zeroes" in out:
        die("SWD 无应答（DP IDR 读不到）。先确认板子已上电："
            "24 V 母线 -> 5 V -> 3.3 V LDO，量 TP4 与 MCU pin 16/32；"
            "J-Link 的 VTarget 可能由适配器自己送出，不代表 MCU 有电。")

    m = re.search(r"device idcode = 0x([0-9A-Fa-f]+)", out)
    if not m:
        die(f"未解析到 device idcode：\n{out}")
    idcode = int(m.group(1), 16)
    dev_id = idcode & 0xFFF
    if dev_id != DEV_ID_G47X:
        die(f"IDCODE 0x{idcode:08X} DEV_ID=0x{dev_id:03X}，不是 STM32G47x/G48x(0x469)")

    flash = re.search(r"flash size = (\d+) KiB", out)
    rdp = re.search(r"RDP level (\d)", out)
    dpidr = re.search(r"SWD DPIDR (0x[0-9A-Fa-f]+)", out)
    info = {
        "idcode": idcode,
        "flash_kib": int(flash.group(1)) if flash else None,
        "rdp": int(rdp.group(1)) if rdp else None,
        "dpidr": dpidr.group(1) if dpidr else "?",
        "log": out,
    }
    print(f"chip : STM32G47x/G48x  idcode=0x{idcode:08X}  DPIDR={info['dpidr']}")
    print(f"flash: {info['flash_kib']} KiB  RDP level {info['rdp']}")
    if info["flash_kib"] not in (None, 128):
        print(f"warn : 实测 {info['flash_kib']} KiB，memory.x 按 128 KiB 链接", file=sys.stderr)
    if info["rdp"] != 0:
        die(f"RDP level {info['rdp']}，调试口受限；先解除读保护再烧录")
    return info


def elf_path(release: bool) -> Path:
    return TARGET_DIR / ("release" if release else "debug") / ELF_STEM


def build(release: bool) -> Path:
    argv = ["cargo", "build"] + (["--release"] if release else [])
    print(f"$ {' '.join(argv)}   (cwd=software/firmware)")
    code, _ = run(argv, cwd=FW, timeout=900.0, echo=True)
    if code != 0:
        die("cargo build 失败")
    elf = elf_path(release)
    if not elf.is_file():
        die(f"构建成功但找不到 ELF: {elf}")
    print(f"elf  : {elf}  ({elf.stat().st_size} B)")
    return elf


def flash(openocd: str, jlink_dir: str | None, settings: dict, speed: int,
          elf: Path) -> None:
    out = openocd_call(
        openocd, jlink_dir, settings, speed,
        [f'program "{elf.as_posix()}" verify reset exit'],
        timeout=300.0, echo=True,
    )
    if "Error:" in out or "** Verified OK **" not in out:
        die("烧录/校验失败（见上方 openocd 输出）")


def reset_target(openocd: str, jlink_dir: str | None, settings: dict,
                 speed: int) -> None:
    openocd_call(openocd, jlink_dir, settings, speed,
                 ["init", "reset run", "exit"], echo=True)
    print("reset: target running")


def main() -> None:
    ap = argparse.ArgumentParser(
        description="编译 mit-motor 固件并经 J-Link + OpenOCD 烧录/识别 STM32G474RB")
    ap.add_argument("--openocd", help="openocd 可执行文件路径")
    ap.add_argument("--jlink-dir", help="含 JLinkARM.dll 的 SEGGER 安装目录")
    ap.add_argument("--speed", type=int,
                    help=f"SWD 时钟 kHz（默认取 .embeddedskills/config.json，"
                         f"否则 {DEFAULT_SPEED}；jlink 驱动实际恒 2000）")

    sub = ap.add_subparsers(dest="cmd", required=True)
    p_build = sub.add_parser("build", help="cargo build 固件")
    p_build.add_argument("--debug", action="store_true", help="debug 构型（仅本机试验，勿上台架）")

    sub.add_parser("id", help="只识别芯片（不改 Flash；会短暂 halt 并复位 CPU）")

    p_flash = sub.add_parser("flash", help="编译并烧录（verify + reset）")
    p_flash.add_argument("--debug", action="store_true", help="debug 构型")
    p_flash.add_argument("--no-build", action="store_true", help="跳过编译，直接烧已有 ELF")
    p_flash.add_argument("--elf", help="直接指定 ELF 路径")

    sub.add_parser("reset", help="复位目标并让它继续运行（不写 Flash）")

    args = ap.parse_args()

    if args.cmd == "build":
        build(release=not args.debug)
        return

    settings = load_settings()
    speed = args.speed or settings["adapter_speed"]
    openocd = find_openocd(args.openocd)
    jlink_dir = find_jlink_dir(args.jlink_dir)
    if not jlink_dir:
        print("warn : 未找到 SEGGER JLinkARM.dll，若 openocd 报 'adapter driver jlink' 失败，"
              "请用 --jlink-dir 指定", file=sys.stderr)
    print(settings_banner(settings, speed))

    if args.cmd == "id":
        probe(openocd, jlink_dir, settings, speed)
        return

    if args.cmd == "reset":
        reset_target(openocd, jlink_dir, settings, speed)
        return

    # flash
    if args.elf:
        elf = Path(args.elf).resolve()
        if not elf.is_file():
            die(f"ELF 不存在: {elf}")
    elif args.no_build:
        elf = elf_path(not args.debug)
        if not elf.is_file():
            die(f"没有现成 ELF: {elf}（先跑 build 或去掉 --no-build）")
    else:
        elf = build(release=not args.debug)

    probe(openocd, jlink_dir, settings, speed)
    flash(openocd, jlink_dir, settings, speed, elf)


if __name__ == "__main__":
    main()
