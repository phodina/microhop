use std::fs::File;
use std::io::Read;

fn main() {
    if let Ok(binary_path) = std::env::var("MICROHOP_BINARY_PATH") {
        let path = std::path::Path::new(&binary_path);
        if !path.exists() {
            panic!("MICROHOP_BINARY_PATH points to non-existent file: {}", binary_path);
        }

        if !path.is_file() {
            panic!("MICROHOP_BINARY_PATH is not a regular file: {}", binary_path);
        }

        match File::open(path) {
            Ok(mut file) => {
                let mut magic = [0u8; 4];
                match file.read_exact(&mut magic) {
                    Ok(_) => {
                        if magic != [0x7F, 0x45, 0x4C, 0x46] {
                            panic!(
                                "MICROHOP_BINARY_PATH is not an ELF executable: {}\n\
                                 Expected ELF magic header [0x7F, 'E', 'L', 'F'], but found [{:#04x}, {:#04x}, {:#04x}, {:#04x}]",
                                binary_path, magic[0], magic[1], magic[2], magic[3]
                            );
                        }
                    }
                    Err(e) => {
                        panic!("Failed to read magic header from MICROHOP_BINARY_PATH ({}): {}", binary_path, e);
                    }
                }
            }
            Err(e) => {
                panic!("Failed to open MICROHOP_BINARY_PATH ({}): {}", binary_path, e);
            }
        }

        println!("cargo:rustc-cfg=microhop_binary_path");
    }

    if let Ok(e2path) = std::env::var("E2FSCK_BINARY_PATH") {
        let path = std::path::Path::new(&e2path);
        if !path.exists() {
            panic!("E2FSCK_BINARY_PATH points to non-existent file: {}", e2path);
        }

        if !path.is_file() {
            panic!("E2FSCK_BINARY_PATH is not a regular file: {}", e2path);
        }

        match File::open(path) {
            Ok(mut file) => {
                let mut magic = [0u8; 4];
                match file.read_exact(&mut magic) {
                    Ok(_) => {
                        if magic != [0x7F, 0x45, 0x4C, 0x46] {
                            panic!(
                                "E2FSCK_BINARY_PATH is not an ELF executable: {}\n\
                                 Expected ELF magic header [0x7F, 'E', 'L', 'F'], but found [{:#04x}, {:#04x}, {:#04x}, {:#04x}]",
                                e2path, magic[0], magic[1], magic[2], magic[3]
                            );
                        }
                    }
                    Err(e) => {
                        panic!("Failed to read magic header from E2FSCK_BINARY_PATH ({}): {}", e2path, e);
                    }
                }
            }
            Err(e) => {
                panic!("Failed to open E2FSCK_BINARY_PATH ({}): {}", e2path, e);
            }
        }

        println!("cargo:rustc-cfg=e2fsck_binary_path");
    }

    // Inform cargo/rustc about the custom cfgs so `check-cfg` lint is happy.
    println!("cargo:rustc-check-cfg=cfg(microhop_binary_path)");
    println!("cargo:rustc-check-cfg=cfg(e2fsck_binary_path)");
}
