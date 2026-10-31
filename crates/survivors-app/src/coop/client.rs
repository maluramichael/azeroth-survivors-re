//! The guest's half of a co-op session: it follows the host into its run, builds the other
//! heroes' bodies and sends where its own hero walks. The shape (watching `ClientState`, a body
//! put on the replicated entity, a guest easing toward the host's place) follows deanmathieson's
//! fork (`coop/client.rs`).

use benilla_app::survivors_host::creature_anim::MovementState;
use bevy::prelude::*;
use bevy_replicon::prelude::{ClientState, ClientSystems, Remote};

use super::Session;
use super::smooth;
use super::wire::{EndRun, Hello, Look, MoveInput, NetHero, NetPos, Refused, StartRun, Welcome};
use crate::chatlog::ChatLine;
use crate::hero::Hero;
use crate::profile::Profile;
use crate::units::unit_bundle;
use crate::{NetRole, Phase, Run, RunEntity, Selection, data, hud};

/// How often the guest reports where its hero is, in seconds of wall clock.
const MOVE_EVERY: f32 = 1.0 / 20.0;

/// The first serial of the bodies built here. A run's own serials count up from 1 ([`Run::guid`]),
/// so this keeps the two apart for good.
const BODY_SERIAL_BASE: u64 = 1 << 40;

/// A body this guest built for a hero another machine plays. It lives and dies with the
/// replicated entity it sits on, not with the run.
#[derive(Component)]
pub(super) struct RemoteBody;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (watch_link, greet, receive, step_aside, build_bodies, smooth_bodies)
            .chain()
            .run_if(resource_equals(NetRole::Client)),
    )
    // After the hero's own walk, before replicon packs the messages.
    .add_systems(
        PostUpdate,
        send_moves
            .before(ClientSystems::Send)
            .run_if(resource_equals(NetRole::Client)),
    );
}

/// How far apart the party's heroes start, per player number: every hero is set down at the
/// battleground's centre, and two heroes on one spot look like one.
const ASIDE_YARDS: f32 = 3.0;

/// Where player `player`'s hero starts, relative to the centre the host's hero stands on.
fn aside(player: u8) -> Vec3 {
    Vec3::X * ASIDE_YARDS * f32::from(player)
}

/// Marks this guest's run hero once it has been set down beside the host's.
#[derive(Component)]
struct SteppedAside;

/// The guest's run hero steps aside from the centre once, so it does not stand inside the host's.
fn step_aside(
    mut commands: Commands,
    phase: Res<State<Phase>>,
    session: Res<Session>,
    mut heroes: Query<(Entity, &mut Transform), (With<Hero>, Without<SteppedAside>)>,
) {
    let (Phase::Playing, Some(player)) = (phase.get(), session.player) else {
        return;
    };
    for (e, mut tf) in &mut heroes {
        tf.translation += aside(player);
        commands.entity(e).insert(SteppedAside);
    }
}

/// Leave the session: the link goes, every replicated entity goes, and this machine is a solo
/// player at its menu again.
fn leave(world: &mut World) {
    use bevy_replicon_renet2::netcode::NetcodeClientTransport;
    use bevy_replicon_renet2::renet2::RenetClient;
    if let Some(mut client) = world.remove_resource::<RenetClient>() {
        client.disconnect();
    }
    if let Some(mut transport) = world.remove_resource::<NetcodeClientTransport>() {
        transport.disconnect();
    }
    let mut remote = world.query_filtered::<Entity, With<Remote>>();
    let gone: Vec<Entity> = remote.iter(world).collect();
    for e in gone {
        world.despawn(e);
    }
    *world.resource_mut::<NetRole>() = NetRole::Solo;
    world.resource_mut::<Session>().player = None;
    if *world.resource::<State<Phase>>().get() != Phase::Menu {
        world.resource_mut::<NextState<Phase>>().set(Phase::Menu);
    }
}

/// The link ending: refused, never reached or lost. Said in the chat, then [`leave`].
fn say_and_leave(commands: &mut Commands, chat: &mut MessageWriter<ChatLine>, text: &str) {
    info!("survivors: co-op: {text}");
    chat.write(ChatLine::system(text));
    commands.queue(leave);
}

/// Watches the link: a client that was connecting or connected and is now disconnected has lost
/// the host (or never reached it).
fn watch_link(
    mut commands: Commands,
    state: Res<State<ClientState>>,
    mut seen: Local<(bool, bool)>,
    mut chat: MessageWriter<ChatLine>,
) {
    // The state follows the client resource a frame late: `Disconnected` before the first
    // `Connecting` is no failure.
    match state.get() {
        ClientState::Connecting => seen.0 = true,
        ClientState::Connected => *seen = (true, true),
        ClientState::Disconnected if seen.0 => {
            let text = if seen.1 {
                "Lost the host."
            } else {
                "Could not reach the host."
            };
            *seen = (false, false);
            say_and_leave(&mut commands, &mut chat, text);
        }
        ClientState::Disconnected => {}
    }
}

/// Once connected, and once the menu has a hero picked, say hello: who this player is and what its
/// hero looks like. Sent once per connection.
fn greet(
    state: Res<State<ClientState>>,
    selection: Res<Selection>,
    session: Res<Session>,
    heroes: Query<(), With<Hero>>,
    mut greeted: Local<bool>,
    mut hello: MessageWriter<Hello>,
) {
    if *state.get() != ClientState::Connected {
        *greeted = false;
        return;
    }
    if *greeted || heroes.is_empty() {
        return;
    }
    let (Some(class), Some(&(display, race, sex))) = (
        selection.class_def(),
        selection.look.and_then(|i| selection.looks.get(i)),
    ) else {
        return;
    };
    *greeted = true;
    let name = if session.name.trim().is_empty() {
        class.name.to_string()
    } else {
        session.name.clone()
    };
    hello.write(Hello {
        name,
        class_id: class.id,
        display,
        race,
        sex,
    });
}

