mod cmdline;
mod fsck;
mod kmodprobe;
mod logger;
mod microhop;
mod overlayfs;

use crate::microhop::{get_blk_devices, greet, mount_fs, SYS_MPT};
use nix::{mount::MsFlags, unistd};
use std::{ffi::CString, fs::DirBuilder, io::Error, os::unix::fs::DirBuilderExt, path::Path};

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

    if let Some(metadata) = cfg.get_metadata() {
        log::debug!("Configuration metadata:");
        log::debug!("  Git commit: {}", metadata.git_commit);
        log::debug!("  Generated at: {}", metadata.generated_at);
    }

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

    mount_fs(SYS_MPT);

    let (root_fstype, blk_mpt) = get_blk_devices(&cfg)?;
    if root_fstype.is_empty() {
        log::error!("Type of the root filesystem was not detected. Please double-check the configuration!");
    }
    mount_fs(&blk_mpt);

    let mut use_overlayfs = cfg.use_overlayfs();
    let mut final_root = cfg.get_sysroot_path();

    if let Some(overlay_cfg) = cfg.get_overlayfs() {
        log::info!("Overlayfs enabled in configuration");

        if !syslib::fs::is_overlayfs_supported() {
            log::error!("Overlayfs requested but not supported by kernel!");
            log::error!("Make sure CONFIG_OVERLAY_FS is enabled in kernel config");
            return Err(Error::new(std::io::ErrorKind::Unsupported, "Overlayfs not supported"));
        }

        match overlayfs::try_setup_overlay(&cfg, temp_mpt, overlay_cfg) {
            Ok(Some(mounted_root)) => {
                final_root = mounted_root;
            }
            Ok(None) => {
                use_overlayfs = false;
                final_root = cfg.get_sysroot_path();
            }
            Err(e) => return Err(e),
        }
    }

    // Remount sysfs, switch root
    log::debug!("switching root");
    // Debug: log what we intend to pivot to and current mounts for diagnosis
    log::info!("DEBUG: final_root='{}', use_overlayfs={}", final_root, use_overlayfs);
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        for line in mounts.lines() {
            if line.contains(&final_root) || line.contains("/overlay") || line.starts_with("/") {
                log::info!("DEBUG: mount: {}", line);
            }
        }
    } else {
        log::info!("DEBUG: could not read /proc/mounts");
    }
    for t in SYS_MPT {
        let tgt = format!("{}{}", final_root, t.dst);
        nix::mount::mount(Some(t.dst), tgt.as_str(), Some(t.fstype), MsFlags::MS_MOVE, Option::<&str>::None)?;
    }

    // Pivot the system
    syslib::fs::pivot(&final_root, if use_overlayfs { "overlay" } else { root_fstype.as_str() })?;

    // Post-pivot verification: ensure the root filesystem is the expected one.
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        // find root entry
        for line in mounts.lines() {
            if line.split_whitespace().nth(1).map(|s| s == "/").unwrap_or(false) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let fstype_now = parts[2];
                    let expected = if use_overlayfs { "overlay" } else { root_fstype.as_str() };
                    if fstype_now != expected {
                        log::warn!(
                            "Post-pivot root fstype '{}' != expected '{}', attempting a second MS_MOVE of {} -> /",
                            fstype_now,
                            expected,
                            final_root
                        );
                        // Try to move the final_root mount onto /
                        match nix::mount::mount(
                            Some(final_root.as_str()),
                            "/",
                            Some(expected),
                            nix::mount::MsFlags::MS_MOVE,
                            Option::<&str>::None,
                        ) {
                            Ok(()) => log::info!("Second MS_MOVE succeeded: {} is now root", final_root),
                            Err(e) => log::error!("Second MS_MOVE failed: {}", e),
                        }
                    }
                }
                break;
            }
        }
    } else {
        log::warn!("Could not read /proc/mounts for post-pivot verification");
    }

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
