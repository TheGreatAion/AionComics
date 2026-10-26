//! Where each file sits in a RAR, read from its headers (RAR 4 and RAR 5). unrar can only walk an
//! archive from the start, reading through every header up to the file it's after; this walks
//! only the headers, seeking past the data, once, so any page can be got at directly later:
//! - a page stored as it is: its bytes, straight out of the file, like a ZIP's;
//! - a compressed one that doesn't depend on the files before it (any archive but a solid one):
//!   its block alone, copied behind the archive's opening headers into a tiny archive of its own,
//!   which unrar unpacks in a moment (see `unpack_alone`).

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{bail, Result};

/// A file in the archive, in archive order.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub unpacked: u64,
    /// Its block: where its header starts, and its length with the data after it.
    pub block: (u64, u64),
    /// Where its bytes start, and how many there are, when they're stored as they are (not
    /// compressed, encrypted or split across volumes). None otherwise.
    pub stored: Option<(u64, u64)>,
    /// Unpackable by itself: not solid, encrypted or split across volumes.
    pub alone: bool,
}

/// What `entries` reads.
#[derive(Debug)]
pub struct Index {
    /// Where the archive's opening headers (its signature and main header) end.
    pub head: u64,
    pub files: Vec<Entry>,
}

const RAR4: &[u8] = b"Rar!\x1a\x07\x00";
const RAR5: &[u8] = b"Rar!\x1a\x07\x01\x00";

/// The files (not folders) in a RAR, in archive order. Err for an encrypted or unreadable one.
pub fn entries(path: &Path) -> Result<Index> {
    let mut f = BufReader::new(File::open(path)?);
    let mut sig = [0u8; 8];
    f.read_exact(&mut sig)?;
    if &sig[..7] == RAR4 {
        f.seek(SeekFrom::Start(7))?;
        rar4(&mut f)
    } else if sig == RAR5 {
        rar5(&mut f)
    } else {
        bail!("not a RAR archive")
    }
}

fn u16le(b: &[u8]) -> u64 {
    u16::from_le_bytes([b[0], b[1]]) as u64
}
fn u32le(b: &[u8]) -> u64 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64
}

fn rar4(f: &mut BufReader<File>) -> Result<Index> {
    let (mut files, mut head) = (Vec::new(), 0);
    loop {
        let start = f.stream_position()?;
        let mut lead = [0u8; 7];
        if f.read_exact(&mut lead).is_err() {
            break;
        }
        let (kind, flags, size) = (lead[2], u16le(&lead[3..5]), u16le(&lead[5..7]));
        if size < 7 {
            bail!("bad RAR header");
        }
        let mut rest = vec![0u8; size as usize - 7];
        f.read_exact(&mut rest)?;
        // Blocks with data after the header say how much (ADD_SIZE, the first four bytes).
        let mut data = if flags & 0x8000 != 0 && rest.len() >= 4 { u32le(&rest[0..4]) } else { 0 };
        match kind {
            0x73 if flags & 0x0080 != 0 => bail!("encrypted headers"),
            0x73 => head = start + size + data,
            0x74 => {
                if rest.len() < 25 {
                    bail!("short RAR file header");
                }
                let (mut packed, mut unpacked) = (u32le(&rest[0..4]), u32le(&rest[4..8]));
                let method = rest[18];
                if flags & 0x0100 != 0 && rest.len() >= 33 {
                    packed |= u32le(&rest[25..29]) << 32;
                    unpacked |= u32le(&rest[29..33]) << 32;
                }
                data = packed;
                let folder = flags & 0x00e0 == 0x00e0;
                // Split across volumes (0x01, 0x02), encrypted (0x04) or solid (0x10): not by itself.
                let alone = flags & 0x0017 == 0;
                let stored = (alone && method == 0x30).then_some((start + size, packed));
                if !folder {
                    files.push(Entry { unpacked, block: (start, size + packed), stored, alone });
                }
            }
            0x7b => break, // end of archive
            _ => {}
        }
        f.seek(SeekFrom::Start(start + size + data))?;
    }
    Ok(Index { head, files })
}

