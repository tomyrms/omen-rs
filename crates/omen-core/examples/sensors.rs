//! Prints the driver-free sensors and game detection a few times.
fn main() {
    let mut pdh = omen_core::sensors::PdhSensors::new().expect("pdh");
    let nvml = omen_core::sensors::Nvml::load();
    println!("nvml: {:?}", nvml.as_ref().map(|n| n.name.clone()));
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("pdh {:?} gpu {:?} power {:?}", pdh.sample(), nvml.as_ref().map(|n| n.sample()), omen_core::sensors::power_state());
    }
    for p in omen_core::games::processes() { if omen_core::games::is_game(&p, &[], &[]) { println!("  game-like: {} {:?}", p.name, p.path); } }
    println!("game: {:?}", omen_core::games::detect(&[], &[]));
    println!("autostart enabled: {}", omen_core::autostart::is_enabled());
}
