//! The guest's half of a co-op session: it follows the host into its run, builds the other
//! heroes' bodies and sends where its own hero walks. The shape (watching `ClientState`, a body
//! put on the replicated entity, a guest easing toward the host's place) follows deanmathieson's
//! fork (`coop/client.rs`).

use benilla_app::survivors_host::creature_anim::MovementState;
use bevy::prelude::*;
use bevy_replicon::prelude::{ClientState, ClientSystems, Remote};

use super::{Session, ASIDE_YARDS};
use super::smooth;
use super::wire::{
    CardAction, EndRun, FxBatch, NetPicked, Hello, Look, MoveInput, NetAuras, NetBuild, NetEnemy, NetFx, NetHero, NetPos,
    NetRun, NetVitals, Refused, RunOver, StartRun, Welcome,
};
use crate::chatlog::ChatLine;
use crate::enemies::{self, Elite, Enemy};
use crate::hero::Hero;
use crate::profile::Profile;
use crate::units::{self, area_bundle, object_bundle, unit_bundle, AreaLook, Fx, Marks, ObjLook};
use crate::{NetRole, Owned, Phase, Run, RunEntity, Selection, data, hud};
use benilla_app::survivors_host::net::{FieldChanged, Guid, ObjectStore};

/// How often the guest reports where its hero is, in seconds of wall clock.
const MOVE_EVERY: f32 = 1.0 / 20.0;

/// The first serial of the bodies built here. A run's own serials count up from 1 ([`Run::guid`]),
/// so this keeps the two apart for good.
const BODY_SERIAL_BASE: u64 = 1 << 40;

/// A body this guest built for a hero another machine plays. It lives and dies with the
/// replicated entity it sits on, not with the run.
#[derive(Component)]
pub(super) struct RemoteBody;

/// This guest's own hero as the host keeps it: it gets no body here; what the host says of it is
/// this machine's hero's ([`sync_me`]), and its effects play on this machine's hero.
#[derive(Component)]
pub(super) struct OwnPuppet;

/// A replicated body whose death has been played.
#[derive(Component)]
struct Killed;

/// When this guest last sent a level-up pick (wall-clock seconds): until the host's word on the
/// hero's build has had time to come back, an older word does not undo the pick here.
#[derive(Resource, Default)]
struct RecentPick(f64);

/// Seconds a pick here outranks the host's word on the build.
const PICK_GRACE: f64 = 0.6;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<RecentPick>().add_systems(
        Update,
        (
            watch_link,
            greet,
            receive,
            step_aside,
            build_bodies,
            build_objects,
            smooth_bodies,
            sync_enemies,
            sync_auras,
            sync_me,
            sync_run,
            play_fx,
        )
            .chain()
            .run_if(resource_equals(NetRole::Client)),
    )
    // After the hero's own walk, before replicon packs the messages.
    .add_systems(
        OnEnter(Phase::Menu),
        left_on_own.run_if(resource_equals(NetRole::Client)),
    )
    .add_systems(
        PostUpdate,
        (send_moves, send_picks)
            .before(ClientSystems::Send)
            .run_if(resource_equals(NetRole::Client)),
    );
}

/// Where player `player`'s hero starts, relative to the centre the host's hero stands on.
fn aside(player: u8) -> Vec3 {
    Vec3::X * ASIDE_YARDS * f32::from(player)
}

/// Back at the menu without the host's word (the pause menu's Main Menu): this guest left the
/// host's run, and with it the party, since the host does not start a run twice.
fn left_on_own(mut commands: Commands, mut session: ResMut<Session>, mut chat: MessageWriter<ChatLine>) {
    if std::mem::take(&mut session.host_sent_menu) || session.link != super::Link::Joined {
        return;
    }
    say_and_leave(&mut commands, &mut chat, "You left the host's run, and with it the party.");
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
pub(super) fn leave(world: &mut World) {
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
    world.resource_mut::<hud::Offer>().log = None;
    if *world.resource::<State<Phase>>().get() != Phase::Menu {
        world.resource_mut::<NextState<Phase>>().set(Phase::Menu);
    }
}

/// The link ending: refused, never reached or lost. Said in the chat and on the co-op window's
/// status line (the chat is not drawn at the menu), then [`leave`].
fn say_and_leave(commands: &mut Commands, chat: &mut MessageWriter<ChatLine>, text: &str) {
    info!("survivors: co-op: {text}");
    chat.write(ChatLine::system(text));
    let status = text.to_string();
    commands.queue(move |world: &mut World| {
        world.resource_mut::<Session>().status = status;
        leave(world);
    });
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
                "Could not reach the host. Check the address, and that the host's firewall lets UDP port 37373 in."
            };
            *seen = (false, false);
            say_and_leave(&mut commands, &mut chat, text);
        }
        ClientState::Disconnected => {}
    }
}

