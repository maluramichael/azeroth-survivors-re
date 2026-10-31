//! The hero: its body, WASD movement over the terrain, the top-down camera, and its health.

use benilla_world::view::WorldCamera;
use benilla_world::world_point::WorldPoint;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

use super::data::{self, ClassDef};
use super::units::{self, flat, wow_to_bevy, yaw_toward, Fx, Marks, UnitSpec};
use super::{Phase, Run, Selection};
use benilla_app::survivors_host::creature_anim::{move_flags, MovementState};
use benilla_app::survivors_host::net::{FieldChanged, Guid, ObjectStore};

#[derive(Component)]
pub(super) struct Hero {
    /// Vertical velocity in yards/second while airborne; 0 on the ground.
    pub(super) vy: f32,
    /// Height above the ground while airborne. Kept here, not in the transform: the creature
    /// ground clamp (`net::motion::ground_clamp_creatures`) re-seats every unit on the floor at
    /// the top of the frame, so the arc is re-applied over the ground each frame.
    pub(super) air: f32,
}

/// The body radius enemies stop at.
pub(super) const HERO_RADIUS: f32 = 0.7;

/// The hop's initial upward speed and the gravity that brings it back down — the reference
/// `CMovement` jump seed and gravity (`player/state.rs`'s `JUMP_SPEED` / `GRAVITY`).
const JUMP_SPEED: f32 = 7.955_547;
const GRAVITY: f32 = 19.291_105;

/// Radians of orbit per raw mouse unit while right-dragging.
const ORBIT_RATE: f32 = 0.008;
/// Elevation clamp (radians): ~10°..~83° above the horizon, so the camera never dives underground.
const PITCH_MIN: f32 = 0.18;
const PITCH_MAX: f32 = 1.45;
/// The tilt a run starts at.
const START_PITCH: f32 = 61.0 * std::f32::consts::PI / 180.0;
/// Left and right arrow turn the camera this fast, radians per second: WoW's keyboard turn rate.
const TURN_RATE: f32 = std::f32::consts::PI;
/// Wheel zoom range, yards from the hero's shoulders.
const ZOOM_MIN: f32 = 1.5;
const ZOOM_MAX: f32 = 70.0;

/// The pick screen's idle orbit, radians per second.
const MENU_IDLE_RATE: f32 = 0.06;
/// Seconds after letting go of the drag before the idle orbit starts again, then ramps in over
/// [`MENU_IDLE_RAMP`].
const MENU_IDLE_DELAY: f32 = 3.0;
const MENU_IDLE_RAMP: f32 = 1.5;
/// The pick screen's default camera distance and zoom range, yards from the look-at point.
const MENU_DIST: f32 = 7.0;
const MENU_ZOOM_MIN: f32 = 3.5;
const MENU_ZOOM_MAX: f32 = 12.0;
/// The pick screen's default elevation (2.4 yards up at 7 out, seen from the 1.1 look-at height)
/// and the tilt range a drag may use, radians.
const MENU_PITCH: f32 = 0.184;
const MENU_PITCH_MIN: f32 = 0.05;
const MENU_PITCH_MAX: f32 = 0.5;
/// How fast the menu zoom settles on its target, per second.
const MENU_ZOOM_SMOOTH: f32 = 9.0;

#[derive(Resource)]
pub(super) struct CamRig {
    pub dist: f32,
    pub orbit: f32,
    /// Orbit azimuth (radians) around the hero; right-drag swings it during play.
    pub yaw: f32,
    /// Elevation above the horizon (radians), clamped so the camera never dives underground.
    pub pitch: f32,
    /// Pick-screen camera distance as drawn, easing toward `menu_zoom`; apart from `dist` so the
    /// menu zoom leaves the game camera alone.
    pub menu_dist: f32,
    /// Pick-screen zoom the wheel asks for.
    pub menu_zoom: f32,
    /// Pick-screen elevation (radians); a drag tilts it a little.
    pub menu_pitch: f32,
    /// A left drag that began over the 3D scene is under way at the pick screen.
    pub menu_drag: bool,
    /// Seconds since the pick-screen drag ended; the idle orbit waits for it to pass the delay.
    pub menu_idle: f32,
}

impl Default for CamRig {
    fn default() -> Self {
        Self {
            dist: 30.0,
            orbit: 0.6,
            yaw: 0.0,
            pitch: START_PITCH,
            menu_dist: MENU_DIST,
            menu_zoom: MENU_DIST,
            menu_pitch: MENU_PITCH,
            menu_drag: false,
            menu_idle: MENU_IDLE_DELAY + MENU_IDLE_RAMP,
        }
    }
}

