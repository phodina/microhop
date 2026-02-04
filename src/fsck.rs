use std::process::Command;
use std::path::Path;

/// Filesystem check mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FsckMode {
    /// Non-destructive checks (equivalent to `-n`).
    NoWrite,
    /// Automatic preen / repair (equivalent to `-p`).
    AutoFix,
}

/// Run an external `e2fsck` for checks or automatic fixes.
pub fn fsck_ext2(device: &str, mode: FsckMode) -> Result<i32, String> {
    let e2fsck_path = "/bin/e2fsck";
    if !Path::new(e2fsck_path).exists() {
        return Err(format!("{} not found in initramfs", e2fsck_path));
    }

    let mut cmd = Command::new(e2fsck_path);
    match mode {
        FsckMode::NoWrite => { cmd.arg("-n").arg(device); }
        FsckMode::AutoFix => { cmd.arg("-p").arg(device); }
    }

    match cmd.status() {
        Ok(st) => match st.code() {
            Some(code) => Ok(code),
            None => Err("e2fsck terminated by signal".to_string()),
        },
        Err(e) => Err(format!("failed to execute e2fsck: {}", e)),
    }
}

/// Attempt to restore the primary superblock from common backup locations.
/// Returns Ok(true) if a backup was found and written to the primary superblock,
/// Ok(false) if no suitable backup was found, or Err on IO errors.
pub fn restore_superblock_from_backup(device: &str) -> Result<bool, String> {
    let e2fsck_path = "/bin/e2fsck";
    if !Path::new(e2fsck_path).exists() {
        return Err(format!("{} not found in initramfs", e2fsck_path));
    }
    // Use candidate backup superblocks by invoking `e2fsck -b <blk>` for each.
    let backups: [u64;20] = [
        8193, 16384, 24576, 32768, 49152, 65536, 98304, 131072, 163840, 196608,
        229376, 262144, 294912, 327680, 360448, 393216, 425984, 458752, 491520, 524288,
    ];

    for &bnum in &backups {
        let bstr = format!("{}", bnum);
        let out = Command::new(e2fsck_path)
            .arg("-b").arg(&bstr)
            .arg("-f").arg("-y")
            .arg(device)
            .output();

        match out {
            Ok(o) => {
                log::info!("running: e2fsck -b {} -f -y {}", bstr, device);
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                if !stdout.is_empty() {
                    log::info!("e2fsck -b {} stdout: {}", bstr, stdout);
                }
                if !stderr.is_empty() {
                    log::info!("e2fsck -b {} stderr: {}", bstr, stderr);
                }
                if let Some(code) = o.status.code() {
                    if code == 0 || code == 1 {
                        log::info!("e2fsck -b {} succeeded with code {}", bstr, code);
                        return Ok(true);
                    } else {
                        log::info!("e2fsck -b {} exited with code {}", bstr, code);
                    }
                }
            }
            Err(e) => {
                log::error!("failed to execute e2fsck -b {}: {}", bnum, e);
            }
        }
    }

    Ok(false)
}
