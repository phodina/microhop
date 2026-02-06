use std::{fs::File, io::Read, time::Duration};
use nix::sys::reboot::RebootMode;

/// Interactive recovery mode with keyboard input handling
pub fn interactive_recovery_mode(reason: &str) -> ! {
    log::error!("=== MICROHOP RECOVERY MODE ===");
    log::error!("Reason: {}", reason);
    log::error!("");
    log::error!("Available actions:");
    log::error!("  p - Power off the device");
    log::error!("  r - Reboot the device");  
    log::error!("");
    log::error!("Use hardware keys or connect keyboard:");
    log::error!("  Volume Up = 'r' (reboot)");
    log::error!("  Volume Down = 'p' (poweroff)");  
    log::error!("  Power Button = 'r' (reboot)");
    log::error!("");

    // Try to open keyboard/input devices
    let mut input_sources = Vec::new();
    
    // Standard keyboard input (for USB keyboards)
    if let Ok(stdin) = File::open("/dev/tty") {
        input_sources.push(("tty".to_string(), stdin));
        log::info!("Opened tty for keyboard input");
    }
    
    // Hardware buttons (volume keys, power button) 
    for i in 0..8 {
        let input_path = format!("/dev/input/event{}", i);
        if let Ok(dev) = File::open(&input_path) {
            input_sources.push((input_path.clone(), dev));
            log::info!("Opened input device: {}", input_path);
        }
    }
    
    if input_sources.is_empty() {
        log::warn!("No input devices available - using default poweroff after timeout");
        std::thread::sleep(Duration::from_secs(30));
        power_off();
    }

    log::error!("Waiting for user input...");
    
    let mut event_buffer = [0u8; 24]; // Linux input event is 24 bytes
    let mut char_buffer = [0u8; 1];
    
    loop {
        // Check each input source for data
        for (name, input) in &mut input_sources {
            if name.contains("event") {
                // Handle hardware button events
                match input.read(&mut event_buffer) {
                    Ok(24) => { // Full input event received
                        // Parse input event structure:
                        // struct input_event { time_t sec; suseconds_t usec; u16 type; u16 code; s32 value; }
                        let event_type = u16::from_le_bytes([event_buffer[16], event_buffer[17]]);
                        let event_code = u16::from_le_bytes([event_buffer[18], event_buffer[19]]);
                        let event_value = i32::from_le_bytes([event_buffer[20], event_buffer[21], event_buffer[22], event_buffer[23]]);
                        
                        // Only handle key press events (type=1, value=1 for press)
                        if event_type == 1 && event_value == 1 {
                            match event_code {
                                115 => { // KEY_VOLUMEUP
                                    log::info!("Volume Up pressed - rebooting");
                                    reboot();
                                }
                                114 => { // KEY_VOLUMEDOWN  
                                    log::info!("Volume Down pressed - powering off");
                                    power_off();
                                }
                                116 => { // KEY_POWER
                                    log::info!("Power button pressed - rebooting");
                                    reboot();
                                }
                                _ => {
                                    log::debug!("Unknown hardware key code: {}", event_code);
                                }
                            }
                        }
                    }
                    Ok(_) => continue, // Partial read, wait for more data
                    Err(_) => continue, // Read error, try next source
                }
            } else {
                // Handle regular keyboard input  
                match input.read(&mut char_buffer) {
                    Ok(0) => continue, // No data
                    Ok(_) => {
                        let key = char_buffer[0] as char;
                        match key.to_ascii_lowercase() {
                            'p' => {
                                log::info!("Power off requested by user");
                                power_off();
                            }
                            'r' => {
                                log::info!("Reboot requested by user");
                                reboot();
                            }
                            '\n' | '\r' => {
                                // Show menu again on Enter
                                log::error!("Available actions: (p)oweroff, (r)eboot");
                            }
                            _ => {
                                log::debug!("Unknown key pressed: '{}'", key);
                            }
                        }
                    }
                    Err(_) => continue, // Read error, try next source
                }
            }
        }
        
        // Small delay to prevent busy waiting
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Power off the system
pub fn power_off() -> ! {
    log::info!("Powering off system...");
    
    // Use direct syscall since binaries are not available in initrd
    let _ = nix::sys::reboot::reboot(RebootMode::RB_POWER_OFF);
    
    // If syscall fails, infinite loop to keep PID 1 alive
    loop {
        log::error!("Power off failed - system halted");
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// Reboot the system  
pub fn reboot() -> ! {
    log::info!("Rebooting system...");
    
    // Use direct syscall since binaries are not available in initrd
    let _ = nix::sys::reboot::reboot(RebootMode::RB_AUTOBOOT);
    
    // If syscall fails, infinite loop to keep PID 1 alive
    loop {
        log::error!("Reboot failed - system halted");
        std::thread::sleep(Duration::from_secs(60));
    }
}