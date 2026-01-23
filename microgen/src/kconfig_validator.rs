use colored::Colorize;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Error},
    path::Path,
};

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
        let option = if option.starts_with("CONFIG_") { option.to_string() } else { format!("CONFIG_{}", option) };

        match self.config.get(&option) {
            Some(val) => val == "y" || val == "m",
            None => false,
        }
    }

    pub fn is_module(&self, option: &str) -> bool {
        let option = if option.starts_with("CONFIG_") { option.to_string() } else { format!("CONFIG_{}", option) };

        match self.config.get(&option) {
            Some(val) => val == "m",
            None => false,
        }
    }

    pub fn get_value(&self, option: &str) -> Option<&String> {
        let option = if option.starts_with("CONFIG_") { option.to_string() } else { format!("CONFIG_{}", option) };

        self.config.get(&option)
    }

    pub fn validate(&self, mh_config: &profile::cfg::MhConfig) -> Result<ValidationResult, Error> {
        let mut result = ValidationResult::new();

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

        if let Ok(disks) = mh_config.get_disks() {
            for disk in disks {
                let fstype = disk.get_fstype().to_uppercase();
                self.validate_filesystem(&fstype, &mut result);
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
                &format!("Module {} is not enabled in kernel config. Required by microhop.conf", module_name),
            );
        }
    }

    fn validate_filesystem(&self, fstype: &str, result: &mut ValidationResult) {
        let config_name = format!("{}_FS", fstype);

        if self.is_enabled(&config_name) {
            result.add_success(&config_name, &format!("Filesystem {} is supported", fstype));
        } else {
            result.add_error(&config_name, &format!("Filesystem {} is not enabled. Required by microhop.conf", fstype));
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
        ValidationResult { errors: Vec::new(), warnings: Vec::new(), successes: Vec::new(), info: Vec::new() }
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