/// Once connected, and once the menu has a hero picked, say hello: who this player is and what its
/// hero looks like. Said again whenever the pick changes, so the host shows the hero this guest
/// plays.
fn greet(
    state: Res<State<ClientState>>,
    selection: Res<Selection>,
    session: Res<Session>,
    heroes: Query<(), With<Hero>>,
    mut said: Local<Option<Hello>>,
    mut hello: MessageWriter<Hello>,
) {
    if *state.get() != ClientState::Connected {
        *said = None;
        return;
    }
    if heroes.is_empty() {
        return;
    }
    let (Some(class), Some(&(display, race, sex))) = (
        selection.class_def(),
        selection.look.and_then(|i| selection.looks.get(i)),
    ) else {
        return;
    };
    let name = if session.name.trim().is_empty() {
        class.name.to_string()
    } else {
        session.name.clone()
    };
    let now = Hello {
        name,
        class_id: class.id,
        display,
        race,
        sex,
        password: session.join_password.clone(),
    };
    if said.as_ref() != Some(&now) {
        hello.write(now.clone());
        *said = Some(now);
    }
}

/// What the host says: a seat, a refusal, the run beginning or ending.
#[allow(clippy::too_many_arguments)]
fn receive(
    mut commands: Commands,
    mut welcomes: MessageReader<Welcome>,
    mut refusals: MessageReader<Refused>,
    mut starts: MessageReader<StartRun>,
    mut ends: MessageReader<EndRun>,
    mut overs: MessageReader<RunOver>,
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
        session.link = super::Link::Joined;
        session.status = format!(
            "In the party as player {}. The run starts when the host starts it.",
            w.player
        );
        info!("survivors: co-op: seated as player {}", w.player);
    }
    // The end first: a run ended and a new one begun within one tick keeps the new one.
    if ends.read().last().is_some() {
        *pending = None;
        if *phase.get() != Phase::Menu {
            session.host_sent_menu = true;
            next.set(Phase::Menu);
        }
    }
    // The host's run is over: this guest's ends with it, and shows its results.
    if let Some(over) = overs.read().last() {
        if matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused) {
            run.won = over.won;
            next.set(Phase::GameOver);
            // Leaving the results for the menu keeps the party: the host's next run follows.
            session.host_sent_menu = true;
        }
    }
    for s in starts.read() {
        *pending = Some(s.map);
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
        let text = "The host's run could not start here: pick a class and a zone you have unlocked.";
        session.status = text.to_string();
        chat.write(ChatLine::system(text));
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
        picking: *phase.get() == Phase::LevelUp,
    });
}

/// This guest's level-up picks and skips, to the host, which applies them to the guest's hero
/// there. The picker keeps a log of them for as long as this machine is a guest.
fn send_picks(
    time: Res<Time<Real>>,
    mut offer: ResMut<hud::Offer>,
    mut recent: ResMut<RecentPick>,
    mut out: MessageWriter<CardAction>,
) {
    // Without touching the change ticks: the cards are dealt again whenever the offer changes.
    let offer = offer.bypass_change_detection();
    let log = offer.log.get_or_insert_with(Vec::new);
    if log.is_empty() {
        return;
    }
    for p in log.drain(..) {
        out.write(CardAction(NetPicked::from_picked(p)));
    }
    recent.0 = time.elapsed_secs_f64();
}

/// The serials of the bodies built here, counting up from [`BODY_SERIAL_BASE`].
#[derive(Resource, Default)]
struct Serials(u64);

impl Serials {
    fn next(&mut self) -> u64 {
        self.0 += 1;
        BODY_SERIAL_BASE + self.0
    }
}

