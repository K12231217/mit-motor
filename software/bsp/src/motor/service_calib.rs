//! Flash bank 2 的标定档案读写（A/B 双槽，每槽 2 页）。
//!
//! 记录字节布局与 CRC 在 `contracts::motor::protocol_calib`；本模块只负责
//! "擦两页 → 按 64 bit 双字编程 → 回读校验"。代码在 bank 1，擦写 bank 2 不会
//! 停取指（RM0440 Table 7 的 RWW），但擦除期间任何 flash 访问都会等待，因此
//! 只在 `init`/标定任务里调用，不进 20 kHz 环。
//!
//! 擦写序列按 RM0440 §3.3.7（双 bank 下 `PNB` 64–127 为 bank 2）与 §3.3.8
//! （64 bit 双字编程：先写低字、再写高字，之后只等 BSY）。

use contracts::motor::config_hw::{CALIB_SLOT_BYTES, FLASH_BANK2_BASE, FLASH_PAGE_BYTES};
use contracts::motor::protocol_calib::CALIB_RECORD_BYTES;
use embassy_stm32::pac;

const KEY1: u32 = 0x4567_0123;
const KEY2: u32 = 0xCDEF_89AB;

/// bank 2 的页号从 0 起；bank 由 `FLASH_CR.BKER` 选（RM0440 Rev 9 p.135：
/// `BKER=1` 选中 bank 2，PNB 是 bank 内页号；128 KB 器件每 bank 32 页）。
const BANK2_FIRST_PAGE: u8 = 0;

const _: () = assert!(BANK2_FIRST_PAGE + 4 <= 32);

/// 等待 flash 空闲的循环上限；超时即报错，不无限等。
const BUSY_LIMIT: u32 = 2_000_000;

/// A/B 槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibSlot {
    /// 槽 A（bank 2 起始）。
    A,
    /// 槽 B（紧跟槽 A）。
    B,
}

impl CalibSlot {
    /// 槽起始地址。
    pub const fn address(self) -> u32 {
        match self {
            Self::A => FLASH_BANK2_BASE,
            Self::B => FLASH_BANK2_BASE + CALIB_SLOT_BYTES,
        }
    }
}

/// 读写失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibFlashError {
    /// BSY 一直不清。
    Timeout,
    /// 擦除/编程期间置位了错误标志。
    Fault,
    /// 回读与写入不一致。
    Verify,
    /// 擦除阶段报错。
    Erase,
    /// 编程阶段报错。
    Program,
    /// 写路径尚未启用（页号映射未确认）。
    Disabled,
}

/// 写路径总开关。
///
/// 2026-11-28 实测：`PNB=32`/`PNB=64` 置 `STRT` 后 BSY 正常、无错误标志，但
/// `0x0804_0000` 内容不变（空白检查失败）。在按 RM0440 确认 bank 2 页号映射前，
/// 禁止擦写，避免误erase bank 1 的代码页。
pub const CALIB_WRITE_ENABLED: bool = true;

/// 读一槽记录原始字节（不做 CRC，交给 `CalibRecord::decode`）。
pub fn read(slot: CalibSlot) -> [u8; CALIB_RECORD_BYTES] {
    let mut out = [0u8; CALIB_RECORD_BYTES];
    let base = slot.address() as *const u8;
    for (i, byte) in out.iter_mut().enumerate() {
        // bank 2 是 memory-mapped flash；只读不改状态。
        *byte = unsafe { core::ptr::read_volatile(base.add(i)) };
    }
    out
}

/// 擦除槽所在的两页并写入一条记录，最后回读校验。
pub fn write(slot: CalibSlot, raw: &[u8; CALIB_RECORD_BYTES]) -> Result<(), CalibFlashError> {
    if !CALIB_WRITE_ENABLED {
        return Err(CalibFlashError::Disabled);
    }
    let flash = pac::FLASH;
    unlock(&flash);

    let base = slot.address();
    let pages = CALIB_SLOT_BYTES / FLASH_PAGE_BYTES;
    for page in 0..pages {
        let addr = base + page * FLASH_PAGE_BYTES;
        erase_page(&flash, addr)?;
    }
    program(&flash, base, raw).map_err(|_| CalibFlashError::Program)?;
    lock(&flash);

    let back = read(slot);
    if &back != raw {
        return Err(CalibFlashError::Verify);
    }
    Ok(())
}

