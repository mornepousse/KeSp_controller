mod app;
#[cfg(not(target_arch = "wasm32"))]
mod backend;
#[cfg(target_arch = "wasm32")]
#[path = "backend_web.rs"]
mod backend;
mod bg;
mod boards;
mod checks;
mod demo;
mod editor;
mod keys;
mod practice;
mod practice_game;
mod protocol;
mod shape;
mod theme;
mod ui_behaviors;
mod ui_common;
mod ui_device;
mod ui_keymap;
mod ui_layout;
mod ui_selector;
mod ui_typing;
mod ui_practice;
#[cfg(test)]
mod ui_tests;

use app::{App, Options};
#[cfg(not(target_arch = "wasm32"))]
use app::Tab;

/// The browser's way in. The page's script calls this once it has loaded.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn web_start() {
    use wasm_bindgen::JsCast;
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().and_then(|w| w.document()).expect("a page");
        let canvas = document
            .get_element_by_id("kesp")
            .expect("the page's canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("a canvas");
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|_cc| Ok(Box::new(App::new(Options::default())))),
            )
            .await;
        if let Err(e) = started {
            web_sys::console::error_1(&e);
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn parse_args() -> Result<Options, String> {
    let mut opt = Options::default();
    let mut args = std::env::args().skip(1);
    let mut force_connect = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => {
                let path = args.next().ok_or("--screenshot needs a file")?;
                opt.screenshot = Some(path.into());
                // A check must never talk to a keyboard that happens to be plugged in.
                opt.auto_connect = false;
            }
            "--no-connect" => opt.auto_connect = false,
            // Connect even for a screenshot, to look at a real keyboard on purpose.
            "--connect" => force_connect = true,
            // Sample data, nothing reaches a keyboard: for trying the app without one.
            "--demo" => opt.demo = true,
            "--selector" => opt.selector = true,
            "--check" => opt.check = true,
            "--tab" => {
                let name = args.next().ok_or("--tab needs a name")?;
                opt.tab = Tab::from_name(&name).ok_or(format!("unknown tab {name}"))?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    opt.auto_connect |= force_connect;
    Ok(opt)
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    let opt = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("KeSp Controller")
            .with_app_id("kesp-controller")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([600.0, 450.0]),
        ..Default::default()
    };
    let failed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = failed.clone();
    let run = eframe::run_native(
        "KeSp Controller",
        native,
        Box::new(move |_cc| Ok(Box::new(Guard { app: App::new(opt), failed: flag }))),
    );
    if let Err(e) = run {
        eprintln!("window error: {e}");
        return std::process::ExitCode::FAILURE;
    }
    if failed.load(std::sync::atomic::Ordering::SeqCst) {
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

/// Passes a failed screenshot on to the exit code.
#[cfg(not(target_arch = "wasm32"))]
struct Guard {
    app: App,
    failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl eframe::App for Guard {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.frame(ui);
        if self.app.screenshot_failed() {
            self.failed.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}
