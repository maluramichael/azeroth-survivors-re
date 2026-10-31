//! The player's own settings beside the CVars: how the camera turns and whether it shakes, kept
//! in `survivors-prefs.toml` next to the profile. [`Shake`] is the camera's shake for the moment,
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
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            camera_sensitivity: 1.0,
            invert_pitch: false,
            camera_shake: true,
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