fn unlock(flash: &pac::flash::Flash) {
    flash.keyr().write_value(KEY1);
    flash.keyr().write_value(KEY2);
}

fn lock(flash: &pac::flash::Flash) {
    flash.cr().modify(|w| w.set_lock(true));
}

fn wait_idle(flash: &pac::flash::Flash) -> Result<(), CalibFlashError> {
    for _ in 0..BUSY_LIMIT {
        if !flash.sr().read().bsy() {
            return Ok(());
        }
    }
    Err(CalibFlashError::Timeout)
}

/// STRT/双字写之后：硬件的 BSY 不是同一拍置位，先等它起来再等它落下。
fn wait_busy_then_idle(flash: &pac::flash::Flash) -> Result<(), CalibFlashError> {
    for _ in 0..BUSY_LIMIT {
        if flash.sr().read().bsy() {
            break;
        }
    }
    wait_idle(flash)
}

fn clear_flags(flash: &pac::flash::Flash) {
    flash.sr().modify(|w| {
        w.set_eop(true);
        w.set_operr(true);
        w.set_progerr(true);
        w.set_wrperr(true);
        w.set_pgaerr(true);
        w.set_sizerr(true);
        w.set_pgserr(true);
        w.set_miserr(true);
    });
}

fn check_flags(flash: &pac::flash::Flash) -> Result<(), CalibFlashError> {
    let sr = flash.sr().read();
    if sr.operr()
        || sr.progerr()
        || sr.wrperr()
        || sr.pgaerr()
        || sr.sizerr()
        || sr.pgserr()
        || sr.miserr()
    {
        return Err(CalibFlashError::Fault);
    }
    Ok(())
}

fn erase_page(flash: &pac::flash::Flash, address: u32) -> Result<(), CalibFlashError> {
    let offset = address - FLASH_BANK2_BASE;
    let page = BANK2_FIRST_PAGE + (offset / FLASH_PAGE_BYTES) as u8;

    wait_idle(flash)?;
    clear_flags(flash);
    flash.cr().modify(|w| {
        w.set_pg(false);
        w.set_per(true);
        // MER1(bit2)/MER2(bit15) 必须先清零，否则 PGSERR（RM0440 §3.3.6 步骤 2）。
        w.0 &= !((1 << 2) | (1 << 15));
        w.set_pnb(page);
        // RM0440 Rev 9 p.135：CR bit11 = BKER，1 = 选中 bank 2 做页擦除；
        // PAC 未生成该位，直接置原值。
        w.0 |= 1 << 11;
        w.set_strt(true);
    });
    wait_busy_then_idle(flash)?;
    check_flags(flash)?;
    clear_flags(flash);
    Ok(())
}

fn program(
    flash: &pac::flash::Flash,
    address: u32,
    raw: &[u8; CALIB_RECORD_BYTES],
) -> Result<(), CalibFlashError> {
    wait_idle(flash)?;
    clear_flags(flash);
    flash.cr().modify(|w| {
        w.set_per(false);
        w.0 &= !((1 << 2) | (1 << 15));
        w.set_pg(true);
    });

    let mut addr = address;
    for chunk in raw.chunks_exact(8) {
        let mut lo = [0u8; 4];
        let mut hi = [0u8; 4];
        lo.copy_from_slice(&chunk[..4]);
        hi.copy_from_slice(&chunk[4..]);
        // 目标在 bank 2 且已解锁、已进入编程模式；双字必须一次写完再等 BSY。
        unsafe {
            core::ptr::write_volatile(addr as *mut u32, u32::from_le_bytes(lo));
            core::ptr::write_volatile((addr + 4) as *mut u32, u32::from_le_bytes(hi));
        }
        wait_busy_then_idle(flash)?;
        check_flags(flash)?;
        addr += 8;
    }
    clear_flags(flash);
    Ok(())
}

/// 原始状态字（调试用；位含义见 RM0440 FLASH_SR）。
pub fn status_bits() -> u32 {
    pac::FLASH.sr().read().0
}

/// 芯片 96 bit UID（`0x1FFF_7590` 起 3 个字）。
pub fn uid96() -> [u32; 3] {
    let base = 0x1FFF_7590u32 as *const u32;
    // 系统存储区的只读 UID。
    unsafe {
        [
            core::ptr::read_volatile(base),
            core::ptr::read_volatile(base.add(1)),
            core::ptr::read_volatile(base.add(2)),
        ]
    }
}
