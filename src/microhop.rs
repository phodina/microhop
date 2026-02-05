use profile::cfg::MhConfig;
use std::io::Error;
use std::path::Path;
use std::process::Command;
use syslib::blk::BlkInfo;
use uuid::Uuid;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct SystemDir<T: AsRef<str>> {
    pub fstype: T,
    pub dev: T,
    pub dst: T,
    pub mode: Option<T>,
}

impl<T: AsRef<str>> SystemDir<T> {
    const fn new(fstype: T, dev: T, dst: T) -> Self {
        Self { fstype, dev, dst, mode: None }
    }

    pub fn new_with_mode(fstype: T, dev: T, dst: T, mode: T) -> Self {
        Self { fstype, dev, dst, mode: Some(mode) }
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

/// Run embedded /bin/e2fsck directly instead of using in-process fsck
fn run_external_e2fsck(device: &str) -> Result<i32, String> {
    let e2fsck_path = "/bin/e2fsck";
    if !Path::new(e2fsck_path).exists() {
        return Err(format!("{} not found in initramfs", e2fsck_path));
    }

    let mut cmd = Command::new(e2fsck_path);
    cmd.arg("-p");
    cmd.arg(device);

    match cmd.status() {
        Ok(st) => match st.code() {
            Some(c) => Ok(c),
            None => Err("e2fsck terminated by signal".to_string()),
        },
        Err(e) => Err(format!("failed to execute /bin/e2fsck: {}", e)),
    }
}

/// Mount configured filesystems in a batch
pub fn mount_fs<T: AsRef<str>>(filesystems: &[SystemDir<T>]) {
    use nix::mount::MsFlags;

    for t in filesystems {
        let mut flags = MsFlags::MS_NOATIME;
        if let Some(mode) = &t.mode {
            if mode.as_ref() == "ro" {
                flags |= MsFlags::MS_RDONLY;
            }
        }

        if let Err(err) = syslib::fs::mount_with_flags(t.fstype.as_ref(), t.dev.as_ref(), t.dst.as_ref(), flags) {
            log::error!("Error mounting {}: {}", t.dst.as_ref(), err);

            // Attempt a conservative filesystem check using the embedded fsck wrapper
            let fstype = t.fstype.as_ref();
            if fstype.starts_with("ext") {
                match run_external_e2fsck(t.dev.as_ref()) {
                    Ok(code) => {
                        log::info!("e2fsck returned exit code {} for device {}", code, t.dev.as_ref());
                        if code == 0 {
                            log::info!("Retrying mount for {} after successful e2fsck", t.dst.as_ref());
                            if let Err(err2) =
                                syslib::fs::mount_with_flags(t.fstype.as_ref(), t.dev.as_ref(), t.dst.as_ref(), flags)
                            {
                                log::error!("Retry mount failed {}: {}", t.dst.as_ref(), err2);
                            }
                        } else {
                            log::error!("e2fsck reported non-zero status, not retrying mount");
                        }
                    }
                    Err(e) => {
                        log::error!("e2fsck failed to run for {}: {}", t.dev.as_ref(), e);
                    }
                }
            }
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

/// List all available block devices with their UUIDs and labels
fn list_available_devices(blkid: &BlkInfo) {
    let devices = blkid.get_devices();

    if devices.is_empty() {
        log::error!("No block devices found on the system!");
        return;
    }

    log::error!("Available block devices:");

    for d in devices {
        let path = d.get_path().to_str().unwrap_or("<invalid>");
        let uuid = d.get_uuid();
        let label = d.get_label();
        let fstype = d.get_fstype();

        if !fstype.is_empty() {
            log::error!("Device: {}", path);

            if !uuid.is_empty() {
                log::error!("  UUID:  {}", uuid);
                log::error!("  Use:   uuid={}", uuid);
            } else {
                log::error!("  UUID:  <none>");
            }

            if !label.is_empty() {
                log::error!("  LABEL: {}", label);
                log::error!("  Use:   label={}", label);
            } else {
                log::error!("  LABEL: <none>");
            }

            log::error!("  Type:  {}", fstype);
            log::error!("  Use:   {}", path);
        }
    }
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

    let mask = cfg.get_mask_cmdline();

    if !mask.is_empty() {
        log::info!("Masking kernel cmdline parameters: {:?}", mask);
    } else {
        log::debug!("No cmdline parameters to mask");
    }

    let cmdline = crate::cmdline::CmdLine::new_with_mask(&mask).unwrap_or_default();

    if let Some(root_device) = cmdline.get_root_device() {
        log::info!("Using root device from kernel cmdline: {}", root_device);

        let root_fs = cmdline.get_root_fstype().unwrap_or("ext4");
        let _root_opts = cmdline.get_root_options().unwrap_or("rw");

        root_fstype = root_fs.to_string();

        let devpath = resolve_device_path(root_device, &blkid);

        if let Some(devpath) = devpath {
            log::info!("Resolved root device to: {}", devpath);
            let dir = SystemDir::new(root_fs.to_string(), devpath.to_string(), cfg.get_sysroot_path());
            blk_mpt.push(dir);
        } else {
            log::error!("Could not resolve root device from cmdline: {}", root_device);
            log::error!("");
            list_available_devices(&blkid);
            return Err(Error::new(std::io::ErrorKind::NotFound, "Root device not found"));
        }
    } else {
        log::info!("No root device in kernel cmdline, using configuration file");
        for dev in cfg.get_disks()? {
            let mpt = dev.get_mountpoint().trim_end_matches('/').to_string();
            if mpt.is_empty() && root_fstype.is_empty() {
                root_fstype = dev.get_fstype().into();
            }

            let device_spec = dev.get_device();
            let devpath = resolve_device_path(device_spec, &blkid);

            if let Some(devpath) = devpath {
                let dir = SystemDir::new_with_mode(
                    dev.get_fstype().into(),
                    devpath.into(),
                    format!("{}{}", &cfg.get_sysroot_path(), mpt),
                    dev.get_mode().into(),
                );
                blk_mpt.push(dir);
            } else {
                log::error!("Could not resolve device from config: {}", device_spec);
                log::error!("");
                list_available_devices(&blkid);
                return Err(Error::new(std::io::ErrorKind::NotFound, format!("Device not found: {}", device_spec)));
            }
        }
    }

    // Sort mountpoints, so the "/" goes always first
    blk_mpt.sort_by(|ela, elb| ela.dev.cmp(&elb.dev));

    Ok((root_fstype, blk_mpt))
}
