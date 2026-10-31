//! Boss enrage: below a share of its health a boss turns faster and hits harder.

use benilla_world::schedule::WorldStage;
use bevy::prelude::*;

use super::chatlog::{ChatKind, ChatLine};
use super::enemies::{Dying, Enemy};
use super::units::Fx;
use super::{Phase, Run};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        enrage_bosses
            .after(WorldStage::Input)
            .run_if(in_state(Phase::Playing))
            .run_if(super::simulates)
            .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
    );
}

/// A boss enrages once its health falls below this share of its maximum.
const ENRAGE_BELOW: f32 = 0.4;
/// An enraged boss walks this much faster.
const ENRAGE_SPEED: f32 = 1.3;
/// An enraged boss hits this much harder.
const ENRAGE_DAMAGE: f32 = 1.5;
/// The visual of the roar: the creature spell "Enrage". Assumption, not verified against the
/// client's spell data; no other angry or red effect id is in use in this codebase to fall back on.
const ENRAGE_SPELL: u32 = 8599;
/// Seconds the centre banner stays up.
const ENRAGE_BANNER_SECS: f32 = 3.0;

/// Marks a boss that has already enraged, so it happens once.
#[derive(Component)]
pub(super) struct Enraged;

/// Whether a boss with this much health left is low enough to enrage.
fn should_enrage(hp: f32, max_hp: f32) -> bool {
    max_hp > 0.0 && hp > 0.0 && hp < max_hp * ENRAGE_BELOW
}

/// The first time a living boss drops below the threshold: faster, harder, announced, and marked.
/// Speed and damage are the stored base values the chase and the swing read, so scaling them in
/// place stacks correctly with the separate `slow` and `weaken` multipliers.
fn enrage_bosses(
    mut commands: Commands,
    mut bosses: Query<(Entity, &mut Enemy), (Without<Enraged>, Without<Dying>)>,
    mut run: ResMut<Run>,
    mut chat: MessageWriter<ChatLine>,
    mut fx: Fx,
) {
    for (ent, mut e) in &mut bosses {
        if !e.boss || e.dead || !should_enrage(e.hp, e.max_hp) {
            continue;
        }
        e.speed *= ENRAGE_SPEED;
        e.damage *= ENRAGE_DAMAGE;
        commands.entity(ent).insert(Enraged);
        let line = format!("{} becomes enraged!", e.def.name);
        run.banner = Some((line.clone(), ENRAGE_BANNER_SECS));
        chat.write(ChatLine::text(ChatKind::Boss, line));
        fx.impact(ent, ENRAGE_SPELL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrages_only_below_the_threshold() {
        assert!(!should_enrage(100.0, 100.0));
        assert!(!should_enrage(40.0, 100.0));
        assert!(should_enrage(39.0, 100.0));
        assert!(should_enrage(1.0, 100.0));
    }

    #[test]
    fn dead_or_degenerate_bosses_do_not_enrage() {
        assert!(!should_enrage(0.0, 100.0));
        assert!(!should_enrage(-5.0, 100.0));
        assert!(!should_enrage(0.0, 0.0));
    }
}
