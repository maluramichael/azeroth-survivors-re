//! Graphics presets for the Options window: Low, Medium, High and Ultra, each a set of engine
//! CVars (view distance, ground clutter, spell detail, weather, glow, MSAA) and the survivors'
//! own picture settings in [`super::prefs::Prefs`]. A preset only writes values; the window shows
//! "Custom" once a single value differs from every preset.
//!
//! The values live in one plain [`Look`] per preset, so [`apply`] and [`matching`] are the same
//! table read two ways and the tests can compare them without a window. Anisotropic filtering is
//! part of a preset although the engine takes it at the next start only (`video::on_cvar`); the
//! window says so beside its row.

use bevy::prelude::*;

use super::prefs::{AntiAlias, Prefs};
use benilla_app::survivors_host::cvars::Cvars;
use benilla_world::view::FARCLIP_RANGE;

pub(super) fn plugin(_app: &mut App) {}

/// The engine CVars a preset writes (the registry matches the names case-insensitively).
pub(super) const FARCLIP: &str = "farclip";
pub(super) const WORLD_DETAIL: &str = "worldDetail";
pub(super) const SPELL_DETAIL: &str = "spellEffectLevel";
pub(super) const WEATHER: &str = "weatherDensity";
pub(super) const GLOW: &str = "ffxGlow";
pub(super) const DEATH_FX: &str = "ffxDeath";
/// Latched (`gx*`): staged by `set`, applied by [`Cvars::commit_latched`].
pub(super) const MSAA: &str = "gxMultisample";
pub(super) const ANISO: &str = "anisotropic";

/// The sample counts the window offers; `1` is off.
pub(super) const MSAA_STEPS: [u32; 4] = [1, 2, 4, 8];
/// The filtering levels the window offers; `1` is off.
pub(super) const ANISO_STEPS: [u32; 5] = [1, 2, 4, 8, 16];

/// The registered defaults, for a CVar that is missing.
const DEFAULT_FARCLIP: f32 = 350.0;
const DEFAULT_WORLD_DETAIL: usize = 1;
const DEFAULT_SPELL_DETAIL: usize = 2;
const DEFAULT_WEATHER: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Preset {
    Low,
    Medium,
    High,
    Ultra,
}

impl Preset {
    pub(super) const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Ultra];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }

    /// The values the preset stands for. Low is for weak hardware: a short view, no ground
    /// clutter, no post effects, no multisampling. Ultra is the engine's maximum.
    fn look(self) -> Look {
        match self {
            Self::Low => Look {
                farclip: 200.0,
                world_detail: 0,
                spell_detail: 0,
                weather: 0,
                glow: false,
                death_fx: false,
                msaa: 1,
                aniso: 1,
                anti_alias: AntiAlias::Off,
                grade: false,
                sharpness: 0.4,
                hit_flash: true,
                death_fade: false,
            },
            Self::Medium => Look {
                farclip: 300.0,
                world_detail: 1,
                spell_detail: 1,
                weather: 1,
                glow: true,
                death_fx: true,
                msaa: 1,
                aniso: 4,
                anti_alias: AntiAlias::Fxaa,
                grade: true,
                sharpness: 0.4,
                hit_flash: true,
                death_fade: true,
            },
            Self::High => Look {
                farclip: 450.0,
                world_detail: 2,
                spell_detail: 2,
                weather: 2,
                glow: true,
                death_fx: true,
                msaa: 4,
                aniso: 8,
                anti_alias: AntiAlias::Smaa,
                grade: true,
                sharpness: 0.5,
                hit_flash: true,
                death_fade: true,
            },
            Self::Ultra => Look {
                farclip: *FARCLIP_RANGE.end(),
                world_detail: 2,
                spell_detail: 2,
                weather: 3,
                glow: true,
                death_fx: true,
                msaa: 8,
                aniso: 16,
                anti_alias: AntiAlias::Smaa,
                grade: true,
                sharpness: 0.6,
                hit_flash: true,
                death_fade: true,
            },
        }
    }
}

/// Every value a preset sets, in the units of its CVar or pref.
#[derive(Clone, Copy, Debug)]
struct Look {
    farclip: f32,
    world_detail: usize,
    spell_detail: usize,
    weather: usize,
    glow: bool,
    death_fx: bool,
    msaa: u32,
    aniso: u32,
    anti_alias: AntiAlias,
    grade: bool,
    sharpness: f32,
    hit_flash: bool,
    death_fade: bool,
}

