//! Read every site. With `--check`, report whether each site matches;
//! otherwise, write the ones that differ and read each write back to
//! verify. Each site produces one line in one of these forms:
//!
//!   check {addr:05X} {cur} OK
//!   check {addr:05X} {cur} ** MISMATCH (want {new})
//!   apply {addr:05X} {cur} -> {got} OK
//!   apply {addr:05X} {cur} -> {got} ** VERIFY FAIL

use crate::error::Error;
use crate::hex::PatchSite;
use crate::unlock::UnlockedSmu;

pub(crate) fn apply(
    mut smu: UnlockedSmu,
    sites: &[PatchSite],
    check_only: bool,
) -> Result<bool, Error> {
    let mut ok = true;

    for site in sites {
        let cur = smu.read_bytes(site.addr, site.data.len())?;
        if check_only || cur == site.data {
            let status = if cur == site.data {
                "OK".to_string()
            } else {
                format!("** MISMATCH (want {})", hexstr(&site.data))
            };
            println!("check {:05X} {} {}", site.addr, hexstr(&cur), status);
            ok &= cur == site.data;
        } else {
            smu.write_bytes(site.addr, &site.data)?;
            let got = smu.read_bytes(site.addr, site.data.len())?;
            let status = if got == site.data {
                "OK".to_string()
            } else {
                "** VERIFY FAIL".to_string()
            };
            println!(
                "apply {:05X} {} -> {} {}",
                site.addr,
                hexstr(&cur),
                hexstr(&got),
                status
            );
            ok &= got == site.data;
        }
    }

    Ok(ok)
}

fn hexstr(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
