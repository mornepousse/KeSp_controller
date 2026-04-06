use super::BgResult;

/// Map a query tag to its binary command ID.
#[cfg(target_arch = "wasm32")]
use crate::binary_protocol as bp;
#[cfg(target_arch = "wasm32")]
fn tag_to_binary_cmd(tag: &str) -> Option<u8> {
    match tag {
        "td" => Some(bp::cmd::TD_LIST),
        "combo" => Some(bp::cmd::COMBO_LIST),
        "leader" => Some(bp::cmd::LEADER_LIST),
        "ko" => Some(bp::cmd::KO_LIST),
        "bt" => Some(bp::cmd::BT_QUERY),
        "tama" => Some(bp::cmd::TAMA_QUERY),
        "wpm" => Some(bp::cmd::WPM_QUERY),
        "macros" => Some(bp::cmd::LIST_MACROS),
        "keystats" => Some(bp::cmd::KEYSTATS_BIN),
        "features" => Some(bp::cmd::FEATURES),
        _ => None,
    }
}

impl super::KaSeApp {
    // ---- Background helpers ----

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn bg_query(&mut self, tag: &str, cmd: &str) {
        self.busy = true;
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();
        let tag = tag.to_string();
        let cmd = cmd.to_string();

        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|p| p.into_inner());
            let lines = ser.query_command(&cmd).unwrap_or_default();
            let _ = tx.send(BgResult::TextLines(tag, lines));
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn bg_query(&mut self, tag: &str, cmd: &str) {
        if self.web_busy.get() { return; }
        self.busy = true;
        self.web_busy.set(true);
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();
        let web_busy = self.web_busy.clone();
        let tag = tag.to_string();
        let cmd = cmd.to_string();

        wasm_bindgen_futures::spawn_local(async move {
            let v2 = serial.borrow().v2;
            let handles = serial.borrow().io_handles();
            if let Ok((reader, writer)) = handles {
                if let Some(cmd_id) = v2.then(|| tag_to_binary_cmd(&tag)).flatten() {
                    match crate::serial::send_binary(&reader, &writer, cmd_id, &[]).await {
                        Ok(resp) => {
                            let _ = tx.send(BgResult::BinaryPayload(tag, resp.payload));
                        }
                        Err(_) => {}
                    }
                } else {
                    let result = crate::serial::query_command(&reader, &writer, &cmd).await;
                    match result {
                        Ok(lines) => {
                            let _ = tx.send(BgResult::TextLines(tag, lines));
                        }
                        Err(e) => {
                            if e == "timeout_refresh" {
                                serial.borrow_mut().refresh_reader();
                            }
                        }
                    }
                }
            }
            web_busy.set(false);
        });
    }

    /// Run multiple queries sequentially (avoids mutex contention).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn bg_query_batch(&mut self, queries: &[(&str, &str)]) {
        self.busy = true;
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();

        let owned_queries: Vec<(String, String)> = queries
            .iter()
            .map(|(t, c)| (t.to_string(), c.to_string()))
            .collect();

        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|p| p.into_inner());

            for (tag, cmd) in owned_queries {
                let lines = ser.query_command(&cmd).unwrap_or_default();
                let _ = tx.send(BgResult::TextLines(tag, lines));
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn bg_query_batch(&mut self, queries: &[(&str, &str)]) {
        if self.web_busy.get() { return; }
        self.busy = true;
        self.web_busy.set(true);
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();
        let web_busy = self.web_busy.clone();

        let owned: Vec<(String, String)> = queries
            .iter()
            .map(|(t, c)| (t.to_string(), c.to_string()))
            .collect();

        wasm_bindgen_futures::spawn_local(async move {
            let v2 = serial.borrow().v2;

            for (tag, text_cmd) in &owned {
                let handles = serial.borrow().io_handles();
                let (reader, writer) = match handles {
                    Ok(h) => h,
                    Err(_) => break,
                };

                if let Some(cmd_id) = v2.then(|| tag_to_binary_cmd(tag)).flatten() {
                    match crate::serial::send_binary(&reader, &writer, cmd_id, &[]).await {
                        Ok(resp) => {
                            let _ = tx.send(BgResult::BinaryPayload(tag.clone(), resp.payload));
                        }
                        Err(_) => {}
                    }
                } else {
                    match crate::serial::query_command(&reader, &writer, text_cmd).await {
                        Ok(lines) => {
                            let _ = tx.send(BgResult::TextLines(tag.clone(), lines));
                        }
                        Err(e) => {
                            if e == "timeout_refresh" {
                                serial.borrow_mut().refresh_reader();
                            }
                        }
                    }
                }
            }
            web_busy.set(false);
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn bg_send(&self, cmd: &str) {
        let serial = self.serial.clone();
        let cmd = cmd.to_string();

        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|p| p.into_inner());
            let _ = ser.send_command(&cmd);
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn bg_send(&self, cmd: &str) {
        let serial = self.serial.clone();
        let cmd = cmd.to_string();

        wasm_bindgen_futures::spawn_local(async move {
            let handles = serial.borrow().io_handles();
            if let Ok((_reader, writer)) = handles {
                let _ = crate::serial::send_command(&writer, &cmd).await;
            }
        });
    }

    // ---- Data helpers ----

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load_keymap(&mut self) {
        self.busy = true;
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();
        let layer = self.current_layer as u8;

        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|p| p.into_inner());

            match ser.get_keymap(layer) {
                Ok(km) => { let _ = tx.send(BgResult::Keymap(km)); }
                Err(e) => { let _ = tx.send(BgResult::Error(e)); }
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn load_keymap(&mut self) {
        if self.web_busy.get() { return; }
        self.busy = true;
        self.web_busy.set(true);
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();
        let web_busy = self.web_busy.clone();
        let layer = self.current_layer as u8;

        wasm_bindgen_futures::spawn_local(async move {
            let ser = serial.borrow();
            let v2 = ser.v2;
            let handles = ser.io_handles();
            drop(ser);
            match handles {
                Ok((reader, writer)) => {
                    match crate::serial::get_keymap(&reader, &writer, layer, v2).await {
                        Ok(km) => { let _ = tx.send(BgResult::Keymap(km)); }
                        Err(e) => { let _ = tx.send(BgResult::Error(e)); }
                    }
                }
                Err(e) => { let _ = tx.send(BgResult::Error(e)); }
            }
            web_busy.set(false);
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load_layer_names(&self) {
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();

        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|p| p.into_inner());

            match ser.get_layer_names() {
                Ok(names) if !names.is_empty() => {
                    let _ = tx.send(BgResult::LayerNames(names));
                }
                _ => {}
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn load_layer_names(&self) {
        if self.web_busy.get() { return; }
        self.web_busy.set(true);
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();
        let web_busy = self.web_busy.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let ser = serial.borrow();
            let v2 = ser.v2;
            let handles = ser.io_handles();
            drop(ser);
            if let Ok((reader, writer)) = handles {
                match crate::serial::get_layer_names(&reader, &writer, v2).await {
                    Ok(names) if !names.is_empty() => {
                        let _ = tx.send(BgResult::LayerNames(names));
                    }
                    _ => {}
                }
            }
            web_busy.set(false);
        });
    }
}
