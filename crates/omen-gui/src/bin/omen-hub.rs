#![windows_subsystem = "windows"]

fn main() {
    if let Err(e) = omen_gui::run() {
        omen_gui::log_line(&format!("omen-hub: fatal: {e}"));
        std::process::exit(1);
    }
}