/// Whether the cursor is over a piece of UI: a node that draws something (an image, a visible
/// background) or takes clicks, so panels without buttons count and the empty layout wrappers
/// around them do not. Nodes that cover most of the window (vignettes, fade layers) are not
/// panels and are skipped.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct UiCover<'w, 's> {
    windows: Query<'w, 's, &'static Window, With<bevy::window::PrimaryWindow>>,
    nodes: Query<'w, 's, UiNode>,
}

/// What [`UiCover`] reads off a UI node to tell a panel from an empty layout wrapper.
type UiNode = (
    &'static ComputedNode,
    &'static bevy::ui::UiGlobalTransform,
    &'static InheritedVisibility,
    Option<&'static BackgroundColor>,
    Option<&'static ImageNode>,
    Option<&'static Interaction>,
);

impl UiCover<'_, '_> {
    pub(super) fn cursor_over_ui(&self) -> bool {
        let Ok(window) = self.windows.single() else {
            return false;
        };
        let Some(cursor) = window.physical_cursor_position() else {
            return false;
        };
        let screen = window.physical_size().as_vec2();
        self.nodes.iter().any(|(node, at, shown, fill, image, interaction)| {
            let size = node.size();
            let solid = interaction.is_some()
                || image.is_some_and(|i| i.color.alpha() > 0.0)
                || fill.is_some_and(|f| f.0.alpha() > 0.0);
            shown.get()
                && solid
                && size.x * size.y < 0.7 * screen.x * screen.y
                && node.contains_point(*at, cursor)
        })
    }
}

/// Where a battleground's runs start.
pub(super) fn arena(map: &data::MapDef) -> Vec3 {
    wow_to_bevy(map.center)
}

/// The world is the battleground being stood in ([`Selection::stage`]): entering one on another
/// continent worldports, which raises the loading screen until the new ground is in.
pub(super) fn follow_map(
    selection: Res<Selection>,
    current: Option<ResMut<benilla_world::world_map::CurrentMap>>,
) {
    let Some(mut current) = current else {
        return;
    };
    let want = selection.stage_def().map;
    if current.0 != want {
        current.0 = want;
    }
}

/// The hero in the class trainer's look, weapons drawn.
pub(super) fn spawn_hero(
    commands: &mut Commands,
    class: &'static ClassDef,
    display: u32,
    race: u8,
    sex: u8,
    serial: u64,
    pos: Vec3,
) -> Entity {
    let bytes0 = u32::from(race)
        | u32::from(class.id) << 8
        | u32::from(sex) << 16
        | u32::from(class.power) << 24;
    let e = units::spawn_unit(
        commands,
        &UnitSpec {
            serial,
            display,
            scale: 1.0,
            level: 60,
            faction: units::FACTION_FRIENDLY,
            bytes0,
            weapons: class.weapons,
            sheath: class.sheath,
        },
        pos,
        0.0,
    );
    commands
        .entity(e)
        .insert((Hero { vy: 0.0, air: 0.0 }, Marks::buffs()));
    e
}

/// Midday over the battleground, whatever the clock says.
pub(super) const MINUTE_OF_DAY: u32 = 780;

pub(super) fn daylight(debug: Option<ResMut<benilla_world::dev_state::DebugState>>) {
    let Some(mut debug) = debug else { return };
    if debug.lighting.follow_server_time || debug.lighting.manual_minute != MINUTE_OF_DAY {
        debug.lighting.follow_server_time = false;
        debug.lighting.manual_minute = MINUTE_OF_DAY;
    }
}

/// The ground under the mouse cursor, `None` with the cursor off the window. The aimed mobility
/// spells (Blink, Charge) go toward it.
#[derive(Resource, Default)]
pub(super) struct AimPoint(pub(super) Option<Vec3>);

