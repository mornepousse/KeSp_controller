/// ESP32 ROM bootloader flasher via serial (CH340/CP2102 programming port).
/// Implements minimal SLIP-framed bootloader protocol for firmware flashing
/// without requiring esptool.

#[cfg(not(target_arch = "wasm32"))]
use serialport::SerialPort;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

// ==================== SLIP framing ====================

const SLIP_END: u8 = 0xC0;
const SLIP_ESC: u8 = 0xDB;
const SLIP_ESC_END: u8 = 0xDC;
const SLIP_ESC_ESC: u8 = 0xDD;

#[cfg(not(target_arch = "wasm32"))]
fn slip_encode(data: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(data.len() + 10);
    frame.push(SLIP_END);
    for &byte in data {
        match byte {
            SLIP_END => {
                frame.push(SLIP_ESC);
                frame.push(SLIP_ESC_END);
            }
            SLIP_ESC => {
                frame.push(SLIP_ESC);
                frame.push(SLIP_ESC_ESC);
            }
            _ => frame.push(byte),
        }
    }
    frame.push(SLIP_END);
    frame
}

#[cfg(not(target_arch = "wasm32"))]
fn slip_decode(frame: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(frame.len());
    let mut escaped = false;
    for &byte in frame {
        if escaped {
            match byte {
                SLIP_ESC_END => data.push(SLIP_END),
                SLIP_ESC_ESC => data.push(SLIP_ESC),
                _ => data.push(byte),
            }
            escaped = false;
        } else if byte == SLIP_ESC {
            escaped = true;
        } else if byte != SLIP_END {
            data.push(byte);
        }
    }
    data
}

// ==================== Bootloader commands ====================

const CMD_SYNC: u8 = 0x08;
const CMD_CHANGE_BAUDRATE: u8 = 0x0F;
const CMD_SPI_ATTACH: u8 = 0x0D;
const CMD_FLASH_BEGIN: u8 = 0x02;
const CMD_FLASH_DATA: u8 = 0x03;
const CMD_FLASH_END: u8 = 0x04;

const FLASH_BLOCK_SIZE: u32 = 1024;
const INITIAL_BAUD: u32 = 115200;
const FLASH_BAUD: u32 = 460800;

#[cfg(not(target_arch = "wasm32"))]
fn xor_checksum(data: &[u8]) -> u32 {
    let mut chk: u8 = 0xEF;
    for &b in data {
        chk ^= b;
    }
    chk as u32
}

/// Build a bootloader command packet (before SLIP encoding).
/// Format: [0x00][cmd][size:u16 LE][checksum:u32 LE][data...]
#[cfg(not(target_arch = "wasm32"))]
fn build_command(cmd: u8, data: &[u8], checksum: u32) -> Vec<u8> {
    let size = data.len() as u16;
    let mut pkt = Vec::with_capacity(8 + data.len());
    pkt.push(0x00); // direction: command
    pkt.push(cmd);
    pkt.push((size & 0xFF) as u8);
    pkt.push((size >> 8) as u8);
    pkt.push((checksum & 0xFF) as u8);
    pkt.push(((checksum >> 8) & 0xFF) as u8);
    pkt.push(((checksum >> 16) & 0xFF) as u8);
    pkt.push(((checksum >> 24) & 0xFF) as u8);
    pkt.extend_from_slice(data);
    pkt
}

/// Extract complete SLIP frames from a byte buffer.
/// Returns (frames, remaining_bytes_not_consumed).
#[cfg(not(target_arch = "wasm32"))]
fn extract_slip_frames(raw: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut in_frame = false;
    let mut current = Vec::new();

    for &byte in raw {
        if byte == SLIP_END {
            if in_frame && !current.is_empty() {
                // End of frame
                frames.push(current.clone());
                current.clear();
                in_frame = false;
            } else {
                // Start of frame (or consecutive 0xC0)
                in_frame = true;
                current.clear();
            }
        } else if in_frame {
            current.push(byte);
        }
        // If !in_frame and byte != SLIP_END, it's garbage — skip
    }
    frames
}

