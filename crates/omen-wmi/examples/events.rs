//! Prints BIOS hotkey events (press the OMEN key): `cargo run -p omen-wmi --example events [seconds]`.
fn main() {
    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let ev = omen_wmi::BiosEvents::subscribe().expect("subscribe");
    println!("subscribed, waiting {secs} s");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < end {
        match ev.next(500) {
            Ok(Some(e)) => println!("event {e:?} omen_key={}", e.is_omen_key()),
            Ok(None) => {}
            Err(e) => { println!("error {e}"); break; }
        }
    }
}
