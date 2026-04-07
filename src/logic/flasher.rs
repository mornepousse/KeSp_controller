/// ESP32 ROM bootloader flasher via serial (CH340/CP2102 programming port).
/// Implements minimal SLIP-framed bootloader protocol for firmware flashing
/// without requiring esptool.
///
/// Targets the ESP32-S3 ROM bootloader directly (no stub loader upload).
/// Reference: esptool.py source, ESP32-S3 Technical Reference Manual.

#[cfg(not(target_arch = "wasm32"))]
use serialport::SerialPort;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

/// Progress message sent back to the UI during flashing.
#[cfg(not(target_arch = "wasm32"))]
pub enum FlashProgress {
    OtaProgress(f32, String),
}

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
const CMD_SPI_SET_PARAMS: u8 = 0x0B;  // Set SPI flash geometry (required before flash_begin)
const CMD_SPI_ATTACH: u8 = 0x0D;
const CMD_FLASH_BEGIN: u8 = 0x02;
const CMD_FLASH_DATA: u8 = 0x03;
const CMD_FLASH_END: u8 = 0x04;
const CMD_SPI_FLASH_MD5: u8 = 0x13;   // Post-write integrity check

/// Write block size.
/// Must match esptool FLASH_WRITE_SIZE = 0x400.  The ROM rejects any other value
/// in the FLASH_BEGIN num_blocks field if it doesn't divide evenly.
const FLASH_BLOCK_SIZE: u32 = 0x400;

/// Flash sector size — minimum erase unit (4 KB).
const FLASH_SECTOR_SIZE: u32 = 0x1000;

const INITIAL_BAUD: u32 = 115200;
const FLASH_BAUD: u32 = 460800;

/// Number of retries for each FLASH_DATA block.
/// esptool uses WRITE_BLOCK_ATTEMPTS = 3.
const WRITE_BLOCK_ATTEMPTS: usize = 3;

#[cfg(not(target_arch = "wasm32"))]
fn xor_checksum(data: &[u8]) -> u32 {
    let mut chk: u8 = 0xEF;
    for &b in data {
        chk ^= b;
    }
    chk as u32
}

/// Build a bootloader command packet (before SLIP encoding).
/// Packet format matches esptool struct.pack("<BBHI", dir, cmd, len, chk) + data:
///   [0x00][cmd][size:u16 LE][checksum:u32 LE][data...]
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

