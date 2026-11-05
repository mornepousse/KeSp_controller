#[allow(dead_code)]
pub mod binary;
pub mod config_io;
#[allow(dead_code)]
pub mod connection;
pub mod device;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub mod flasher;
#[allow(dead_code)]
pub mod keycode;
pub mod layout;
#[allow(dead_code)]
pub mod layout_remap;
#[allow(dead_code)]
pub mod parsers;
#[allow(dead_code)]
pub mod text_commands;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub mod serial;
#[allow(dead_code)]
pub mod settings;
#[allow(dead_code)]
pub mod stats;
