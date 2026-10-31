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
    EndRun, FxBatch, Hello, Look, MoveInput, NetAuras, NetBuild, NetEnemy, NetFx, NetHero,
    NetPos, NetRun, NetVitals, Refused, RunOver, StartRun, Welcome,
};
use crate::enemies::{self, Elite, Enemy};
use crate::units::{AreaLook, FxOut, Marks, ObjLook};
use crate::RunEntity;
use super::{admits, free_slot, Guest, Lobby, Session};
use crate::chatlog::ChatLine;
use crate::data::{self, ClassDef};
use crate::hero::{self, Hero, Peer};
use benilla_app::survivors_host::creature_anim::MovementState;
use crate::{HeroState, NetRole, Owned, Phase, Rng, Run, Selection};

/// How often the host replicates, per second, on the wall clock.
const RATE: f32 = 20.0;
/// How far past the leash a guest's reported position may be before it is dropped, yards.
const SLACK: f32 = 20.0;
/// The fastest walk a guest may report, yards per second (mounts and charges included).
const MAX_PACE: f32 = 50.0;
/// How far above or below the arena's centre a guest may report its hero, in yards.
const MAX_RISE: f32 = 200.0;
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
        .add_systems(Update, (tag_local_hero, farewell, log_traffic).run_if(hosting))
        .add_systems(PreUpdate, count_party)
        .add_systems(
            Update,
            level_puppets
                .after(crate::hero::hero_vitals)
                .run_if(hosting)
                .run_if(in_state(Phase::Playing)),
        )
        .add_systems(
            PostUpdate,
            (
                pulse,
                tag_world,
                keep_run_entity,
                mirror.run_if(due),
                mirror_state.run_if(due),
                send_fx,
                tick.run_if(due),
            )
                .chain()
                .before(ServerSystems::Send)
                .run_if(hosting),
        )
        .add_systems(OnEnter(Phase::GameOver), run_over.run_if(hosting))
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
        && (at.y - arena_center.y).abs() <= MAX_RISE
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
    // The guest plays with the plain numbers of its class for now; its profile's bonuses come
    // with stage three.
    let state = HeroState::new(class, h.race, Default::default(), Default::default());
    commands.entity(e).insert((
        state,
        Replicated,
        Look::from_spec(&hero::hero_spec(class, h.display, h.race, h.sex, serial)),
        NetHero {
            player: guest.player,
            name: h.name.clone(),
            class_id: class.id,
        },
        NetPos::new(at, 0.0, 0.0, 0),
        NetVitals::default(),
        NetBuild::default(),
        NetAuras {
            buffs: true,
            spells: [0; 8],
        },
    ));
}

/// Where a guest's puppet appears until its first move: where the guest's own hero is set down,
/// [`super::ASIDE_YARDS`] per seat from the battleground's centre.
fn spot(_hero: Option<&Transform>, map: &data::MapDef, player: u8) -> Vec3 {
    hero::arena(map) + Vec3::X * super::ASIDE_YARDS * f32::from(player)
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
    puppets: Query<(Entity, &Peer, &Transform)>,
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
        // A seated guest says hello again when its pick changes: the new look replaces the old,
        // and in a run its puppet is built again.
        if let Some(seated) = session.guests.iter_mut().find(|g| g.client == client) {
            let mut hello = hello.message.clone();
            if let Some(class) = class_of(hello.class_id) {
                hello.name = clean_name(&hello.name, class);
                seated.hello = hello;
                let player = seated.player;
                if matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused) {
                    let mut at = spot(None, selection.stage_def(), player);
                    for (e, peer, tf) in &puppets {
                        if peer.player == player {
                            at = tf.translation;
                            commands.entity(e).try_despawn();
                        }
                    }
                    let guest = session.guests.iter().find(|g| g.client == client).expect("seated");
                    spawn_puppet(&mut commands, &mut run, guest, class, at);
                }
            }
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
        if !admits(&session.lobby.password, &hello.message.password) {
            refuse.write(ToClients {
                targets: to,
                message: Refused {
                    reason: "Wrong password".into(),
                },
            });
            continue;
        }
        let Some(player) = free_slot(&session.guests, Lobby::seats(session.lobby.max_players)) else {
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
            NetVitals::default(),
            NetBuild::default(),
            NetAuras {
                buffs: true,
                spells: [0; 8],
            },
        ));
    }
}

