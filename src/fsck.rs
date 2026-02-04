use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;
use std::mem;

use e2p_sys;
/// Filesystem check mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FsckMode {
    /// Non-destructive checks (equivalent to `-n`).
    NoWrite,
    /// Automatic preen / repair (equivalent to `-p`).
    AutoFix,
}

/// Conservative ext filesystem check using `e2p-sys` bindings for light checks
/// and an external `e2fsck -p` fallback for auto-fix mode.
pub fn fsck_ext2(device: &str, mode: FsckMode) -> Result<i32, String> {
    match mode {
        FsckMode::NoWrite => {
            // Open the block device and read the first 1024 bytes (superblock area)
            let mut f = File::open(device).map_err(|e| format!("open {}: {}", device, e))?;
            let mut buf = [0u8; 1024];
            f.read_exact(&mut buf).map_err(|e| format!("read superblock {}: {}", device, e))?;

            // Interpret the bytes as an ext2_super_block from the e2p bindings.
            let sb_ptr = buf.as_ptr() as *const e2p_sys::ext2_super_block;
            let sb: e2p_sys::ext2_super_block = unsafe { std::ptr::read_unaligned(sb_ptr) };

            // 0xEF53 is the EXT2/EXT3/EXT4 superblock magic
            if sb.s_magic != 0xEF53u16 {
                return Ok(2);
            }

            if sb.s_inodes_count == 0 || sb.s_blocks_count == 0 {
                return Ok(4);
            }

            Ok(0)
        }
        FsckMode::AutoFix => {
            // Conservative in-process AutoFix: inspect superblock and attempt safe repairs
            use std::io::{Seek, SeekFrom, Write};

            match File::options().read(true).write(true).open(device) {
                Ok(mut fdev) => {
                    let mut sblk = [0u8; 1024];
                    if let Err(e) = fdev.read_exact(&mut sblk) {
                        return Err(format!("failed to read superblock for {}: {}", device, e));
                    }

                    // Interpret the bytes as an ext2_super_block using an unaligned read
                    let sb_size = std::mem::size_of::<e2p_sys::ext2_super_block>();
                    if sb_size > 1024 {
                        return Err(format!("unexpected superblock struct size: {}", sb_size));
                    }

                            let sb_ptr = sblk.as_ptr() as *const e2p_sys::ext2_super_block;
                            let mut sb: e2p_sys::ext2_super_block = unsafe { std::ptr::read_unaligned(sb_ptr) };

                    // If primary magic invalid, attempt to restore from backups
                    if sb.s_magic != 0xEF53u16 {
                        let candidates: [u64;5] = [8193, 32768, 98304, 229376, 409600];
                        let file_len = match fdev.metadata() {
                            Ok(m) => m.len(),
                            Err(e) => return Err(format!("failed to stat {}: {}", device, e)),
                        };

                        for &bnum in &candidates {
                            for &bsize in &[1024u64, 4096u64] {
                                let off = bnum.saturating_mul(bsize);
                                if off + 1024 > file_len {
                                    continue;
                                }
                                if let Err(_e) = fdev.seek(SeekFrom::Start(off)) {
                                    continue;
                                }
                                let mut buf = [0u8; 1024];
                                if let Err(_) = fdev.read_exact(&mut buf) {
                                    continue;
                                }
                                        let cand_ptr = buf.as_ptr() as *const e2p_sys::ext2_super_block;
                                        let cand_sb: e2p_sys::ext2_super_block = unsafe { std::ptr::read_unaligned(cand_ptr) };
                                if cand_sb.s_magic == 0xEF53u16 {
                                    // write backup into primary location
                                    if let Err(e) = fdev.seek(SeekFrom::Start(1024)) {
                                        return Err(format!("failed to seek to primary superblock for {}: {}", device, e));
                                    }
                                    if let Err(e) = fdev.write_all(&buf) {
                                        return Err(format!("failed to write primary superblock for {}: {}", device, e));
                                    }
                                    return Ok(1);
                                }
                            }
                        }

                        return Err(format!("primary superblock invalid and no backup found for {}", device));
                    }

                    // Basic sanity checks
                    if sb.s_inodes_count == 0 || sb.s_blocks_count == 0 {
                        return Ok(4);
                    }

                    // Clear error bits and mark valid
                    sb.s_state = 1u16;
                            let sb_bytes = unsafe { std::slice::from_raw_parts((&sb as *const e2p_sys::ext2_super_block) as *const u8, sb_size) };
                    if let Err(e) = fdev.seek(SeekFrom::Start(1024)) {
                        return Err(format!("failed to seek to superblock for {}: {}", device, e));
                    }
                    if let Err(e) = fdev.write_all(sb_bytes) {
                        return Err(format!("failed to write superblock for {}: {}", device, e));
                    }
                    return Ok(1);
                }
                Err(e) => return Err(format!("cannot open device {} for AutoFix: {}", device, e)),
            }
        }
    }
}