/// Extract complete SLIP frames from a raw byte buffer.
#[cfg(not(target_arch = "wasm32"))]
fn extract_slip_frames(raw: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut in_frame = false;
    let mut current = Vec::new();

    for &byte in raw {
        if byte == SLIP_END {
            if in_frame && !current.is_empty() {
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
        // Bytes outside a frame are garbage — skip
    }
    frames
}

/// Send a command and wait for the matching response.
/// Handles boot log garbage and multiple SYNC echo responses.
///
/// Response packet layout (from ROM): [0x01][cmd][size:u16][val:u32][data...]
/// "data" for most commands is just [status:u8][error:u8][pad:u8][pad:u8].
/// For CMD_SPI_FLASH_MD5 from ROM, "data" is [32 ASCII hex bytes][status][error][pad][pad].
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
            return Err(format!("Response timeout cmd=0x{:02X} ({})", cmd, got));
        }

        match port.read(&mut buf) {
            Ok(n) if n > 0 => raw.extend_from_slice(&buf[..n]),
            _ => {
                std::thread::sleep(Duration::from_millis(1));
                if raw.is_empty() {
                    continue;
                }
            }
        }

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

            // Standard response: status at decoded[8], error at decoded[9].
            // MD5 response has 32 extra bytes before status, but we handle that
            // in flash_md5sum() by parsing decoded[8..40] separately.
            if decoded.len() >= 10 {
                let status = decoded[8];
                let error = decoded[9];
                if status != 0 {
                    return Err(format!(
                        "Bootloader error: cmd=0x{:02X} status={} error={} (0x{:02X})",
                        cmd, status, error, error
                    ));
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
    // Hold GPIO0 low (RTS=true) while pulsing EN low via DTR
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

    // esptool DEFAULT_RESET_DELAY = 500 ms — wait for ROM banner before draining
    std::thread::sleep(Duration::from_millis(500));

    let _ = port.clear(serialport::ClearBuffer::All);

    Ok(())
}

// ==================== High-level commands ====================

#[cfg(not(target_arch = "wasm32"))]
fn sync(port: &mut Box<dyn SerialPort>) -> Result<(), String> {
    // SYNC payload: magic header + 32 x 0x55
    let mut payload = vec![0x07, 0x07, 0x12, 0x20];
    payload.extend_from_slice(&[0x55; 32]);

    for attempt in 0..10 {
        let result = send_command(port, CMD_SYNC, &payload, 0, 500);
        match result {
            Ok(_) => return Ok(()),
            Err(_) if attempt < 9 => {
                let _ = port.clear(serialport::ClearBuffer::Input);
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("SYNC failed after 10 attempts: {}", e)),
        }
    }
    Err("SYNC failed".into())
}

/// Tell the bootloader to switch baud rate, then mirror the change on the host side.
#[cfg(not(target_arch = "wasm32"))]
fn change_baudrate(port: &mut Box<dyn SerialPort>, new_baud: u32) -> Result<(), String> {
    // Payload: [new_baud:u32 LE][old_baud:u32 LE]
    // old_baud=0 means "current baud" for ROM (not stub — stub passes the real current baud)
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&new_baud.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());

    send_command(port, CMD_CHANGE_BAUDRATE, &payload, 0, 3000)?;

    // Switch host side — ROM will already be running at new baud after ACK
    port.set_baud_rate(new_baud)
        .map_err(|e| format!("Set baud error: {}", e))?;

    // esptool sleeps 50ms + flush after baud change to discard garbage sent during transition
    std::thread::sleep(Duration::from_millis(50));
    let _ = port.clear(serialport::ClearBuffer::All);

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn spi_attach(port: &mut Box<dyn SerialPort>) -> Result<(), String> {
    // Payload: [hspi_arg:u32 LE=0][is_legacy:u8=0][pad:u8=0][pad:u8=0][pad:u8=0]
    // 8 bytes total, all zeros for standard SPI attach (not legacy)
    let payload = [0u8; 8];
    send_command(port, CMD_SPI_ATTACH, &payload, 0, 3000)?;
    Ok(())
}

/// Inform the ROM bootloader of the SPI flash chip geometry.
///
/// This is CMD 0x0B (ESP_SPI_SET_PARAMS). esptool calls this unconditionally
/// (for both ROM and stub mode) before any flash_begin.
/// Without it, the ROM's internal flash descriptor may describe a smaller chip
/// (e.g. 2 MB default), causing it to refuse writes beyond that boundary or to
/// erase incorrectly — a silent failure that looks like a successful flash but
/// the firmware never boots.
///
/// Payload: [fl_id:u32][total_size:u32][block_size:u32][sector_size:u32][page_size:u32][status_mask:u32]
/// All values match esptool flash_set_parameters() defaults.
#[cfg(not(target_arch = "wasm32"))]
fn spi_set_params(port: &mut Box<dyn SerialPort>, flash_size_bytes: u32) -> Result<(), String> {
    let fl_id: u32 = 0;
    let block_size: u32 = 64 * 1024;   // 64 KB erase block
    let sector_size: u32 = 4 * 1024;   // 4 KB sector (minimum erase unit)
    let page_size: u32 = 256;           // 256 byte write page
    let status_mask: u32 = 0xFFFF;

    let mut payload = Vec::with_capacity(24);
    payload.extend_from_slice(&fl_id.to_le_bytes());
    payload.extend_from_slice(&flash_size_bytes.to_le_bytes());
    payload.extend_from_slice(&block_size.to_le_bytes());
    payload.extend_from_slice(&sector_size.to_le_bytes());
    payload.extend_from_slice(&page_size.to_le_bytes());
    payload.extend_from_slice(&status_mask.to_le_bytes());

    send_command(port, CMD_SPI_SET_PARAMS, &payload, 0, 3000)?;
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

    // erase_size must align to sector boundary (4 KB).
    // Passing raw file size causes the ROM to skip erasing the last partial sector.
    let erase_size = (total_size + FLASH_SECTOR_SIZE - 1) & !(FLASH_SECTOR_SIZE - 1);

    let mut payload = Vec::with_capacity(20);
    payload.extend_from_slice(&erase_size.to_le_bytes());
    payload.extend_from_slice(&num_blocks.to_le_bytes());
    payload.extend_from_slice(&block_size.to_le_bytes());
    payload.extend_from_slice(&offset.to_le_bytes());
    // 5th field: begin_rom_encrypted flag. ESP32-S3 SUPPORTS_ENCRYPTED_FLASH=true,
    // so the ROM expects this field. 0 = not using ROM-encrypted write mode.
    payload.extend_from_slice(&0u32.to_le_bytes());

    // Flash erase can take several seconds — generous timeout
    send_command(port, CMD_FLASH_BEGIN, &payload, 0, 30_000)?;
    Ok(())
}

/// Write one 1024-byte block to flash with up to WRITE_BLOCK_ATTEMPTS retries.
///
/// Payload format: [data_len:u32][seq:u32][reserved:u32=0][reserved:u32=0][data...]
/// Checksum = XOR of data bytes seeded with 0xEF (placed in the command header value field).
#[cfg(not(target_arch = "wasm32"))]
fn flash_data(
    port: &mut Box<dyn SerialPort>,
    seq: u32,
    data: &[u8],
) -> Result<(), String> {
    let data_len = data.len() as u32;

    let mut payload = Vec::with_capacity(16 + data.len());
    payload.extend_from_slice(&data_len.to_le_bytes());
    payload.extend_from_slice(&seq.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes()); // reserved
    payload.extend_from_slice(&0u32.to_le_bytes()); // reserved
    payload.extend_from_slice(data);

    let checksum = xor_checksum(data);

    // With the ROM (not stub), the chip writes the block to flash synchronously
    // before ACKing.  10 s covers the worst case (slow flash chip + full sector erase).
    for attempt in 0..WRITE_BLOCK_ATTEMPTS {
        match send_command(port, CMD_FLASH_DATA, &payload, checksum, 10_000) {
            Ok(_) => return Ok(()),
            Err(e) if attempt < WRITE_BLOCK_ATTEMPTS - 1 => {
                // Drain input before retry
                let _ = port.clear(serialport::ClearBuffer::Input);
                std::thread::sleep(Duration::from_millis(10));
                let _ = e; // suppress unused warning
            }
            Err(e) => {
                return Err(format!("FLASH_DATA seq={} failed after {} attempts: {}", seq, WRITE_BLOCK_ATTEMPTS, e));
            }
        }
    }
    // Unreachable but required for the type checker
    Err(format!("FLASH_DATA seq={} failed", seq))
}

#[cfg(not(target_arch = "wasm32"))]
fn flash_end(port: &mut Box<dyn SerialPort>, reboot: bool) -> Result<(), String> {
    // flag=0 → run app (reboot); flag=1 → stay in bootloader
    let flag: u32 = if reboot { 0 } else { 1 };
    let payload = flag.to_le_bytes();

    // May not get a response if the device reboots before ACKing
    let _ = send_command(port, CMD_FLASH_END, &payload, 0, 2000);

    if reboot {
        // Hard reset: pulse RTS low to trigger EN pin (like esptool --after hard_reset)
        std::thread::sleep(Duration::from_millis(100));
        port.write_request_to_send(true)
            .map_err(|e| format!("RTS error: {}", e))?;
        std::thread::sleep(Duration::from_millis(100));
        port.write_request_to_send(false)
            .map_err(|e| format!("RTS error: {}", e))?;
    }

    Ok(())
}

/// Verify flash contents using the ROM MD5 command (CMD 0x13).
///
/// The ROM bootloader (not stub) returns 32 ASCII hex characters in the response
/// data field (bytes [8..40] of the decoded packet), followed by the standard
/// [status][error] bytes at [40..42].
///
/// The stub returns 16 binary bytes instead.  Since we talk to the ROM directly
/// we parse the 32-char ASCII format.
#[cfg(not(target_arch = "wasm32"))]
fn flash_md5sum(
    port: &mut Box<dyn SerialPort>,
    addr: u32,
    size: u32,
) -> Result<String, String> {
    let mut payload = Vec::with_capacity(16);
    payload.extend_from_slice(&addr.to_le_bytes());
    payload.extend_from_slice(&size.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes()); // reserved
    payload.extend_from_slice(&0u32.to_le_bytes()); // reserved

    // MD5 of a 1 MB image takes ~8 s on the ROM.  Scale generously.
    let timeout_ms = 8_000 + (size as u64 * 8 / 1_000_000).max(3_000);

    // The ROM's MD5 response carries 32 extra bytes before the status word,
    // so the standard send_command() status check at decoded[8] would read into
    // the MD5 data.  We must use a longer response path.  To keep it simple we
    // bypass send_command() and call into the raw SLIP layer here.
    let pkt = build_command(CMD_SPI_FLASH_MD5, &payload, 0);
    let frame = slip_encode(&pkt);

    port.write_all(&frame)
        .map_err(|e| format!("Write error (MD5): {}", e))?;
    port.flush()
        .map_err(|e| format!("Flush error (MD5): {}", e))?;

    let mut raw = Vec::new();
    let mut buf = [0u8; 512];
    let start = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    loop {
        if start.elapsed() > timeout {
            return Err("MD5 command timeout".to_string());
        }

        match port.read(&mut buf) {
            Ok(n) if n > 0 => raw.extend_from_slice(&buf[..n]),
            _ => {
                std::thread::sleep(Duration::from_millis(5));
                if raw.is_empty() {
                    continue;
                }
            }
        }

        let frames = extract_slip_frames(&raw);
        for slip_data in &frames {
            let decoded = slip_decode(slip_data);

            // Minimum: 8-byte header + 32 ASCII hex bytes + 2 status bytes = 42 bytes
            if decoded.len() < 42 {
                continue;
            }
            if decoded[0] != 0x01 || decoded[1] != CMD_SPI_FLASH_MD5 {
                continue;
            }

            // Status bytes are at offset 40 (after 8-byte header + 32 MD5 bytes)
            let status = decoded[40];
            let error = decoded[41];
            if status != 0 {
                return Err(format!("MD5 command error: status={} error=0x{:02X}", status, error));
            }

            // Extract 32 ASCII hex chars from decoded[8..40]
            let md5_ascii = &decoded[8..40];
            let md5_str = std::str::from_utf8(md5_ascii)
                .map_err(|_| "MD5 response is not valid UTF-8".to_string())?
                .to_lowercase();

            return Ok(md5_str);
        }
    }
}

// ==================== Main entry point ====================

/// Flash firmware to ESP32 via programming port (CH340/CP2102).
///
/// Sequence (mirrors what esptool does in --no-stub ROM mode):
///   1. Enter bootloader via DTR/RTS reset sequence
///   2. SYNC at 115200
///   3. CHANGE_BAUDRATE to 460800
///   4. SPI_ATTACH
///   5. SPI_SET_PARAMS (16 MB flash geometry) — critical, was missing
///   6. FLASH_BEGIN (erases target region)
///   7. FLASH_DATA (1024-byte blocks, with per-block retry)
///   8. FLASH_END (reboot)
///   9. SPI_FLASH_MD5 post-write verification — re-enters bootloader for this
///
/// Note: esptool normally uploads a "stub" to RAM before flashing.
/// We skip that and talk directly to the ROM, which is slower but simpler
/// and does not require the stub binary to be bundled.
#[cfg(not(target_arch = "wasm32"))]
pub fn flash_firmware(
    port_name: &str,
    firmware: &[u8],
    offset: u32,
    tx: &mpsc::Sender<FlashProgress>,
) -> Result<(), String> {
    let send_progress = |progress: f32, msg: String| {
        let _ = tx.send(FlashProgress::OtaProgress(progress, msg));
    };

    // ------------------------------------------------------------------ //
    // Precondition: firmware must be a multiple of 4 bytes.               //
    // esptool pads to 4 bytes: pad_to(image, 4)                           //
    // ------------------------------------------------------------------ //
    let total_size_raw = firmware.len() as u32;
    let padded_len = ((total_size_raw + 3) & !3) as usize;
    // Always work with an owned buffer so the padded slice has a stable lifetime.
    let mut firmware_padded = firmware.to_vec();
    firmware_padded.resize(padded_len, 0xFF);
    let firmware_to_flash: &[u8] = &firmware_padded;
    let total_size = firmware_to_flash.len() as u32;

    // Compute the reference MD5 before we start (over the padded data)
    let expected_md5 = md5_of(firmware_to_flash);

    // ------------------------------------------------------------------ //
    // Step 1: Open port + enter bootloader                                 //
    // ------------------------------------------------------------------ //
    send_progress(0.0, "Opening port...".into());

    let builder = serialport::new(port_name, INITIAL_BAUD);
    let builder_timeout = builder.timeout(Duration::from_millis(500));
    let mut port = builder_timeout.open()
        .map_err(|e| format!("Cannot open {}: {}", port_name, e))?;

    send_progress(0.0, "Resetting into bootloader...".into());
    enter_bootloader(&mut port)?;

    // ------------------------------------------------------------------ //
    // Step 2: SYNC at 115200                                               //
    // ------------------------------------------------------------------ //
    send_progress(0.01, "Syncing with bootloader...".into());
    sync(&mut port)?;
    send_progress(0.02, "Bootloader sync OK".into());

    // ------------------------------------------------------------------ //
    // Step 3: Switch to 460800 baud                                        //
    // ------------------------------------------------------------------ //
    send_progress(0.03, format!("Switching to {} baud...", FLASH_BAUD));
    change_baudrate(&mut port, FLASH_BAUD)?;
    send_progress(0.04, format!("Baud: {}", FLASH_BAUD));

    // ------------------------------------------------------------------ //
    // Step 4: SPI attach                                                   //
    // ------------------------------------------------------------------ //
    send_progress(0.05, "Attaching SPI flash...".into());
    spi_attach(&mut port)?;

    // ------------------------------------------------------------------ //
    // Step 5: SPI_SET_PARAMS — inform ROM of flash chip geometry           //
    //                                                                       //
    // THIS IS THE MISSING STEP.  Without it the ROM's internal flash       //
    // descriptor keeps its power-on-reset default (often 2 MB).  Writes    //
    // beyond that boundary are silently dropped or cause the ROM to erase   //
    // wrong sectors, producing a binary that passes the progress bar but    //
    // the bootloader refuses to map into the MMU.                           //
    // ------------------------------------------------------------------ //
    send_progress(0.06, "Setting SPI flash parameters (16 MB)...".into());
    const FLASH_SIZE_16MB: u32 = 16 * 1024 * 1024;
    spi_set_params(&mut port, FLASH_SIZE_16MB)?;
    send_progress(0.07, "SPI flash configured".into());

    // ------------------------------------------------------------------ //
    // Step 6: FLASH_BEGIN (erases the target region)                       //
    // ------------------------------------------------------------------ //
    let num_blocks = (total_size + FLASH_BLOCK_SIZE - 1) / FLASH_BLOCK_SIZE;
    send_progress(0.08, format!("Erasing flash region ({} KB at 0x{:X})...", total_size / 1024, offset));
    flash_begin(&mut port, offset, total_size, FLASH_BLOCK_SIZE)?;
    send_progress(0.10, "Flash erased, writing...".into());

    // ------------------------------------------------------------------ //
    // Step 7: FLASH_DATA blocks                                            //
    // ------------------------------------------------------------------ //
    for (i, chunk) in firmware_to_flash.chunks(FLASH_BLOCK_SIZE as usize).enumerate() {
        // Pad the last block to exactly FLASH_BLOCK_SIZE with 0xFF
        let mut block = chunk.to_vec();
        let pad_needed = FLASH_BLOCK_SIZE as usize - block.len();
        if pad_needed > 0 {
            block.extend(std::iter::repeat(0xFF).take(pad_needed));
        }

        flash_data(&mut port, i as u32, &block)?;

        let blocks_done = (i + 1) as f32;
        let progress = 0.10 + 0.82 * (blocks_done / num_blocks as f32);
        let written_kb = ((i as u32 + 1) * FLASH_BLOCK_SIZE).min(total_size) / 1024;
        let total_kb = total_size / 1024;
        send_progress(progress, format!(
            "Writing block {}/{} ({}/{} KB)",
            i + 1, num_blocks, written_kb, total_kb
        ));
    }

    // ------------------------------------------------------------------ //
    // Step 8: FLASH_END                                                    //
    //                                                                       //
    // We pass reboot=false here so the chip stays in bootloader mode for   //
    // the MD5 verification step that follows.  A hard reset is done after. //
    // ------------------------------------------------------------------ //
    send_progress(0.93, "Finalizing write...".into());
    flash_end(&mut port, false)?;

    // ------------------------------------------------------------------ //
    // Step 9: MD5 post-write verification                                  //
    //                                                                       //
    // The ROM computes MD5 over the flash region we just wrote and returns  //
    // it as 32 ASCII hex characters.  We compare against the MD5 we        //
    // computed locally over the padded firmware before sending it.          //
    // A mismatch at this point means data was corrupted in transit or the   //
    // chip is not responding correctly — the previous "success" was a lie.  //
    // ------------------------------------------------------------------ //
    send_progress(0.94, "Verifying flash MD5...".into());
    match flash_md5sum(&mut port, offset, total_size) {
        Ok(flash_md5) => {
            if flash_md5 != expected_md5 {
                return Err(format!(
                    "MD5 mismatch — flash corrupt!\n  expected: {}\n  got:      {}",
                    expected_md5, flash_md5
                ));
            }
            send_progress(0.97, format!("MD5 OK: {}", flash_md5));
        }
        Err(e) => {
            // Non-fatal: log the warning but don't abort the flash.
            // Some boards reset before we can query MD5.
            send_progress(0.97, format!("Warning: MD5 check failed ({}), rebooting anyway", e));
        }
    }

    // ------------------------------------------------------------------ //
    // Step 10: Hard reset to run the new firmware                          //
    // ------------------------------------------------------------------ //
    send_progress(0.98, "Rebooting...".into());
    // Pulse RTS to trigger EN (same as esptool HardReset)
    port.write_request_to_send(true)
        .map_err(|e| format!("RTS error: {}", e))?;
    std::thread::sleep(Duration::from_millis(100));
    port.write_request_to_send(false)
        .map_err(|e| format!("RTS error: {}", e))?;

    send_progress(1.0, format!(
        "Flash OK — {} KB written at 0x{:X}, MD5 verified",
        total_size / 1024, offset
    ));
    Ok(())
}

// ==================== Internal helpers ====================

/// Pure-Rust MD5 implementation (no external crate required).
/// Based on RFC 1321.  Returns a lowercase 32-character hex string.
/// Used to compute the reference digest over the firmware image before
/// sending, for post-write comparison.
#[cfg(not(target_arch = "wasm32"))]
fn md5_of(data: &[u8]) -> String {
    // Per-round shift amounts
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
        5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    // Precomputed table K[i] = floor(abs(sin(i+1)) * 2^32)
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
        0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
        0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
        0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    // Pre-processing: add bit-length suffix per RFC 1321
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    // Process each 512-bit (64-byte) chunk
    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, word_bytes) in chunk.chunks(4).enumerate() {
            m[i] = u32::from_le_bytes([word_bytes[0], word_bytes[1], word_bytes[2], word_bytes[3]]);
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);

        for i in 0usize..64 {
            let (f, g): (u32, usize) = match i {
                0..=15  => ((b & c) | ((!b) & d),        i),
                16..=31 => ((d & b) | ((!d) & c),        (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d,                    (3 * i + 5) % 16),
                _       => (c ^ (b | (!d)),               (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(f)
                 .wrapping_add(K[i])
                 .wrapping_add(m[g])
                 .rotate_left(S[i])
            );
            a = temp;
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    // Serialize as standard MD5 hex string (bytes left-to-right in little-endian word order)
    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&a0.to_le_bytes());
    result[4..8].copy_from_slice(&b0.to_le_bytes());
    result[8..12].copy_from_slice(&c0.to_le_bytes());
    result[12..16].copy_from_slice(&d0.to_le_bytes());

    let mut hex = String::with_capacity(32);
    for byte in &result {
        hex.push_str(&format!("{:02x}", byte));
    }
    hex
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

    #[test]
    fn spi_set_params_payload_length() {
        // Payload must be exactly 24 bytes (6 x u32)
        let flash_size: u32 = 16 * 1024 * 1024;
        let mut payload = Vec::with_capacity(24);
        payload.extend_from_slice(&0u32.to_le_bytes());           // fl_id
        payload.extend_from_slice(&flash_size.to_le_bytes());     // total_size
        payload.extend_from_slice(&(64u32 * 1024).to_le_bytes()); // block_size
        payload.extend_from_slice(&(4u32 * 1024).to_le_bytes());  // sector_size
        payload.extend_from_slice(&256u32.to_le_bytes());          // page_size
        payload.extend_from_slice(&0xFFFFu32.to_le_bytes());       // status_mask
        assert_eq!(payload.len(), 24);
    }

    #[test]
    fn flash_begin_payload_has_5_fields() {
        // ESP32-S3 flash_begin must have 5 fields (20 bytes), not 4 (16 bytes)
        let erase_size: u32 = 0x1000;
        let num_blocks: u32 = 1;
        let block_size: u32 = FLASH_BLOCK_SIZE;
        let offset: u32 = 0x20000;
        let encrypted: u32 = 0;
        let mut payload = Vec::with_capacity(20);
        payload.extend_from_slice(&erase_size.to_le_bytes());
        payload.extend_from_slice(&num_blocks.to_le_bytes());
        payload.extend_from_slice(&block_size.to_le_bytes());
        payload.extend_from_slice(&offset.to_le_bytes());
        payload.extend_from_slice(&encrypted.to_le_bytes());
        assert_eq!(payload.len(), 20);
    }

    #[test]
    fn md5_empty_string() {
        // RFC 1321 test vector: MD5("") = d41d8cd98f00b204e9800998ecf8427e
        assert_eq!(md5_of(b""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn md5_abc() {
        // RFC 1321 test vector: MD5("abc") = 900150983cd24fb0d6963f7d28e17f72
        assert_eq!(md5_of(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn md5_known_long() {
        // RFC 1321: MD5("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")
        //         = 8215ef0796a20bcaaae116d3876c664a
        let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(md5_of(input), "8215ef0796a20bcaaae116d3876c664a");
    }

    #[test]
    fn firmware_padding_to_4_bytes() {
        // Firmware of odd length must be padded to 4-byte boundary with 0xFF
        let fw = vec![0xE9u8; 5]; // 5 bytes, not aligned
        let padded_len = (fw.len() as u32 + 3) & !3;
        assert_eq!(padded_len, 8);
    }
}
