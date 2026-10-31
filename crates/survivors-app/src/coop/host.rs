//! The host's half of a co-op session: it seats guests, moves their heroes as puppets and
//! replicates every hero. The shape follows deanmathieson's fork (`coop/host.rs`, MIT OR
//! Apache-2.0): replicon ticks on the wall clock, a guest is a connected-client entity, a guest's
//! walk arrives as a message and is applied to a body the host owns.
//!
//! A guest plays its own hero on its machine; here it is a puppet ([`Peer`]), a unit body without
//! the [`Hero`] component, so the systems that expect one hero never see it.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use bevy_replicon::server::server_tick::ServerTick;

use super::wire::{
    EndRun, Hello, Look, MoveInput, NetHero, NetPos, Refused, StartRun, Welcome,
};
use super::{free_slot, Guest, Session};
use crate::chatlog::ChatLine;
use crate::data::{self, ClassDef};
use crate::hero::{self, Hero, Peer};
use benilla_app::survivors_host::creature_anim::MovementState;
use crate::{NetRole, Phase, Run, Selection};

/// How often the host replicates, per second, on the wall clock.
const RATE: f32 = 20.0;
/// How far past the leash a guest's reported position may be before it is dropped, yards.
const SLACK: f32 = 20.0;
/// The fastest walk a guest may report, yards per second (mounts and charges included).
const MAX_PACE: f32 = 50.0;
/// A name is cut to this many characters.
const NAME_LEN: usize = 24;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Pulse>()
        .add_systems(
            PreUpdate,
            (greet, apply_moves)
                .chain()
                .after(ServerSystems::Receive)
                .run_if(hosting),
        )
        .add_systems(Update, (tag_local_hero, farewell).run_if(hosting))
        .add_systems(
            PostUpdate,
            (pulse, mirror.run_if(due), tick.run_if(due))
                .chain()
                .before(ServerSystems::Send)
                .run_if(hosting),
        )
        .add_systems(
            OnTransition {
                exited: Phase::Menu,
                entered: Phase::Playing,
            },
            begin_run.run_if(hosting),
        )
        .add_systems(OnEnter(Phase::Menu), end_run.run_if(hosting));
}

/// Run condition: this machine hosts a party.
fn hosting(role: Res<NetRole>) -> bool {
    *role == NetRole::Host
}

/// Run condition: a replication round is due this frame ([`pulse`]).
fn due(pulse: Res<Pulse>) -> bool {
    pulse.due
}

/// The wall-clock beat of replication: [`RATE`] times a second, `due` is set for one frame.
#[derive(Resource, Default)]
struct Pulse {
    acc: f32,
    due: bool,
}

impl Pulse {
    /// Advances by `dt` seconds; true when a round falls due.
    fn advance(&mut self, dt: f32) -> bool {
        let every = 1.0 / RATE;
        self.acc += dt;
        self.due = self.acc >= every;
        if self.due {
            // After a stall of more than a round the lost rounds are not made up.
            self.acc = if self.acc >= 2.0 * every { 0.0 } else { self.acc - every };
        }
        self.due
    }
}

fn pulse(time: Res<Time<Real>>, mut pulse: ResMut<Pulse>) {
    pulse.advance(time.delta_secs());
}

/// Replication rounds on the wall clock, so a paused run still syncs.
fn tick(mut server_tick: ResMut<ServerTick>) {
    server_tick.increment();
}

/// Whether a guest's report is sane enough to act on: finite numbers, a walk speed a hero can have,
/// and a place near the battleground ([`SLACK`] past the leash, measured on the ground plane).
pub(super) fn valid_move(m: &MoveInput, arena_center: Vec3, leash: f32) -> bool {
    let at = Vec3::from_array(m.at);
    at.is_finite()
        && m.yaw.is_finite()
        && m.pace.is_finite()
        && (0.0..=MAX_PACE).contains(&m.pace)
        && Vec2::new(at.x - arena_center.x, at.z - arena_center.z).length() <= leash + SLACK
}