/// Whether the run is on (or showing its results): the time the world is the guests' too.
fn in_run(phase: &Phase) -> bool {
    matches!(
        phase,
        Phase::Playing | Phase::LevelUp | Phase::Paused | Phase::GameOver
    )
}

/// The run's world goes to the guests: every enemy, totem, pickup, chest, quest item and ground
/// area is replicated once, from the frame it appears, with where it stands. An enemy also says
/// which creature it is and shows its marks; one whose creature cannot be named goes as a body only.
#[allow(clippy::type_complexity)]
fn tag_world(
    mut commands: Commands,
    phase: Res<State<Phase>>,
    selection: Res<Selection>,
    units: Query<
        (Entity, &Transform, Option<&Enemy>, Has<Elite>, Option<&Marks>),
        (
            With<Look>,
            With<RunEntity>,
            Without<Replicated>,
            Without<Hero>,
            Without<Peer>,
        ),
    >,
    objects: Query<
        (Entity, &Transform),
        (
            Or<(With<ObjLook>, With<AreaLook>)>,
            With<RunEntity>,
            Without<Replicated>,
        ),
    >,
) {
    if !in_run(phase.get()) {
        return;
    }
    for (e, tf, enemy, elite, marks) in &units {
        let mut entity = commands.entity(e);
        entity.insert((Replicated, NetPos::new(tf.translation, yaw_of(tf.rotation), 0.0, 0)));
        if let Some(en) = enemy {
            if let Some(def) = enemies::def_ref(selection.stage, en.def) {
                entity.insert(net_enemy(def, en, elite));
            }
        }
        if let Some(marks) = marks {
            entity.insert(NetAuras {
                buffs: marks.is_buffs(),
                spells: marks.spells(),
            });
        }
    }
    for (e, tf) in &objects {
        commands
            .entity(e)
            .insert((Replicated, NetPos::new(tf.translation, yaw_of(tf.rotation), 0.0, 0)));
    }
}

fn net_enemy(def: (u8, u16), en: &Enemy, elite: bool) -> NetEnemy {
    NetEnemy {
        def,
        hp: en.hp.max(0.0),
        max_hp: en.max_hp,
        boss: en.boss,
        elite,
        dead: en.dead,
    }
}

/// The party's shared numbers ride on an entity of their own, made once per run (the run's cleanup
/// takes it).
fn keep_run_entity(
    mut commands: Commands,
    phase: Res<State<Phase>>,
    present: Query<(), With<NetRun>>,
) {
    if in_run(phase.get()) && present.is_empty() {
        commands.spawn((NetRun::default(), Replicated, RunEntity));
    }
}

/// Every replicated body writes where it stands into its [`NetPos`], when that changed.
fn mirror(mut bodies: Query<(&Transform, Option<&MovementState>, &mut NetPos)>) {
    for (tf, mv, mut pos) in &mut bodies {
        let (speed, flags) = mv.map_or((0.0, 0), |m| (m.speed, m.flags));
        pos.set_if_neq(NetPos::new(tf.translation, yaw_of(tf.rotation), speed, flags));
    }
}

/// A hero's health and gold as the guests see them.
fn vitals_of(hero: &HeroState) -> NetVitals {
    NetVitals {
        hp: hero.hp.max(0.0),
        max_hp: hero.stats().max_hp,
        shield: if hero.shield_t > 0.0 { hero.shield } else { 0.0 },
        dead: hero.dead_t.is_some(),
        gold: hero.gold,
        damage: hero.damage_done.round(),
    }
}

/// A hero's spells and passives as the guests see them.
fn build_of(hero: &HeroState) -> NetBuild {
    NetBuild {
        abilities: hero
            .abilities
            .iter()
            .map(|o| {
                let cd = (o.cd.max(0.0) * 100.0).round().min(f32::from(u16::MAX)) as u16;
                (o.def.key.to_string(), o.level, cd)
            })
            .collect(),
        passives: hero
            .passives
            .iter()
            .map(|(p, level)| (p.key.to_string(), *level))
            .collect(),
    }
}

