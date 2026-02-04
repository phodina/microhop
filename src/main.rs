mod cmdline;
mod fsck;
mod kmodprobe;
mod logger;
mod microhop;

use crate::microhop::{get_blk_devices, greet, mount_fs, SYS_MPT};
use nix::{mount::MsFlags, unistd};
use std::{ffi::CString, fs::DirBuilder, io::Error, os::unix::fs::DirBuilderExt, path::Path};
use std::process::Command;

static LOGGER: logger::STDOUTLogger = logger::STDOUTLogger;

fn main() -> Result<(), Error> {
    // Set logger
    let cfg = match profile::cfg::get_mh_config(None) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("ERROR: Failed to load configuration file /etc/microhop.conf");
            match err.kind() {
                std::io::ErrorKind::NotFound => {
                    eprintln!("  Reason: Configuration file not found");
                }
                std::io::ErrorKind::InvalidData => {
                    eprintln!("  Reason: Configuration file is invalid or contains syntax errors");
                    eprintln!("  Details: {}", err);
                }
                _ => {
                    eprintln!("  Details: {}", err);
                }
            }
            return Err(err);
        }
    };

    // Set up logger, defaulting to Info level if there's an issue
    if let Err(err) = log::set_logger(&LOGGER) {
        eprintln!("WARNING: Failed to set up logger: {}", err);
    }

    // Set log level from config, with validation
    let log_level = cfg.get_log_level();
    if let Some(level_str) = cfg.get_log_level_as_str() {
        match level_str.as_str() {
            "debug" | "info" | "quiet" => {
                log::set_max_level(log_level);
            }
            _ => {
                eprintln!("WARNING: Invalid log level '{}' in configuration. Using 'info' as default.", level_str);
                eprintln!("  Valid options are: debug, info, quiet");
                log::set_max_level(log::LevelFilter::Info);
            }
        }
    } else {
        log::set_max_level(log_level);
    }

    greet(&cfg)?;

    // Load required modules
    let mpb = kmodprobe::KModProbe::new();
    for mname in cfg.get_modules() {
        mpb.modprobe(mname);
    }
    if !cfg.get_modules().is_empty() {
        log::info!("loaded required kernel modules");
    }

    // Create sysroot entry point
    let temp_mpt = &cfg.get_sysroot_path();
    if !Path::new(temp_mpt).exists() {
        DirBuilder::new().recursive(true).mode(0o755).create(temp_mpt.as_str())?;
        log::debug!("Init sysroot path: {}", temp_mpt);
    }

    mount_fs(SYS_MPT, &cfg);

    let (root_fstype, blk_mpt) = get_blk_devices(&cfg)?;
    if root_fstype.is_empty() {
        log::error!("Type of the root filesystem was not detected. Please double-check the configuration!");
    }
    mount_fs(&blk_mpt, &cfg);

    let mut use_overlayfs = cfg.use_overlayfs();
    let mut final_root = cfg.get_sysroot_path();

    if let Some(overlay_cfg) = cfg.get_overlayfs() {
        log::info!("Overlayfs enabled in configuration");

        if !syslib::fs::is_overlayfs_supported() {
            log::error!("Overlayfs requested but not supported by kernel!");
            log::error!("Make sure CONFIG_OVERLAY_FS is enabled in kernel config");
            return Err(Error::new(std::io::ErrorKind::Unsupported, "Overlayfs not supported"));
        }

        use crate::microhop::resolve_device_path;
        let mut blkid = syslib::blk::BlkInfo::new();
        blkid.probe_devices()?;

        let overlay_dev_path = resolve_device_path(&overlay_cfg.device, &blkid).ok_or_else(|| {
            Error::new(std::io::ErrorKind::NotFound, format!("Could not resolve overlayfs device: {}", overlay_cfg.device))
        })?;

        log::info!("Using overlayfs device: {} ({})", overlay_dev_path, overlay_cfg.device);

        let lower_dir = temp_mpt;
        let overlay_mount = "/overlay";
        let merged_dir = "/overlay/merged";

        DirBuilder::new().recursive(true).mode(0o755).create(overlay_mount)?;

        let mut overlay_ok = false;
        match syslib::fs::mount("ext4", overlay_dev_path, overlay_mount) {
            Ok(()) => {
                log::info!("Mounted overlayfs backing device at {}", overlay_mount);
                overlay_ok = true;
            }
            Err(err) => {
                log::error!("Failed to mount overlay backing device {}: {}", overlay_dev_path, err);

                // Attempt conservative fsck on the overlay device according to config
                let mode = cfg.get_fsck_mode().map(|s| s.as_str()).unwrap_or("n");
                let fsck_mode = match mode {
                    "p" | "preen" | "auto" => crate::fsck::FsckMode::AutoFix,
                    _ => crate::fsck::FsckMode::NoWrite,
                };

                // Helper to run embedded /bin/e2fsck if present
                fn run_external_e2fsck(device: &str, auto: bool) -> Result<i32, String> {
                    let mut cmd = Command::new("/bin/e2fsck");
                    if auto {
                        cmd.arg("-p");
                    } else {
                        cmd.arg("-n");
                    }
                    cmd.arg(device);
                    match cmd.status() {
                        Ok(st) => match st.code() {
                            Some(c) => Ok(c),
                            None => Err("e2fsck terminated by signal".to_string()),
                        },
                        Err(e) => Err(format!("failed to execute /bin/e2fsck: {}", e)),
                    }
                }

                // If AutoFix requested, prefer calling the external e2fsck in initrd
                if fsck_mode == crate::fsck::FsckMode::AutoFix {
                    match run_external_e2fsck(overlay_dev_path, true) {
                        Ok(code) => {
                            log::info!("external e2fsck returned {} for {}", code, overlay_dev_path);
                            match code {
                                0 | 1 => {
                                    log::info!("Retrying mount for overlay device {} after external fsck", overlay_dev_path);
                                    if let Err(err2) = syslib::fs::mount("ext4", overlay_dev_path, overlay_mount) {
                                        log::error!("Retry mount failed for overlay {}: {}", overlay_dev_path, err2);
                                        log::info!("Falling back to read-only root without overlay");
                                    } else {
                                        overlay_ok = true;
                                    }
                                }
                                2 => {
                                    log::info!("external e2fsck returned 2 for {}, attempting rescan and retry", overlay_dev_path);
                                }
                                _ => {
                                    log::error!("external e2fsck returned unexpected code {} for {}", code, overlay_dev_path);
                                    return Err(Error::new(std::io::ErrorKind::Other, format!("e2fsck returned code {}", code)));
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to run external e2fsck for {}: {}", overlay_dev_path, e);
                        }
                    }
                } else {
                    // NoWrite requested: perform a non-destructive in-process check first
                    match crate::fsck::fsck_ext2(overlay_dev_path, crate::fsck::FsckMode::NoWrite) {
                        Ok(code) => {
                            log::info!("fsck returned exit code {} for overlay {}", code, overlay_dev_path);
                            match code {
                                0 => {
                                    log::info!("Retrying mount for overlay device {} after successful fsck", overlay_dev_path);
                                    if let Err(err2) = syslib::fs::mount("ext4", overlay_dev_path, overlay_mount) {
                                        log::error!("Retry mount failed for overlay {}: {}", overlay_dev_path, err2);
                                        log::info!("Falling back to read-only root without overlay");
                                    } else {
                                        overlay_ok = true;
                                    }
                                }
                                1 => {
                                    log::info!("Filesystem indicated recoverable errors on overlay {}, attempting external e2fsck -p", overlay_dev_path);
                                    match run_external_e2fsck(overlay_dev_path, true) {
                                        Ok(rc2) => {
                                            if rc2 == 0 || rc2 == 1 {
                                                if let Err(err2) = syslib::fs::mount("ext4", overlay_dev_path, overlay_mount) {
                                                    log::error!("Retry mount failed for overlay {}: {}", overlay_dev_path, err2);
                                                    return Err(Error::new(std::io::ErrorKind::Other, format!("Failed to mount overlay device: {}", err2)));
                                                }
                                            } else if rc2 == 2 {
                                                log::info!("external e2fsck returned 2 for {}, will attempt rescan", overlay_dev_path);
                                            } else {
                                                return Err(Error::new(std::io::ErrorKind::Other, format!("external e2fsck returned {}", rc2)));
                                            }
                                        }
                                        Err(e) => {
                                            log::error!("Failed to run external e2fsck for {}: {}", overlay_dev_path, e);
                                            return Err(Error::new(std::io::ErrorKind::Other, format!("fsck failed: {}", e)));
                                        }
                                    }
                                }
                                2 => {
                                    log::info!("fsck returned code 2 for overlay {}: primary superblock invalid", overlay_dev_path);
                                    // Try to restore primary superblock from a backup before rescan
                                    match crate::fsck::restore_superblock_from_backup(overlay_dev_path) {
                                        Ok(true) => {
                                            log::info!("Restored primary superblock from backup for {}: retrying mount", overlay_dev_path);
                                            if let Err(err2) = syslib::fs::mount("ext4", overlay_dev_path, overlay_mount) {
                                                log::error!("Retry mount failed for overlay {} after restore: {}", overlay_dev_path, err2);
                                            } else {
                                                overlay_ok = true;
                                            }
                                        }
                                        Ok(false) => {
                                            log::info!("No backup superblock found for {}", overlay_dev_path);

                                            log::info!("No backup superblock found for {}", overlay_dev_path);
                                            log::info!("Attempting kernel rescan and retry");
                                        }
                                        Err(e) => {
                                            log::error!("Failed to attempt superblock restore for {}: {}", overlay_dev_path, e);
                                        }
                                    }
                                }
                                _ => {
                                    log::error!("fsck returned unexpected code {} for overlay {}, not retrying mount", code, overlay_dev_path);
                                    return Err(Error::new(std::io::ErrorKind::Other, format!("fsck returned code {} for {}", code, overlay_dev_path)));
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("fsck failed to run for overlay {}: {}", overlay_dev_path, e);
                            return Err(Error::new(std::io::ErrorKind::Other, format!("fsck failed: {}", e)));
                        }
                    }
                }
            }
        }

        if overlay_ok {
            let upper_full = format!("{}/{}", overlay_mount, overlay_cfg.upper);
            let work_full = format!("{}/{}", overlay_mount, overlay_cfg.workdir);

            DirBuilder::new().recursive(true).mode(0o755).create(merged_dir)?;

            syslib::fs::mount_overlayfs(lower_dir, &upper_full, &work_full, merged_dir)?;

            final_root = merged_dir.to_string();
        } else {
            use_overlayfs = false;
            final_root = cfg.get_sysroot_path();
        }
    }

    // Remount sysfs, switch root
    log::debug!("switching root");
    for t in SYS_MPT {
        let tgt = format!("{}{}", final_root, t.dst);
        nix::mount::mount(Some(t.dst), tgt.as_str(), Some(t.fstype), MsFlags::MS_MOVE, Option::<&str>::None)?;
    }

    // Pivot the system
    syslib::fs::pivot(&final_root, if use_overlayfs { "overlay" } else { root_fstype.as_str() })?;

    // Start external init
    log::info!("Launching init at {}", cfg.get_init_path());

    let init_path = cfg.get_init_path();
    let init_cstring = match CString::new(init_path.clone()) {
        Ok(s) => s,
        Err(err) => {
            log::error!("Failed to create init path argument: contains null bytes");
            log::error!("Init path: {}", init_path);
            log::error!("Details: {}", err);
            return Err(Error::new(std::io::ErrorKind::InvalidInput, format!("Init path '{}' contains null bytes", init_path)));
        }
    };

    let argv: Vec<CString> = vec![init_cstring.clone()];

    #[allow(irrefutable_let_patterns)]
    if let Err(err) = unistd::execv(&init_cstring, &argv) {
        log::error!("Failed to execute init process: {:?}", err);
        log::error!("Init path was: {}", init_path);
    }

    Ok(())
}
