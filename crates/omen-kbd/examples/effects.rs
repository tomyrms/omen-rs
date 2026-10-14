//! Tries every effect with theme / 1 / 2 colors and each speed; prints the MCU answer.
use omen_kbd::protocol::*;
use omen_kbd::{EffectSettings, Keyboard, Rgb};

fn main() {
    let mut kb = Keyboard::open().expect("keyboard");
    let variants: Vec<(&str, Vec<Rgb>)> = vec![
        ("theme", vec![]),
        ("1col", vec![Rgb::new(255, 0, 0)]),
        ("2col", vec![Rgb::new(255, 0, 0), Rgb::new(0, 0, 255)]),
        ("4col", vec![Rgb::new(255, 0, 0), Rgb::new(0, 0, 255), Rgb::new(0, 255, 0), Rgb::new(255, 255, 0)]),
    ];
    for e in Effect::ALL {
        let mut line = format!("{:<11}", format!("{e:?}"));
        for (vn, cols) in &variants {
            for speed in [0u8, 1, 2] {
                let st = EffectSettings { effect: e, speed, colors: cols.clone(), ..Default::default() };
                let r = kb.transact(&effect(&st));
                let tag = match r {
                    Ok(r) => match r.ack { Some(Ack::Accepted) => "ok".to_string(), Some(Ack::Rejected) => format!("NO{:02x}", r.raw.as_ref().map(|v| v[7]).unwrap_or(0)), _ => "?".into() },
                    Err(_) => "err".into(),
                };
                line += &format!(" {vn}/{speed}:{tag}");
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        }
        println!("{line}");
    }
    let st = EffectSettings::default();
    println!("restore wave: {:?}", kb.apply_effect(&st));
}