/// The state behind the bodies: each enemy's health and fall, each unit's marks, each hero's
/// health, gold and build, and the party's clock and level.
#[allow(clippy::type_complexity)]
fn mirror_state(
    run: Res<Run>,
    mut enemies: Query<(&Enemy, &mut NetEnemy)>,
    mut auras: Query<(&Marks, &mut NetAuras)>,
    mut mine: Query<(&mut NetVitals, &mut NetBuild), (With<Hero>, Without<Peer>)>,
    mut puppets: Query<(&HeroState, &mut NetVitals, &mut NetBuild), (With<Peer>, Without<Hero>)>,
    mut shared: Query<&mut NetRun>,
) {
    for (en, mut net) in &mut enemies {
        let now = net_enemy(net.def, en, net.elite);
        net.set_if_neq(now);
    }
    for (marks, mut net) in &mut auras {
        net.set_if_neq(NetAuras {
            buffs: marks.is_buffs(),
            spells: marks.spells(),
        });
    }
    for (mut vitals, mut build) in &mut mine {
        vitals.set_if_neq(vitals_of(&run.me));
        build.set_if_neq(build_of(&run.me));
    }
    for (hero, mut vitals, mut build) in &mut puppets {
        vitals.set_if_neq(vitals_of(hero));
        build.set_if_neq(build_of(hero));
    }
    for mut net in &mut shared {
        net.set_if_neq(NetRun {
            elapsed: run.elapsed,
            kills: run.kills,
            level: run.level,
            xp: run.xp,
            banner: run.party_banner.clone(),
        });
    }
}

/// The frame's effects go to the guests, those naming only bodies they have.
fn send_fx(
    mut out: ResMut<FxOut>,
    replicated: Query<(), With<Replicated>>,
    mut batch: MessageWriter<ToClients<FxBatch>>,
) {
    if out.0.is_empty() {
        return;
    }
    let fx: Vec<NetFx> = out
        .0
        .drain(..)
        .map(|r| NetFx::from_record(&r))
        .filter(|f| f.entities().iter().all(|e| replicated.contains(*e)))
        .collect();
    if !fx.is_empty() {
        batch.write(ToClients {
            targets: SendTargets::CLIENTS_ONLY,
            message: FxBatch(fx),
        });
    }
}

/// Every ten seconds of a run, what each guest's link carries, in the log: the horde's share of
/// the bandwidth is the thing to watch.
fn log_traffic(
    time: Res<Time<Real>>,
    phase: Res<State<Phase>>,
    server: Option<Res<bevy_replicon_renet2::renet2::RenetServer>>,
    replicated: Query<(), With<Replicated>>,
    mut since: Local<f32>,
) {
    *since += time.delta_secs();
    if *since < 10.0 || !in_run(phase.get()) {
        return;
    }
    *since = 0.0;
    let Some(server) = server else { return };
    for id in server.clients_id() {
        if let Ok(net) = server.network_info(id) {
            info!(
                "survivors: co-op: guest {id}: {:.1} KB/s out, {:.1} KB/s in, rtt {:.0} ms, loss {:.1} %, {} entities replicated",
                net.bytes_sent_per_second / 1024.0,
                net.bytes_received_per_second / 1024.0,
                net.rtt * 1000.0,
                net.packet_loss * 100.0,
                replicated.iter().count(),
            );
        }
    }
}

