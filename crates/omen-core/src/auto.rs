//! Automatic mode: picks a hardware level from load, heat and running games.
//!
//! Same spirit as the vendor application (games on AC power switch to
//! Performance; its software fan control smooths temperatures with an EWMA,
//! fast upward / slow downward), reimplemented as a small state machine:
//!
//! - **too hot** (GPU temperature or BIOS thermal throttling) -> `Max`
//!   immediately, held until clearly cooled down;
//! - **game running** -> `Performance`;
//! - **heavy CPU/GPU load** (smoothed) -> `Performance`;
//! - **idle** for a while -> `Silent`;
//! - otherwise `Balanced`.
//!
//! Going up waits a few seconds (ignores short spikes); going down waits
//! longer, so fans never "yo-yo".

use crate::config::{AutoSettings, Sensitivity};
use crate::mode::Level;

/// One sample of the machine state.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// Total CPU load in percent.
    pub cpu_load: f32,
    /// Highest GPU engine load in percent.
    pub gpu_load: Option<f32>,
    /// Discrete GPU temperature (°C).
    pub gpu_temp: Option<f32>,
    /// CPU-side temperature when available (°C).
    pub cpu_temp: Option<f32>,
    /// BIOS reports thermal throttling.
    pub throttling: bool,
    pub on_ac: bool,
    /// Name of the detected game, if any.
    pub game: Option<String>,
}

struct Thresholds {
    heavy_cpu: f32,
    heavy_gpu: f32,
    idle_cpu: f32,
    idle_gpu: f32,
    hot_gpu: f32,
    hot_cpu: f32,
    cool_margin: f32,
    up_delay: f64,
    down_delay: f64,
    idle_delay: f64,
}

fn thresholds(s: Sensitivity) -> Thresholds {
    match s {
        Sensitivity::Calm => Thresholds {
            heavy_cpu: 75.0,
            heavy_gpu: 60.0,
            idle_cpu: 10.0,
            idle_gpu: 8.0,
            hot_gpu: 87.0,
            hot_cpu: 95.0,
            cool_margin: 8.0,
            up_delay: 8.0,
            down_delay: 25.0,
            idle_delay: 45.0,
        },
        Sensitivity::Normal => Thresholds {
            heavy_cpu: 60.0,
            heavy_gpu: 45.0,
            idle_cpu: 15.0,
            idle_gpu: 10.0,
            hot_gpu: 84.0,
            hot_cpu: 92.0,
            cool_margin: 8.0,
            up_delay: 4.0,
            down_delay: 40.0,
            idle_delay: 60.0,
        },
        Sensitivity::Reactive => Thresholds {
            heavy_cpu: 45.0,
            heavy_gpu: 30.0,
            idle_cpu: 20.0,
            idle_gpu: 12.0,
            hot_gpu: 80.0,
            hot_cpu: 88.0,
            cool_margin: 7.0,
            up_delay: 2.0,
            down_delay: 60.0,
            idle_delay: 90.0,
        },
    }
}

/// Decision of the automatic mode.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub level: Level,
    /// Human explanation shown in the interface.
    pub reason: String,
}

pub struct AutoEngine {
    level: Level,
    reason: String,
    ewma_cpu: Option<f32>,
    ewma_gpu: Option<f32>,
    hot: bool,
    /// Since when the wanted level has been above the current one.
    up_since: Option<f64>,
    /// Since when the wanted level has been below the current one.
    down_since: Option<f64>,
    idle_since: Option<f64>,
}

impl Default for AutoEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn ewma(prev: Option<f32>, value: f32) -> f32 {
    match prev {
        // Fast attack, slow release.
        Some(p) if value > p => p + 0.5 * (value - p),
        Some(p) => p + 0.15 * (value - p),
        None => value,
    }
}

impl AutoEngine {
    pub fn new() -> Self {
        Self {
            level: Level::Balanced,
            reason: "démarrage".into(),
            ewma_cpu: None,
            ewma_gpu: None,
            hot: false,
            up_since: None,
            down_since: None,
            idle_since: None,
        }
    }

    pub fn level(&self) -> Level {
        self.level
    }

