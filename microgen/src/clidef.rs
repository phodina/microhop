use clap::{builder::styling, Arg, Command};
use nix::sys::utsname::uname;

/// CLI definition
pub fn clidef(version: &'static str, appname: &'static str) -> Command {
    let styles = styling::Styles::styled()
        .header(styling::AnsiColor::Yellow.on_default() | styling::Effects::BOLD)
        .usage(styling::AnsiColor::Yellow.on_default() | styling::Effects::BOLD)
        .literal(styling::AnsiColor::BrightGreen.on_default())
        .placeholder(styling::AnsiColor::BrightRed.on_default());

    Command::new(appname)
        .version(version)
        .about(format!("{} - utility for generating microhop-based initramfs", appname))
        .arg(Arg::new("version").short('v').long("version").action(clap::ArgAction::SetTrue).help("Show version of Microhop"))
        .subcommand(
            Command::new("info")
                .about("Information about current system")
                .arg_required_else_help(true)
                .arg(
                    Arg::new("list")
                        .short('l')
                        .long("list")
                        .value_name("PATH")
                        .help("List available kernel versions in a given root filesystem")
                        .conflicts_with_all(["lsmod", "filesystems", "block-devices"]),
                )
                .arg(Arg::new("lsmod").short('m').long("lsmod").action(clap::ArgAction::SetTrue).help("Just a fancy lsmod")
                    .conflicts_with_all(["filesystems", "block-devices"]))
                .arg(Arg::new("filesystems").long("filesystems").action(clap::ArgAction::SetTrue)
                    .help("Show supported filesystems and their kernel config options"))
                .arg(Arg::new("block-devices").long("block-devices").action(clap::ArgAction::SetTrue)
                    .help("Show supported block devices and their kernel config options")),
        )
        .subcommand(Command::new("analyse").about("Analyse current system and generate a profile from it"))
        .subcommand(
            Command::new("new")
                .about("Create a new initramfs from a specified profile")
                .arg_required_else_help(true)
                .arg(
                    Arg::new("config")
                        .short('c')
                        .long("config")
                        .aliases(["profile"])
                        .short_alias('p')
                        .conflicts_with_all(["extract"])
                        .help("Path to the initramfs configuration (profile)"),
                )
                .arg(
                    Arg::new("kernel-config")
                        .long("kernel-config")
                        .value_name("PATH")
                        .help("Path to kernel .config file for validation against microhop.conf requirements"),
                )
                .arg(
                    Arg::new("firmware-list")
                        .long("firmware-list")
                        .value_name("PATH")
                        .help("Path to a file containing list of firmware files to include in initramfs"),
                )
                .arg(
                    Arg::new("validate-only")
                        .long("validate-only")
                        .action(clap::ArgAction::SetTrue)
                        .requires("kernel-config")
                        .requires("config")
                        .help("Only validate kernel config against microhop.conf, don't generate initramfs"),
                )
                .arg(
                    Arg::new("filesystems")
                        .long("filesystems")
                        .value_name("FS_TYPES")
                        .help("Comma-separated list of filesystem types to validate (e.g., squashfs,ext4). If not specified, filesystem validation will issue warnings only")
                        .value_delimiter(','),
                )
                .arg(
                    Arg::new("block-devices")
                        .long("block-devices")
                        .value_name("BLK_TYPES")
                        .help("Comma-separated list of block device types to validate (e.g., virtio_blk,nvme). If not specified, block device validation will issue warnings only")
                        .value_delimiter(','),
                )
                .arg(
                    Arg::new("extract")
                        .short('x')
                        .long("extract")
                        .help("Specify comma-separated list of kernel modules to be used.")
                        .value_delimiter(','),
                )
                .arg(
                    Arg::new("kernel")
                        .short('k')
                        .long("kernel")
                        .help("Kernel release")
                        .default_value(format!("{:?}", uname().unwrap().release())),
                )
                .arg(
                    Arg::new("root")
                        .short('r')
                        .long("root")
                        .help("Path to the root filesystem.")
                        .conflicts_with_all(["kernel"])
                        .default_value("/"),
                )
                .arg(
                    Arg::new("output")
                        .short('o')
                        .long("output")
                        .help("Set output directory for the initramfs build")
                        .default_value("./build"),
                )
                .arg(Arg::new("file").short('f').long("file").help("Output file.").default_value("./initramfs-microhop.zst")),
        )
        .disable_version_flag(true)
        .disable_colored_help(false)
        .styles(styles)
}