/// A RAR 5 variable-length number, and how many bytes it took.
fn vint(b: &[u8]) -> Result<(u64, usize)> {
    let mut v = 0u64;
    for (i, &x) in b.iter().enumerate().take(10) {
        v |= ((x & 0x7f) as u64) << (7 * i);
        if x & 0x80 == 0 {
            return Ok((v, i + 1));
        }
    }
    bail!("bad RAR number")
}

/// The vint at `h[*at]`, moving past it.
fn take(h: &[u8], at: &mut usize) -> Result<u64> {
    let (v, n) = vint(h.get(*at..).unwrap_or(&[]))?;
    *at += n;
    Ok(v)
}

/// Whether a RAR 5 extra area holds a record of type 1 (file encryption).
fn encrypted(area: &[u8]) -> bool {
    let mut k = 0;
    while k < area.len() {
        let Ok((len, n)) = vint(&area[k..]) else { return false };
        if vint(&area[k + n..]).is_ok_and(|(t, _)| t == 1) {
            return true;
        }
        k += n + len as usize;
    }
    false
}

fn rar5(f: &mut BufReader<File>) -> Result<Index> {
    let (mut files, mut head) = (Vec::new(), 0);
    loop {
        let start = f.stream_position()?;
        // CRC32, then the header's size as a vint (at most 3 bytes for a header under 2 MB).
        let mut lead = [0u8; 7];
        let got = f.read(&mut lead)?;
        if got < 5 {
            break;
        }
        let (size, n) = vint(&lead[4..got])?;
        let body_at = start + 4 + n as u64;
        f.seek(SeekFrom::Start(body_at))?;
        let mut h = vec![0u8; size as usize];
        f.read_exact(&mut h)?;
        let mut at = 0;
        let kind = take(&h, &mut at)?;
        let flags = take(&h, &mut at)?;
        let extra = if flags & 1 != 0 { take(&h, &mut at)? } else { 0 };
        let data = if flags & 2 != 0 { take(&h, &mut at)? } else { 0 };
        let data_at = body_at + size;
        match kind {
            1 => head = data_at + data,
            4 => bail!("encrypted archive"),
            2 => {
                let file_flags = take(&h, &mut at)?;
                let unpacked = take(&h, &mut at)?;
                let _attributes = take(&h, &mut at)?;
                if file_flags & 2 != 0 {
                    at += 4; // modification time
                }
                if file_flags & 4 != 0 {
                    at += 4; // CRC32
                }
                let compression = take(&h, &mut at)?;
                let method = (compression >> 7) & 7;
                let solid = compression & 0x40 != 0;
                // Split across volumes: header flags 0x08 (continues from) and 0x10 (continues in).
                let split = flags & 0x18 != 0;
                let locked = extra > 0 && encrypted(&h[h.len().saturating_sub(extra as usize)..]);
                let alone = !solid && !split && !locked;
                let stored = (alone && method == 0).then_some((data_at, data));
                if file_flags & 1 == 0 {
                    files.push(Entry { unpacked, block: (start, data_at + data - start), stored, alone });
                }
            }
            5 => break, // end of archive
            _ => {}
        }
        f.seek(SeekFrom::Start(data_at + data))?;
    }
    Ok(Index { head, files })
}

