//! A page of anonymous memory locked in RAM, whose physical address is
//! resolved through `/proc/self/pagemap`. The SMU transfer engine moves
//! data to and from these pages by physical address.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use crate::error::Error;
use crate::sys::{
    mlock, mmap, munlock, munmap, page_size, MAP_ANONYMOUS, MAP_PRIVATE, PROT_READ, PROT_WRITE,
};

pub(crate) struct PinnedPage {
    ptr: *mut u8,
    pub(crate) phys: u64,
    size: usize,
}

impl PinnedPage {
    pub(crate) fn new() -> Result<Self, Error> {
        let size = page_size();
        let ptr = mmap_anon(size)
            .ok_or_else(|| Error::Page(format!("mmap({size}): {}", io::Error::last_os_error())))?;
        if unsafe { mlock(ptr, size) } != 0 {
            let e = io::Error::last_os_error();
            unsafe { munmap(ptr, size) };
            return Err(Error::Page(format!("mlock: {e}")));
        }
        let phys = match virt_to_phys(ptr, size) {
            Ok(p) => p,
            Err(e) => {
                unsafe {
                    munlock(ptr, size);
                    munmap(ptr, size);
                }
                return Err(e);
            }
        };
        Ok(Self { ptr, phys, size })
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.size) }
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }
}

impl Drop for PinnedPage {
    fn drop(&mut self) {
        unsafe {
            munlock(self.ptr, self.size);
            munmap(self.ptr, self.size);
        }
    }
}

fn mmap_anon(size: usize) -> Option<*mut u8> {
    let p = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    // MAP_FAILED is (void *)-1.
    if p as usize == usize::MAX {
        None
    } else {
        Some(p)
    }
}

fn virt_to_phys(ptr: *mut u8, page: usize) -> Result<u64, Error> {
    let mut f = File::open("/proc/self/pagemap")?;
    let idx = (ptr as usize / page) as u64 * 8;
    f.seek(SeekFrom::Start(idx))?;
    let mut buf = [0u8; 8];
    f.read_exact(&mut buf)?;
    let entry = u64::from_le_bytes(buf);
    if entry & (1 << 63) == 0 {
        return Err(Error::Page("page not present in pagemap".into()));
    }
    Ok((entry & ((1 << 55) - 1)) * page as u64)
}
