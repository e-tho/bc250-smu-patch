//! Bindings to the libc functions the tool uses.

extern "C" {
    pub(crate) fn mmap(
        addr: *mut u8,
        len: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        off: i64,
    ) -> *mut u8;
    pub(crate) fn munmap(addr: *mut u8, len: usize) -> i32;
    pub(crate) fn mlock(addr: *mut u8, len: usize) -> i32;
    pub(crate) fn munlock(addr: *mut u8, len: usize) -> i32;
    fn sysconf(name: i32) -> i64;
    fn geteuid() -> u32;
    pub(crate) fn pread(fd: i32, buf: *mut u8, count: usize, offset: i64) -> isize;
    pub(crate) fn pwrite(fd: i32, buf: *const u8, count: usize, offset: i64) -> isize;
    pub(crate) fn flock(fd: i32, op: i32) -> i32;
}

pub(crate) const LOCK_EX: i32 = 2;
pub(crate) const LOCK_UN: i32 = 8;
pub(crate) const PROT_READ: i32 = 1;
pub(crate) const PROT_WRITE: i32 = 2;
pub(crate) const MAP_PRIVATE: i32 = 2;
pub(crate) const MAP_ANONYMOUS: i32 = 0x20;
const SC_PAGESIZE: i32 = 30;

pub(crate) fn is_root() -> bool {
    unsafe { geteuid() == 0 }
}

pub(crate) fn page_size() -> usize {
    unsafe { sysconf(SC_PAGESIZE) as usize }
}
