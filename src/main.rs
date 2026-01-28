mod cmdline;
mod kmodprobe;
mod logger;
mod microhop;

use crate::microhop::{get_blk_devices, greet, mount_fs, SYS_MPT};
use nix::{mount::MsFlags, unistd};
use std::{ffi::CString, fs::DirBuilder, io::Error, os::unix::fs::DirBuilderExt, path::Path};

static LOGGER: logger::STDOUTLogger = logger::STDOUTLogger;

fn main() -> Result<(), Error> {
    // Set logger
    let cfg = profile::cfg::get_mh_config(None)?;
    log::set_logger(&LOGGER).map(|()| log::set_max_level(cfg.get_log_level())).unwrap();

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

    mount_fs(SYS_MPT);

    let (root_fstype, blk_mpt) = get_blk_devices(&cfg)?;
    if root_fstype.is_empty() {
        log::error!("Type of the root filesystem was not detected. Please double-check the configuration!");
    }
    mount_fs(&blk_mpt);

    let use_overlayfs = cfg.use_overlayfs();
    let final_root = if let Some(overlay_cfg) = cfg.get_overlayfs() {
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

        syslib::fs::mount("ext4", overlay_dev_path, overlay_mount)?;
        log::info!("Mounted overlayfs backing device at {}", overlay_mount);

        let upper_full = format!("{}/{}", overlay_mount, overlay_cfg.upper);
        let work_full = format!("{}/{}", overlay_mount, overlay_cfg.workdir);

        DirBuilder::new().recursive(true).mode(0o755).create(merged_dir)?;

        syslib::fs::mount_overlayfs(lower_dir, &upper_full, &work_full, merged_dir)?;

        merged_dir
    } else {
        temp_mpt
    };

    // Remount sysfs, switch root
    log::debug!("switching root");
    for t in SYS_MPT {
        let tgt = format!("{}{}", final_root, t.dst);
        nix::mount::mount(Some(t.dst), tgt.as_str(), Some(t.fstype), MsFlags::MS_MOVE, Option::<&str>::None)?;
    }

    // Pivot the system
    syslib::fs::pivot(final_root, if use_overlayfs { "overlay" } else { root_fstype.as_str() })?;

    // Start external init
    log::info!("Launching init at {}", cfg.get_init_path());

    let argv: Vec<CString> = vec![CString::new(cfg.get_init_path()).unwrap()];

    #[allow(irrefutable_let_patterns)]
    if let Err(err) = unistd::execv(&CString::new(cfg.get_init_path()).unwrap(), &argv) {
        log::error!("{:?}", err);
    }

    Ok(())
}