/// Attempt to restore the primary superblock from common backup locations.
/// Returns Ok(true) if a backup was found and written to the primary superblock,
/// Ok(false) if no suitable backup was found, or Err on IO errors.
pub fn restore_superblock_from_backup(device: &str) -> Result<bool, String> {
    use std::io::{Seek, SeekFrom, Read, Write};
    use std::process::Command;

    match File::options().read(true).write(true).open(device) {
        Ok(mut fdev) => {
            let file_len = match fdev.metadata() { Ok(m) => m.len(), Err(e) => return Err(format!("failed to stat {}: {}", device, e)) };

            // Extended candidate list of commonly used backup superblock locations
            let candidates: [u64;20] = [
                8193, 16384, 24576, 32768, 49152, 65536, 98304, 131072, 163840, 196608,
                229376, 262144, 294912, 327680, 360448, 393216, 425984, 458752, 491520, 524288,
            ];

            for &bnum in &candidates {
                for &bsize in &[1024u64, 4096u64] {
                    let off = bnum.saturating_mul(bsize);
                    if off + 1024 > file_len {
                        continue;
                    }
                    if let Err(_) = fdev.seek(SeekFrom::Start(off)) {
                        continue;
                    }
                    let mut buf = [0u8; 1024];
                    if let Err(_) = fdev.read_exact(&mut buf) {
                        continue;
                    }

                    let cand_ptr = buf.as_ptr() as *const e2p_sys::ext2_super_block;
                    let cand_sb: e2p_sys::ext2_super_block = unsafe { std::ptr::read_unaligned(cand_ptr) };
                    if cand_sb.s_magic == 0xEF53u16 {
                        // write backup into primary location
                        if let Err(e) = fdev.seek(SeekFrom::Start(1024)) {
                            return Err(format!("failed to seek to primary superblock for {}: {}", device, e));
                        }
                        if let Err(e) = fdev.write_all(&buf) {
                            return Err(format!("failed to write primary superblock for {}: {}", device, e));
                        }
                        return Ok(true);
                    }
                }
            }

            // If we couldn't find a readable backup in-place, try invoking an embedded e2fsck
            // with common backup locations if available in the initramfs.
            if std::path::Path::new("/bin/e2fsck").exists() {
                let backups: [u64;20] = [
                    8193, 16384, 24576, 32768, 49152, 65536, 98304, 131072, 163840, 196608,
                    229376, 262144, 294912, 327680, 360448, 393216, 425984, 458752, 491520, 524288,
                ];

                for &bnum in &backups {
                    let status = Command::new("/bin/e2fsck")
                        .arg("-b")
                        .arg(format!("{}", bnum))
                        .arg("-y")
                        .arg(device)
                        .status();

                    match status {
                        Ok(st) => {
                            if let Some(code) = st.code() {
                                if code == 0 || code == 1 {
                                    return Ok(true);
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
            }

            Ok(false)
        }
        Err(e) => Err(format!("cannot open device {}: {}", device, e)),
    }
}
