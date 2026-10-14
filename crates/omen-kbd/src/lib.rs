//! OMEN laptop RGB keyboard control through direct HID.
//!
//! The per-key RGB keyboard of 2023+ OMEN laptops is driven by a Primax MCU
//! (USB VID `0x0461`, control collection `MI_02`). The vendor application
//! talks to it with the MCU "general" command set; this crate implements the
//! same wire protocol directly (see `protocol.rs` and
//! `docs/omen-hub/03-clavier.md`). No vendor service or driver is involved:
//! the interface is a standard HID collection.
//!
//! The BIOS WMI keyboard registers (`0x20009`) do not drive these LEDs on
//! per-key RGB models; this HID path is the one that lights the keys.

pub mod hid;
pub mod protocol;

use std::thread::sleep;
use std::time::Duration;

pub use protocol::{Ack, DeviceInfo, Direction, Effect, EffectSettings, Rgb, Theme, KEYMAP_LEN};

/// Primax keyboard MCU vendor id.
pub const VENDOR_ID: u16 = 0x0461;

/// Known OMEN laptop keyboard MCU product ids (vendor device table).
pub const KNOWN_PRODUCTS: &[(u16, &str)] = &[
    (0x4E9A, "OMEN 2023 (Cybug)"),
    (0x4E9B, "OMEN 2023 (Ralph)"),
    (0x4F03, "OMEN (Hendricks)"),
    (0x4F11, "OMEN (Brunobear/Quaker)"),
    (0x4F1E, "OMEN (Brunobear/Quaker)"),
    (0x4ECB, "OMEN (Starmade)"),
];

#[derive(Debug)]
pub enum KbdError {
    NotFound,
    Io(windows::core::Error),
    /// The MCU answered `EC FA` (command refused).
    Rejected(&'static str),
}

impl std::fmt::Display for KbdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KbdError::NotFound => write!(f, "clavier RGB introuvable (HID VID 0461, interface MI_02)"),
            KbdError::Io(e) => write!(f, "erreur HID : {e}"),
            KbdError::Rejected(what) => write!(f, "le clavier a refusé la commande « {what} »"),
        }
    }
}

impl std::error::Error for KbdError {}

impl From<windows::core::Error> for KbdError {
    fn from(e: windows::core::Error) -> Self {
        KbdError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, KbdError>;

/// Result of one command: the MCU answer, if it sent one in time.
#[derive(Debug, Clone)]
pub struct Reply {
    pub ack: Option<Ack>,
    pub raw: Option<Vec<u8>>,
}

/// An open keyboard MCU.
pub struct Keyboard {
    dev: hid::HidDevice,
    pub product_id: u16,
}

const STEP_DELAY: Duration = Duration::from_millis(30);
const REPLY_TIMEOUT: Duration = Duration::from_millis(250);

impl Keyboard {
    /// Finds and opens the keyboard control interface.
    pub fn open() -> Result<Self> {
        let candidates = hid::enumerate("vid_0461");
        let pick = candidates
            .iter()
            .filter(|d| d.vendor_id == VENDOR_ID && d.path.to_ascii_lowercase().contains("mi_02"))
            .min_by_key(|d| {
                // Prefer known products, then the first one found.
                KNOWN_PRODUCTS
                    .iter()
                    .position(|(pid, _)| *pid == d.product_id)
                    .unwrap_or(usize::MAX)
            })
            .ok_or(KbdError::NotFound)?;
        let dev = hid::HidDevice::open(&pick.path)?;
        Ok(Self {
            dev,
            product_id: pick.product_id,
        })
    }

    pub fn path(&self) -> &str {
        &self.dev.path
    }

    pub fn model_name(&self) -> &'static str {
        KNOWN_PRODUCTS
            .iter()
            .find(|(pid, _)| *pid == self.product_id)
            .map(|(_, n)| *n)
            .unwrap_or("clavier OMEN")
    }

    /// Sends one report and waits for the matching answer (same command and
    /// parameter bytes). Unrelated input reports (key events) are skipped.
    pub fn transact(&mut self, report: &protocol::Report) -> Result<Reply> {
        self.dev.write(report)?;
        let deadline = std::time::Instant::now() + REPLY_TIMEOUT;
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            match self.dev.read(left)? {
                Some(resp) if resp.len() > 2 && resp[1] == report[1] && resp[2] == report[2] => {
                    return Ok(Reply {
                        ack: Some(protocol::classify(&resp)),
                        raw: Some(resp),
                    });
                }
                Some(_) => continue,
                None => break,
            }
        }
        Ok(Reply { ack: None, raw: None })
    }

