//! OMEN control logic shared by the GUI and the CLI: user modes, the
//! automatic mode, system sensors, game detection, settings and autostart.

pub mod auto;
pub mod autostart;
pub mod config;
pub mod controller;
pub mod games;
pub mod mode;
pub mod sensors;

pub use config::{log_line, AutoSettings, Config, KeyboardLook, Sensitivity};
pub use controller::{Command, Controller, HistoryPoint, KeyboardCommand, Snapshot};
pub use mode::{Level, Mode};
