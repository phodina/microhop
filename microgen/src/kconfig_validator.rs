use colored::Colorize;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Error, ErrorKind},
    path::Path,
};

pub const SUPPORTED_FILESYSTEMS: &[(&str, &str, &str)] = &[
    ("ext4", "CONFIG_EXT4_FS", "Fourth Extended Filesystem"),
    ("ext2", "CONFIG_EXT2_FS", "Second Extended Filesystem"),
    ("squashfs", "CONFIG_SQUASHFS", "Compressed read-only filesystem"),
    ("btrfs", "CONFIG_BTRFS_FS", "B-tree filesystem"),
    ("xfs", "CONFIG_XFS_FS", "SGI XFS filesystem"),
    ("f2fs", "CONFIG_F2FS_FS", "Flash-Friendly File System"),
    ("jffs2", "CONFIG_JFFS2_FS", "Journalling Flash File System v2"),
    ("ubifs", "CONFIG_UBIFS_FS", "UBIFS file system"),
    ("tmpfs", "CONFIG_TMPFS", "Temporary filesystem"),
    ("iso9660", "CONFIG_ISO9660_FS", "ISO9660 filesystem"),
    ("vfat", "CONFIG_VFAT_FS", "VFAT filesystem"),
    ("ntfs", "CONFIG_NTFS_FS", "NTFS filesystem"),
];

pub const SUPPORTED_BLOCK_DEVICES: &[(&str, &str, &str)] = &[
    ("virtio_blk", "CONFIG_VIRTIO_BLK", "Virtio block driver"),
    ("virtio_mmio", "CONFIG_VIRTIO_MMIO", "Platform bus driver for memory mapped virtio devices"),
    ("nvme", "CONFIG_BLK_DEV_NVME", "NVM Express block device"),
    ("usb_storage", "CONFIG_USB_STORAGE", "USB Mass Storage support"),
    ("uas", "CONFIG_USB_UAS", "USB Attached SCSI"),
    ("sd_mod", "CONFIG_BLK_DEV_SD", "SCSI disk support"),
    ("sr_mod", "CONFIG_BLK_DEV_SR", "SCSI CDROM support"),
    ("mmc_block", "CONFIG_MMC_BLOCK", "MMC block device driver"),
    ("nand_block", "CONFIG_MTD_NAND_CORE", "MTD NAND core device driver"),
    ("sdhci", "CONFIG_MMC_SDHCI", "Secure Digital Host Controller Interface support"),
    ("ahci", "CONFIG_SATA_AHCI", "AHCI SATA support"),
    ("nvme", "CONFIG_BLK_DEV_NVME", "NVME support"),
    ("ufs", "CONFIG_SCSI_UFSHCD", "UFS support"),
    ("ata_piix", "CONFIG_ATA_PIIX", "Intel PIIX/ICH SATA support"),
    ("loop", "CONFIG_BLK_DEV_LOOP", "Loopback device support"),
];

pub fn print_supported_filesystems() {
    println!("\n{}", "=== Supported Filesystems ===".bright_cyan().bold());
    println!("{}", "Use these names with --filesystems option during validation\n".white());

    for (name, config, desc) in SUPPORTED_FILESYSTEMS {
        println!("  {:<15} {:<25} {}",
            name.bright_green().bold(),
            config.bright_yellow(),
            desc.white()
        );
    }
    println!();
}

pub fn print_supported_block_devices() {
    println!("\n{}", "=== Supported Block Devices ===".bright_cyan().bold());
    println!("{}", "Use these names with --block-devices option during validation\n".white());

    for (name, config, desc) in SUPPORTED_BLOCK_DEVICES {
        println!("  {:<20} {:<30} {}",
            name.bright_green().bold(),
            config.bright_yellow(),
            desc.white()
        );
    }
    println!();
}

pub struct KConfigValidator {
    config: HashMap<String, String>,
}