    fn command(&mut self, report: &protocol::Report, what: &'static str) -> Result<Reply> {
        let reply = self.transact(report)?;
        if reply.ack == Some(Ack::Rejected) {
            return Err(KbdError::Rejected(what));
        }
        sleep(STEP_DELAY);
        Ok(reply)
    }

    pub fn device_info(&mut self) -> Result<Option<DeviceInfo>> {
        self.dev.drain();
        let reply = self.transact(&protocol::get_device_info())?;
        Ok(reply.raw.as_deref().and_then(DeviceInfo::parse))
    }

    pub fn set_on(&mut self, on: bool) -> Result<()> {
        self.command(&protocol::lighting(on), "allumage")?;
        Ok(())
    }

    /// Brightness in percent (0..=100).
    pub fn set_brightness(&mut self, percent: u8) -> Result<()> {
        self.command(&protocol::brightness(percent), "luminosité")?;
        Ok(())
    }

    /// Lights every key with one color.
    pub fn set_color(&mut self, color: Rgb, brightness: u8) -> Result<()> {
        self.set_per_key(&[color; KEYMAP_LEN], brightness)
    }

    /// Per-LED colors (LED index order of the MCU).
    pub fn set_per_key(&mut self, colors: &[Rgb; KEYMAP_LEN], brightness: u8) -> Result<()> {
        self.dev.drain();
        let (mut r, mut g, mut b) = ([0u8; KEYMAP_LEN], [0u8; KEYMAP_LEN], [0u8; KEYMAP_LEN]);
        for (i, c) in colors.iter().enumerate() {
            r[i] = c.r;
            g[i] = c.g;
            b[i] = c.b;
        }
        self.command(&protocol::lighting(true), "allumage")?;
        self.set_brightness_lenient(brightness);
        if self.upload_keymaps(&r, &g, &b).is_err() {
            // Seen live: after an interrupted upload the MCU refuses key maps
            // (status 02) until an effect resets its state machine.
            sleep(STEP_DELAY);
            let _ = self.transact(&protocol::effect(&EffectSettings::default()));
            sleep(STEP_DELAY);
            self.upload_keymaps(&r, &g, &b)?;
        }
        self.command(&protocol::store_to_flash(), "sauvegarde")?;
        Ok(())
    }

    fn upload_keymaps(&mut self, r: &[u8; KEYMAP_LEN], g: &[u8; KEYMAP_LEN], b: &[u8; KEYMAP_LEN]) -> Result<()> {
        for rep in protocol::static_keymaps(r, g, b) {
            self.command(&rep, "couleurs")?;
        }
        Ok(())
    }

    /// Starts an animated effect.
    pub fn apply_effect(&mut self, settings: &EffectSettings) -> Result<()> {
        if settings.effect == Effect::Steady {
            let c = settings.colors.first().copied().unwrap_or(Rgb::new(255, 0, 0));
            return self.set_color(c, settings.brightness);
        }
        if !settings.effect.supported() {
            return Err(KbdError::Rejected(settings.effect.label()));
        }
        self.dev.drain();
        self.command(&protocol::lighting(true), "allumage")?;
        self.set_brightness_lenient(settings.brightness);
        self.command(&protocol::effect(settings), "effet")?;
        self.command(&protocol::store_to_flash(), "sauvegarde")?;
        Ok(())
    }

    /// Turns the backlight off and remembers it across reboots.
    pub fn turn_off(&mut self) -> Result<()> {
        self.dev.drain();
        self.command(&protocol::lighting(false), "extinction")?;
        self.command(&protocol::store_to_flash(), "sauvegarde")?;
        Ok(())
    }

    /// Restores the factory lighting.
    pub fn restore_default(&mut self) -> Result<()> {
        self.dev.drain();
        self.command(&protocol::restore_lighting_default(), "réglages d'usine")?;
        Ok(())
    }

    /// Raw effect readback (`0x83`) for diagnostics.
    pub fn read_effect(&mut self) -> Result<Option<Vec<u8>>> {
        self.dev.drain();
        Ok(self.transact(&protocol::get_effect())?.raw)
    }

    /// Brightness is best-effort inside multi-step sequences: some firmware
    /// revisions refuse values while an effect is starting, and the color
    /// itself must still be applied.
    fn set_brightness_lenient(&mut self, percent: u8) {
        let _ = self.transact(&protocol::brightness(percent.max(1)));
        sleep(STEP_DELAY);
    }
}
