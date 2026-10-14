fn main() {
    if let Err(e) = omen_gui::run() {
        eprintln!("omen-hub: {e}");
        std::process::exit(1);
    }
}