/// A unit arrives (another player's hero, an enemy, a totem): its replicated entity gets a body
/// built from its look, with aura slots when the host shows marks on it. This player's own puppet
/// on the host gets none, and nothing is built before the host has said which seat this player
/// has (the arrival waits) or while the menu is up (there is no run to stand in).
#[allow(clippy::type_complexity)]
fn build_bodies(
    mut commands: Commands,
    session: Res<Session>,
    phase: Res<State<Phase>>,
    fresh: Query<
        (Entity, &Look, Option<&NetHero>, &NetPos, Option<&NetAuras>),
        (With<Remote>, Without<RemoteBody>, Without<OwnPuppet>),
    >,
    mut serials: Local<Serials>,
) {
    let Some(me) = session.player else { return };
    if *phase.get() == Phase::Menu {
        return;
    }
    for (e, look, hero, pos, auras) in &fresh {
        if hero.is_some_and(|h| h.player == me) {
            commands.entity(e).insert(OwnPuppet);
            continue;
        }
        let spec = look.spec(serials.next());
        let mut entity = commands.entity(e);
        entity
            .insert((unit_bundle(&spec, pos.pos(), pos.yaw()), RemoteBody))
            // The replicated entity belongs to replicon, not to the run: the run's cleanup must
            // not despawn it.
            .remove::<RunEntity>();
        if let Some(auras) = auras {
            entity.insert(if auras.buffs { Marks::buffs() } else { Marks::debuffs() });
        }
    }
}

/// A pickup, chest, quest item or ground spell arrives: it gets its object, as the host built it.
#[allow(clippy::type_complexity)]
fn build_objects(
    mut commands: Commands,
    phase: Res<State<Phase>>,
    objects: Query<(Entity, &ObjLook, &NetPos), (With<Remote>, Without<RemoteBody>)>,
    areas: Query<(Entity, &AreaLook, &NetPos), (With<Remote>, Without<RemoteBody>)>,
    mut serials: Local<Serials>,
) {
    if *phase.get() == Phase::Menu {
        return;
    }
    for (e, look, pos) in &objects {
        commands
            .entity(e)
            .insert((object_bundle(serials.next(), look.display, look.scale, pos.pos()), RemoteBody))
            .remove::<RunEntity>();
    }
    for (e, look, pos) in &areas {
        commands
            .entity(e)
            .insert((area_bundle(serials.next(), look.spell, look.radius, pos.pos()), RemoteBody))
            .remove::<RunEntity>();
    }
}

/// An enemy's body knows what it is and how it stands, for the plates, frames and minimap, and
/// plays its death once when the host says it fell.
#[allow(clippy::type_complexity)]
fn sync_enemies(
    mut commands: Commands,
    selection: Res<Selection>,
    mut bodies: Query<
        (Entity, Ref<NetEnemy>, Option<&mut Enemy>, &mut ObjectStore, Has<Killed>),
        With<RemoteBody>,
    >,
) {
    for (e, net, enemy, mut store, killed) in &mut bodies {
        match enemy {
            None => {
                if let Some(def) = enemies::def_at(selection.stage, net.def) {
                    let mut entity = commands.entity(e);
                    entity.insert(enemies::shown_enemy(
                        def, net.hp, net.max_hp, net.boss, net.elite, net.dead,
                    ));
                    if net.elite {
                        entity.insert(Elite);
                    }
                }
            }
            Some(mut en) if net.is_changed() => {
                en.hp = net.hp;
                en.max_hp = net.max_hp;
                en.dead = net.dead;
            }
            Some(_) => {}
        }
        if net.dead && !killed {
            units::kill_unit(&mut store);
            commands.entity(e).insert(Killed);
        }
    }
}

