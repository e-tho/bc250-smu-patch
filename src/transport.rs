//! SMN access through the PCI config space of the root complex. The
//! pair of registers at offsets 0xB8 and 0xBC form an index/data
//! window that reaches the SMU register space.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;

use crate::error::Error;
use crate::sys::{flock, pread, pwrite, LOCK_EX, LOCK_UN};

const PCI_CONFIG_PATH: &str = "/sys/bus/pci/devices/0000:00:00.0/config";
const SMN_INDEX_OFF: i64 = 0xB8;
const SMN_DATA_OFF: i64 = 0xBC;

pub(crate) struct Transport {
    file: File,
}

impl Transport {
    pub(crate) fn open() -> Result<Self, Error> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0o2000000) // O_CLOEXEC
            .open(PCI_CONFIG_PATH)?;
        Ok(Self { file })
    }

    pub(crate) fn smn_read(&mut self, addr: u32) -> Result<u32, Error> {
        let fd = self.file.as_raw_fd();
        if unsafe { flock(fd, LOCK_EX) } != 0 {
            return Err(Error::Io(io::Error::last_os_error()));
        }
        let res = (|| -> Result<u32, Error> {
            let idx = addr.to_le_bytes();
            if unsafe { pwrite(fd, idx.as_ptr(), 4, SMN_INDEX_OFF) } != 4 {
                return Err(Error::Io(io::Error::last_os_error()));
            }
            let mut buf = [0u8; 4];
            if unsafe { pread(fd, buf.as_mut_ptr(), 4, SMN_DATA_OFF) } != 4 {
                return Err(Error::Io(io::Error::last_os_error()));
            }
            Ok(u32::from_le_bytes(buf))
        })();
        unsafe { flock(fd, LOCK_UN) };
        res
    }

    pub(crate) fn smn_write(&mut self, addr: u32, val: u32) -> Result<(), Error> {
        let fd = self.file.as_raw_fd();
        if unsafe { flock(fd, LOCK_EX) } != 0 {
            return Err(Error::Io(io::Error::last_os_error()));
        }
        let res = (|| -> Result<(), Error> {
            let idx = addr.to_le_bytes();
            if unsafe { pwrite(fd, idx.as_ptr(), 4, SMN_INDEX_OFF) } != 4 {
                return Err(Error::Io(io::Error::last_os_error()));
            }
            let data = val.to_le_bytes();
            if unsafe { pwrite(fd, data.as_ptr(), 4, SMN_DATA_OFF) } != 4 {
                return Err(Error::Io(io::Error::last_os_error()));
            }
            Ok(())
        })();
        unsafe { flock(fd, LOCK_UN) };
        res
    }
}
