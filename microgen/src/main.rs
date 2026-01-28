mod analyser;
mod clidef;
mod kconfig_validator;
mod rdgen;
mod rdpack;

use clap::ArgMatches;
use colored::Colorize;
use kmoddep::{kerman::KernelInfo, modinfo::lsmod};
use rdgen::IrfsGen;
use std::{error::Error, io, path::PathBuf};

static VERSION: &str = "0.1.0";
static APPNAME: &str = "microgen";

fn run_info(params: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let rfs = params.get_one::<String>("list").map(|v| v.as_str());

    if params.get_flag("list-filesystems") {
        kconfig_validator::print_supported_filesystems();
        return Ok(());
    }

    if params.get_flag("list-block-devices") {
        kconfig_validator::print_supported_block_devices();
        return Ok(());
    }

    let k_info = kmoddep::get_kernel_infos(rfs)?;

    if rfs.is_some() {
        println!("{}", "Available kernels:".bright_yellow());
        for i in k_info {
            println!(
                "  {}",
                i.get_kernel_path().file_name().unwrap_or_default().to_str().unwrap_or_default().bright_yellow().bold()
            );
        }
    } else if params.get_flag("lsmod") {
        println!("\n{:<30} {:<10} {}", "Name".bright_yellow(), "Size".bright_yellow(), "Used by".bright_yellow());
        for m in lsmod() {
            println!(
                "{:<30} {:<10} {} {}",
                m.name.bright_green().bold(),
                m.mem_size.to_string().green(),
                m.instances.to_string().bright_white().bold(),
                m.dependencies.join(", ").white()
            );
        }
    }

    Ok(())
}

/// Run analysis and profile generator
fn run_analyse(_params: &ArgMatches) -> Result<(), Box<dyn Error>> {
    if !nix::unistd::Uid::effective().is_root() {
        return Err(Box::new(io::Error::new(io::ErrorKind::Unsupported, "error: superuser privileges required")));
    }

    let _cfg = analyser::SysAnalyser::new().get_config(kmoddep::get_kernel_infos(None)?)?;

    Ok(())
}

/// Create a new initramfs
fn run_new(params: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let x_mods: Vec<String> = params.get_many::<String>("extract").unwrap_or_default().map(|s| s.to_string()).collect();
    let k_info = kmoddep::get_kernel_infos(Some(params.get_one::<String>("root").unwrap()));
    let profile = params.get_one::<String>("config");
    let kernel_config = params.get_one::<String>("kernel-config");
    let validate_only = params.get_flag("validate-only");

    if let Err(k_info) = k_info {
        println!("Unable to get the information about the kernel: {}", k_info);
        return Ok(());
    }

    if !x_mods.is_empty() {
        let krel = params.get_one::<String>("kernel").unwrap().replace('"', "");
        for knfo in k_info.unwrap() {
            let kn = knfo.get_kernel_path().file_name().unwrap().to_str().unwrap().to_string();
            if krel == kn {
                println!("{:?}", knfo.get_deps_for(&x_mods.iter().map(|x| x.to_string()).collect::<Vec<String>>()));
            }
        }
    } else if let Some(profile) = profile {
        let cfg = profile::cfg::get_mh_config(Some(profile))?;

        if let Some(kconfig_path) = kernel_config {
            println!("{}", "Validating kernel configuration...".bright_cyan().bold());

            let filesystems: Option<Vec<String>> =
                params.get_many::<String>("filesystems").map(|values| values.map(|s| s.to_string()).collect());

            // Get optional block devices list from CLI
            let block_devices: Option<Vec<String>> =
                params.get_many::<String>("block-devices").map(|values| values.map(|s| s.to_string()).collect());

            match kconfig_validator::KConfigValidator::from_file(kconfig_path) {
                Ok(validator) => {
                    match validator.validate(&cfg, filesystems.as_deref(), block_devices.as_deref()) {
                        Ok(result) => {
                            result.print();

                            if validate_only {
                                // Exit after validation
                                if result.has_errors() {
                                    return Err(Box::new(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        "Kernel configuration validation failed",
                                    )));
                                }
                                return Ok(());
                            }

                            if result.has_errors() {
                                return Err(Box::new(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "Cannot generate initramfs: kernel configuration is incompatible",
                                )));
                            }

                            if result.has_warnings() {
                                println!("{}", "⚠ Continuing with warnings...".bright_yellow());
                            }
                        }
                        Err(e) => {
                            eprintln!("{}", format!("Validation error: {}", e).bright_red());
                            return Err(Box::new(e));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{}", format!("Failed to parse kernel config: {}", e).bright_red());
                    return Err(Box::new(e));
                }
            }
        } else if validate_only {
            return Err(Box::new(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--validate-only requires --kernel-config to be specified",
            )));
        }

        // Generate initramfs if not validation-only
        if !validate_only {
            if let Ok(k_info) = k_info {
                let kfo: Option<KernelInfo> = match k_info.len() {
                    0 => {
                        println!("ℹ No kernel modules found, assuming monolithic kernel");
                        None
                    }
                    1 => Some(k_info[0].to_owned()),
                    _ => {
                        return Err(Box::new(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "Multiple kernels found; please select one explicitly",
                        )))
                    }
                };
                println!("Generating initramfs");

                let firmware_list_path = params.get_one::<String>("firmware-list").map(PathBuf::from);
                let firmware_path = PathBuf::from(params.get_one::<String>("root").unwrap());

                IrfsGen::generate(
                    kfo.as_ref(),
                    cfg,
                    PathBuf::from(params.get_one::<String>("output").unwrap()),
                    PathBuf::from(params.get_one::<String>("file").unwrap()),
                    firmware_list_path,
                    firmware_path,
                )?;
            }
        }
    } else {
        clidef::clidef(VERSION, APPNAME).print_help().unwrap();
    }

    Ok(())
}

#[allow(clippy::unit_arg)]
fn main() -> Result<(), Box<dyn Error>> {
    let mut cli = clidef::clidef(VERSION, APPNAME);
    let params = cli.to_owned().get_matches();
    if params.get_flag("version") {
        println!("Version: {}", VERSION);
    } else {
        match match params.subcommand() {
            Some(("new", args)) => run_new(args),
            Some(("analyse", args)) => run_analyse(args),
            Some(("info", args)) => run_info(args),
            _ => Ok(cli.print_help()?),
        } {
            Ok(_) => {}
            Err(err) => {
                println!("{}", err);
            }
        }
    }

    Ok(())
}