/// Send a command and receive a valid response.
/// Handles boot log garbage and multiple SYNC responses.
#[cfg(not(target_arch = "wasm32"))]
fn send_command(
    port: &mut Box<dyn SerialPort>,
    cmd: u8,
    data: &[u8],
    checksum: u32,
    timeout_ms: u64,
) -> Result<Vec<u8>, String> {
    let pkt = build_command(cmd, data, checksum);
    let frame = slip_encode(&pkt);

    port.write_all(&frame)
        .map_err(|e| format!("Write error: {}", e))?;
    port.flush()
        .map_err(|e| format!("Flush error: {}", e))?;

    // Read bytes and extract SLIP frames, looking for a valid response
    let mut raw = Vec::new();
    let mut buf = [0u8; 512];
    let start = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    loop {
        let elapsed = start.elapsed();
        if elapsed > timeout {
            let got = if raw.is_empty() {
                "nothing".to_string()
            } else {
                format!("{} raw bytes, no valid response", raw.len())
            };
            return Err(format!("Response timeout (got {})", got));
        }

        let read_result = port.read(&mut buf);
        match read_result {
            Ok(n) if n > 0 => {
                raw.extend_from_slice(&buf[..n]);
            }
            _ => {
                std::thread::sleep(Duration::from_millis(1));
                if raw.is_empty() {
                    continue;
                }
            }
        }

        // Try to find a valid response in accumulated data
        let frames = extract_slip_frames(&raw);
        for slip_data in &frames {
            let decoded = slip_decode(slip_data);

            if decoded.len() < 8 {
                continue;
            }

            let direction = decoded[0];
            let resp_cmd = decoded[1];

            if direction != 0x01 || resp_cmd != cmd {
                continue;
            }

            // ROM bootloader status is at offset 8 (right after 8-byte header)
            // Format: [dir][cmd][size:u16][value:u32][status][error][pad][pad]
            if decoded.len() >= 10 {
                let status = decoded[8];
                let error = decoded[9];
                if status != 0 {
                    return Err(format!("Bootloader error: cmd=0x{:02X} status={}, error={} (0x{:02X})",
                        cmd, status, error, error));
                }
            }

            return Ok(decoded);
        }
    }
}

// ==================== Bootloader entry ====================

/// Toggle DTR/RTS to reset ESP32 into bootloader mode.
/// Standard auto-reset circuit: DTR→EN, RTS→GPIO0.
#[cfg(not(target_arch = "wasm32"))]
fn enter_bootloader(port: &mut Box<dyn SerialPort>) -> Result<(), String> {
    // Hold GPIO0 low (RTS=true) while pulsing EN (DTR)
    port.write_data_terminal_ready(false)
        .map_err(|e| format!("DTR error: {}", e))?;
    port.write_request_to_send(true)
        .map_err(|e| format!("RTS error: {}", e))?;
    std::thread::sleep(Duration::from_millis(100));

    // Release EN (DTR=true) while keeping GPIO0 low
    port.write_data_terminal_ready(true)
        .map_err(|e| format!("DTR error: {}", e))?;
    port.write_request_to_send(false)
        .map_err(|e| format!("RTS error: {}", e))?;
    std::thread::sleep(Duration::from_millis(50));

    // Release all
    port.write_data_terminal_ready(false)
        .map_err(|e| format!("DTR error: {}", e))?;

    // Drain any boot message
    let _ = port.clear(serialport::ClearBuffer::All);
    std::thread::sleep(Duration::from_millis(200));

    Ok(())
}

// ==================== High-level commands ====================

#[cfg(not(target_arch = "wasm32"))]
fn sync(port: &mut Box<dyn SerialPort>) -> Result<(), String> {
    // SYNC payload: [0x07, 0x07, 0x12, 0x20] + 32 x 0x55
    let mut payload = vec![0x07, 0x07, 0x12, 0x20];
    payload.extend_from_slice(&[0x55; 32]);

    for attempt in 0..10 {
        let result = send_command(port, CMD_SYNC, &payload, 0, 500);
        match result {
            Ok(_) => return Ok(()),
            Err(_) if attempt < 9 => {
                // Drain any pending data before retry
                let _ = port.clear(serialport::ClearBuffer::Input);
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("SYNC failed after 10 attempts: {}", e)),
        }
    }
    Err("SYNC failed".into())
}