/// Cast the cursor onto the terrain: the camera ray meets the plane at the hero's height, then the
/// terrain height there, a few times over, which settles on hills within a frame.
pub(super) fn aim_cursor(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cams: Query<(&Camera, &Transform), With<WorldCamera>>,
    hero: Query<&Transform, (With<Hero>, Without<WorldCamera>)>,
    world: WorldPoint,
    mut aim: ResMut<AimPoint>,
) {
    let point = (|| {
        let cursor = windows.single().ok()?.cursor_position()?;
        let (cam, tf) = cams.single().ok()?;
        let ray = cam
            .viewport_to_world(&GlobalTransform::from(*tf), cursor)
            .ok()?;
        let mut y = hero.single().ok()?.translation.y;
        let mut at = None;
        for _ in 0..4 {
            if ray.direction.y.abs() < 1e-4 {
                break;
            }
            let t = (y - ray.origin.y) / ray.direction.y;
            if t <= 0.0 {
                break;
            }
            let p = ray.origin + *ray.direction * t;
            at = Some(p);
            match world.terrain_height_under(p) {
                Some(h) if (h - y).abs() > 0.05 => y = h,
                _ => break,
            }
        }
        at
    })();
    if aim.0 != point {
        aim.0 = point;
    }
}

/// The hero's collision capsule: the player's radius, and a height that with [`BODY_LIFT`] reaches
/// the player's 2.03 yd.
const BODY_RADIUS: f32 = 1.0 / 3.0;
const BODY_HEIGHT: f32 = 1.5;
/// How far the capsule's foot hangs above the hero's feet (yd). The height follows the terrain
/// whatever the walls say, so kerbs, steps and ground lumps below this are walked over, and a jump
/// clears more.
const BODY_LIFT: f32 = 0.5;

/// Moves `feet` by `vel` over `dt`, stopped and slid along by the solid world (trees, rocks,
/// buildings, cliffs): the walk of `player/mover.rs`, minus its step and ground logic. Only the
/// horizontal part comes back; the caller owns the height.
fn collide_step(
    collision: &benilla_world::collision::WorldCollision,
    body: &avian3d::prelude::Collider,
    feet: Vec3,
    vel: Vec3,
    dt: std::time::Duration,
) -> Vec3 {
    let from = feet + Vec3::Y * (BODY_LIFT + BODY_HEIGHT * 0.5);
    let out = collision.slide_body(
        body,
        from,
        vel,
        dt,
        &avian3d::character_controller::move_and_slide::MoveAndSlideConfig::default(),
        |_| avian3d::character_controller::move_and_slide::MoveAndSlideHitResponse::Accept,
    );
    feet + Vec3::new(out.position.x - from.x, 0.0, out.position.z - from.z)
}