/// The marks the host shows on a unit show here too; those on this guest's puppet show on this
/// machine's hero.
#[allow(clippy::type_complexity)]
fn sync_auras(
    mut bodies: Query<
        (Entity, &Guid, Ref<NetAuras>, &mut Marks, &mut ObjectStore),
        (With<RemoteBody>, Without<Hero>),
    >,
    puppet: Query<Ref<NetAuras>, (With<OwnPuppet>, Without<RemoteBody>)>,
    mut hero: Query<(Entity, &Guid, &mut Marks, &mut ObjectStore), (With<Hero>, Without<RemoteBody>)>,
    run: Res<Run>,
    mut fields: MessageWriter<FieldChanged>,
) {
    for (e, guid, net, mut marks, mut store) in &mut bodies {
        if net.is_changed() || marks.is_added() {
            marks.set(&net.spells);
            marks.flush(e, guid.0, &mut store, &mut fields);
        }
    }
    let (Ok(net), Ok((e, guid, mut marks, mut store))) = (puppet.single(), hero.single_mut()) else {
        return;
    };
    if net.is_changed() && run.dead_t.is_none() {
        marks.set(&net.spells);
        marks.flush(e, guid.0, &mut store, &mut fields);
    }
}

/// What the host says of this guest's hero is this machine's hero: its health, shield, gold and
/// fall, and its spells, ranks and cooldowns. A guest plays with its class's plain numbers for
/// now, so the profile's bonuses are taken off here as they are on the host.
#[allow(clippy::type_complexity)]
fn sync_me(
    mut run: ResMut<Run>,
    phase: Res<State<Phase>>,
    time: Res<Time<Real>>,
    recent: Res<RecentPick>,
    puppet: Query<(&NetVitals, &NetBuild), With<OwnPuppet>>,
    mut hero: Query<(Entity, &Guid, &mut ObjectStore, &mut Marks), (With<Hero>, Without<OwnPuppet>)>,
    mut fields: MessageWriter<FieldChanged>,
    mut seen: Local<Option<(NetVitals, NetBuild)>>,
) {
    if *phase.get() == Phase::Menu {
        *seen = None;
        return;
    }
    let Ok((vitals, build)) = puppet.single() else {
        return;
    };
    // Compared by value, not by change ticks: the host's word may have come while this waited
    // at the menu, and the run's start set this machine's hero to its own numbers since.
    let fresh_vitals = seen.as_ref().is_none_or(|(v, _)| v != vitals);
    let fresh_build = seen.as_ref().is_none_or(|(_, b)| b != build);
    if fresh_vitals || fresh_build {
        *seen = Some((*vitals, build.clone()));
    }
    let me = &mut run.me;
    me.meta = Default::default();
    me.talents = Default::default();
    me.gear = Default::default();
    me.buffs = Default::default();
    if fresh_vitals {
        me.hp = vitals.hp;
        me.shield = vitals.shield;
        me.shield_t = if vitals.shield > 0.0 { 1.0 } else { 0.0 };
        me.gold = vitals.gold;
        me.damage_done = vitals.damage;
        if vitals.dead && me.dead_t.is_none() {
            me.hp = 0.0;
            me.dead_t = Some(0.0);
            if let Ok((e, guid, mut store, mut marks)) = hero.single_mut() {
                units::kill_unit(&mut store);
                marks.clear();
                marks.flush(e, guid.0, &mut store, &mut fields);
            }
        }
    }
    // Right after a pick here the host's build may still be the one from before it.
    let settled = time.elapsed_secs_f64() - recent.0 > PICK_GRACE;
    if fresh_build && (settled || same_lists(me, build)) {
        apply_build(me, build);
    } else if fresh_build {
        // Looked at again next frame.
        *seen = None;
    }
}

/// `build` into `hero`: the ranks and cooldowns, and the lists themselves when a spell or passive
/// came or went (the HUD rebuilds its icon rows then).
fn apply_build(hero: &mut crate::HeroState, build: &NetBuild) {
    if same_lists(hero, build) {
        for (o, (_, _, cd)) in hero.abilities.iter_mut().zip(&build.abilities) {
            o.cd = f32::from(*cd) / 100.0;
        }
        return;
    }
    hero.abilities = build
        .abilities
        .iter()
        .filter_map(|(key, level, cd)| {
            let def = data::ABILITIES.iter().find(|a| a.key == key)?;
            Some(Owned {
                def,
                level: *level,
                cd: f32::from(*cd) / 100.0,
            })
        })
        .collect();
    hero.passives = build
        .passives
        .iter()
        .filter_map(|(key, level)| {
            data::PASSIVES
                .iter()
                .find(|p| p.key == key)
                .map(|p| (p, *level))
        })
        .collect();
    hero.version += 1;
}

