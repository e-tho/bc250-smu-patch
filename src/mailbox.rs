//! One mailbox per queue: a command register, a response register,
//! and six argument registers. A message is written to the argument
//! registers, the response register is cleared, the command register
//! is written, and the response register is polled until it holds a
//! known status byte.

use std::time::{Duration, Instant};

use crate::error::Error;
use crate::transport::Transport;

pub(crate) const SMU_RETURN_OK: u8 = 0x01;
pub(crate) const SMU_RETURN_FAILED: u8 = 0xFF;
pub(crate) const SMU_RETURN_UNKNOWN_CMD: u8 = 0xFE;
pub(crate) const SMU_RETURN_REJECTED_PREREQ: u8 = 0xFD;
pub(crate) const SMU_RETURN_REJECTED_BUSY: u8 = 0xFC;

const DONE_STATUSES: [u8; 5] = [
    SMU_RETURN_OK,
    SMU_RETURN_FAILED,
    SMU_RETURN_UNKNOWN_CMD,
    SMU_RETURN_REJECTED_PREREQ,
    SMU_RETURN_REJECTED_BUSY,
];

// Normal responses complete in a few hundred microseconds; the 5 s
// budget lets a wedged mailbox fail cleanly instead of hanging.
const MAILBOX_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(1);

pub(crate) struct Mailbox<'a> {
    transport: &'a mut Transport,
    queue: u8,
    cmd: u32,
    rsp: u32,
    arg: u32,
}

impl<'a> Mailbox<'a> {
    pub(crate) fn new(
        transport: &'a mut Transport,
        queue: u8,
        cmd: u32,
        rsp: u32,
        arg: u32,
    ) -> Self {
        Self {
            transport,
            queue,
            cmd,
            rsp,
            arg,
        }
    }

    /// Send one message and return its status byte and first response
    /// word. All six argument registers are written each time so stale
    /// values from a previous message cannot leak into the handler.
    pub(crate) fn send(&mut self, msg: u8, args: &[u32; 6]) -> Result<(u8, u32), Error> {
        self.transport.smn_write(self.rsp, 0)?;
        for (i, a) in args.iter().enumerate() {
            self.transport.smn_write(self.arg + 4 * i as u32, *a)?;
        }
        self.transport.smn_write(self.cmd, msg as u32)?;

        let deadline = Instant::now() + MAILBOX_TIMEOUT;
        while Instant::now() < deadline {
            let st = self.transport.smn_read(self.rsp)? as u8;
            if DONE_STATUSES.contains(&st) {
                let ret = self.transport.smn_read(self.arg)?;
                return Ok((st, ret));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        Err(Error::MailboxTimeout {
            queue: self.queue,
            msg,
        })
    }

    /// Send one message and require an OK status.
    pub(crate) fn send_ok(&mut self, msg: u8, args: &[u32; 6]) -> Result<(), Error> {
        let (st, _) = self.send(msg, args)?;
        if st != SMU_RETURN_OK {
            return Err(Error::MailboxStatus {
                queue: self.queue,
                msg,
                status: st,
            });
        }
        Ok(())
    }
}
