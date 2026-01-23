use profile::cfg::MhConfig;
use std::io::Error;
use syslib::blk::BlkInfo;
use uuid::Uuid;

static VERSION: &str = "0.1.0";

pub struct SystemDir<T: AsRef<str>> {
    pub fstype: T,
    pub dev: T,
    pub dst: T,
}

impl<T: AsRef<str>> SystemDir<T> {
    const fn new(fstype: T, dev: T, dst: T) -> Self {
        Self { fstype, dev, dst }
    }
}

// Mount required system dirs
pub const SYS_MPT: &[SystemDir<&'static str>] = &[
    // Has to go always first
    SystemDir::new("proc", "none", "/proc"),
    SystemDir::new("sysfs", "none", "/sys"),
    SystemDir::new("devtmpfs", "devtmpfs", "/dev"),
];

// Initial greetings
pub fn greet(cfg: &MhConfig) -> Result<(), Error> {
    // Say hello
    log::info!("Welcome to the Microhop {}!", VERSION);

    // Debug itsel
    log::debug!("Init program: {}", cfg.get_init_path());
    for dsk in cfg.get_disks()? {
        log::debug!(
            "Disk device: {}, fs type: {}, mountpoint: {:?}, mode: {}",
            dsk.get_device(),
            dsk.get_fstype(),
            dsk.as_pathbuf(),
            dsk.get_mode()
        );
    }
    log::debug!("Kernel modules:");
    for m in cfg.get_modules() {
        log::debug!("- {}", m);
    }

    Ok(())
}

/// Mount configured filesystems in a batch
pub fn mount_fs<T: AsRef<str>>(filesystems: &[SystemDir<T>]) {
    for t in filesystems {
        if let Err(err) = syslib::fs::mount(t.fstype.as_ref(), t.dev.as_ref(), t.dst.as_ref()) {
            log::error!("Error mounting {}: {}", t.dst.as_ref(), err);
        };
    }
}

/// Parse device specification and resolve to device path
/// Supports: uuid=<UUID>, label=<LABEL>, /dev/path, plain UUID, plain label
pub fn resolve_device_path<'a>(device_spec: &'a str, blkid: &'a BlkInfo) -> Option<&'a str> {
    if device_spec.starts_with("uuid=") || device_spec.to_lowercase().starts_with("uuid=") {
        let uuid = &device_spec[5..];

        if let Some(blkdev) = blkid.by_uuid(uuid) {
            return blkdev.get_path().to_str();
        }
    } else if device_spec.starts_with("label=") || device_spec.to_lowercase().starts_with("label=") {
        let label = &device_spec[6..];

        if let Some(blkdev) = blkid.by_label(label) {
            return blkdev.get_path().to_str();
        }
    } else if device_spec.starts_with("/dev/") {
        // Use device path directly without adding any suffix
        return Some(device_spec);
    } else if Uuid::parse_str(device_spec).is_ok() {
        // Plain UUID (for backward compatibility)
        if let Some(blkdev) = blkid.by_uuid(device_spec) {
            return blkdev.get_path().to_str();
        }
    } else {
        // Assume plain label (for backward compatibility)
        if let Some(blkdev) = blkid.by_label(device_spec) {
            return blkdev.get_path().to_str();
        }
    }

    None
}

/// Get block devices
pub fn get_blk_devices(cfg: &MhConfig) -> Result<(String, Vec<SystemDir<String>>), Error> {
    let mut root_fstype = String::new();
    let mut blkid = BlkInfo::new();

    blkid.probe_devices()?;

    for d in blkid.get_devices() {
        if !d.get_fstype().is_empty() {
            log::info!("{} partition at {:?} ({}) \"{}\"", d.get_fstype(), d.get_path(), d.get_uuid(), d.get_label());
        }
    }

    let mut blk_mpt: Vec<SystemDir<String>> = Vec::new();
    for dev in cfg.get_disks()? {
        let mpt = dev.get_mountpoint().trim_end_matches('/').to_string();
        if mpt.is_empty() && root_fstype.is_empty() {
            root_fstype = dev.get_fstype().into();
        }

        let mut devpath = "";
        if Uuid::parse_str(dev.get_device()).is_ok() {
            if let Some(blkdev) = blkid.by_uuid(dev.get_device()) {
                devpath = blkdev.get_path().to_str().unwrap();
            }
        } else if dev.get_device().starts_with("/dev") {
            devpath = dev.get_device();
        } else {
            // label
            if let Some(blkdev) = blkid.by_label(dev.get_device()) {
                devpath = blkdev.get_path().to_str().unwrap();
            }
        }

        if devpath.is_empty() {
            log::warn!("Unknown device: {}", dev.get_device());
        } else {
            let dir = SystemDir::new(dev.get_fstype().into(), devpath.into(), format!("{}{}", &cfg.get_sysroot_path(), mpt));
            blk_mpt.push(dir);
        }
    }

    // Sort mountpoints, so the "/" goes always first
    blk_mpt.sort_by(|ela, elb| ela.dev.cmp(&elb.dev));

    Ok((root_fstype, blk_mpt))
}