impl Look {
    /// What the CVars and the prefs say now, each value snapped to what the window can show.
    fn read(cvars: &Cvars, prefs: &Prefs) -> Self {
        Self {
            farclip: view_distance(cvars),
            world_detail: level(cvars, WORLD_DETAIL, DEFAULT_WORLD_DETAIL, 2),
            spell_detail: level(cvars, SPELL_DETAIL, DEFAULT_SPELL_DETAIL, 2),
            weather: level(cvars, WEATHER, DEFAULT_WEATHER, 3),
            glow: cvars.flag(GLOW).unwrap_or(true),
            death_fx: cvars.flag(DEATH_FX).unwrap_or(true),
            msaa: MSAA_STEPS[nearest(&MSAA_STEPS, cvars.num(MSAA).unwrap_or(1.0))],
            aniso: ANISO_STEPS[nearest(&ANISO_STEPS, cvars.num(ANISO).unwrap_or(1.0))],
            anti_alias: prefs.anti_alias,
            grade: prefs.grade,
            sharpness: prefs.sharpness,
            hit_flash: prefs.hit_flash,
            death_fade: prefs.death_fade,
        }
    }

    fn same(&self, other: &Self) -> bool {
        (self.farclip - other.farclip).abs() < 0.5
            && self.world_detail == other.world_detail
            && self.spell_detail == other.spell_detail
            && self.weather == other.weather
            && self.glow == other.glow
            && self.death_fx == other.death_fx
            && self.msaa == other.msaa
            && self.aniso == other.aniso
            && self.anti_alias == other.anti_alias
            && self.grade == other.grade
            && (self.sharpness - other.sharpness).abs() < 0.005
            && self.hit_flash == other.hit_flash
            && self.death_fade == other.death_fade
    }
}

/// The `farclip` CVar in yards, inside the engine's range.
pub(super) fn view_distance(cvars: &Cvars) -> f32 {
    cvars
        .num(FARCLIP)
        .filter(|v| v.is_finite())
        .unwrap_or(DEFAULT_FARCLIP)
        .clamp(*FARCLIP_RANGE.start(), *FARCLIP_RANGE.end())
}

/// A stepped CVar (0..=`max`) as an index; `default` when it is missing.
pub(super) fn level(cvars: &Cvars, name: &str, default: usize, max: usize) -> usize {
    cvars
        .num(name)
        .filter(|v| v.is_finite())
        .map_or(default, |v| v.round().max(0.0) as usize)
        .min(max)
}

/// The index of the step nearest `value`, so a hand-edited `config.toml` still lands on a stop.
pub(super) fn nearest(steps: &[u32], value: f32) -> usize {
    steps
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (**a as f32 - value)
                .abs()
                .total_cmp(&(**b as f32 - value).abs())
        })
        .map_or(0, |(i, _)| i)
}

/// Write the preset: its CVars through [`Cvars::set`] (the engine's observers pick them up on the
/// next sync), the latched MSAA committed as `RestartGx` would, and its prefs. The caller saves
/// [`Prefs`].
pub(super) fn apply(preset: Preset, cvars: &mut Cvars, prefs: &mut Prefs) {
    let look = preset.look();
    let flag = |on: bool| if on { "1" } else { "0" };
    cvars.set(FARCLIP, &look.farclip.to_string());
    cvars.set(WORLD_DETAIL, &look.world_detail.to_string());
    cvars.set(SPELL_DETAIL, &look.spell_detail.to_string());
    cvars.set(WEATHER, &look.weather.to_string());
    cvars.set(GLOW, flag(look.glow));
    cvars.set(DEATH_FX, flag(look.death_fx));
    cvars.set(MSAA, &look.msaa.to_string());
    cvars.commit_latched();
    cvars.set(ANISO, &look.aniso.to_string());
    prefs.anti_alias = look.anti_alias;
    prefs.grade = look.grade;
    prefs.sharpness = look.sharpness;
    prefs.hit_flash = look.hit_flash;
    prefs.death_fade = look.death_fade;
}