/// WASD over the terrain within the battleground's leash, and the streaming focus on the hero. The
/// walk is blocked by the world's colliders ([`collide_step`]), auto mode's too.
#[allow(clippy::too_many_arguments)]
pub(super) fn drive_hero(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    phase: Res<State<Phase>>,
    run: Res<Run>,
    selection: Res<Selection>,
    rig: Res<CamRig>,
    world: WorldPoint,
    collision: benilla_world::collision::WorldCollision,
    mut body: Local<Option<avian3d::prelude::Collider>>,
    auto: Option<Res<super::auto::Auto>>,
    mut player: ResMut<benilla_app::survivors_host::player::Player>,
    mut hero: Query<(&mut Transform, &mut MovementState, &mut Hero)>,
) {
    let focus = match hero.single_mut() {
        Ok((mut tf, mut mv, mut hm)) => {
            let dt = time.delta_secs();
            let mut dir = Vec3::ZERO;
            // Under the player's keys the hero faces the camera's forward and strafes or
            // backpedals against it; driven, he faces where he runs.
            let mut face = None;
            if let Some(auto) = auto
                .as_deref()
                .filter(|_| *phase.get() == Phase::Playing && run.dead_t.is_none())
            {
                dir = super::auto::steer(auto);
            } else if *phase.get() == Phase::Playing && run.dead_t.is_none() {
                if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
                    dir.z -= 1.0;
                }
                if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
                    dir.z += 1.0;
                }
                // A and D strafe; the left and right arrows turn the camera instead
                // ([`place_camera`]).
                if keys.pressed(KeyCode::KeyA) {
                    dir.x -= 1.0;
                }
                if keys.pressed(KeyCode::KeyD) {
                    dir.x += 1.0;
                }
                // Camera-relative.
                let (sy, cy) = rig.yaw.sin_cos();
                dir = Vec3::new(dir.x * cy + dir.z * sy, 0.0, dir.z * cy - dir.x * sy);
                face = Some(Vec3::new(-sy, 0.0, -cy));
            }
            let mut pos = tf.translation;
            if dir != Vec3::ZERO && dt > 0.0 {
                let dir = dir.normalize();
                let speed = run.stats().speed * run.haste_mult();
                let body = body.get_or_insert_with(|| {
                    avian3d::prelude::Collider::capsule(
                        BODY_RADIUS,
                        BODY_HEIGHT - 2.0 * BODY_RADIUS,
                    )
                });
                pos = collide_step(&collision, body, pos, dir * speed, time.delta());
                mv.speed = speed;
                let look = face.unwrap_or(dir);
                mv.flags = gait(look, dir);
                let target = Quat::from_rotation_y(yaw_toward(look));
                tf.rotation = tf.rotation.slerp(target, (dt * 20.0).min(1.0));
            } else if mv.speed != 0.0 || mv.flags != 0 {
                mv.speed = 0.0;
                mv.flags = 0;
            }
            let map = selection.stage_def();
            let center = arena(map);
            let off = flat(pos - center);
            if off.length() > map.leash * 2.0 {
                // Just entered another battleground: start at its centre.
                pos = center;
            } else if off.length() > map.leash {
                let held = center + off.normalize() * map.leash;
                pos.x = held.x;
                pos.z = held.z;
            }
            let ground = world.terrain_height_under(pos).unwrap_or(pos.y);
            let playing = *phase.get() == Phase::Playing && run.dead_t.is_none();
            // Space hops: lift off the terrain, then gravity brings the hero back down.
            if playing && keys.just_pressed(KeyCode::Space) && hm.vy == 0.0 && hm.air == 0.0 {
                hm.vy = JUMP_SPEED;
            }
            if playing && (hm.vy != 0.0 || hm.air > 0.0) {
                hm.vy -= GRAVITY * dt;
                hm.air += hm.vy * dt;
                if hm.air <= 0.0 {
                    hm.air = 0.0;
                    hm.vy = 0.0;
                }
            } else {
                hm.air = 0.0;
                hm.vy = 0.0;
            }
            pos.y = ground + hm.air;
            // Feed the animation driver: `vertical_speed`'s sign and the `FALLING` flag are what
            // select the Jump (37/38) / Fall (40) animation (`creature_anim::select`).
            mv.vertical_speed = hm.vy;
            if hm.vy != 0.0 {
                mv.flags |= move_flags::FALLING;
            }
            if tf.translation != pos {
                tf.translation = pos;
            }
            pos
        }
        Err(_) => {
            let mut p = arena(selection.stage_def());
            if let Some(h) = world.terrain_height_under(p) {
                p.y = h;
            }
            p
        }
    };
    // The terrain streams and the loading screen waits on the ground under this.
    player.active = true;
    player.detached = false;
    player.settling = false;
    player.world_stale = false;
    player.pos = focus;
}

/// Let go of the mouse buttons when the page loses focus or the pointer leaves it. A release that
/// lands elsewhere (alt-tab mid-drag, a browser or OS popup) never reaches the game, so the button
/// would read as held and the unlocked camera would swing with every move until both buttons were
/// clicked again. Survivors only: the engine's mouse-look keeps its buttons across a deactivate,
/// as 1.12.1 does.
pub(super) fn drop_stray_buttons(
    mut focus: MessageReader<bevy::window::WindowFocused>,
    mut left: MessageReader<bevy::window::CursorLeft>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
) {
    let lost = focus.read().any(|f| !f.focused);
    if lost | (left.read().count() > 0) {
        mouse.release_all();
    }
}

