//! A zip file, stored rather than compressed.
//!
//! The sprite editor hands out PNGs, which are already deflated; running them
//! through a second round of deflate would cost code and save nothing. So this
//! writes the one archive format every desktop opens without any compression
//! at all, which is a header, the bytes, and a table at the end.

/// One file, remembered until the central directory is written.
struct Entry {
    name: Vec<u8>,
    crc: u32,
    size: u32,
    /// Where this file's local header starts, from the front of the archive.
    at: u32,
}

#[derive(Default)]
pub struct Zip {
    out: Vec<u8>,
    entries: Vec<Entry>,
}

/// Zip carries no timezone, so anything with a clock in it would make two runs
/// of the same export differ. Everything is stamped with the epoch the format
/// counts from instead.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0x0021;

/// Names are written as UTF-8, which the format only allows if this says so.
const UTF8_NAMES: u16 = 1 << 11;

impl Zip {
    pub fn new() -> Zip {
        Zip::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn add(&mut self, name: &str, data: &[u8]) {
        let name = name.as_bytes().to_vec();
        let entry = Entry {
            crc: crc32(data),
            size: data.len() as u32,
            at: self.out.len() as u32,
            name,
        };
        self.out.extend_from_slice(b"PK\x03\x04");
        self.out.extend_from_slice(&20u16.to_le_bytes());
        self.out.extend_from_slice(&UTF8_NAMES.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&DOS_TIME.to_le_bytes());
        self.out.extend_from_slice(&DOS_DATE.to_le_bytes());
        self.out.extend_from_slice(&entry.crc.to_le_bytes());
        // Stored, so the two sizes are the same number twice.
        self.out.extend_from_slice(&entry.size.to_le_bytes());
        self.out.extend_from_slice(&entry.size.to_le_bytes());
        self.out.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&entry.name);
        self.out.extend_from_slice(data);
        self.entries.push(entry);
    }

    pub fn finish(mut self) -> Vec<u8> {
        let start = self.out.len() as u32;
        for e in &self.entries {
            self.out.extend_from_slice(b"PK\x01\x02");
            self.out.extend_from_slice(&20u16.to_le_bytes());
            self.out.extend_from_slice(&20u16.to_le_bytes());
            self.out.extend_from_slice(&UTF8_NAMES.to_le_bytes());
            self.out.extend_from_slice(&0u16.to_le_bytes());
            self.out.extend_from_slice(&DOS_TIME.to_le_bytes());
            self.out.extend_from_slice(&DOS_DATE.to_le_bytes());
            self.out.extend_from_slice(&e.crc.to_le_bytes());
            self.out.extend_from_slice(&e.size.to_le_bytes());
            self.out.extend_from_slice(&e.size.to_le_bytes());
            self.out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            for _ in 0..3 {
                self.out.extend_from_slice(&0u16.to_le_bytes());
            }
            self.out.extend_from_slice(&0u16.to_le_bytes());
            self.out.extend_from_slice(&0u32.to_le_bytes());
            self.out.extend_from_slice(&e.at.to_le_bytes());
            self.out.extend_from_slice(&e.name);
        }
        let size = self.out.len() as u32 - start;
        let n = self.entries.len() as u16;
        self.out.extend_from_slice(b"PK\x05\x06");
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&n.to_le_bytes());
        self.out.extend_from_slice(&n.to_le_bytes());
        self.out.extend_from_slice(&size.to_le_bytes());
        self.out.extend_from_slice(&start.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out
    }
}

/// The reflected CRC-32 the format uses, worked a byte at a time off a table
/// built on first use.
pub fn crc32(data: &[u8]) -> u32 {
    let table = table();
    let mut crc = !0u32;
    for b in data {
        crc = (crc >> 8) ^ table[((crc ^ *b as u32) & 0xff) as usize];
    }
    !crc
}

fn table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    table
}

/// Standard base64, for turning a data URL back into the bytes behind it.
pub fn from_base64(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in text.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = ALPHABET.iter().position(|a| *a == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}
