use super::BgResult;

#[cfg(target_arch = "wasm32")]
use eframe::egui;

impl super::KaSeApp {
    // ---- Connection helpers ----

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn is_connected(&self) -> bool {
        // Utilise le cache — PAS de serial.lock() ici !
        // Sinon le thread UI bloque quand un thread background
        // (OTA, query batch) tient le lock.
        self.connected_cache
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn is_connected(&self) -> bool {
        let ser = self.serial.borrow();
        ser.connected
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn connect(&mut self) {
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();
        self.busy = true;
        self.status_msg = "Scanning ports...".into();

        std::thread::spawn(move || {
            let mut ser = match serial.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };

            match ser.auto_connect() {
                Ok(port_name) => {
                    let fw = ser.get_firmware_version().unwrap_or_default();
                    let names = ser.get_layer_names().unwrap_or_default();
                    let km = ser.get_keymap(0).unwrap_or_default();
                    let _ = tx.send(BgResult::Connected(port_name, fw, names, km));

                    // Try to fetch physical layout from firmware
                    let layout = ser.get_layout_json()
                        .ok()
                        .and_then(|json| crate::layout::parse_json(&json).ok());
                    if let Some(keys) = layout {
                        let _ = tx.send(BgResult::LayoutJson(keys));
                    }
                }
                Err(e) => {
                    let _ = tx.send(BgResult::ConnectError(e));
                }
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn connect_web(&mut self, ctx: &egui::Context) {
        self.busy = true;
        self.web_busy.set(true);
        self.status_msg = "Selecting port...".into();
        let serial = self.serial.clone();
        let tx = self.bg_tx.clone();
        let web_busy = self.web_busy.clone();
        let ctx = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            // Step 1: async port selection (no borrow held)
            let conn = match crate::serial::request_port().await {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(BgResult::ConnectError(e));
                    web_busy.set(false);
                    ctx.request_repaint();
                    return;
                }
            };

            // Step 2: extract handles before moving conn
            let reader = conn.reader.clone();
            let writer = conn.writer.clone();
            let v2 = conn.v2;

            // Step 3: store connection
            serial.borrow_mut().apply_connection(conn);

            // Step 4: async queries (no borrow held)
            let fw = crate::serial::get_firmware_version(&reader, &writer, v2)
                .await
                .unwrap_or_default();
            let names = crate::serial::get_layer_names(&reader, &writer, v2)
                .await
                .unwrap_or_default();
            let km = crate::serial::get_keymap(&reader, &writer, 0, v2)
                .await
                .unwrap_or_default();

            let _ = tx.send(BgResult::Connected("WebSerial".into(), fw, names, km));

            // Try to fetch physical layout from firmware
            let layout_json = crate::serial::get_layout_json(&reader, &writer, v2).await;
            if let Ok(json) = layout_json {
                if let Ok(keys) = crate::layout::parse_json(&json) {
                    let _ = tx.send(BgResult::LayoutJson(keys));
                }
            }

            web_busy.set(false);
            ctx.request_repaint();
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn disconnect(&mut self) {
        let mut guard = match self.serial.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.disconnect();
        drop(guard);

        self.connected_cache = false;
        self.firmware_version.clear();
        self.keymap.clear();
        self.layer_names = vec!["Layer 0".into()];
        self.status_msg = "Disconnected".into();
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn disconnect(&mut self) {
        self.serial.borrow_mut().disconnect();

        self.firmware_version.clear();
        self.keymap.clear();
        self.layer_names = vec!["Layer 0".into()];
        self.status_msg = "Disconnected".into();
    }
}
