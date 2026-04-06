use serialport::SerialPort;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::logic::binary_protocol::{self as bp, KrResponse};
use crate::logic::parsers::{ROWS, COLS};

const BAUD_RATE: u32 = 115200;
const CONNECT_TIMEOUT_MS: u64 = 300;
const QUERY_TIMEOUT_MS: u64 = 800;
const BINARY_READ_TIMEOUT_MS: u64 = 1500;
const LEGACY_BINARY_SETTLE_MS: u64 = 50;
const BINARY_SETTLE_MS: u64 = 30;
const JSON_TIMEOUT_SECS: u64 = 3;

pub struct SerialManager {
    port: Option<Box<dyn SerialPort>>,
    pub port_name: String,
    pub connected: bool,
    pub v2: bool, // true if firmware supports binary protocol v2
}

impl SerialManager {
    pub fn new() -> Self {
        Self {
            port: None,
            port_name: String::new(),
            connected: false,
            v2: false,
        }
    }

    #[allow(dead_code)]
    pub fn list_ports() -> Vec<String> {
        let available = serialport::available_ports();
        let ports = available.unwrap_or_default();
        ports.into_iter().map(|p| p.port_name).collect()
    }

    /// List only ports that look like ESP32 programming ports (CH340, CP210x, FTDI).
    pub fn list_prog_ports() -> Vec<String> {
        const CH340_VID: u16 = 0x1A86;
        const CP210X_VID: u16 = 0x10C4;
        const FTDI_VID: u16 = 0x0403;

        let available = serialport::available_ports();
        let ports = available.unwrap_or_default();
        ports.into_iter()
            .filter(|p| {
                matches!(&p.port_type, serialport::SerialPortType::UsbPort(usb)
                    if usb.vid == CH340_VID || usb.vid == CP210X_VID || usb.vid == FTDI_VID)
            })
            .map(|p| p.port_name)
            .collect()
    }

    pub fn connect(&mut self, port_name: &str) -> Result<(), String> {
        let builder = serialport::new(port_name, BAUD_RATE);
        let builder_with_timeout = builder.timeout(Duration::from_millis(CONNECT_TIMEOUT_MS));
        let open_result = builder_with_timeout.open();
        let port = open_result.map_err(|e| format!("Failed to open {}: {}", port_name, e))?;

        self.port = Some(port);
        self.port_name = port_name.to_string();
        self.connected = true;
        self.v2 = false;

        // Detect v2: try PING
        if let Some(p) = self.port.as_mut() {
            let _ = p.clear(serialport::ClearBuffer::All);
        }
        std::thread::sleep(Duration::from_millis(LEGACY_BINARY_SETTLE_MS));

        let ping_result = self.send_binary(bp::cmd::PING, &[]);
        if ping_result.is_ok() {
            self.v2 = true;
        }

        Ok(())
    }

    pub fn auto_connect(&mut self) -> Result<String, String> {
        let port_name = Self::find_kase_port()?;
        self.connect(&port_name)?;
        Ok(port_name)
    }

    pub fn find_kase_port() -> Result<String, String> {
        const TARGET_VID: u16 = 0xCAFE;
        const TARGET_PID: u16 = 0x4001;

        let available = serialport::available_ports();
        let ports = available.unwrap_or_default();
        if ports.is_empty() {
            return Err("No serial ports found".into());
        }

        // First pass: check USB VID/PID and product name
        for port in &ports {
            let port_type = &port.port_type;
            match port_type {
                serialport::SerialPortType::UsbPort(usb) => {
                    let vid_matches = usb.vid == TARGET_VID;
                    let pid_matches = usb.pid == TARGET_PID;
                    if vid_matches && pid_matches {
                        return Ok(port.port_name.clone());
                    }

                    match &usb.product {
                        Some(product) => {
                            let is_kase = product.contains("KaSe");
                            let is_kesp = product.contains("KeSp");
                            if is_kase || is_kesp {
                                return Ok(port.port_name.clone());
                            }
                        }
                        None => {}
                    }
                }
                _ => {}
            }
        }

        // Second pass (Linux only): check udevadm info
        #[cfg(target_os = "linux")]
        for port in &ports {
            let udevadm_result = std::process::Command::new("udevadm")
                .args(["info", "-n", &port.port_name])
                .output();

            match udevadm_result {
                Ok(output) => {
                    let stdout_bytes = &output.stdout;
                    let text = String::from_utf8_lossy(stdout_bytes);
                    let has_kase = text.contains("KaSe");
                    let has_kesp = text.contains("KeSp");
                    if has_kase || has_kesp {
                        return Ok(port.port_name.clone());
                    }
                }
                Err(_) => {}
            }
        }

        let scanned_count = ports.len();
        Err(format!("No KaSe keyboard found ({} port(s) scanned)", scanned_count))
    }

