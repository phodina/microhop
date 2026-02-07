use std::path::Path;
use std::process::{Command, Stdio};
use std::str;

/// Run an external `e2fsck` for automatic fixes.
pub fn fsck_ext2(device: &str) -> Result<i32, String> {
    let e2fsck_path = "/bin/e2fsck";
    if !Path::new(e2fsck_path).exists() {
        return Err(format!("{} not found in initramfs", e2fsck_path));
    }

    // Attempt to autofix the device
    let mut cmd = Command::new(e2fsck_path);
    cmd.arg("-p");
    cmd.arg(device);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    match cmd.status() {
        Ok(st) => match st.code() {
            Some(code) => Ok(code),
            None => Err("e2fsck terminated by signal".to_string()),
        },
        Err(e) => Err(format!("failed to execute e2fsck: {}", e)),
    }
}

/// Get dynamic superblock locations using mke2fs -n (dry run)
pub fn get_superblock_locations(device: &str) -> Result<Vec<u64>, String> {
    let mke2fs_path = "/bin/mke2fs";
    if !Path::new(mke2fs_path).exists() {
        log::warn!("mke2fs not available, falling back to static superblock list");
        // Fallback to hardcoded list
        return Ok(vec![
            8193, 16384, 24576, 32768, 49152, 65536, 98304, 131072, 163840, 196608, 
            229376, 262144, 294912, 327680, 360448, 393216, 425984, 458752, 491520, 524288,
        ]);
    }

    let output = Command::new(mke2fs_path)
        .arg("-n")
        .arg("-t")
        .arg("ext4")
        .arg(device)
        .output()
        .map_err(|e| format!("Failed to run mke2fs -n: {}", e))?;

    if !output.status.success() {
        log::warn!("mke2fs -n failed, using static superblock list");
        return Ok(vec![
            8193, 16384, 24576, 32768, 49152, 65536, 98304, 131072, 163840, 196608, 
            229376, 262144, 294912, 327680, 360448, 393216, 425984, 458752, 491520, 524288,
        ]);
    }

    let output_str = str::from_utf8(&output.stderr)
        .map_err(|e| format!("Invalid UTF-8 in mke2fs output: {}", e))?;

    let mut locations = Vec::new();
    for line in output_str.lines() {
        if line.contains("Superblock backups stored on blocks:") {
            // Parse the line containing backup superblock locations
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 {
                let blocks_str = parts[1];
                for block_str in blocks_str.split(',') {
                    if let Ok(block) = block_str.trim().parse::<u64>() {
                        locations.push(block);
                    }
                }
            }
            break;
        }
    }

    if locations.is_empty() {
        log::warn!("Could not parse superblock locations from mke2fs, using static list");
        locations = vec![
            8193, 16384, 24576, 32768, 49152, 65536, 98304, 131072, 163840, 196608, 
            229376, 262144, 294912, 327680, 360448, 393216, 425984, 458752, 491520, 524288,
        ];
    }

    log::info!("Found {} backup superblock locations", locations.len());
    Ok(locations)
}

/// Analyze filesystem structure using dumpe2fs (if available)
pub fn analyze_filesystem_structure(device: &str) -> Result<(), String> {
    let dumpe2fs_path = "/bin/dumpe2fs";
    if !Path::new(dumpe2fs_path).exists() {
        log::warn!("dumpe2fs not available, skipping detailed filesystem analysis");
        return Ok(());
    }

    log::info!("Analyzing filesystem structure with dumpe2fs...");
    let output = Command::new(dumpe2fs_path)
        .arg("-h")
        .arg(device)
        .output()
        .map_err(|e| format!("Failed to run dumpe2fs: {}", e))?;

    if output.status.success() {
        let output_str = str::from_utf8(&output.stdout)
            .map_err(|e| format!("Invalid UTF-8 in dumpe2fs output: {}", e))?;
        
        // Extract useful information
        for line in output_str.lines() {
            if line.starts_with("Filesystem state:") ||
               line.starts_with("Errors behavior:") ||
               line.starts_with("Block count:") ||
               line.starts_with("Block size:") ||
               line.starts_with("Filesystem created:") ||
               line.starts_with("Last mount time:") ||
               line.starts_with("Last write time:") {
                log::info!("FS Info: {}", line);
            }
        }
    } else {
        log::warn!("dumpe2fs failed, filesystem may be severely corrupted");
    }

    Ok(())
}

