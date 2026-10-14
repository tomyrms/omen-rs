//! Hardware probe: `cargo run -p omen-kbd --example probe [info|effect|color RRGGBB|off|wave]`.
use omen_kbd::{EffectSettings, Keyboard, Rgb};

fn hex(b: &[u8]) -> String {
    b.iter().take(24).map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    for d in omen_kbd::hid::enumerate("vid_0461") {
        println!("found {:04X}:{:04X} {}", d.vendor_id, d.product_id, d.path);
    }
    let mut kb = match Keyboard::open() {
        Ok(k) => k,
        Err(e) => return println!("open: {e}"),
    };
    println!("opened {} ({})", kb.path(), kb.model_name());
    let arg = std::env::args().nth(1).unwrap_or_else(|| "info".into());
    match arg.as_str() {
        "info" => {
            let r = kb.transact(&omen_kbd::protocol::get_device_info());
            match r {
                Ok(r) => println!("info ack={:?} raw={}", r.ack, r.raw.as_deref().map(hex).unwrap_or_default()),
                Err(e) => println!("info err {e}"),
            }
            println!("parsed {:?}", kb.device_info());
            println!("effect {:?}", kb.read_effect().map(|o| o.map(|v| hex(&v))));
        }
        "color" => {
            let c = Rgb::parse(&std::env::args().nth(2).unwrap_or("ff0000".into())).unwrap();
            println!("set_color -> {:?}", kb.set_color(c, 100));
        }
        "wave" => println!("effect -> {:?}", kb.apply_effect(&EffectSettings::default())),
        "off" => println!("off -> {:?}", kb.turn_off()),
        "step" => {
            use omen_kbd::protocol::*;
            for (n, rep) in [("on", lighting(true)), ("bright", brightness(100)), ("store", store_to_flash())] {
                match kb.transact(&rep) { Ok(r) => println!("{n}: ack={:?} raw={}", r.ack, r.raw.as_deref().map(hex).unwrap_or_default()), Err(e) => println!("{n}: {e}") }
            }
        }
        "bsweep" => {
            for v in [0u8, 1, 2, 3, 4, 50, 100, 101, 150, 199, 200] {
                let mut rep = omen_kbd::protocol::brightness(0);
                rep[5] = v;
                match kb.transact(&rep) { Ok(r) => println!("b={v}: ack={:?} raw={}", r.ack, r.raw.as_deref().map(hex).unwrap_or_default()), Err(e) => println!("b={v}: {e}") }
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
        }
        "trace" => {
            use omen_kbd::protocol::*;
            let c = Rgb::parse(&std::env::args().nth(2).unwrap_or("ff0000".into())).unwrap();
            let (r, g, b) = ([c.r; KEYMAP_LEN], [c.g; KEYMAP_LEN], [c.b; KEYMAP_LEN]);
            let mut seq = vec![("on", lighting(true))];
            for (i, rep) in static_keymaps(&r, &g, &b).into_iter().enumerate() { seq.push((["R0","R1","R2","G0","G1","G2","B0","B1","B2"][i], rep)); }
            seq.push(("store", store_to_flash()));
            for (n, rep) in seq {
                match kb.transact(&rep) { Ok(r) => println!("{n}: ack={:?} raw={}", r.ack, r.raw.as_deref().map(hex).unwrap_or_default()), Err(e) => println!("{n}: {e}") }
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            std::thread::sleep(std::time::Duration::from_millis(800));
            println!("effect {:?}", kb.read_effect().map(|o| o.map(|v| hex(&v))));
        }
        "etrace" => {
            use omen_kbd::protocol::*;
            let st = EffectSettings { effect: Effect::Breathing, colors: vec![Rgb::new(0,0,255)], ..Default::default() };
            for (n, rep) in [("on", lighting(true)), ("effect", effect(&st))] {
                match kb.transact(&rep) { Ok(r) => println!("{n}: ack={:?} raw={}", r.ack, r.raw.as_deref().map(hex).unwrap_or_default()), Err(e) => println!("{n}: {e}") }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            println!("effect {:?}", kb.read_effect().map(|o| o.map(|v| hex(&v))));
            let mut kb2 = kb;
            let st2 = EffectSettings { effect: Effect::Wave, ..Default::default() };
            match kb2.transact(&effect(&st2)) { Ok(r) => println!("wave: ack={:?}", r.ack), Err(e) => println!("wave {e}") }
            std::thread::sleep(std::time::Duration::from_millis(500));
            println!("effect {:?}", kb2.read_effect().map(|o| o.map(|v| hex(&v))));
            let r = kb2.transact(&store_to_flash());
            println!("store {:?}", r.map(|r| r.ack));
            std::thread::sleep(std::time::Duration::from_millis(1500));
            println!("late {:?}", kb2.read_effect().map(|o| o.map(|v| hex(&v))));
        }
        _ => println!("unknown"),
    }
}