    /// Feeds one sample taken at `now` (seconds, monotonic) and returns the
    /// level to apply.
    pub fn update(&mut self, now: f64, input: &Inputs, settings: &AutoSettings) -> Decision {
        let t = thresholds(settings.sensitivity);
        let cpu = ewma(self.ewma_cpu, input.cpu_load);
        self.ewma_cpu = Some(cpu);
        let gpu = input.gpu_load.map(|g| ewma(self.ewma_gpu, g));
        self.ewma_gpu = gpu;
        let gpu = gpu.unwrap_or(0.0);

        // Heat, with hysteresis so Max is not left at the first degree.
        let gpu_temp = input.gpu_temp.unwrap_or(0.0);
        let cpu_temp = input.cpu_temp.unwrap_or(0.0);
        let too_hot = gpu_temp >= t.hot_gpu || cpu_temp >= t.hot_cpu || input.throttling;
        let cooled = gpu_temp < t.hot_gpu - t.cool_margin
            && cpu_temp < t.hot_cpu - t.cool_margin
            && !input.throttling;
        if too_hot {
            self.hot = true;
        } else if cooled {
            self.hot = false;
        }

        let idle = cpu < t.idle_cpu && gpu < t.idle_gpu;
        if idle {
            self.idle_since.get_or_insert(now);
        } else {
            self.idle_since = None;
        }
        let idle_long = self.idle_since.map(|s| now - s >= t.idle_delay).unwrap_or(false);

        let game_counts = settings.performance_for_games
            && input.game.is_some()
            && (input.on_ac || !settings.games_only_on_ac);

        let (wanted, why) = if self.hot && settings.max_fans_when_hot {
            let why = if input.throttling {
                "le BIOS signale une surchauffe".to_string()
            } else if gpu_temp >= cpu_temp {
                format!("GPU chaud ({gpu_temp:.0} °C)")
            } else {
                format!("CPU chaud ({cpu_temp:.0} °C)")
            };
            (Level::Max, why)
        } else if game_counts {
            (
                Level::Performance,
                format!("jeu détecté : {}", input.game.as_deref().unwrap_or("?")),
            )
        } else if cpu >= t.heavy_cpu || gpu >= t.heavy_gpu {
            (
                Level::Performance,
                format!("forte charge (CPU {cpu:.0} %, GPU {gpu:.0} %)"),
            )
        } else if idle_long && settings.silent_when_idle {
            (Level::Silent, "PC au repos".to_string())
        } else {
            (Level::Balanced, "utilisation normale".to_string())
        };

        if wanted == self.level {
            self.up_since = None;
            self.down_since = None;
            self.reason = why;
        } else if wanted.rank() > self.level.rank() {
            self.down_since = None;
            let since = *self.up_since.get_or_insert(now);
            // Overheating does not wait.
            if wanted == Level::Max || now - since >= t.up_delay {
                self.level = wanted;
                self.reason = why;
                self.up_since = None;
            }
        } else {
            self.up_since = None;
            let since = *self.down_since.get_or_insert(now);
            if now - since >= t.down_delay {
                self.level = wanted;
                self.reason = why;
                self.down_since = None;
            }
        }

        Decision {
            level: self.level,
            reason: self.reason.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(engine: &mut AutoEngine, from: f64, to: f64, input: &Inputs) -> Decision {
        let s = AutoSettings::default();
        let mut d = engine.update(from, input, &s);
        let mut t = from + 1.0;
        while t <= to {
            d = engine.update(t, input, &s);
            t += 1.0;
        }
        d
    }

    #[test]
    fn game_switches_to_performance_after_delay() {
        let mut e = AutoEngine::new();
        let game = Inputs {
            cpu_load: 30.0,
            gpu_load: Some(20.0),
            on_ac: true,
            game: Some("eldenring.exe".into()),
            ..Default::default()
        };
        assert_eq!(e.update(0.0, &game, &AutoSettings::default()).level, Level::Balanced);
        let d = run(&mut e, 1.0, 5.0, &game);
        assert_eq!(d.level, Level::Performance);
        assert!(d.reason.contains("eldenring"));
    }

    #[test]
    fn short_spike_is_ignored() {
        let mut e = AutoEngine::new();
        let calm = Inputs { cpu_load: 20.0, on_ac: true, ..Default::default() };
        let spike = Inputs { cpu_load: 100.0, on_ac: true, ..Default::default() };
        run(&mut e, 0.0, 3.0, &calm);
        run(&mut e, 4.0, 5.0, &spike);
        let d = run(&mut e, 6.0, 8.0, &calm);
        assert_eq!(d.level, Level::Balanced);
    }

    #[test]
    fn heat_goes_to_max_immediately_and_holds() {
        let mut e = AutoEngine::new();
        let hot = Inputs { cpu_load: 50.0, gpu_temp: Some(90.0), on_ac: true, ..Default::default() };
        assert_eq!(e.update(0.0, &hot, &AutoSettings::default()).level, Level::Max);
        // Slightly cooler but still within hysteresis: stays Max.
        let warm = Inputs { gpu_temp: Some(80.0), ..hot.clone() };
        assert_eq!(run(&mut e, 1.0, 120.0, &warm).level, Level::Max);
        // Clearly cooled: steps down after the down delay.
        let cool = Inputs { gpu_temp: Some(60.0), cpu_load: 30.0, ..hot };
        assert_eq!(run(&mut e, 121.0, 125.0, &cool).level, Level::Max);
        assert_eq!(run(&mut e, 126.0, 200.0, &cool).level, Level::Balanced);
    }

    #[test]
    fn idle_goes_silent_after_a_minute() {
        let mut e = AutoEngine::new();
        let idle = Inputs { cpu_load: 3.0, gpu_load: Some(0.0), on_ac: true, ..Default::default() };
        assert_eq!(run(&mut e, 0.0, 50.0, &idle).level, Level::Balanced);
        assert_eq!(run(&mut e, 51.0, 120.0, &idle).level, Level::Silent);
    }

    #[test]
    fn games_on_battery_respect_setting() {
        let mut e = AutoEngine::new();
        let s = AutoSettings { games_only_on_ac: true, ..Default::default() };
        let game = Inputs { cpu_load: 30.0, on_ac: false, game: Some("x.exe".into()), ..Default::default() };
        let mut d = e.update(0.0, &game, &s);
        for t in 1..10 {
            d = e.update(t as f64, &game, &s);
        }
        assert_eq!(d.level, Level::Balanced);
    }
}