/// Attempt to restore the primary superblock from backup locations.
pub fn restore_superblock_from_backup(device: &str) -> Result<bool, String> {
    let e2fsck_path = "/bin/e2fsck";
    if !Path::new(e2fsck_path).exists() {
        return Err(format!("{} not found in initramfs", e2fsck_path));
    }

    // Try to analyze filesystem structure first
    let _ = analyze_filesystem_structure(device);

    // Get dynamic superblock locations
    let backups = get_superblock_locations(device)?;
    
    log::info!("Attempting backup superblock recovery with {} locations", backups.len());

    for &bnum in &backups {
        let bstr = format!("{}", bnum);
        let out = Command::new(e2fsck_path)
            .arg("-b")
            .arg(&bstr)
            .arg("-f")
            .arg("-y")
            .arg(device)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output();

        match out {
            Ok(o) => {
                log::info!("running: e2fsck -b {} -f -y {}", bstr, device);
                if let Some(code) = o.status.code() {
                    if code == 0 || code == 1 {
                        log::info!("e2fsck -b {} succeeded with code {}", bstr, code);
                        return Ok(true);
                    } else if code == 2 {
                        log::warn!("e2fsck -b {} completed with warnings (code {}), but filesystem should be usable", bstr, code);
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

    log::error!("All backup superblock recovery attempts failed");
    Ok(false)
}

/// Attempt advanced recovery methods when standard recovery fails
pub fn attempt_advanced_recovery(device: &str) -> Result<bool, String> {
    log::warn!("Attempting advanced recovery methods...");
    
    // Try zero-superblock recovery with debugfs (if available)
    if let Ok(true) = try_zero_superblock_recovery(device) {
        return Ok(true);
    }

    // Try force fsck with minimal repairs
    if let Ok(true) = try_minimal_repair(device) {
        return Ok(true);
    }

    // Last resort: try read-only recovery
    try_readonly_recovery_info(device)
}

/// Try zero-superblock recovery using debugfs
fn try_zero_superblock_recovery(device: &str) -> Result<bool, String> {
    let debugfs_path = "/bin/debugfs";
    if !Path::new(debugfs_path).exists() {
        log::debug!("debugfs not available for zero-superblock recovery");
        return Ok(false);
    }

    log::info!("Attempting zero-superblock recovery with debugfs...");
    
    // This is a very advanced technique - debugfs can sometimes recover
    // filesystems where even backup superblocks are corrupted
    let output = Command::new(debugfs_path)
        .arg("-R")
        .arg("stats")
        .arg(device)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run debugfs: {}", e))?;

    if output.status.success() {
        log::info!("debugfs was able to read filesystem structure");
        log::info!("This suggests the filesystem might be recoverable with manual intervention");
        return Ok(true);
    }
    
    Ok(false)
}

/// Try minimal repair with force flags
fn try_minimal_repair(device: &str) -> Result<bool, String> {
    let e2fsck_path = "/bin/e2fsck";
    if !Path::new(e2fsck_path).exists() {
        return Ok(false);
    }

    log::info!("Attempting minimal repair with force flags...");
    
    // Try with -p (preen) and -f (force) for minimal fixes
    let output = Command::new(e2fsck_path)
        .arg("-p")
        .arg("-f")
        .arg(device)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("Failed to run minimal e2fsck: {}", e))?;

    if let Some(code) = output.status.code() {
        if code <= 2 {
            log::info!("Minimal repair succeeded with exit code {}", code);
            return Ok(true);
        }
    }
    
    Ok(false)
}

/// Provide information for read-only recovery
fn try_readonly_recovery_info(device: &str) -> Result<bool, String> {
    log::error!("=== FILESYSTEM RECOVERY FAILED ===");
    log::error!("All automated recovery attempts have failed for {}", device);
    log::error!("This indicates severe filesystem corruption.");
    log::error!("");
    log::error!("Possible manual recovery steps:");
    log::error!("1. Try mounting read-only: mount -o ro {} /mnt", device);
    log::error!("2. Use ddrescue to create a disk image for data recovery");
    log::error!("3. Run testdisk/photorec for file-level recovery");
    log::error!("4. Consider professional data recovery services");
    log::error!("");
    log::error!("System will attempt read-only mount as last resort...");
    
    Ok(false)
}
