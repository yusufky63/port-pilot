// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Console-helper mode: attach to another console, deliver CTRL+C there and exit. The stop
    // flow starts this executable that way; no window opens.
    if let Some(code) = portpilot_engine::helper_mode(&args) {
        std::process::exit(code);
    }
    portpilot_app::run();
}
