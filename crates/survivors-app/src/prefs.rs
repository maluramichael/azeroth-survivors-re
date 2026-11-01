//! The player's own settings beside the CVars: how the camera turns and whether it shakes, the
//! frame-rate cap and the frame-rate readout, kept in `survivors-prefs.toml` next to the profile. [`Shake`] is the camera's shake for the moment,
//! raised by hits and read by [`super::hero::place_camera`].

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(Prefs::load()).init_resource::<Shake>();
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Prefs {
    /// Scales how far a mouse drag turns the camera (1.0 is the default rate).
    pub camera_sensitivity: f32,
    /// A drag up tilts the camera down instead of up.
    pub invert_pitch: bool,
    /// Hits shake the camera.
    pub camera_shake: bool,
    /// The frame-rate cap, frames a second; `0` is no cap ([`super::framerate`]).
    pub frame_cap: u32,
    /// The render scale follows the frame times instead of staying at the chosen value.
    pub dynamic_scale: bool,
    /// The corner label with the frame rate.
    pub show_fps: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            camera_sensitivity: 1.0,
            invert_pitch: false,
            camera_shake: true,
            frame_cap: 0,
            dynamic_scale: false,
            show_fps: false,
        }
    }
}

impl Prefs {
    fn path() -> Option<std::path::PathBuf> {
        benilla_app::survivors_host::local_state::home().map(|h| h.join("survivors-prefs.toml"))
    }

    /// The saved settings, or the defaults when there are none or they do not parse.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| benilla_app::survivors_host::local_state::read_to_string(&p).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        match toml::to_string(self) {
            Ok(text) => {
                if let Err(e) = benilla_app::survivors_host::local_state::write_atomic(&path, &text) {
                    warn!("survivors: prefs not saved ({e})");
                }
            }
            Err(e) => warn!("survivors: prefs not serialized ({e})"),
        }
    }
}

/// Camera shake, 0..1: a hit adds to it ([`Shake::add`]), the camera decays it and turns it into
/// an offset. Ignored when [`Prefs::camera_shake`] is off.
#[derive(Resource, Default)]
pub(crate) struct Shake {
    pub trauma: f32,
}

impl Shake {
    pub fn add(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount).min(1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_settings_survive_a_save_and_a_load() {
        let prefs = Prefs {
            frame_cap: 144,
            dynamic_scale: true,
            show_fps: true,
            camera_sensitivity: 1.25,
            ..Prefs::default()
        };
        let text = toml::to_string(&prefs).unwrap();
        let back: Prefs = toml::from_str(&text).unwrap();
        assert_eq!(back, prefs);
    }

    #[test]
    fn a_file_from_before_the_frame_settings_loads_with_their_defaults() {
        let old = "camera_sensitivity = 0.5\ninvert_pitch = true\ncamera_shake = false\n";
        let prefs: Prefs = toml::from_str(old).unwrap();
        assert_eq!(prefs.camera_sensitivity, 0.5);
        assert!(prefs.invert_pitch && !prefs.camera_shake);
        assert_eq!(prefs.frame_cap, 0, "no cap unless asked");
        assert!(!prefs.dynamic_scale && !prefs.show_fps);
    }
}
