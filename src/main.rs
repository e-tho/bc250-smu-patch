//! Apply the BC-250 SMU firmware patches.
//!
//! Reads patch sites from an embedded Intel HEX file, opens the SMU
//! debug gate if it is closed, and either reports each site or writes
//! the ones that differ and reads them back to verify.

mod apply;
mod error;
mod hex;
mod mailbox;
mod page;
mod smu;
mod sys;
mod transport;
mod unlock;

use std::env;
use std::fs;
use std::process::ExitCode;

use crate::apply::apply;
use crate::hex::parse_hex;
use crate::unlock::{LockedSmu, UnlockedSmu};

const EMBEDDED_HEX: &[u8] = include_bytes!("../patches.hex");

fn usage() {
    eprintln!("usage: bc250-smu-patch [--check] [--hex PATH]");
}

fn main() -> ExitCode {
    let mut hex_path: Option<String> = None;
    let mut check_only = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => check_only = true,
            "--hex" => hex_path = args.next(),
            "-h" | "--help" => {
                usage();
                return ExitCode::from(0);
            }
            other => {
                eprintln!("unknown argument: {other}");
                usage();
                return ExitCode::from(1);
            }
        }
    }

    if !sys::is_root() {
        eprintln!("needs root");
        return ExitCode::from(1);
    }

    let hex_text = match hex_path {
        Some(path) => match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cannot read {path}: {e}");
                return ExitCode::from(1);
            }
        },
        None => match std::str::from_utf8(EMBEDDED_HEX) {
            Ok(s) => s.to_string(),
            Err(e) => {
                eprintln!("embedded patches.hex is not valid UTF-8: {e}");
                return ExitCode::from(1);
            }
        },
    };

    let sites = match parse_hex(&hex_text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    if sites.is_empty() {
        eprintln!("no patch sites found");
        return ExitCode::from(1);
    }

    let mut locked = match LockedSmu::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    match locked.alive() {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("SMU not alive, cold power cycle required");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    }

    let gate_open = match locked.gate_open() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    let unlocked = if gate_open {
        println!("debug access already open");
        UnlockedSmu::from(locked)
    } else {
        println!("opening debug access");
        match locked.unlock() {
            Ok(u) => u,
            Err(e) => {
                eprintln!("unlock failed: {e}");
                return ExitCode::from(1);
            }
        }
    };

    match apply(unlocked, &sites, check_only) {
        Ok(true) => ExitCode::from(0),
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
