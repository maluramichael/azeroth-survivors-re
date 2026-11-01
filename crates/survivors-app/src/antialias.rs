//! Edge smoothing and sharpening on the world camera: the anti-aliasing mode in
//! [`super::prefs::Prefs::anti_alias`] (off, FXAA or SMAA, after the world's own MSAA from the
//! `gxMultisample` CVar), and the grade's on/off and sharpness from [`super::prefs::Prefs`].
//! The interface is drawn by its own camera and stays untouched.
//!
//! FXAA and SMAA are Bevy's own post-process components. They sit between the tonemapping node
//! and the end of the world's post-processing, ahead of the grade ([`super::grade::Grade`] orders
//! itself after them), so the grade sharpens the already smoothed picture. The world camera runs
//! at the render scale into its own target, so the pass smooths exactly the pixels that are
//! scaled up afterwards.
//!
//! MSAA is the engine's: `benilla_world::view::MsaaSetting` follows the CVar, and this module
//! carries it onto the live camera's [`Msaa`] component. Every pipeline of ours that bakes in a
//! sample count (`static_gx`, the particle effects) re-specialises on the view's count, and the
//! glow's own targets are single-sampled, so a change shows on the next frame.

use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::prelude::*;
use bevy::render::view::Msaa;

use benilla_world::view::{MsaaSetting, WorldCamera};

use super::prefs::{AntiAlias, Prefs};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (apply_anti_alias, apply_msaa));
}

/// Which of Bevy's post-process components a mode wants: `(fxaa, smaa)`. At most one.
fn wanted(mode: AntiAlias) -> (bool, bool) {
    match mode {
        AntiAlias::Off => (false, false),
        AntiAlias::Fxaa => (true, false),
        AntiAlias::Smaa => (false, true),
    }
}

/// Puts [`Fxaa`] or [`Smaa`] on every world camera, or takes them off, to match the saved mode.
/// Runs every frame so a camera spawned later (a new run) is picked up too; it queues commands
/// only for a camera that differs.
fn apply_anti_alias(
    mut commands: Commands,
    prefs: Res<Prefs>,
    cams: Query<(Entity, Has<Fxaa>, Has<Smaa>), (With<WorldCamera>, With<Camera3d>)>,
) {
    let (fxaa, smaa) = wanted(prefs.anti_alias);
    for (entity, has_fxaa, has_smaa) in &cams {
        let mut cam = commands.entity(entity);
        match (fxaa, has_fxaa) {
            (true, false) => {
                cam.insert(Fxaa::default());
            }
            (false, true) => {
                cam.remove::<Fxaa>();
            }
            _ => {}
        }
        match (smaa, has_smaa) {
            (true, false) => {
                cam.insert(Smaa::default());
            }
            (false, true) => {
                cam.remove::<Smaa>();
            }
            _ => {}
        }
    }
}

/// Carries the `gxMultisample` CVar's level ([`MsaaSetting`], already clamped to what the GPU
/// offers) onto the world camera's [`Msaa`]. The camera reads the setting once at its spawn;
/// this makes a later change live.
fn apply_msaa(
    setting: Res<MsaaSetting>,
    mut cams: Query<&mut Msaa, (With<WorldCamera>, With<Camera3d>)>,
) {
    let level = setting.level();
    for mut msaa in &mut cams {
        msaa.set_if_neq(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_wants_at_most_one_component() {
        assert_eq!(wanted(AntiAlias::Off), (false, false));
        assert_eq!(wanted(AntiAlias::Fxaa), (true, false));
        assert_eq!(wanted(AntiAlias::Smaa), (false, true));
    }

    fn app_with(prefs: Prefs, msaa: MsaaSetting) -> App {
        let mut app = App::new();
        app.insert_resource(prefs).insert_resource(msaa);
        plugin(&mut app);
        app
    }

    #[test]
    fn the_world_camera_follows_the_mode_live() {
        let mut app = app_with(Prefs::default(), MsaaSetting { samples: 1 });
        let world_cam = app.world_mut().spawn((Camera3d::default(), WorldCamera)).id();
        // A booth camera: a `Camera3d` without the marker stays untouched.
        let booth = app.world_mut().spawn(Camera3d::default()).id();

        app.update();
        assert!(!app.world().entity(world_cam).contains::<Fxaa>());

        app.world_mut().resource_mut::<Prefs>().anti_alias = AntiAlias::Fxaa;
        app.update();
        assert!(app.world().entity(world_cam).contains::<Fxaa>());
        assert!(!app.world().entity(world_cam).contains::<Smaa>());

        app.world_mut().resource_mut::<Prefs>().anti_alias = AntiAlias::Smaa;
        app.update();
        assert!(!app.world().entity(world_cam).contains::<Fxaa>());
        assert!(app.world().entity(world_cam).contains::<Smaa>());

        app.world_mut().resource_mut::<Prefs>().anti_alias = AntiAlias::Off;
        app.update();
        assert!(!app.world().entity(world_cam).contains::<Smaa>());

        assert!(!app.world().entity(booth).contains::<Fxaa>());
        assert!(!app.world().entity(booth).contains::<Smaa>());
    }

    #[test]
    fn the_msaa_setting_reaches_the_world_camera() {
        let mut app = app_with(Prefs::default(), MsaaSetting { samples: 1 });
        // Without the render plugins `Camera3d` brings no `Msaa`; the game's camera spawns with one.
        let cam = app
            .world_mut()
            .spawn((Camera3d::default(), WorldCamera, Msaa::Sample4))
            .id();
        app.update();
        assert_eq!(*app.world().entity(cam).get::<Msaa>().unwrap(), Msaa::Off);

        app.world_mut().resource_mut::<MsaaSetting>().samples = 4;
        app.update();
        assert_eq!(*app.world().entity(cam).get::<Msaa>().unwrap(), Msaa::Sample4);

        app.world_mut().resource_mut::<MsaaSetting>().samples = 1;
        app.update();
        assert_eq!(*app.world().entity(cam).get::<Msaa>().unwrap(), Msaa::Off);
    }
}
