mod protocol;
mod context;
mod models;
mod config;
mod dispatch;
mod keymap;
mod macros;
mod advanced;
mod stats;
mod settings;
mod flasher;
mod layout;
mod connection;
mod key_selector;
mod tools;

slint::include_modules!();

use context::{AppContext, BgMsg};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::rc::Rc;

fn main() {
    let keys = protocol::layout::default_layout();

    let window = MainWindow::new().unwrap();

    let saved_settings = protocol::settings::load();
    models::init_models(&window, &keys, &saved_settings);

    let (bg_tx, bg_rx) = mpsc::channel::<BgMsg>();

    let ctx = AppContext {
        serial: Arc::new(Mutex::new(protocol::serial::SerialManager::new())),
        bg_tx,
        keys: Rc::new(std::cell::RefCell::new(keys)),
        current_keymap: Rc::new(std::cell::RefCell::new(Vec::new())),
        current_layer: Rc::new(std::cell::Cell::new(0)),
        keyboard_layout: Rc::new(std::cell::RefCell::new(
            protocol::layout_remap::KeyboardLayout::from_name(&saved_settings.keyboard_layout),
        )),
        heatmap_data: Rc::new(std::cell::RefCell::new(Vec::new())),
        macro_steps: Rc::new(std::cell::RefCell::new(Vec::new())),
    };

    connection::auto_connect(&window, &ctx);
    connection::setup(&window, &ctx);
    keymap::setup(&window, &ctx);
    stats::setup(&window, &ctx);
    macros::setup(&window, &ctx);
    advanced::setup(&window, &ctx);
    key_selector::setup(&window, &ctx);
    settings::setup(&window, &ctx);
    flasher::setup(&window, &ctx);
    layout::setup(&window, &ctx);
    tools::setup(&window, &ctx);

    dispatch::run(&window, &ctx, bg_rx);
}