/// The preset whose values all hold right now, or `None` for "Custom".
pub(super) fn matching(cvars: &Cvars, prefs: &Prefs) -> Option<Preset> {
    let now = Look::read(cvars, prefs);
    Preset::ALL.into_iter().find(|p| p.look().same(&now))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied(preset: Preset) -> (Cvars, Prefs) {
        let mut cvars = Cvars::default();
        let mut prefs = Prefs::default();
        apply(preset, &mut cvars, &mut prefs);
        (cvars, prefs)
    }

    #[test]
    fn every_preset_reads_back_as_itself() {
        for preset in Preset::ALL {
            let (cvars, prefs) = applied(preset);
            assert_eq!(matching(&cvars, &prefs), Some(preset), "{preset:?}");
        }
    }

    #[test]
    fn one_changed_value_makes_it_custom() {
        let (mut cvars, prefs) = applied(Preset::High);
        cvars.set(WEATHER, "0");
        assert_eq!(matching(&cvars, &prefs), None);

        let (cvars, mut prefs) = applied(Preset::Medium);
        prefs.anti_alias = AntiAlias::Off;
        assert_eq!(matching(&cvars, &prefs), None);

        let (cvars, mut prefs) = applied(Preset::Ultra);
        prefs.sharpness = 0.2;
        assert_eq!(matching(&cvars, &prefs), None);
    }

    #[test]
    fn the_latched_multisample_is_committed_by_apply() {
        let (cvars, _) = applied(Preset::Ultra);
        assert_eq!(cvars.num(MSAA), Some(8.0));
        let (cvars, _) = applied(Preset::Low);
        assert_eq!(cvars.num(MSAA), Some(1.0));
    }

    #[test]
    fn no_two_presets_are_the_same() {
        for a in Preset::ALL {
            for b in Preset::ALL {
                assert_eq!(a == b, a.look().same(&b.look()), "{a:?} / {b:?}");
            }
        }
    }

    #[test]
    fn the_presets_climb_from_low_to_ultra() {
        let looks = Preset::ALL.map(Preset::look);
        for pair in looks.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            assert!(lo.farclip < hi.farclip);
            assert!(lo.world_detail <= hi.world_detail);
            assert!(lo.spell_detail <= hi.spell_detail);
            assert!(lo.weather <= hi.weather);
            assert!(lo.msaa <= hi.msaa);
            assert!(lo.aniso <= hi.aniso);
        }
    }

    #[test]
    fn low_is_light_and_ultra_is_the_maximum() {
        let low = Preset::Low.look();
        assert_eq!((low.world_detail, low.msaa, low.weather), (0, 1, 0));
        assert!(!low.glow && !low.death_fx && !low.grade);
        assert_eq!(low.anti_alias, AntiAlias::Off);
        assert!(low.farclip < DEFAULT_FARCLIP);

        let ultra = Preset::Ultra.look();
        assert_eq!(ultra.farclip, *FARCLIP_RANGE.end());
        assert_eq!(
            (ultra.world_detail, ultra.spell_detail, ultra.weather),
            (2, 2, 3)
        );
        assert_eq!(ultra.msaa, *MSAA_STEPS.last().unwrap());
        assert_eq!(ultra.aniso, *ANISO_STEPS.last().unwrap());
    }

    #[test]
    fn every_preset_value_is_one_the_engine_and_the_window_take() {
        for preset in Preset::ALL {
            let look = preset.look();
            assert!(FARCLIP_RANGE.contains(&look.farclip), "{preset:?}");
            assert!(look.world_detail <= 2 && look.spell_detail <= 2 && look.weather <= 3);
            assert!(MSAA_STEPS.contains(&look.msaa) && ANISO_STEPS.contains(&look.aniso));
            assert!((0.0..=1.0).contains(&look.sharpness));
        }
    }

    #[test]
    fn a_hand_edited_value_snaps_to_the_nearest_stop() {
        assert_eq!(nearest(&MSAA_STEPS, 16.0), 3);
        assert_eq!(nearest(&MSAA_STEPS, 3.0), 1);
        assert_eq!(nearest(&ANISO_STEPS, 1.0), 0);
        assert_eq!(nearest(&ANISO_STEPS, 12.0), 3);
    }
}