/// At the menu a slow orbit the player can take over: a left drag that began over the scene swings
/// and tilts it, the wheel zooms, and the orbit resumes where it was left. In play, a drag with
/// either mouse button swings and tilts it around the hero, as in WoW.
pub(super) fn place_camera(
    time: Res<Time<Real>>,
    phase: Res<State<Phase>>,
    mut rig: ResMut<CamRig>,
    mut wheel: MessageReader<MouseWheel>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    keys: Res<ButtonInput<KeyCode>>,
    player: Res<benilla_app::survivors_host::player::Player>,
    ui: UiCover,
    world: WorldPoint,
    hero: Query<&Transform, (With<Hero>, Without<WorldCamera>)>,
    mut cams: Query<&mut Transform, With<WorldCamera>>,
) {
    // With no hero yet, the focus `drive_hero` grounded.
    let focus = hero.single().map_or(player.pos, |t| t.translation);
    let menu = *phase.get() == Phase::Menu;
    let dt = time.delta_secs();
    // Multiplicative, so a notch moves as far by eye close in as far out; all the way down to an
    // over-the-shoulder view. At the menu it moves the pick screen's own zoom, and not at all
    // while the cursor is over a panel.
    let over_ui = menu && ui.cursor_over_ui();
    for w in wheel.read() {
        let notches = match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y * 0.02,
        };
        let step = (1.0 - 0.12 * notches).clamp(0.5, 1.5);
        if !menu {
            rig.dist = (rig.dist * step).clamp(ZOOM_MIN, ZOOM_MAX);
        } else if !over_ui {
            rig.menu_zoom = (rig.menu_zoom * step).clamp(MENU_ZOOM_MIN, MENU_ZOOM_MAX);
        }
    }
    let (eye, look) = if menu {
        // A drag counts only if the press landed on the scene, and ends with the button.
        if mouse.just_pressed(MouseButton::Left) && !over_ui {
            rig.menu_drag = true;
        }
        if rig.menu_drag && !mouse.pressed(MouseButton::Left) {
            rig.menu_drag = false;
        }
        if rig.menu_drag {
            rig.menu_idle = 0.0;
            rig.orbit -= motion.delta.x * ORBIT_RATE;
            rig.menu_pitch = (rig.menu_pitch + motion.delta.y * ORBIT_RATE)
                .clamp(MENU_PITCH_MIN, MENU_PITCH_MAX);
        } else {
            rig.menu_idle += dt;
            // Ease the idle orbit in, so it does not lurch into motion; the tilt drifts home with it.
            let ease = ((rig.menu_idle - MENU_IDLE_DELAY) / MENU_IDLE_RAMP).clamp(0.0, 1.0);
            rig.orbit += dt * MENU_IDLE_RATE * ease;
            rig.menu_pitch += (MENU_PITCH - rig.menu_pitch) * (1.0 - (-0.8 * ease * dt).exp());
        }
        rig.menu_dist += (rig.menu_zoom - rig.menu_dist) * (1.0 - (-MENU_ZOOM_SMOOTH * dt).exp());
        let (sy, cy) = rig.orbit.sin_cos();
        let (sp, cp) = rig.menu_pitch.sin_cos();
        let look = focus + Vec3::Y * 1.1;
        let mut eye = look + Vec3::new(cp * sy, sp, cp * cy) * rig.menu_dist;
        // Never under the ground: a low tilt on a slope would bury the lens.
        if let Some(h) = world.terrain_height_under(eye) {
            eye.y = eye.y.max(h + 0.6);
        }
        (eye, look)
    } else {
        // Either mouse button drags the camera around the hero, as in WoW.
        if mouse.any_pressed([MouseButton::Left, MouseButton::Right]) {
            rig.yaw -= motion.delta.x * ORBIT_RATE;
            rig.pitch = (rig.pitch + motion.delta.y * ORBIT_RATE).clamp(PITCH_MIN, PITCH_MAX);
        }
        // The left and right arrows turn it, so one hand runs and strafes on WASD while the
        // other turns.
        if *phase.get() == Phase::Playing {
            let turn = f32::from(u8::from(keys.pressed(KeyCode::ArrowLeft)))
                - f32::from(u8::from(keys.pressed(KeyCode::ArrowRight)));
            rig.yaw += turn * TURN_RATE * dt;
        }
        let (sy, cy) = rig.yaw.sin_cos();
        let (sp, cp) = rig.pitch.sin_cos();
        let look = focus + Vec3::Y * 1.4;
        let mut eye = look + Vec3::new(cp * sy, sp, cp * cy) * rig.dist;
        // Never under the ground: a low tilt on a slope would bury the lens.
        if let Some(h) = world.terrain_height_under(eye) {
            eye.y = eye.y.max(h + 0.6);
        }
        (eye, look)
    };
    let placed = Transform::from_translation(eye).looking_at(look, Vec3::Y);
    for mut cam in &mut cams {
        *cam = placed;
    }
}

/// Damage the hero takes from one blow, after immunity, armour and the shield; returns what got
/// through.
pub(super) fn hurt_hero(run: &mut Run, raw: f32) -> f32 {
    if run.immune_t > 0.0 || run.dead_t.is_some() {
        return 0.0;
    }
    let mut dmg = (raw - run.stats().armor).max(1.0);
    if run.shield_t > 0.0 && run.shield > 0.0 {
        let absorbed = dmg.min(run.shield);
        run.shield -= absorbed;
        dmg -= absorbed;
    }
    run.hp -= dmg;
    dmg
}