/// Whether `hero` has exactly the spells, passives and ranks `build` names.
fn same_lists(hero: &crate::HeroState, build: &NetBuild) -> bool {
    hero.abilities.len() == build.abilities.len()
        && hero
            .abilities
            .iter()
            .zip(&build.abilities)
            .all(|(o, (key, level, _))| o.def.key == key && o.level == *level)
        && hero.passives.len() == build.passives.len()
        && hero
            .passives
            .iter()
            .zip(&build.passives)
            .all(|((p, l), (key, level))| p.key == key && l == level)
}

/// The party's clock, kills, level and experience are the host's; a new banner shows here too.
fn sync_run(
    mut run: ResMut<Run>,
    phase: Res<State<Phase>>,
    mut next: ResMut<NextState<Phase>>,
    shared: Query<Ref<NetRun>>,
    mut shown: Local<Option<u32>>,
) {
    if *phase.get() == Phase::Menu {
        *shown = None;
        return;
    }
    let Ok(net) = shared.single() else { return };
    if !net.is_changed() {
        return;
    }
    let was = run.level;
    run.elapsed = net.elapsed;
    run.kills = net.kills;
    run.level = net.level;
    run.xp = net.xp;
    // The first word only marks where the numbering stands: a banner from before this guest
    // came in is old news.
    let (seq, text) = &net.banner;
    if shown.is_some_and(|s| s != *seq) && !text.is_empty() {
        run.banner = Some((text.clone(), 3.5));
    }
    *shown = Some(*seq);
    // Each level the party reaches is a hand of cards for this hero too; the world goes on
    // behind them.
    if run.level > was {
        run.pending_levels += run.level - was;
        if *phase.get() == Phase::Playing {
            next.set(Phase::LevelUp);
        }
    }
    // The party's level teaches this hero its mobility, as it does the host's.
    if was < crate::mobility::LEARN_LEVEL && run.level >= crate::mobility::LEARN_LEVEL {
        if let Some(m) = crate::mobility::for_class(run.class.id) {
            run.banner = Some((format!("You have learned {}! Press F.", m.name), 4.0));
        }
    }
}

/// The host's effects play here as they played there: casts and their missiles, impacts, swings
/// and numbers. One on this guest's puppet plays on this machine's hero.
fn play_fx(
    mut batches: MessageReader<FxBatch>,
    phase: Res<State<Phase>>,
    puppet: Query<Entity, With<OwnPuppet>>,
    hero: Query<Entity, With<Hero>>,
    bodies: Query<(), With<Guid>>,
    mut fx: Fx,
) {
    if *phase.get() == Phase::Menu {
        batches.clear();
        return;
    }
    let own = puppet.single().ok();
    let me = hero.single().ok();
    for batch in batches.read() {
        for f in &batch.0 {
            let mut f = f.clone();
            f.map(|e| if Some(e) == own { me.unwrap_or(e) } else { e });
            // A body still being built (or never built) cannot show it.
            if !f.entities().iter().all(|e| bodies.contains(*e)) {
                continue;
            }
            match f {
                NetFx::Cast {
                    caster,
                    spell,
                    hits,
                    dest,
                } => fx.cast(caster, spell, &hits, dest.map(Vec3::from_array)),
                NetFx::Impact { target, spell } => fx.impact(target, spell),
                NetFx::Swing {
                    attacker,
                    victim,
                    damage,
                } => fx.swing(attacker, victim, damage),
                NetFx::Number {
                    anchor,
                    amount,
                    crit,
                    color,
                } => fx.number(anchor, amount, crit, color),
            }
        }
    }
}

/// Every frame the bodies ease toward the host's last word, face as it faced and walk the pace it
/// walked. At the menu they are hidden: they stand in the run's zone, not the menu's.
fn smooth_bodies(
    time: Res<Time<Real>>,
    phase: Res<State<Phase>>,
    mut bodies: Query<
        (&NetPos, &mut Transform, Option<&mut MovementState>, &mut Visibility),
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
        if let Some(mv) = mv.as_deref_mut() {
            if mv.speed != pos.pace() || mv.flags != pos.flags {
                mv.speed = pos.pace();
                mv.flags = pos.flags;
            }
        }
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