/// Unpack one file by itself (see `Entry::alone`): its block copied behind the archive's opening
/// headers into `tmp`, a tiny archive of its own that unrar reads straight through.
pub fn unpack_alone(path: &Path, head: u64, block: (u64, u64), tmp: &Path) -> Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut bytes = vec![0u8; (head + block.1) as usize];
    f.read_exact(&mut bytes[..head as usize])?;
    f.seek(SeekFrom::Start(block.0))?;
    f.read_exact(&mut bytes[head as usize..])?;
    std::fs::write(tmp, &bytes)?;
    let res = (|| {
        let arc = unrar::Archive::new(tmp).open_for_processing()?;
        match arc.read_header()? {
            Some(h) => Ok(h.read()?.0),
            None => bail!("nothing in the copied block"),
        }
    })();
    let _ = std::fs::remove_file(tmp);
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rar5_numbers() {
        assert_eq!(vint(&[0x05]).unwrap(), (5, 1));
        assert_eq!(vint(&[0x80 | 0x2c, 0x02]).unwrap(), (0x2c + (2 << 7), 2));
        assert!(vint(&[0x80, 0x80]).is_err());
    }

    /// AION_RAR=file: print what the headers say about it, and time getting at pages directly.
    #[test]
    #[ignore]
    fn show_rar() {
        let p = std::path::PathBuf::from(std::env::var("AION_RAR").unwrap());
        let t = std::time::Instant::now();
        let ix = entries(&p).unwrap();
        let es = &ix.files;
        let stored = es.iter().filter(|e| e.stored.is_some()).count();
        let alone = es.iter().filter(|e| e.alone).count();
        println!("headers read in {:?}: head {}, {} files, {stored} stored, {alone} alone", t.elapsed(), ix.head, es.len());
        let tmp = std::env::temp_dir().join("aion-rar-show.rar");
        for e in es.iter().step_by((es.len() / 4).max(1)) {
            let t = std::time::Instant::now();
            let got = unpack_alone(&p, ix.head, e.block, &tmp).map(|b| b.len());
            println!("  {e:?}: {got:?} in {:?}", t.elapsed());
        }
    }

    /// Every CBR under AION_TEST_ROOT: as many files as unrar finds, the same sizes, and pages got
    /// at directly (a few per book) match unrar's CRC.
    #[test]
    #[ignore]
    fn real_rars() {
        let root = std::path::PathBuf::from(std::env::var("AION_TEST_ROOT").unwrap());
        let tmp = std::env::temp_dir().join("aion-rar-alone.rar");
        let (mut books, mut direct, mut pages) = (0, 0, 0);
        for e in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            let p = e.path();
            if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("cbr")) || crate::archive::detect(p).ok() != Some(crate::archive::Kind::Rar) {
                continue;
            }
            let Ok(listing) = unrar::Archive::new(p).open_for_listing() else { continue };
            let theirs: Vec<(u64, u32)> = listing.filter_map(|h| h.ok()).filter(|h| h.is_file()).map(|h| (h.unpacked_size, h.file_crc)).collect();
            let ix = match entries(p) {
                Ok(ix) => ix,
                Err(e) => {
                    println!("can't read headers of {}: {e}", p.display());
                    continue;
                }
            };
            let mine = &ix.files;
            assert_eq!(mine.len(), theirs.len(), "{}", p.display());
            for (m, t) in mine.iter().zip(&theirs) {
                assert_eq!(m.unpacked, t.0, "{}", p.display());
            }
            books += 1;
            if mine.iter().all(|m| m.alone) {
                direct += 1;
            }
            let mut f = File::open(p).unwrap();
            for (m, t) in mine.iter().zip(&theirs).step_by(41) {
                let buf = if let Some((at, len)) = m.stored {
                    let mut buf = vec![0; len as usize];
                    f.seek(SeekFrom::Start(at)).unwrap();
                    f.read_exact(&mut buf).unwrap();
                    buf
                } else if m.alone {
                    unpack_alone(p, ix.head, m.block, &tmp).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
                } else {
                    continue;
                };
                assert_eq!(crc32fast::hash(&buf), t.1, "{} at {:?}", p.display(), m.block);
                pages += 1;
            }
            if books % 100 == 0 {
                println!("{books} CBRs checked, {direct} with every page reachable directly, {pages} pages checked");
            }
        }
        println!("{books} CBRs checked, {direct} with every page reachable directly, {pages} pages checked");
    }
}