/// The facing of a body that turns about the vertical only, as the heroes do.
fn yaw_of(rotation: Quat) -> f32 {
    rotation.to_euler(EulerRot::YXZ).0
}

/// A guest's name as the party sees it: trimmed, cut short, the class's when empty.
fn clean_name(name: &str, class: &ClassDef) -> String {
    let name: String = name.trim().chars().take(NAME_LEN).collect();
    if name.is_empty() {
        class.name.to_string()
    } else {
        name
    }
}

/// A new body in the run: replicated with the look, name and place a guest sees it by.
fn spawn_puppet(commands: &mut Commands, run: &mut Run, guest: &Guest, class: &'static ClassDef, at: Vec3) {
    let h = &guest.hello;
    let serial = run.guid();
    let e = hero::spawn_peer(commands, class, h.display, h.race, h.sex, serial, at, guest.player);
    commands.entity(e).insert((
        Replicated,
        Look::from_spec(&hero::hero_spec(class, h.display, h.race, h.sex, serial)),
        NetHero {
            player: guest.player,
            name: h.name.clone(),
            class_id: class.id,
        },
        NetPos::new(at, 0.0, 0.0, 0),
    ));
}

/// Where a guest's puppet appears: beside the host's hero, a couple of yards apart per seat.
fn spot(hero: Option<&Transform>, map: &data::MapDef, player: u8) -> Vec3 {
    let base = hero.map_or_else(|| hero::arena(map), |t| t.translation);
    base + Vec3::X * 2.0 * f32::from(player)
}

fn class_of(id: u8) -> Option<&'static ClassDef> {
    data::CLASSES.iter().find(|c| c.id == id)
}

/// A guest says who it is: it gets a seat, or is told the party is full; a run under way is
/// started for it at once, with its puppet in it.
#[allow(clippy::too_many_arguments)]
fn greet(
    mut commands: Commands,
    mut hellos: MessageReader<FromClient<Hello>>,
    mut session: ResMut<Session>,
    selection: Res<Selection>,
    phase: Res<State<Phase>>,
    mut run: ResMut<Run>,
    hero: Query<&Transform, With<Hero>>,
    mut welcome: MessageWriter<ToClients<Welcome>>,
    mut refuse: MessageWriter<ToClients<Refused>>,
    mut start: MessageWriter<ToClients<StartRun>>,
    mut chat: MessageWriter<ChatLine>,
) {
    for hello in hellos.read() {
        let ClientId::Client(client) = hello.client_id else {
            continue;
        };
        let to = SendTargets::Single(hello.client_id);
        // A hello sent again (until welcomed) gets its welcome again and no second seat.
        if let Some(seated) = session.guests.iter().find(|g| g.client == client) {
            welcome.write(ToClients {
                targets: to,
                message: Welcome {
                    player: seated.player,
                },
            });
            continue;
        }
        let Some(class) = class_of(hello.message.class_id) else {
            refuse.write(ToClients {
                targets: to,
                message: Refused {
                    reason: "Unknown class".into(),
                },
            });
            continue;
        };
        let Some(player) = free_slot(&session.guests) else {
            warn!("survivors: co-op: the party is full; turning a guest away");
            refuse.write(ToClients {
                targets: to,
                message: Refused {
                    reason: "The party is full".into(),
                },
            });
            continue;
        };
        let mut hello = hello.message.clone();
        hello.name = clean_name(&hello.name, class);
        info!("survivors: co-op: {} joined as player {}", hello.name, player);
        chat.write(ChatLine::system(format!("{} joined the party.", hello.name)));
        session.guests.push(Guest {
            client,
            player,
            hello,
        });
        welcome.write(ToClients {
            targets: to,
            message: Welcome { player },
        });
        if matches!(
            phase.get(),
            Phase::Playing | Phase::LevelUp | Phase::Paused
        ) {
            start.write(ToClients {
                targets: to,
                message: StartRun {
                    map: selection.stage,
                },
            });
            let guest = session.guests.last().expect("just pushed");
            let at = spot(hero.single().ok(), selection.stage_def(), player);
            spawn_puppet(&mut commands, &mut run, guest, class, at);
        }
    }
}