    pub fn port_mut(&mut self) -> Option<&mut Box<dyn SerialPort>> {
        self.port.as_mut()
    }

    pub fn disconnect(&mut self) {
        self.port = None;
        self.port_name.clear();
        self.connected = false;
        self.v2 = false;
    }

    // ==================== LOW-LEVEL: ASCII LEGACY ====================

    pub fn send_command(&mut self, cmd: &str) -> Result<(), String> {
        let port = self.port.as_mut().ok_or("Not connected")?;
        let data = format!("{}\r\n", cmd);
        let bytes = data.as_bytes();

        let write_result = port.write_all(bytes);
        write_result.map_err(|e| format!("Write: {}", e))?;

        let flush_result = port.flush();
        flush_result.map_err(|e| format!("Flush: {}", e))?;

        Ok(())
    }

    pub fn query_command(&mut self, cmd: &str) -> Result<Vec<String>, String> {
        self.send_command(cmd)?;

        let port = self.port.as_mut().ok_or("Not connected")?;
        let cloned_port = port.try_clone();
        let port_clone = cloned_port.map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(port_clone);

        let mut lines = Vec::new();
        let start = Instant::now();
        let max_wait = Duration::from_millis(QUERY_TIMEOUT_MS);

        loop {
            let elapsed = start.elapsed();
            if elapsed > max_wait {
                break;
            }

            let mut line = String::new();
            let read_result = reader.read_line(&mut line);

            match read_result {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim().to_string();
                    let is_terminal = trimmed == "OK" || trimmed == "ERROR";
                    if is_terminal {
                        break;
                    }
                    let is_not_empty = !trimmed.is_empty();
                    if is_not_empty {
                        lines.push(trimmed);
                    }
                }
                Err(_) => break,
            }
        }

