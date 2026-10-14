//! User-facing modes and what each one writes to the hardware.

use omen_wmi::PerformanceProfile;
use serde::{Deserialize, Serialize};

/// What the user picks in the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Mode {
    /// The app chooses among the fixed modes from load, heat and games.
    #[default]
    Auto,
    Silent,
    Balanced,
    Performance,
    /// Fans at full speed.
    Max,
    /// User fan curve driven by the app (advanced).
    Custom,
}

impl Mode {
    pub const MAIN: [Mode; 5] = [Mode::Auto, Mode::Silent, Mode::Balanced, Mode::Performance, Mode::Max];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Auto => "Auto",
            Mode::Silent => "Silencieux",
            Mode::Balanced => "Équilibré",
            Mode::Performance => "Performance",
            Mode::Max => "Max",
            Mode::Custom => "Courbe perso",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Mode::Auto => "✨",
            Mode::Silent => "🌙",
            Mode::Balanced => "⚖",
            Mode::Performance => "🚀",
            Mode::Max => "🌀",
            Mode::Custom => "📈",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Mode::Auto => "S'adapte tout seul : calme au repos, à fond en jeu ou quand ça chauffe.",
            Mode::Silent => "Ventilateurs discrets, GPU limité. Idéal bureautique et films.",
            Mode::Balanced => "Le réglage de base de HP, bon compromis.",
            Mode::Performance => "Puissance maximale CPU + GPU, ventilation plus forte.",
            Mode::Max => "Ventilateurs à fond en permanence, pour refroidir au plus vite.",
            Mode::Custom => "Votre courbe ventilateurs (onglet Avancé).",
        }
    }
}

/// A concrete hardware state (what Auto resolves to, or a fixed mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Level {
    Silent,
    Balanced,
    Performance,
    Max,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Silent => "Silencieux",
            Level::Balanced => "Équilibré",
            Level::Performance => "Performance",
            Level::Max => "Max",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Level::Silent => 0,
            Level::Balanced => 1,
            Level::Performance => 2,
            Level::Max => 3,
        }
    }

    pub fn profile(self) -> PerformanceProfile {
        match self {
            Level::Silent => PerformanceProfile::Quiet,
            Level::Balanced => PerformanceProfile::Balanced,
            Level::Performance | Level::Max => PerformanceProfile::Performance,
        }
    }

    /// GPU power: (custom TGP, PPAB/Dynamic Boost).
    pub fn gpu_power(self) -> (bool, bool) {
        match self {
            Level::Silent => (false, false),
            Level::Balanced => (true, false),
            Level::Performance | Level::Max => (true, true),
        }
    }

    pub fn max_fan(self) -> bool {
        self == Level::Max
    }
}

impl Mode {
    /// The fixed level of a non-automatic mode.
    pub fn fixed_level(self) -> Option<Level> {
        match self {
            Mode::Silent => Some(Level::Silent),
            Mode::Balanced | Mode::Custom => Some(Level::Balanced),
            Mode::Performance => Some(Level::Performance),
            Mode::Max => Some(Level::Max),
            Mode::Auto => None,
        }
    }
}
