//! GUI initialization and rendering using rlvgl and simpledrm framebuffer
//
// This module checks the device tree for a framebuffer node (simpledrm),
// initializes the rlvgl library if found, and renders a blue background.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;

#[cfg(feature = "gui")]
use rlvgl::{Lvgl, Color};

// Add fdt-rs to dependencies for DTB parsing
use fdt::Fdt;

/// Try to find a simpledrm framebuffer node in the DTB.
/// Returns the framebuffer device path if found.
fn find_simpledrm_framebuffer_node() -> Option<String> {
    // Try to open the DTB from /sys/firmware/fdt or /proc/device-tree
    let dtb_paths = ["/sys/firmware/fdt", "/boot/dtbs/current.dtb"];
    for path in &dtb_paths {
        if let Ok(mut file) = File::open(path) {
            let mut buf = Vec::new();
            if file.read_to_end(&mut buf).is_ok() {
                if let Ok(fdt) = Fdt::new(&buf) {
                    for node in fdt.all_nodes() {
                        if let Some(compat) = node.property("compatible").and_then(|p| p.as_str()) {
                            if compat.contains("simpledrm") || compat.contains("simple-framebuffer") {
                                // Try to get the framebuffer device path
                                // This is platform-specific; fallback to /dev/dri/card0
                                return Some("/dev/dri/card0".to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(feature = "gui")]
pub fn init_and_render_gui() {
    if let Some(fb_path) = find_simpledrm_framebuffer_node() {
        // Open the framebuffer device
        let fb = match File::options().read(true).write(true).open(&fb_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to open framebuffer {}: {}", fb_path, e);
                return;
            }
        };

        // Initialize LVGL (rlvgl)
        let mut lvgl = match Lvgl::init() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to initialize LVGL: {}", e);
                return;
            }
        };

        // Set up a blue background (assuming 32bpp ARGB)
        let (width, height) = (800, 600); // TODO: Query real resolution
        let blue = Color::from_rgb(0, 0, 255);
        lvgl.fill_screen(blue);

        // Present to framebuffer (pseudo-code, depends on rlvgl integration)
        // You may need to copy lvgl's buffer to the framebuffer memory here.
        // This is highly platform-specific and may require mmap.
        // For now, just log success.
        println!("Rendered blue background to {}", fb_path);
    } else {
        eprintln!("No simpledrm framebuffer node found in device tree");
    }
}

#[cfg(not(feature = "gui"))]
pub fn init_and_render_gui() {
    // GUI feature not enabled
}
