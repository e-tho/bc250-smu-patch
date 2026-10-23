//! Intel HEX parsing. Only record types 0x00 (data) and 0x02
//! (extended segment address) affect the output; everything else is
//! ignored, and records are not coalesced.

use crate::error::Error;

pub(crate) struct PatchSite {
    pub(crate) addr: u32,
    pub(crate) data: Vec<u8>,
}

pub(crate) fn parse_hex(text: &str) -> Result<Vec<PatchSite>, Error> {
    let mut base: u32 = 0;
    let mut sites: Vec<PatchSite> = Vec::new();

    for (lineno, line) in text.lines().enumerate() {
        let n = lineno + 1;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with(':') {
            return Err(Error::Parse(format!("line {n}: missing ':'")));
        }
        let body = &line[1..];
        if body.len() % 2 != 0 {
            return Err(Error::Parse(format!("line {n}: odd number of hex digits")));
        }
        let mut bytes = Vec::with_capacity(body.len() / 2);
        for i in (0..body.len()).step_by(2) {
            let s = body
                .get(i..i + 2)
                .ok_or_else(|| Error::Parse(format!("line {n}: truncated hex")))?;
            let b = u8::from_str_radix(s, 16)
                .map_err(|_| Error::Parse(format!("line {n}: invalid hex '{s}'")))?;
            bytes.push(b);
        }
        if bytes.len() < 4 {
            return Err(Error::Parse(format!(
                "line {n}: record shorter than 4 bytes"
            )));
        }
        let count = bytes[0] as usize;
        let addr = ((bytes[1] as u32) << 8) | (bytes[2] as u32);
        let typ = bytes[3];
        if bytes.len() < 4 + count + 1 {
            return Err(Error::Parse(format!(
                "line {n}: truncated payload (count={count}, got {})",
                bytes.len() - 4
            )));
        }
        let data = &bytes[4..4 + count];

        match typ {
            0x00 => sites.push(PatchSite {
                addr: base + addr,
                data: data.to_vec(),
            }),
            0x02 if count == 2 => {
                base = (((data[0] as u32) << 8) | (data[1] as u32)) << 4;
            }
            _ => {}
        }
    }

    sites.sort_by(|a, b| a.addr.cmp(&b.addr).then_with(|| a.data.cmp(&b.data)));
    Ok(sites)
}