/// The run is over here, so for the guests too.
fn run_over(run: Res<Run>, mut over: MessageWriter<ToClients<RunOver>>) {
    over.write(ToClients {
        targets: SendTargets::CLIENTS_ONLY,
        message: RunOver { won: run.won },
    });
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

/// The heroes in the run, for the horde's size ([`Run::party`]): this one and every puppet.
fn count_party(mut run: ResMut<Run>, puppets: Query<(), With<Peer>>) {
    let n = 1 + puppets.iter().count() as u32;
    if run.party != n {
        run.party = n;
    }
}

/// Each level the party reaches ranks every guest's hero up by one pick of its own, at random:
/// a new spell or passive, or a rank on one it has. Until the guests get cards (stage three).
fn level_puppets(
    run: Res<Run>,
    mut rng: ResMut<Rng>,
    mut puppets: Query<&mut HeroState, With<Peer>>,
    mut seen: Local<u32>,
) {
    if run.level < *seen {
        // A new run.
        *seen = run.level;
    }
    while *seen < run.level {
        *seen += 1;
        if *seen == 1 {
            continue;
        }
        for mut state in &mut puppets {
            if state.dead_t.is_none() {
                rank_up(&mut state, &mut rng);
            }
        }
    }
}

/// One random rank for `hero`: what the level-up cards would offer, without evolutions; nothing
/// when everything is at its top.
pub(super) fn rank_up(hero: &mut HeroState, rng: &mut Rng) {
    enum Pick {
        Up(usize),
        New(&'static data::AbilityDef),
        UpPassive(usize),
        NewPassive(&'static data::PassiveDef),
    }
    let mut pool: Vec<Pick> = Vec::new();
    for (i, o) in hero.abilities.iter().enumerate() {
        if o.level < data::MAX_ABILITY_LEVEL {
            pool.push(Pick::Up(i));
        }
    }
    if hero.abilities.len() < data::MAX_ABILITIES {
        for key in hero.class.pool {
            if !hero.abilities.iter().any(|o| o.def.key == *key) {
                pool.push(Pick::New(data::ability(key)));
            }
        }
    }
    for (i, (_, level)) in hero.passives.iter().enumerate() {
        if *level < data::MAX_PASSIVE_LEVEL {
            pool.push(Pick::UpPassive(i));
        }
    }
    if hero.passives.len() < data::MAX_PASSIVES {
        for p in data::PASSIVES {
            if !hero.passives.iter().any(|(q, _)| q.key == p.key) {
                pool.push(Pick::NewPassive(p));
            }
        }
    }
    if pool.is_empty() {
        return;
    }
    match pool.swap_remove(rng.index(pool.len())) {
        Pick::Up(i) => hero.abilities[i].level += 1,
        Pick::New(def) => hero.abilities.push(Owned { def, level: 1, cd: 0.3 }),
        Pick::UpPassive(i) => hero.passives[i].1 += 1,
        Pick::NewPassive(p) => hero.passives.push((p, 1)),
    }
    hero.version += 1;
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

/// Stop hosting: every guest is let go, their puppets leave, and the host's hero stops
/// replicating (it is tagged again if this machine hosts once more).
pub(super) fn stop(world: &mut World) {
    use bevy_replicon_renet2::netcode::NetcodeServerTransport;
    use bevy_replicon_renet2::renet2::RenetServer;
    let transport = world.remove_resource::<NetcodeServerTransport>();
    if let Some(mut server) = world.remove_resource::<RenetServer>() {
        if let Some(mut transport) = transport {
            transport.disconnect_all(&mut server);
        } else {
            server.disconnect_all();
        }
    }
    let mut puppets = world.query_filtered::<Entity, With<Peer>>();
    let gone: Vec<Entity> = puppets.iter(world).collect();
    for e in gone {
        world.despawn(e);
    }
    let mut tagged = world.query_filtered::<Entity, With<NetHero>>();
    let tagged: Vec<Entity> = tagged.iter(world).collect();
    for e in tagged {
        world
            .entity_mut(e)
            .remove::<(Replicated, NetHero, NetPos, NetVitals, NetBuild, NetAuras)>();
    }
    world.remove_resource::<FxOut>();
    info!("survivors: co-op: stopped hosting");
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
        // A finite but absurd height would saturate the replicated centimetres.
        assert!(!bad(&|m| m.at = [0.0, 1.0e7, 0.0]));
        assert!(!bad(&|m| m.at = [0.0, -400.0, 0.0]));
        assert!(bad(&|m| m.at = [0.0, 60.0, 0.0]));
    }

    #[test]
    fn a_puppet_ranks_up_until_everything_is_at_the_top() {
        let class = &data::CLASSES[0];
        let mut hero = HeroState::new(class, 1, Default::default(), Default::default());
        let mut rng = Rng::default();
        for _ in 0..500 {
            rank_up(&mut hero, &mut rng);
        }
        assert!(hero.abilities.len() <= data::MAX_ABILITIES);
        assert!(hero.passives.len() <= data::MAX_PASSIVES);
        assert!(hero.abilities.iter().all(|o| o.level == data::MAX_ABILITY_LEVEL));
        assert!(hero.passives.iter().all(|(_, l)| *l == data::MAX_PASSIVE_LEVEL));
        let before = hero.version;
        rank_up(&mut hero, &mut rng);
        assert_eq!(hero.version, before, "nothing left to rank");
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
            password: String::new(),
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
        assert_eq!(free_slot(&app.world().resource::<Session>().guests, 3), Some(2));
    }
}
