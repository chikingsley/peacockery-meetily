//! `audio-levels` payload (ARCHITECTURE.md §9): mic and system levels 0..1 (dBFS mapped
//! −60..0) and how long the system channel has been silent.

use serde::Serialize;

/// Bottom of the level scale.
const FLOOR_DBFS: f32 = -60.0;
/// System audio below this counts as silent.
pub const SYSTEM_SILENT_DBFS: f32 = -55.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AudioLevels {
    pub mic: f32,
    pub system: f32,
    pub system_silent_seconds: f64,
}

/// dBFS → 0..1 on a −60..0 scale.
pub fn level_from_dbfs(db: f32) -> f32 {
    if db.is_nan() {
        return 0.0;
    }
    ((db - FLOOR_DBFS) / -FLOOR_DBFS).clamp(0.0, 1.0)
}

/// Turns per-frame dBFS into `AudioLevels`, counting consecutive silent system time.
#[derive(Debug, Default)]
pub struct LevelMeter {
    system_silent_seconds: f64,
}

impl LevelMeter {
    /// One frame of `seconds` length.
    pub fn frame(&mut self, mic_db: f32, sys_db: f32, seconds: f64) -> AudioLevels {
        if sys_db < SYSTEM_SILENT_DBFS {
            self.system_silent_seconds += seconds;
        } else {
            self.system_silent_seconds = 0.0;
        }
        AudioLevels {
            mic: level_from_dbfs(mic_db),
            system: level_from_dbfs(sys_db),
            system_silent_seconds: self.system_silent_seconds,
        }
    }
}

/// Receives `audio-levels` updates (~10 Hz).
pub type LevelsCallback = Box<dyn FnMut(AudioLevels) + Send>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_dbfs_to_unit_range() {
        assert_eq!(level_from_dbfs(-100.0), 0.0);
        assert_eq!(level_from_dbfs(-60.0), 0.0);
        assert_eq!(level_from_dbfs(-30.0), 0.5);
        assert_eq!(level_from_dbfs(0.0), 1.0);
        assert_eq!(level_from_dbfs(f32::NAN), 0.0);
    }

    #[test]
    fn counts_silent_system_time_until_sound() {
        let mut meter = LevelMeter::default();
        meter.frame(-20.0, -90.0, 0.1);
        let levels = meter.frame(-20.0, -56.0, 0.1);
        assert!((levels.system_silent_seconds - 0.2).abs() < 1e-9);
        let levels = meter.frame(-20.0, -40.0, 0.1);
        assert_eq!(levels.system_silent_seconds, 0.0);
        assert_eq!(levels.system, level_from_dbfs(-40.0));
    }
}