impl KConfigValidator {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut config = HashMap::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim().to_string();
                let value = trimmed[eq_pos + 1..].trim().to_string();
                config.insert(key, value);
            }
        }

        Ok(KConfigValidator { config })
    }

    pub fn is_enabled(&self, option: &str) -> bool {
        let option = if option.starts_with("CONFIG_") {
            option.to_string()
        } else {
            format!("CONFIG_{}", option)
        };

        match self.config.get(&option) {
            Some(val) => val == "y" || val == "m",
            None => false,
        }
    }

    pub fn is_module(&self, option: &str) -> bool {
        let option = if option.starts_with("CONFIG_") {
            option.to_string()
        } else {
            format!("CONFIG_{}", option)
        };

        match self.config.get(&option) {
            Some(val) => val == "m",
            None => false,
        }
    }

    pub fn get_value(&self, option: &str) -> Option<&String> {
        let option = if option.starts_with("CONFIG_") {
            option.to_string()
        } else {
            format!("CONFIG_{}", option)
        };

        self.config.get(&option)
    }

    /// Validate kernel configuration against microhop requirements
    ///
    /// # Arguments
    /// * `mh_config` - The microhop configuration to validate against
    /// * `filesystems` - Optional list of filesystem types to validate. If None, filesystem checks will be warnings only.
    /// * `block_devices` - Optional list of block device types to validate. If None, block device checks will be warnings only.
    pub fn validate(&self, mh_config: &profile::cfg::MhConfig, filesystems: Option<&[String]>, block_devices: Option<&[String]>) -> Result<ValidationResult, Error> {
        let mut result = ValidationResult::new();

        // Check if CONFIG_MODULES is enabled if modules are required
        let modules = mh_config.get_modules();
        if !modules.is_empty() {
            if !self.is_enabled("MODULES") {
                result.add_error(
                    "CONFIG_MODULES",
                    "Kernel module support is disabled, but microhop.conf requires modules to be loaded",
                );
            } else {
                result.add_info("CONFIG_MODULES", "Kernel module support is enabled");
            }
        }

        for module_name in modules {
            self.validate_module(module_name, &mut result);
        }

        if let Some(fs_list) = filesystems {
            for fstype in fs_list {
                let fstype_upper = fstype.to_uppercase();
                self.validate_filesystem(&fstype_upper, &mut result, true);
            }
        } else if let Ok(disks) = mh_config.get_disks() {
            for disk in disks {
                let fstype = disk.get_fstype().to_uppercase();
                self.validate_filesystem(&fstype, &mut result, false);
            }
        }

        if let Some(blk_list) = block_devices {
            for blktype in blk_list {
                let blktype_upper = blktype.to_uppercase();
                self.validate_block_device(&blktype_upper, &mut result, true);
            }
        }

        self.validate_base_features(&mut result);

        Ok(result)
    }

    fn validate_module(&self, module_name: &str, result: &mut ValidationResult) {
        let config_name = module_name.to_uppercase().replace('-', "_");

        if self.is_module(&config_name) {
            result.add_success(&config_name, &format!("Module {} can be loaded", module_name));
        } else if self.is_enabled(&config_name) {
            result.add_warning(
                &config_name,
                &format!(
                    "Module {} is built-in (=y) instead of loadable module (=m). It will be available but cannot be loaded dynamically",
                    module_name
                ),
            );
        } else {
            result.add_error(
                &config_name,
                &format!(
                    "Module {} is not enabled in kernel config. Required by microhop.conf",
                    module_name
                ),
            );
        }
    }

    fn validate_filesystem(&self, fstype: &str, result: &mut ValidationResult, is_error: bool) {
        let fstype_lower = fstype.to_lowercase();
        let fs_entry = SUPPORTED_FILESYSTEMS.iter().find(|(name, config, _)| {
            name.eq_ignore_ascii_case(&fstype_lower) ||
            config.trim_start_matches("CONFIG_").eq_ignore_ascii_case(fstype)
        });

        if let Some((fs_name, config_name, _)) = fs_entry {
            let config_option = config_name.trim_start_matches("CONFIG_");
            if self.is_enabled(config_option) {
                result.add_success(config_option, &format!("Filesystem {} is supported", fs_name));
            } else {
                let msg = if is_error {
                    format!("Filesystem {} is not enabled. Required for specified rootfs", fs_name)
                } else {
                    format!("Filesystem {} is not validated (use --filesystems to validate)", fs_name)
                };

                if is_error {
                    result.add_error(config_option, &msg);
                } else {
                    result.add_warning(config_option, &msg);
                }
            }
        } else {
            let msg = format!("Unknown filesystem type '{}'. Use --list-filesystems to see supported types", fstype);
            if is_error {
                result.add_error(fstype, &msg);
            } else {
                result.add_warning(fstype, &msg);
            }
        }
    }

    fn validate_block_device(&self, blktype: &str, result: &mut ValidationResult, is_error: bool) {
        let blktype_lower = blktype.to_lowercase();
        let blk_entry = SUPPORTED_BLOCK_DEVICES.iter().find(|(name, config, _)| {
            name.eq_ignore_ascii_case(&blktype_lower) ||
            config.trim_start_matches("CONFIG_").eq_ignore_ascii_case(blktype) ||
            config.trim_start_matches("CONFIG_").replace('_', "-").eq_ignore_ascii_case(blktype)
        });

        if let Some((blk_name, config_name, _)) = blk_entry {
            let config_option = config_name.trim_start_matches("CONFIG_");
            if self.is_enabled(config_option) {
                result.add_success(config_option, &format!("Block device {} is supported", blk_name));
            } else {
                let msg = if is_error {
                    format!("Block device {} is not enabled. Required for specified block device", blk_name)
                } else {
                    format!("Block device {} is not validated (use --block-devices to validate)", blk_name)
                };

                if is_error {
                    result.add_error(config_option, &msg);
                } else {
                    result.add_warning(config_option, &msg);
                }
            }
        } else {
            let msg = format!("Unknown block device type '{}'. Use --list-block-devices to see supported types", blktype);
            if is_error {
                result.add_error(blktype, &msg);
            } else {
                result.add_warning(blktype, &msg);
            }
        }
    }

    fn validate_base_features(&self, result: &mut ValidationResult) {
        let required_features = vec![
            ("BLK_DEV_INITRD", "Initial RAM filesystem support"),
            ("DEVTMPFS", "Maintain a devtmpfs filesystem"),
            ("TMPFS", "Tmpfs virtual memory filesystem"),
            ("PROC_FS", "Proc filesystem"),
            ("SYSFS", "Sysfs filesystem"),
        ];

        for (feature, description) in required_features {
            if self.is_enabled(feature) {
                result.add_info(feature, &format!("{} is enabled", description));
            } else {
                result.add_warning(feature, &format!("{} is not enabled. This may cause boot issues", description));
            }
        }
    }
}