        Ok(lines)
    }

    fn read_raw(&mut self, timeout_ms: u64) -> Result<Vec<u8>, String> {
        let port = self.port.as_mut().ok_or("Not connected")?;
        let mut buf = vec![0u8; 4096];
        let mut result = Vec::new();
        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);

        while start.elapsed() < timeout {
            let read_result = port.read(&mut buf);

            match read_result {
                Ok(n) if n > 0 => {
                    let received_bytes = &buf[..n];
                    result.extend_from_slice(received_bytes);
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => {
                    let got_something = !result.is_empty();
                    if got_something {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }

        Ok(result)
    }

    /// Legacy C> binary protocol
    fn query_legacy_binary(&mut self, cmd: &str) -> Result<(u8, Vec<u8>), String> {
        self.send_command(cmd)?;
        std::thread::sleep(Duration::from_millis(LEGACY_BINARY_SETTLE_MS));
        let raw = self.read_raw(BINARY_READ_TIMEOUT_MS)?;

        // Look for the "C>" header in the raw bytes
        let mut windows = raw.windows(2);
        let header_search = windows.position(|w| w == b"C>");
        let pos = header_search.ok_or("No C> header found")?;

        let min_packet_size = pos + 5;
        if raw.len() < min_packet_size {
            return Err("Packet too short".into());
        }

        let cmd_type = raw[pos + 2];
        let low_byte = raw[pos + 3] as u16;
        let high_byte = (raw[pos + 4] as u16) << 8;
        let data_len = low_byte | high_byte;

        let data_start = pos + 5;
        let data_end = data_start.checked_add(data_len as usize)
            .ok_or("Data length overflow")?;

        if raw.len() < data_end {
            return Err(format!("Incomplete: need {}, got {}", data_end, raw.len()));
        }

        let payload = raw[data_start..data_end].to_vec();
        Ok((cmd_type, payload))
    }

    // ==================== LOW-LEVEL: BINARY V2 ====================

    /// Send a KS frame, read KR response.
    pub fn send_binary(&mut self, cmd_id: u8, payload: &[u8]) -> Result<KrResponse, String> {
        let frame = bp::ks_frame(cmd_id, payload);
        let port = self.port.as_mut().ok_or("Not connected")?;

        let write_result = port.write_all(&frame);
        write_result.map_err(|e| format!("Write: {}", e))?;

        let flush_result = port.flush();
        flush_result.map_err(|e| format!("Flush: {}", e))?;

        std::thread::sleep(Duration::from_millis(BINARY_SETTLE_MS));
        let raw = self.read_raw(BINARY_READ_TIMEOUT_MS)?;

        let (resp, _remaining) = bp::parse_kr(&raw)?;
        let firmware_ok = resp.is_ok();
        if !firmware_ok {
            let status = resp.status_name();
            return Err(format!("Firmware error: {}", status));
        }

        Ok(resp)
    }

    // ==================== HIGH-LEVEL: AUTO V2/LEGACY ====================

    pub fn get_firmware_version(&mut self) -> Option<String> {
        // Try v2 binary first
        if self.v2 {
            let binary_result = self.send_binary(bp::cmd::VERSION, &[]);
            match binary_result {
                Ok(resp) => {
                    let raw_bytes = &resp.payload;
                    let lossy_string = String::from_utf8_lossy(raw_bytes);
                    let version = lossy_string.to_string();
                    return Some(version);
                }
                Err(_) => {}
            }
        }

        // Legacy fallback
        let query_result = self.query_command("VERSION?");
        let lines = match query_result {
            Ok(l) => l,
            Err(_) => return None,
        };
        let mut line_iter = lines.into_iter();
        let first_line = line_iter.next();
        first_line
    }

    pub fn get_keymap(&mut self, layer: u8) -> Result<Vec<Vec<u16>>, String> {
        // Try v2 binary first
        if self.v2 {
            let resp = self.send_binary(bp::cmd::KEYMAP_GET, &[layer])?;
            let keymap = self.parse_keymap_payload(&resp.payload)?;
            return Ok(keymap);
        }

        // Legacy
        let cmd = format!("KEYMAP{}", layer);
        let (cmd_type, data) = self.query_legacy_binary(&cmd)?;

        if cmd_type != 1 {
            return Err(format!("Unexpected cmd type: {}", cmd_type));
        }
        if data.len() < 2 {
            return Err("Data too short".into());
        }

        // skip 2-byte layer index in legacy
        let data_without_header = &data[2..];
        self.parse_keymap_payload(data_without_header)
    }

    fn parse_keymap_payload(&self, data: &[u8]) -> Result<Vec<Vec<u16>>, String> {
        // v2: [layer:u8][keycodes...] -- skip first byte
        // legacy: already stripped
        let expected_with_layer_byte = 1 + ROWS * COLS * 2;
        let has_layer_byte = data.len() >= expected_with_layer_byte;
        let offset = if has_layer_byte { 1 } else { 0 };
        let kc_data = &data[offset..];

        let needed_bytes = ROWS * COLS * 2;
        if kc_data.len() < needed_bytes {
            return Err(format!("Keymap data too short: {} bytes (need {})", kc_data.len(), needed_bytes));
        }

        let mut keymap = Vec::with_capacity(ROWS);

        for row_index in 0..ROWS {
            let mut row = Vec::with_capacity(COLS);

            for col_index in 0..COLS {
                let idx = (row_index * COLS + col_index) * 2;
                let low_byte = kc_data[idx] as u16;
                let high_byte = (kc_data[idx + 1] as u16) << 8;
                let keycode = low_byte | high_byte;
                row.push(keycode);
            }

            keymap.push(row);
        }

        Ok(keymap)
    }

    pub fn get_layer_names(&mut self) -> Result<Vec<String>, String> {
        // Try v2 binary first
        if self.v2 {
            let resp = self.send_binary(bp::cmd::LIST_LAYOUTS, &[])?;

            let payload = &resp.payload;
            if payload.is_empty() {
                return Err("Empty response".into());
            }

            let count = payload[0] as usize;
            let mut names = Vec::with_capacity(count);
            let mut i = 1;

            for _ in 0..count {
                let remaining = payload.len();
                let need_header = i + 2;
                if need_header > remaining {
                    break;
                }

                let _layer_index = payload[i];
                let name_len = payload[i + 1] as usize;
                i += 2;

                let need_name = i + name_len;
                if need_name > payload.len() {
                    break;
                }

                let name_bytes = &payload[i..i + name_len];
                let name_lossy = String::from_utf8_lossy(name_bytes);
                let name = name_lossy.to_string();
                names.push(name);
                i += name_len;
            }

            let found_names = !names.is_empty();
            if found_names {
                return Ok(names);
            }
        }

        // Legacy fallback: try C> binary protocol
        let legacy_result = self.query_legacy_binary("LAYOUTS?");
        match legacy_result {
            Ok((cmd_type, data)) => {
                let is_layout_type = cmd_type == 4;
                let has_data = !data.is_empty();

                if is_layout_type && has_data {
                    let text = String::from_utf8_lossy(&data);
                    let parts = text.split(';');
                    let non_empty = parts.filter(|s| !s.is_empty());
                    let trimmed_names = non_empty.map(|s| {
                        let long_enough = s.len() > 1;
                        if long_enough {
                            s[1..].to_string()
                        } else {
                            s.to_string()
                        }
                    });
                    let names: Vec<String> = trimmed_names.collect();

                    let found_names = !names.is_empty();
                    if found_names {
                        return Ok(names);
                    }
                }
            }
            Err(_) => {}
        }

        // Last resort: raw text
        self.send_command("LAYOUTS?")?;
        std::thread::sleep(Duration::from_millis(LEGACY_BINARY_SETTLE_MS * 2));
        let raw = self.read_raw(BINARY_READ_TIMEOUT_MS)?;

        let text = String::from_utf8_lossy(&raw);
        let split_by_delimiters = text.split(|c: char| c == ';' || c == '\n');

        let cleaned = split_by_delimiters.map(|s| {
            let step1 = s.trim();
            let step2 = step1.trim_matches(|c: char| c.is_control() || c == '"');
            step2
        });

        let valid_names = cleaned.filter(|s| {
            let is_not_empty = !s.is_empty();
            let is_short_enough = s.len() < 30;
            let no_header_marker = !s.contains("C>");
            let not_ok = *s != "OK";
            is_not_empty && is_short_enough && no_header_marker && not_ok
        });

        let as_strings = valid_names.map(|s| s.to_string());
        let names: Vec<String> = as_strings.collect();

        let found_any = !names.is_empty();
        if found_any {
            Ok(names)
        } else {
            Err("No layer names found".into())
        }
    }

    pub fn get_layout_json(&mut self) -> Result<String, String> {
        // Try v2 binary first
        if self.v2 {
            let resp = self.send_binary(bp::cmd::GET_LAYOUT_JSON, &[])?;
            let has_payload = !resp.payload.is_empty();
            if has_payload {
                let raw_bytes = &resp.payload;
                let lossy_string = String::from_utf8_lossy(raw_bytes);
                let json = lossy_string.to_string();
                return Ok(json);
            }
        }

        // Legacy: text brace-counting
        self.send_command("LAYOUT?")?;
        std::thread::sleep(Duration::from_millis(LEGACY_BINARY_SETTLE_MS));

        let port = self.port.as_mut().ok_or("Not connected")?;
        let mut result = String::new();
        let mut buf = [0u8; 4096];
        let start = Instant::now();
        let max_wait = Duration::from_secs(JSON_TIMEOUT_SECS);
        let mut brace_count: i32 = 0;
        let mut started = false;

        while start.elapsed() < max_wait {
            let read_result = port.read(&mut buf);

            match read_result {
                Ok(n) if n > 0 => {
                    let received_bytes = &buf[..n];
                    let chunk = String::from_utf8_lossy(received_bytes);

                    for ch in chunk.chars() {
                        let is_open_brace = ch == '{';
                        if is_open_brace {
                            started = true;
                            brace_count += 1;
                        }

                        if started {
                            result.push(ch);
                        }

                        let is_close_brace = ch == '}';
                        if is_close_brace && started {
                            brace_count -= 1;
                            let json_complete = brace_count == 0;
                            if json_complete {
                                return Ok(result);
                            }
                        }
                    }
                }
                _ => {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }

        let got_nothing = result.is_empty();
        if got_nothing {
            Err("No JSON".into())
        } else {
            Err("Incomplete JSON".into())
        }
    }
}

/// Thread-safe wrapper
pub type SharedSerial = Arc<Mutex<SerialManager>>;

pub fn new_shared() -> SharedSerial {
    let manager = SerialManager::new();
    let mutex = Mutex::new(manager);
    let shared = Arc::new(mutex);
    shared
}
