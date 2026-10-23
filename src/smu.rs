//! Queue 2 and queue 3, with the messages and primitives the patcher
//! uses: liveness, debug gate checks, byte and word access through the
//! gated path, the transfer-engine operations, and the ring-buffer
//! append.

use crate::error::Error;
use crate::mailbox::{Mailbox, SMU_RETURN_OK};
use crate::page::PinnedPage;
use crate::transport::Transport;

// Queue 3 mailbox for control messages and secure-access operations.
const Q3_CMD_ADDR: u32 = 0x03B10A20;
const Q3_RSP_ADDR: u32 = 0x03B10A80;
const Q3_ARG_ADDR: u32 = 0x03B10A88;

// Queue 2 mailbox for the transfer engine and ring-buffer commands.
const Q2_CMD_ADDR: u32 = 0x03B10528;
const Q2_RSP_ADDR: u32 = 0x03B10564;
const Q2_ARG_ADDR: u32 = 0x03B10998;

// Message ids. MSG_TEST goes to queue 3; the Q2_ ones go to queue 2.
const MSG_TEST: u8 = 0x01;
const MSG_Q2_TRANSFER: u8 = 0x0A;
const MSG_Q2_APPEND: u8 = 0x23;

// Queue 3 secure-access messages.
const SEC_SET_WRITE_PTR: u8 = 0x28;
const SEC_WRITE_THROUGH32: u8 = 0x29;
const SEC_READ_MEM64: u8 = 0x2A;

// Transfer-engine sub-operations, sent as the first argument of a
// queue 2 message 0x0A.
const TR_SMU2DRAM: u32 = 0x14;
const TR_SRAM_LOAD: u32 = 0x1F;
const TR_DRAM2SMU: u32 = 0x23;

// The transfer engine accepts up to this many 32-bit words per
// transfer operation. Reads of larger spans are chunked.
const SRAM_CHUNK_WORDS: usize = 18;

pub(crate) struct Smu {
    transport: Transport,
}

impl Smu {
    pub(crate) fn open() -> Result<Self, Error> {
        Ok(Self {
            transport: Transport::open()?,
        })
    }