/// Tell the bootloader to switch to a faster baud rate, then reconnect.
#[cfg(not(target_arch = "wasm32"))]
fn change_baudrate(port: &mut Box<dyn SerialPort>, new_baud: u32) -> Result<(), String> {
    // Payload: [new_baud:u32 LE][old_baud:u32 LE] (old_baud=0 means "current")
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&new_baud.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());

    send_command(port, CMD_CHANGE_BAUDRATE, &payload, 0, 3000)?;

    // Switch host side to new baud
    port.set_baud_rate(new_baud)
        .map_err(|e| format!("Set baud error: {}", e))?;

    // Small delay for baud switch to take effect
    std::thread::sleep(Duration::from_millis(50));
    let _ = port.clear(serialport::ClearBuffer::All);

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn spi_attach(port: &mut Box<dyn SerialPort>) -> Result<(), String> {
    let payload = [0u8; 8];
    send_command(port, CMD_SPI_ATTACH, &payload, 0, 3000)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn flash_begin(
    port: &mut Box<dyn SerialPort>,
    offset: u32,
    total_size: u32,
    block_size: u32,
) -> Result<(), String> {
    let num_blocks = (total_size + block_size - 1) / block_size;

    let mut payload = Vec::with_capacity(20);
    // erase_size
    payload.extend_from_slice(&total_size.to_le_bytes());
    // num_blocks
    payload.extend_from_slice(&num_blocks.to_le_bytes());
    // block_size
    payload.extend_from_slice(&block_size.to_le_bytes());
    // offset
    payload.extend_from_slice(&offset.to_le_bytes());
    // encrypted (ESP32-S3 requires this 5th field — 0 = not encrypted)
    payload.extend_from_slice(&0u32.to_le_bytes());

    // FLASH_BEGIN can take a while (flash erase) — long timeout
    send_command(port, CMD_FLASH_BEGIN, &payload, 0, 30_000)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn flash_data(
    port: &mut Box<dyn SerialPort>,
    seq: u32,
    data: &[u8],
) -> Result<(), String> {
    let data_len = data.len() as u32;

    let mut payload = Vec::with_capacity(16 + data.len());
    // data length
    payload.extend_from_slice(&data_len.to_le_bytes());
    // sequence number
    payload.extend_from_slice(&seq.to_le_bytes());
    // reserved (2 x u32)
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    // data
    payload.extend_from_slice(data);

    let checksum = xor_checksum(data);
    send_command(port, CMD_FLASH_DATA, &payload, checksum, 10_000)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn flash_end(port: &mut Box<dyn SerialPort>, reboot: bool) -> Result<(), String> {
    let flag: u32 = if reboot { 0 } else { 1 };
    let payload = flag.to_le_bytes();
    // FLASH_END might not get a response if device reboots
    let _ = send_command(port, CMD_FLASH_END, &payload, 0, 2000);

    if reboot {
        // Hard reset: toggle RTS to pulse EN pin (like esptool --after hard_reset)
        std::thread::sleep(Duration::from_millis(100));
        port.write_request_to_send(true)
            .map_err(|e| format!("RTS error: {}", e))?;
        std::thread::sleep(Duration::from_millis(100));
        port.write_request_to_send(false)
            .map_err(|e| format!("RTS error: {}", e))?;
    }

    Ok(())
}

// ==================== Main entry point ====================

/// Flash firmware to ESP32 via programming port (CH340/CP2102).
/// Sends progress updates via the channel as (progress_0_to_1, status_message).
#[cfg(not(target_arch = "wasm32"))]
pub fn flash_firmware(
    port_name: &str,
    firmware: &[u8],
    offset: u32,
    tx: &mpsc::Sender<super::ui::BgResult>,
) -> Result<(), String> {
    let send_progress = |progress: f32, msg: String| {
        let _ = tx.send(super::ui::BgResult::OtaProgress(progress, msg));
    };

    send_progress(0.0, "Opening port...".into());

    let builder = serialport::new(port_name, INITIAL_BAUD);
    let builder_timeout = builder.timeout(Duration::from_millis(500));
    let mut port = builder_timeout.open()
        .map_err(|e| format!("Cannot open {}: {}", port_name, e))?;

    // Step 1: Enter bootloader
    send_progress(0.0, "Resetting into bootloader...".into());
    enter_bootloader(&mut port)?;

    // Step 2: Sync at 115200
    send_progress(0.0, "Syncing with bootloader...".into());
    sync(&mut port)?;
    send_progress(0.02, "Bootloader sync OK".into());

    // Step 3: Switch to 460800 baud for faster flashing
    send_progress(0.03, format!("Switching to {} baud...", FLASH_BAUD));
    change_baudrate(&mut port, FLASH_BAUD)?;
    send_progress(0.04, format!("Baud: {}", FLASH_BAUD));

    // Step 4: SPI attach
    send_progress(0.05, "Attaching SPI flash...".into());
    spi_attach(&mut port)?;

    // Step 5: Flash begin (this erases the flash — can take several seconds)
    let total_size = firmware.len() as u32;
    let num_blocks = (total_size + FLASH_BLOCK_SIZE - 1) / FLASH_BLOCK_SIZE;
    send_progress(0.05, format!("Erasing flash ({} KB)...", total_size / 1024));
    flash_begin(&mut port, offset, total_size, FLASH_BLOCK_SIZE)?;
    send_progress(0.10, "Flash erased, writing...".into());

    // Step 6: Flash data blocks
    for (i, chunk) in firmware.chunks(FLASH_BLOCK_SIZE as usize).enumerate() {
        // Pad last block to block_size
        let mut block = chunk.to_vec();
        let pad_needed = FLASH_BLOCK_SIZE as usize - block.len();
        if pad_needed > 0 {
            block.extend(std::iter::repeat(0xFF).take(pad_needed));
        }

        flash_data(&mut port, i as u32, &block)?;

        let blocks_done = (i + 1) as f32;
        let total_blocks = num_blocks as f32;
        let progress = 0.10 + 0.85 * (blocks_done / total_blocks);
        let msg = format!("Writing block {}/{} ({} KB / {} KB)",
            i + 1, num_blocks,
            ((i + 1) as u32 * FLASH_BLOCK_SIZE).min(total_size) / 1024,
            total_size / 1024);
        send_progress(progress, msg);
    }

    // Step 7: Flash end + reboot
    send_progress(0.97, "Finalizing...".into());
    flash_end(&mut port, true)?;

    send_progress(1.0, format!("Flash OK — {} KB written at 0x{:X}", total_size / 1024, offset));
    Ok(())
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slip_encode_no_special() {
        let data = vec![0x01, 0x02, 0x03];
        let encoded = slip_encode(&data);
        assert_eq!(encoded, vec![0xC0, 0x01, 0x02, 0x03, 0xC0]);
    }

    #[test]
    fn slip_encode_with_end_byte() {
        let data = vec![0x01, 0xC0, 0x03];
        let encoded = slip_encode(&data);
        assert_eq!(encoded, vec![0xC0, 0x01, 0xDB, 0xDC, 0x03, 0xC0]);
    }

    #[test]
    fn slip_encode_with_esc_byte() {
        let data = vec![0x01, 0xDB, 0x03];
        let encoded = slip_encode(&data);
        assert_eq!(encoded, vec![0xC0, 0x01, 0xDB, 0xDD, 0x03, 0xC0]);
    }

    #[test]
    fn slip_roundtrip() {
        let original = vec![0xC0, 0xDB, 0x00, 0xFF, 0xC0];
        let encoded = slip_encode(&original);
        let decoded = slip_decode(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn xor_checksum_basic() {
        let data = vec![0x01, 0x02, 0x03];
        let chk = xor_checksum(&data);
        let expected = 0xEF ^ 0x01 ^ 0x02 ^ 0x03;
        assert_eq!(chk, expected as u32);
    }

    #[test]
    fn xor_checksum_empty() {
        let chk = xor_checksum(&[]);
        assert_eq!(chk, 0xEF);
    }

    #[test]
    fn build_command_format() {
        let data = vec![0xAA, 0xBB];
        let pkt = build_command(0x08, &data, 0x12345678);
        assert_eq!(pkt[0], 0x00); // direction
        assert_eq!(pkt[1], 0x08); // command
        assert_eq!(pkt[2], 0x02); // size low
        assert_eq!(pkt[3], 0x00); // size high
        assert_eq!(pkt[4], 0x78); // checksum byte 0
        assert_eq!(pkt[5], 0x56); // checksum byte 1
        assert_eq!(pkt[6], 0x34); // checksum byte 2
        assert_eq!(pkt[7], 0x12); // checksum byte 3
        assert_eq!(pkt[8], 0xAA); // data
        assert_eq!(pkt[9], 0xBB);
    }
}