/// The clock, regeneration, the timed buffs and their visuals, death and victory.
pub(super) fn hero_vitals(
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<Phase>>,
    mut hero: Query<(Entity, &Guid, &mut ObjectStore, &mut Marks), With<Hero>>,
    mut fields: MessageWriter<FieldChanged>,
    mut fx: Fx,
) {
    let dt = time.delta_secs();
    let stats = run.stats();
    let Ok((entity, guid, mut store, mut marks)) = hero.single_mut() else {
        return;
    };
    if let Some(t) = run.dead_t.as_mut() {
        *t += dt;
        if *t > 3.0 {
            next.set(Phase::GameOver);
        }
        return;
    }
    run.elapsed += dt;
    let r = &mut *run;
    for t in [&mut r.shield_t, &mut r.immune_t, &mut r.thorns_t] {
        *t = (*t - dt).max(0.0);
    }
    let mut heal = stats.regen * dt;
    if run.heal_t > 0.0 {
        heal += run.heal_rate * dt.min(run.heal_t);
        run.heal_t = (run.heal_t - dt).max(0.0);
    }
    run.hp = (run.hp + heal).min(stats.max_hp);

    // The buffs show as their own state kits: the shield bubble, the immunity glow, the orbs.
    let mut shown: Vec<u32> = Vec::new();
    if run.shield_t > 0.0 && run.shield > 0.0 {
        shown.push(17);
    }
    if run.immune_t > 0.0 {
        shown.push(642);
    }
    if run.thorns_t > 0.0 {
        shown.push(run.thorns_spell);
    }
    if run.heal_t > 0.0 {
        shown.push(774);
    }
    for owned in &run.abilities {
        if owned.def.key == "lightning_shield" {
            shown.push(owned.def.spell);
        }
    }
    if run.passives.iter().any(|(p, _)| p.key == "renew") {
        shown.push(139);
    }
    marks.set(&shown);
    marks.flush(entity, guid.0, &mut store, &mut fields);

    if run.hp <= 0.0 {
        run.hp = 0.0;
        run.dead_t = Some(0.0);
        units::kill_unit(&mut store);
        marks.clear();
        marks.flush(entity, guid.0, &mut store, &mut fields);
    } else if run.elapsed >= data::VICTORY_SECS {
        run.won = true;
        // A last flourish before the results.
        fx.cast(entity, 642, &[entity], None);
        next.set(Phase::GameOver);
    }
}

/// The movement flags for running `dir` while facing `look`: forward, backpedal or strafe, by
/// which quarter the run falls in, so the walk, backpedal and strafe animations play.
fn gait(look: Vec3, dir: Vec3) -> u32 {
    let ahead = look.dot(dir);
    // Positive when `dir` is to the hero's left: WoW faces -Z with +X on the right.
    let left = look.cross(dir).y;
    let quarter = std::f32::consts::FRAC_1_SQRT_2;
    if ahead >= quarter {
        move_flags::FORWARD
    } else if ahead <= -quarter {
        move_flags::BACKWARD
    } else if left > 0.0 {
        move_flags::STRAFE_LEFT
    } else {
        move_flags::STRAFE_RIGHT
    }
}

/// The ground-plane distance from the hero, the measure every range in the mode uses.
pub(super) fn reach(hero: Vec3, other: Vec3) -> f32 {
    flat(other - hero).length()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::window::{CursorLeft, WindowFocused};

    fn held_left() -> World {
        let mut world = World::new();
        world.init_resource::<Messages<WindowFocused>>();
        world.init_resource::<Messages<CursorLeft>>();
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        world.insert_resource(mouse);
        world
    }

    fn left_held(world: &World) -> bool {
        world.resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left)
    }

    #[test]
    fn a_held_button_is_dropped_when_the_page_loses_focus() {
        let mut world = held_left();
        world.run_system_once(drop_stray_buttons).unwrap();
        assert!(left_held(&world), "nothing happened: the drag goes on");
        world.write_message(WindowFocused { window: Entity::PLACEHOLDER, focused: true });
        world.run_system_once(drop_stray_buttons).unwrap();
        assert!(left_held(&world), "gaining focus is no release");
        world.write_message(WindowFocused { window: Entity::PLACEHOLDER, focused: false });
        world.run_system_once(drop_stray_buttons).unwrap();
        assert!(!left_held(&world));
    }

    #[test]
    fn a_held_button_is_dropped_when_the_pointer_leaves_the_page() {
        let mut world = held_left();
        world.write_message(CursorLeft { window: Entity::PLACEHOLDER });
        world.run_system_once(drop_stray_buttons).unwrap();
        assert!(!left_held(&world));
    }
}