    fn q3(&mut self) -> Mailbox<'_> {
        Mailbox::new(
            &mut self.transport,
            3,
            Q3_CMD_ADDR,
            Q3_RSP_ADDR,
            Q3_ARG_ADDR,
        )
    }

    fn q2(&mut self) -> Mailbox<'_> {
        Mailbox::new(
            &mut self.transport,
            2,
            Q2_CMD_ADDR,
            Q2_RSP_ADDR,
            Q2_ARG_ADDR,
        )
    }

    /// True if the SMU answers its test message with argument plus
    /// one. A mailbox timeout counts as not alive; other errors
    /// propagate.
    pub(crate) fn alive(&mut self) -> Result<bool, Error> {
        match self.q3().send(MSG_TEST, &[0x5EED0000, 0, 0, 0, 0, 0]) {
            Ok((st, ret)) => Ok(st == SMU_RETURN_OK && ret == 0x5EED0001),
            Err(Error::MailboxTimeout { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// True if the debug gate is open. Sends message 0x2A with a zero
    /// argument and checks the response status: anything except 0xFD
    /// (precondition rejected) means the gate is open.
    pub(crate) fn gate_open(&mut self) -> Result<bool, Error> {
        let (st, _) = self.q3().send(SEC_READ_MEM64, &[0; 6])?;
        Ok(st != crate::mailbox::SMU_RETURN_REJECTED_PREREQ)
    }

    /// 32-bit read through the gated SMN window. A status other than
    /// 0xFD means the returned value is meaningful.
    pub(crate) fn sec_smn_read32(&mut self, addr: u32) -> Result<(u8, u32), Error> {
        self.q3().send(SEC_READ_MEM64, &[addr, 0, 0, 0, 0, 0])
    }

    fn send_ok_q3(&mut self, msg: u8, arg: u32) -> Result<(), Error> {
        self.q3().send_ok(msg, &[arg, 0, 0, 0, 0, 0])
    }

    fn q2_ok(&mut self, msg: u8, args: &[u32; 6]) -> Result<(), Error> {
        self.q2().send_ok(msg, args)
    }

    /// Queue 2 sub-operation 0x1F: copy `words` 32-bit words from SMU
    /// memory at `src` into the key-3 transfer slot.
    fn tr_sram_load(&mut self, src: u32, words: u32) -> Result<(), Error> {
        self.q2_ok(MSG_Q2_TRANSFER, &[TR_SRAM_LOAD, 0, src, words, 0, 0])
    }

    /// Queue 2 sub-operation 0x14: copy the key-3 transfer slot to DRAM
    /// at the physical address in `hi`:`lo`.
    pub(crate) fn tr_smu2dram(&mut self, hi: u32, lo: u32, words: u32) -> Result<(), Error> {
        self.q2_ok(MSG_Q2_TRANSFER, &[TR_SMU2DRAM, hi, lo, words, 0, 0])
    }

    /// Queue 2 sub-operation 0x23: copy `words` 32-bit words from DRAM at
    /// `hi`:`lo` into the transfer slot identified by `key`.
    pub(crate) fn tr_dram2smu(
        &mut self,
        hi: u32,
        lo: u32,
        words: u32,
        key: u32,
    ) -> Result<(), Error> {
        self.q2_ok(MSG_Q2_TRANSFER, &[TR_DRAM2SMU, hi, lo, words, 0, key])
    }

    pub(crate) fn q2_append(&mut self, args: &[u32; 6]) -> Result<(), Error> {
        self.q2_ok(MSG_Q2_APPEND, args)
    }

    /// Read `len` bytes from SMU memory at `addr`. Reads are dword
    /// granular: unaligned spans are rounded out, fetched as full
    /// words, then sliced back down. Spans longer than
    /// SRAM_CHUNK_WORDS are chunked to honour the transfer engine's
    /// per-operation limit.
    pub(crate) fn read_bytes(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, Error> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let end = addr.checked_add(len as u32).ok_or_else(|| {
            Error::Page(format!("read_bytes: 0x{addr:08X} + {len} overflows u32"))
        })?;
        let aligned_start = addr & !3;
        let aligned_end = end.checked_add(3).map(|e| e & !3).unwrap_or(end);
        let total_words = ((aligned_end - aligned_start) / 4) as usize;

        let mut buf: Vec<u8> = Vec::with_capacity(total_words * 4);
        let mut off = 0usize;
        while off < total_words {
            let chunk = std::cmp::min(SRAM_CHUNK_WORDS, total_words - off);
            let word_addr = aligned_start + 4 * off as u32;
            let page = PinnedPage::new()?;
            self.tr_sram_load(word_addr, chunk as u32)?;
            self.tr_smu2dram((page.phys >> 32) as u32, page.phys as u32, chunk as u32)?;
            buf.extend_from_slice(&page.as_slice()[..chunk * 4]);
            off += chunk;
        }

        let skip = (addr - aligned_start) as usize;
        Ok(buf[skip..skip + len].to_vec())
    }

    /// 32-bit store to an SMU-local address through the gated path.
    pub(crate) fn write32(&mut self, addr: u32, value: u32) -> Result<(), Error> {
        self.send_ok_q3(SEC_SET_WRITE_PTR, addr)?;
        self.send_ok_q3(SEC_WRITE_THROUGH32, value)?;
        Ok(())
    }

    /// Byte-granular write through the gated path. Each byte is a
    /// read-modify-write of the containing dword so surrounding
    /// bytes, including those of an unaligned span, are preserved.
    pub(crate) fn write_bytes(&mut self, addr: u32, data: &[u8]) -> Result<(), Error> {
        for (j, &b) in data.iter().enumerate() {
            let a = addr.checked_add(j as u32).ok_or_else(|| {
                Error::Page(format!("write_bytes: 0x{addr:08X} + {} overflows u32", j))
            })?;
            let aligned = a & !3;
            let shift = (a & 3) * 8;
            let cur = self.read_bytes(aligned, 4)?;
            let dword = u32::from_le_bytes([cur[0], cur[1], cur[2], cur[3]]);
            let new = (dword & !(0xFFu32 << shift)) | ((b as u32) << shift);
            self.write32(aligned, new)?;
        }
        Ok(())
    }
}
