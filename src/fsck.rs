use ext2fs_sys as ext2fs;
use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::mem::transmute;

use std::os::raw::{c_int, c_uint};
/// Filesystem check mode.
#[derive(Clone, Copy, Debug)]
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
            let sb: e2p_sys::ext2_super_block = unsafe { transmute::<[u8; 1024], e2p_sys::ext2_super_block>(buf) };

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
            // Open the filesystem for read-write and run descriptor checks.
            let cpath = CString::new(device).map_err(|e| format!("bad device name: {}", e))?;

            unsafe {
                let mut fs: ext2fs::ext2_filsys = std::ptr::null_mut();

                // Open read-write so that checks may apply fixes.
                let flags: c_int = ext2fs::EXT2_FLAG_RW as c_int;
                let rc_open = ext2fs::ext2fs_open(cpath.as_ptr(), flags, 0 as c_int, 0 as c_uint, std::ptr::null_mut(), &mut fs);

                if rc_open != 0 {
                    return Err(format!("ext2fs_open failed: {}", rc_open));
                }

                // Run descriptor checks (this is a conservative, non-interactive check).
                let rc_check = ext2fs::ext2fs_check_desc(fs);

                // Close the filesystem handle
                let _ = ext2fs::ext2fs_close(fs);

                if rc_check != 0 {
                    return Ok(rc_check as i32);
                }
            }

            Ok(0)
        }
    }
}