/// What the host says: a seat, a refusal, the run beginning or ending.
#[allow(clippy::too_many_arguments)]
fn receive(
    mut commands: Commands,
    mut welcomes: MessageReader<Welcome>,
    mut refusals: MessageReader<Refused>,
    mut starts: MessageReader<StartRun>,
    mut ends: MessageReader<EndRun>,
    mut session: ResMut<Session>,
    mut selection: ResMut<Selection>,
    mut run: ResMut<Run>,
    profile: Res<Profile>,
    phase: Res<State<Phase>>,
    mut next: ResMut<NextState<Phase>>,
    heroes: Query<(), With<Hero>>,
    mut pending: Local<Option<usize>>,
    mut chat: MessageWriter<ChatLine>,
) {
    if let Some(r) = refusals.read().last() {
        say_and_leave(&mut commands, &mut chat, &r.reason);
        return;
    }
    for w in welcomes.read() {
        session.player = Some(w.player);
        info!("survivors: co-op: seated as player {}", w.player);
    }
    for s in starts.read() {
        *pending = Some(s.map);
    }
    if ends.read().last().is_some() {
        *pending = None;
        if *phase.get() != Phase::Menu {
            next.set(Phase::Menu);
        }
    }
    // The run begins as `MenuAction::Enter` begins it, once the menu's world is up.
    let Some(map) = *pending else { return };
    if *phase.get() != Phase::Menu {
        *pending = None;
        return;
    }
    if heroes.is_empty() {
        return;
    }
    *pending = None;
    selection.map = map.min(data::MAPS.len() - 1);
    if hud::start_run(&mut run, &mut selection, &profile) {
        next.set(Phase::Playing);
    } else {
        chat.write(ChatLine::system(
            "The host's run could not start here: pick a class and a zone you have unlocked.",
        ));
    }
}

/// The hero walking, to the host: twenty times a second while a run is on.
fn send_moves(
    time: Res<Time<Real>>,
    phase: Res<State<Phase>>,
    hero: Query<(&Transform, &MovementState), With<Hero>>,
    mut since: Local<f32>,
    mut out: MessageWriter<MoveInput>,
) {
    *since += time.delta_secs();
    if *since < MOVE_EVERY {
        return;
    }
    *since = (*since - MOVE_EVERY).min(MOVE_EVERY);
    // The menu's preview hero stands in another place and is no part of the run.
    if *phase.get() == Phase::Menu {
        return;
    }
    let Ok((tf, mv)) = hero.single() else { return };
    out.write(MoveInput {
        at: tf.translation.to_array(),
        yaw: tf.rotation.to_euler(EulerRot::YXZ).0,
        pace: mv.speed,
        flags: mv.flags,
    });
}

/// Another player's hero arrives: its replicated entity gets a body built from its look. This
/// player's own puppet on the host gets none, and nothing is built before the host has said which
/// seat this player has (the arrival waits) or while the menu is up (there is no run to stand in).
#[allow(clippy::type_complexity)]
fn build_bodies(
    mut commands: Commands,
    session: Res<Session>,
    phase: Res<State<Phase>>,
    fresh: Query<(Entity, &Look, &NetHero, &NetPos), Without<RemoteBody>>,
    mut made: Local<u64>,
) {
    let Some(me) = session.player else { return };
    if *phase.get() == Phase::Menu {
        return;
    }
    for (e, look, hero, pos) in &fresh {
        if hero.player == me {
            continue;
        }
        *made += 1;
        let spec = look.spec(BODY_SERIAL_BASE + *made);
        commands
            .entity(e)
            .insert((unit_bundle(&spec, pos.pos(), pos.yaw()), RemoteBody))
            // The replicated entity belongs to replicon, not to the run: the run's cleanup must
            // not despawn it.
            .remove::<RunEntity>();
    }
}

/// Every frame the bodies ease toward the host's last word, face as it faced and walk the pace it
/// walked. At the menu they are hidden: they stand in the run's zone, not the menu's.
fn smooth_bodies(
    time: Res<Time<Real>>,
    phase: Res<State<Phase>>,
    mut bodies: Query<
        (&NetPos, &mut Transform, &mut MovementState, &mut Visibility),
        With<RemoteBody>,
    >,
) {
    let dt = time.delta_secs();
    let shown = if *phase.get() == Phase::Menu {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for (pos, mut tf, mut mv, mut vis) in &mut bodies {
        tf.translation = smooth::follow(tf.translation, pos.pos(), dt);
        tf.rotation = Quat::from_rotation_y(pos.yaw());
        mv.speed = pos.pace();
        mv.flags = pos.flags;
        if *vis != shown {
            *vis = shown;
        }
    }
}

#[cfg(test)]
mod aside_tests {
    use super::*;

    #[test]
    fn guests_stand_apart_from_the_host() {
        assert_eq!(aside(0), Vec3::ZERO);
        assert!((aside(1).length() - ASIDE_YARDS).abs() < 1e-5);
        assert!((aside(2) - aside(1)).length() >= ASIDE_YARDS - 1e-5);
    }
}