/// The host's run began at the menu: every guest enters the same battleground, and gets its puppet.
fn begin_run(
    mut commands: Commands,
    session: Res<Session>,
    selection: Res<Selection>,
    mut run: ResMut<Run>,
    hero: Query<&Transform, With<Hero>>,
    mut start: MessageWriter<ToClients<StartRun>>,
) {
    start.write(ToClients {
        targets: SendTargets::CLIENTS_ONLY,
        message: StartRun {
            map: selection.stage,
        },
    });
    for guest in &session.guests {
        let Some(class) = class_of(guest.hello.class_id) else {
            continue;
        };
        let at = spot(hero.single().ok(), selection.stage_def(), guest.player);
        spawn_puppet(&mut commands, &mut run, guest, class, at);
    }
}

/// The host is back at the menu: so are the guests. The puppets go with the run's entities.
fn end_run(mut end: MessageWriter<ToClients<EndRun>>) {
    end.write(ToClients {
        targets: SendTargets::CLIENTS_ONLY,
        message: EndRun,
    });
}

/// The host's own hero replicates too: its look, who it is and where it stands. Retried each frame
/// until the pick it was built from can be read.
fn tag_local_hero(
    mut commands: Commands,
    selection: Res<Selection>,
    session: Res<Session>,
    fresh: Query<(Entity, &Transform, &MovementState), (With<Hero>, Without<NetHero>)>,
) {
    let (Some(class), Some(&(display, race, sex))) = (
        selection.class_def(),
        selection.look.and_then(|i| selection.looks.get(i)),
    ) else {
        return;
    };
    for (e, tf, mv) in &fresh {
        let name = if session.name.trim().is_empty() {
            class.name.to_string()
        } else {
            session.name.trim().chars().take(NAME_LEN).collect()
        };
        commands.entity(e).try_insert((
            Replicated,
            Look::from_spec(&hero::hero_spec(class, display, race, sex, 0)),
            NetHero {
                player: 0,
                name,
                class_id: class.id,
            },
            NetPos::new(tf.translation, yaw_of(tf.rotation), mv.speed, mv.flags),
        ));
    }
}

/// The host's hero and every puppet write where they stand into the replicated [`NetPos`], when it
/// changed.
#[allow(clippy::type_complexity)]
fn mirror(
    mut bodies: Query<(&Transform, &MovementState, &mut NetPos), Or<(With<Hero>, With<Peer>)>>,
) {
    for (tf, mv, mut pos) in &mut bodies {
        pos.set_if_neq(NetPos::new(
            tf.translation,
            yaw_of(tf.rotation),
            mv.speed,
            mv.flags,
        ));
    }
}

/// A guest's walk moves its puppet, if the report is sane ([`valid_move`]).
fn apply_moves(
    mut moves: MessageReader<FromClient<MoveInput>>,
    session: Res<Session>,
    selection: Res<Selection>,
    mut puppets: Query<(&Peer, &mut Transform, &mut MovementState)>,
) {
    let map = selection.stage_def();
    let centre = hero::arena(map);
    for m in moves.read() {
        let ClientId::Client(client) = m.client_id else {
            continue;
        };
        if !valid_move(&m.message, centre, map.leash) {
            continue;
        }
        let Some(guest) = session.guests.iter().find(|g| g.client == client) else {
            continue;
        };
        for (peer, mut tf, mut mv) in &mut puppets {
            if peer.player != guest.player {
                continue;
            }
            tf.translation = Vec3::from_array(m.message.at);
            tf.rotation = Quat::from_rotation_y(m.message.yaw);
            if mv.speed != m.message.pace || mv.flags != m.message.flags {
                mv.speed = m.message.pace;
                mv.flags = m.message.flags;
            }
        }
    }
}