#[derive(Debug)]
pub struct ValidationResult {
    errors: Vec<(String, String)>,
    warnings: Vec<(String, String)>,
    successes: Vec<(String, String)>,
    info: Vec<(String, String)>,
}

impl ValidationResult {
    pub fn new() -> Self {
        ValidationResult {
            errors: Vec::new(),
            warnings: Vec::new(),
            successes: Vec::new(),
            info: Vec::new(),
        }
    }

    pub fn add_error(&mut self, option: &str, message: &str) {
        self.errors.push((option.to_string(), message.to_string()));
    }

    pub fn add_warning(&mut self, option: &str, message: &str) {
        self.warnings.push((option.to_string(), message.to_string()));
    }

    pub fn add_success(&mut self, option: &str, message: &str) {
        self.successes.push((option.to_string(), message.to_string()));
    }

    pub fn add_info(&mut self, option: &str, message: &str) {
        self.info.push((option.to_string(), message.to_string()));
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    pub fn print(&self) {
        println!("\n{}", "=== Kernel Configuration Validation ===".bright_cyan().bold());

        if !self.info.is_empty() {
            println!("\n{}", "Information:".bright_blue().bold());
            for (option, msg) in &self.info {
                println!("  {} {}", "ℹ".bright_blue(), format!("[{}] {}", option, msg).white());
            }
        }

        if !self.warnings.is_empty() {
            println!("\n{}", "Warnings:".bright_yellow().bold());
            for (option, msg) in &self.warnings {
                println!("  {} {}", "⚠".bright_yellow(), format!("[{}] {}", option, msg).yellow());
            }
        }

        if !self.errors.is_empty() {
            println!("\n{}", "Errors:".bright_red().bold());
            for (option, msg) in &self.errors {
                println!("  {} {}", "✗".bright_red(), format!("[{}] {}", option, msg).red());
            }
        }

        println!("\n{}", "=== Validation Summary ===".bright_cyan().bold());
        println!(
            "  {} {} | {} {}",
            "Errors:".bright_red().bold(),
            self.errors.len().to_string().bright_red(),
            "Warnings:".bright_yellow().bold(),
            self.warnings.len().to_string().bright_yellow()
        );

        if self.has_errors() {
            println!("\n{}", "Validation FAILED - kernel configuration is incompatible".bright_red().bold());
        } else if self.has_warnings() {
            println!("\n{}", "Validation passed with warnings - check configuration".bright_yellow().bold());
        } else {
            println!("\n{}", "Validation PASSED - kernel configuration is compatible".bright_green().bold());
        }
        println!();
    }
}
