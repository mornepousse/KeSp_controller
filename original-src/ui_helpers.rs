use super::{BgResult, Instant};

impl super::KaSeApp {
    #[allow(dead_code)]
    pub(super) fn notify(&mut self, msg: &str) {
        let timestamp = Instant::now();
        let entry = (msg.to_string(), timestamp);
        self.notifications.push(entry);
    }

    pub(super) fn get_heatmap_intensity(&self, row: usize, col: usize) -> f32 {
        if !self.heatmap_on || self.heatmap_max == 0 {
            return 0.0;
        }

        let row_data = self.heatmap_data.get(row);
        let cell_option = row_data.and_then(|r| r.get(col));
        let count = cell_option.copied().unwrap_or(0);

        let count_float = count as f32;
        let max_float = self.heatmap_max as f32;
        let intensity = count_float / max_float;
        intensity
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load_heatmap(&mut self) {
        self.busy = true;
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();

        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|p| p.into_inner());
            let lines = ser.query_command("KEYSTATS?").unwrap_or_default();
            let (data, max) = crate::parsers::parse_heatmap_lines(&lines);
            let _ = tx.send(BgResult::HeatmapData(data, max));
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn load_heatmap(&mut self) {
        if self.web_busy.get() { return; }
        self.busy = true;
        self.web_busy.set(true);
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();
        let web_busy = self.web_busy.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let handles = serial.borrow().io_handles();
            if let Ok((reader, writer)) = handles {
                let lines = crate::serial::query_command(&reader, &writer, "KEYSTATS?")
                    .await
                    .unwrap_or_default();
                let (data, max) = crate::parsers::parse_heatmap_lines(&lines);
                let _ = tx.send(BgResult::HeatmapData(data, max));
            }
            web_busy.set(false);
        });
    }


    /// Apply a key selection from the key selector.
    /// row == 951: new KO result key
    /// row == 950: new KO trigger key
    /// row 900-949: KO edit (900 + idx*10 + 0=trig, +1=result)
    /// row >= 800: new leader result
    /// row >= 700: new leader sequence key (append)
    /// row >= 600: leader result edit (row-600 = leader index)
    /// row >= 500: leader sequence key edit (row = 500 + idx*10 + seq_pos)
    /// row >= 400: new combo result mode
    /// row >= 300: combo result edit (row-300 = combo index)
    /// row >= 200: macro step mode (add key as step)
    /// row >= 100: TD mode (row-100 = td index, col = action slot)
    /// row < 100: keymap mode
    pub(super) fn apply_key_selection(&mut self, row: usize, col: usize, code: u16) {
        if row == 951 {
            // New KO result key
            self.ko_new_res_key = code as u8;
            self.ko_new_res_set = true;
            self.status_msg = format!("KO result = {}", crate::keycode::hid_key_name(code as u8));
            return;
        }

        if row == 950 {
            // New KO trigger key
            self.ko_new_trig_key = code as u8;
            self.ko_new_trig_set = true;
            self.status_msg = format!("KO trigger = {}", crate::keycode::hid_key_name(code as u8));
            return;
        }

        if row >= 900 {
            // KO edit: row = 900 + idx*10 + field (0=trig, 1=result)
            let offset = row - 900;
            let ko_idx = offset / 10;
            let field = offset % 10;
            let idx_valid = ko_idx < self.ko_data.len();
            if idx_valid {
                if field == 0 {
                    self.ko_data[ko_idx][0] = code as u8;
                    self.status_msg = format!("KO #{} trigger = 0x{:02X}", ko_idx, code);
                } else {
                    self.ko_data[ko_idx][2] = code as u8;
                    self.status_msg = format!("KO #{} result = 0x{:02X}", ko_idx, code);
                }
            }
            return;
        }

        if row >= 800 {
            // New leader result
            self.leader_new_result = code as u8;
            self.leader_new_result_set = true;
            self.status_msg = format!("Leader result = {}", crate::keycode::hid_key_name(code as u8));
            return;
        }

        if row >= 700 {
            // New leader sequence key (append)
            let seq_not_full = self.leader_new_seq.len() < 4;
            if seq_not_full {
                self.leader_new_seq.push(code as u8);
                let key_name = crate::keycode::hid_key_name(code as u8);
                self.status_msg = format!("Leader seq + {}", key_name);
            }
            return;
        }

        if row >= 600 {
            // Leader result edit (existing)
            let leader_idx = row - 600;
            let idx_valid = leader_idx < self.leader_data.len();
            if idx_valid {
                self.leader_data[leader_idx].result = code as u8;
                self.status_msg = format!("Leader #{} result = 0x{:02X}", leader_idx, code);
            }
            return;
        }

        if row >= 500 {
            // Leader sequence key edit (existing)
            // row = 500 + leader_idx*10 + seq_pos
            let offset = row - 500;
            let leader_idx = offset / 10;
            let seq_pos = offset % 10;
            let idx_valid = leader_idx < self.leader_data.len();
            if idx_valid {
                let seq_valid = seq_pos < self.leader_data[leader_idx].sequence.len();
                if seq_valid {
                    self.leader_data[leader_idx].sequence[seq_pos] = code as u8;
                    self.status_msg = format!("Leader #{} key {} = 0x{:02X}", leader_idx, seq_pos, code);
                }
            }
            return;
        }

        if row >= 400 {
            // New combo result mode
            self.combo_new_result = code;
            self.status_msg = format!("New combo result = 0x{:04X}", code);
            return;
        }

        if row >= 300 {
            // Combo result edit mode
            let combo_idx = row - 300;
            let idx_valid = combo_idx < self.combo_data.len();
            if idx_valid {
                self.combo_data[combo_idx].result = code;
                self.status_msg = format!("Combo #{} result = 0x{:04X}", combo_idx, code);
            }
            return;
        }

        if row >= 200 {
            // Macro step mode
            self.apply_macro_step(code);
            return;
        }

        if row >= 100 {
            // TD mode
            let td_idx = row - 100;
            let idx_valid = td_idx < self.td_data.len();
            let col_valid = col < 4;

            if idx_valid && col_valid {
                self.td_data[td_idx][col] = code;
                self.status_msg = format!("TD {} action {} = 0x{:04X}", td_idx, col, code);
            }
        } else {
            // Keymap mode - validate bounds BEFORE sending
            let row_valid = row < self.keymap.len();
            let col_valid = row_valid && col < self.keymap[row].len();

            if col_valid {
                let layer = self.current_layer as u8;
                let row_byte = row as u8;
                let col_byte = col as u8;
                let cmd = crate::protocol::cmd_set_key(layer, row_byte, col_byte, code);
                self.bg_send(&cmd);

                self.keymap[row][col] = code;
                self.status_msg = format!("[{},{}] = 0x{:04X}", row, col, code);
            } else {
                self.status_msg = format!("Invalid key position [{},{}]", row, col);
            }
        }
    }

    pub(super) fn get_key(&self, row: usize, col: usize) -> u16 {
        let row_data = self.keymap.get(row);
        let cell_option = row_data.and_then(|r| r.get(col));
        let value = cell_option.copied();
        value.unwrap_or(0)
    }
}