/// A guest whose connection is gone: its puppet leaves the run and its seat is free again.
fn farewell(
    mut commands: Commands,
    mut session: ResMut<Session>,
    clients: Query<(), With<ConnectedClient>>,
    puppets: Query<(Entity, &Peer)>,
    mut chat: MessageWriter<ChatLine>,
) {
    session.guests.retain(|guest| {
        if clients.contains(guest.client) {
            return true;
        }
        info!("survivors: co-op: {} left", guest.hello.name);
        chat.write(ChatLine::system(format!(
            "{} has left the party.",
            guest.hello.name
        )));
        for (e, peer) in &puppets {
            if peer.player == guest.player {
                commands.entity(e).try_despawn();
            }
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sane() -> MoveInput {
        MoveInput {
            at: [10.0, 5.0, -20.0],
            yaw: 1.5,
            pace: 7.0,
            flags: 1,
        }
    }

    #[test]
    fn bad_moves_are_dropped() {
        let centre = Vec3::new(0.0, 5.0, 0.0);
        let leash = 75.0;
        assert!(valid_move(&sane(), centre, leash));
        let bad = |f: &dyn Fn(&mut MoveInput)| {
            let mut m = sane();
            f(&mut m);
            valid_move(&m, centre, leash)
        };
        assert!(!bad(&|m| m.at[0] = f32::NAN));
        assert!(!bad(&|m| m.at[1] = f32::INFINITY));
        assert!(!bad(&|m| m.at[2] = f32::NEG_INFINITY));
        assert!(!bad(&|m| m.yaw = f32::INFINITY));
        assert!(!bad(&|m| m.yaw = f32::NAN));
        assert!(!bad(&|m| m.pace = f32::NAN));
        assert!(!bad(&|m| m.pace = -1.0));
        assert!(!bad(&|m| m.pace = 50.1));
        assert!(bad(&|m| m.pace = 50.0));
        // Far outside the arena: leash plus the 20 yd of slack.
        assert!(!bad(&|m| m.at = [leash + 20.5, 5.0, 0.0]));
        assert!(!bad(&|m| m.at = [0.0, 5.0, -(leash + 30.0)]));
        assert!(bad(&|m| m.at = [leash + 19.0, 5.0, 0.0]));
    }

    #[test]
    fn replication_runs_twenty_times_a_second() {
        let mut p = Pulse::default();
        let rounds = (0..600).filter(|_| p.advance(1.0 / 60.0)).count();
        // Ten seconds at 60 fps: 200 rounds, give or take the rounding of the last.
        assert!((198..=201).contains(&rounds), "{rounds}");
        // A long stall is one round, not a burst.
        assert!(p.advance(5.0));
        assert!(!p.advance(0.0));
    }

    #[test]
    fn a_guest_who_left_loses_seat_and_puppet() {
        let mut app = App::new();
        app.add_message::<ChatLine>()
            .init_resource::<Session>()
            .add_systems(Update, farewell);
        let here = app.world_mut().spawn(ConnectedClient { max_size: 1200 }).id();
        let gone = app.world_mut().spawn_empty().id();
        app.world_mut().despawn(gone);
        let hello = |name: &str| Hello {
            name: name.into(),
            class_id: 8,
            display: 0,
            race: 1,
            sex: 0,
        };
        let stays = app.world_mut().spawn(Peer { player: 1 }).id();
        let goes = app.world_mut().spawn(Peer { player: 2 }).id();
        app.world_mut().resource_mut::<Session>().guests = vec![
            Guest { client: here, player: 1, hello: hello("Stays") },
            Guest { client: gone, player: 2, hello: hello("Goes") },
        ];
        app.update();
        let session = app.world().resource::<Session>();
        assert_eq!(session.guests.len(), 1);
        assert_eq!(session.guests[0].player, 1);
        assert!(app.world().get_entity(stays).is_ok());
        assert!(app.world().get_entity(goes).is_err());
        assert_eq!(free_slot(&app.world().resource::<Session>().guests), Some(2));
    }
}
