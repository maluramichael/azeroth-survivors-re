//! The chat window: status lines during a run, as WoW's chat frame prints them (experience,
//! gold, level ups, new spells and talents, bosses, the run's payout). Bottom left, newest line at
//! the bottom, a capped history; the lines fade after a while of quiet and the wheel scrolls back
//! while the cursor is over it.
//!
//! Survivors skips the FrameXML UI (`CaptureMode`, see `mod.rs`), so the engine's own chat frame
//! is not live here; this is a plain Bevy UI window. Other modules write [`ChatLine`] messages
//! and this one renders them. Experience and gold come in by the hundred a minute, so they are
//! summed and printed as one line per [`AGG_SECS`]; everything else is a line at once.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use super::hud::{self, HudFont};
use super::{Phase, Run};

/// Lines kept; older ones fall off the top.
const MAX_LINES: usize = 100;
/// Lines on screen.
const VISIBLE: usize = 8;
/// Seconds experience and gold are summed before they print.
const AGG_SECS: f32 = 4.0;
/// Seconds without a new line or the cursor before the window starts to fade, and how long the
/// fade takes.
const FADE_AFTER: f32 = 10.0;
const FADE_SECS: f32 = 3.0;
/// Lines one wheel notch moves.
const NOTCH: usize = 2;

const WIDTH: f32 = 400.0;
const LINE_H: f32 = 16.0;
const PAD: f32 = 6.0;
const FONT_SIZE: f32 = 13.0;
/// Over the HUD (400), under the character window (405) and the level-up cards (410 and up).
const Z: i32 = 402;
const FILL_ALPHA: f32 = 0.35;

/// Whether the cursor is over the window, for whoever else reads the wheel (the camera zoom).
static HOVERED: AtomicBool = AtomicBool::new(false);

#[allow(dead_code)]
pub(super) fn hovered() -> bool {
    HOVERED.load(Ordering::Relaxed)
}

/// What a line is, which sets its colour: the 1.12 client's `ChatTypeInfo` defaults
/// (`ui_chat::event::default_color`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChatKind {
    /// `SYSTEM`, yellow: level ups, new spells and talents, the payout.
    System,
    /// `COMBAT_XP_GAIN`, the blue-violet of "You gain 120 experience."
    Xp,
    /// `MONEY`, yellow.
    Money,
    /// `SKILL`: a rank gained.
    Skill,
    /// `RAID_BOSS_EMOTE`: a boss appears.
    Boss,
    /// Plain white: deaths.
    Combat,
}

impl ChatKind {
    fn color(self) -> Color {
        let (r, g, b) = match self {
            ChatKind::System | ChatKind::Money => (255, 255, 0),
            ChatKind::Xp => (111, 111, 255),
            ChatKind::Skill => (85, 85, 255),
            ChatKind::Boss => (255, 219, 183),
            ChatKind::Combat => (255, 255, 255),
        };
        Color::srgb_u8(r, g, b)
    }
}

/// What the rest of the mode tells the window. Experience and gold are amounts the window sums
/// into one line per window of seconds; text prints as it comes.
#[derive(Message, Clone, Debug, PartialEq)]
pub(super) enum ChatLine {
    /// Experience gained; may be fractional, the window carries the remainder.
    Xp(f32),
    /// Gold looted.
    Gold(u32),
    Text(ChatKind, String),
}

impl ChatLine {
    pub fn system(text: impl Into<String>) -> Self {
        ChatLine::Text(ChatKind::System, text.into())
    }
    pub fn text(kind: ChatKind, text: impl Into<String>) -> Self {
        ChatLine::Text(kind, text.into())
    }
}

#[derive(Resource)]
pub(super) struct ChatLog {
    lines: VecDeque<(ChatKind, String)>,
    /// Experience and gold waiting to print, and seconds since the first of them came.
    xp: f32,
    gold: u32,
    wait: f32,
    /// Seconds since a new line, the cursor or the wheel.
    quiet: f32,
    /// Lines scrolled back from the newest.
    back: usize,
    /// The window needs its rows rewritten.
    dirty: bool,
    shown_alpha: f32,
    greeted: bool,
    /// The abilities and passives the picks were last compared against: key and rank.
    seen_abilities: Vec<(&'static str, u8)>,
    seen_passives: Vec<(&'static str, u8)>,
    root: Option<Entity>,
    /// The row texts, top to bottom.
    rows: Vec<Entity>,
}

impl Default for ChatLog {
    fn default() -> Self {
        Self {
            lines: VecDeque::new(),
            xp: 0.0,
            gold: 0,
            wait: 0.0,
            quiet: 0.0,
            back: 0,
            dirty: true,
            shown_alpha: -1.0,
            greeted: false,
            seen_abilities: Vec::new(),
            seen_passives: Vec::new(),
            root: None,
            rows: Vec::new(),
        }
    }
}

impl ChatLog {
    fn pending(&self) -> bool {
        self.xp >= 0.5 || self.gold > 0
    }

    fn add_xp(&mut self, xp: f32) {
        if !self.pending() {
            self.wait = 0.0;
        }
        self.xp += xp;
    }

    fn add_gold(&mut self, gold: u32) {
        if !self.pending() {
            self.wait = 0.0;
        }
        self.gold += gold;
    }

    /// A line of its own. What was summed so far prints first, so the order holds.
    fn push(&mut self, kind: ChatKind, text: String) {
        self.flush();
        self.append(kind, text);
    }

    fn append(&mut self, kind: ChatKind, text: String) {
        self.lines.push_back((kind, text));
        if self.lines.len() > MAX_LINES {
            self.lines.pop_front();
        }
        // A window scrolled back keeps its place while lines arrive below it.
        if self.back > 0 {
            self.back = (self.back + 1).min(self.max_back());
        }
        self.quiet = 0.0;
        self.dirty = true;
    }

    /// Print the summed experience and gold, one line each.
    fn flush(&mut self) {
        let xp = self.xp.round() as u32;
        if xp > 0 {
            self.xp -= xp as f32;
            self.append(
                ChatKind::Xp,
                format!("You gain {} experience.", hud::grouped(u64::from(xp))),
            );
        }
        if self.gold > 0 {
            let gold = std::mem::take(&mut self.gold);
            self.append(
                ChatKind::Money,
                format!("You loot {} gold.", hud::grouped(u64::from(gold))),
            );
        }
        self.wait = 0.0;
    }

    fn tick(&mut self, dt: f32) {
        self.quiet += dt;
        if self.pending() {
            self.wait += dt;
            if self.wait >= AGG_SECS {
                self.flush();
            }
        }
    }

    fn max_back(&self) -> usize {
        self.lines.len().saturating_sub(VISIBLE)
    }

    /// Wheel up (positive) goes back in the history.
    fn scroll(&mut self, notches: i32) {
        let by = notches.unsigned_abs() as usize * NOTCH;
        self.back = if notches > 0 {
            (self.back + by).min(self.max_back())
        } else {
            self.back.saturating_sub(by)
        };
        self.quiet = 0.0;
        self.dirty = true;
    }

    /// The lines on screen, oldest first.
    fn visible(&self) -> impl Iterator<Item = &(ChatKind, String)> {
        let end = self.lines.len() - self.back.min(self.lines.len());
        self.lines.range(end.saturating_sub(VISIBLE)..end)
    }

    /// How opaque the window is: full while it is busy or scrolled back, then a fade to nothing.
    fn alpha(&self) -> f32 {
        if self.back > 0 || self.quiet < FADE_AFTER {
            1.0
        } else {
            (1.0 - (self.quiet - FADE_AFTER) / FADE_SECS).clamp(0.0, 1.0)
        }
    }

    /// A new run starts clean.
    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Component)]
struct ChatRoot;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ChatLog>()
        .add_message::<ChatLine>()
        .add_systems(Update, collect)
        .add_systems(Update, (watch_picks, show).chain().run_if(in_run))
        .add_systems(OnEnter(Phase::Menu), teardown);
}

/// The phases that put the HUD on screen, and the results, where the payout prints.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(
        phase.get(),
        Phase::Playing | Phase::LevelUp | Phase::Paused | Phase::GameOver
    )
}

/// Take in what the rest of the mode wrote and run the sum's clock. Real time, so the sum prints
/// under the level-up cards too.
fn collect(time: Res<Time<Real>>, mut lines: MessageReader<ChatLine>, mut log: ResMut<ChatLog>) {
    for line in lines.read() {
        match line {
            ChatLine::Xp(xp) => log.add_xp(*xp),
            ChatLine::Gold(gold) => log.add_gold(*gold),
            ChatLine::Text(kind, text) => log.push(*kind, text.clone()),
        }
    }
    log.tick(time.delta_secs());
}

/// Back at the menu: the window goes and the next run builds its own, with an empty history.
fn teardown(mut commands: Commands, mut log: ResMut<ChatLog>) {
    if let Some(root) = log.root {
        commands.entity(root).try_despawn();
    }
    log.clear();
    HOVERED.store(false, Ordering::Relaxed);
}

/// The picks on the level-up cards apply in `hud::pick`; the window finds them by comparing the
/// run's abilities and passives with what it saw last.
fn watch_picks(
    run: Res<Run>,
    spells: Option<Res<crate::ui_action::Spells>>,
    mut log: ResMut<ChatLog>,
) {
    let same = log.seen_abilities.len() == run.abilities.len()
        && log.seen_passives.len() == run.passives.len()
        && log
            .seen_abilities
            .iter()
            .zip(&run.abilities)
            .all(|(s, o)| *s == (o.def.key, o.level))
        && log
            .seen_passives
            .iter()
            .zip(&run.passives)
            .all(|(s, (p, level))| *s == (p.key, *level));
    if same {
        return;
    }
    let mut out: Vec<(ChatKind, String)> = Vec::new();
    for o in &run.abilities {
        let name = hud::spell_name(spells.as_deref(), o.def.spell, o.def.key);
        match log.seen_abilities.iter().find(|(k, _)| *k == o.def.key) {
            None => out.push((
                ChatKind::System,
                format!("You have learned a new spell: {name}."),
            )),
            Some(&(_, level)) if o.level > level => out.push((
                ChatKind::Skill,
                format!("{name} has reached rank {}.", o.level),
            )),
            Some(_) => {}
        }
    }
    for (p, level) in &run.passives {
        match log.seen_passives.iter().find(|(k, _)| *k == p.key) {
            None => out.push((
                ChatKind::System,
                format!("You have learned a new talent: {}.", p.name),
            )),
            Some(&(_, seen)) if *level > seen => out.push((
                ChatKind::Skill,
                format!("{} has reached rank {level}.", p.name),
            )),
            Some(_) => {}
        }
    }
    log.seen_abilities = run.abilities.iter().map(|o| (o.def.key, o.level)).collect();
    log.seen_passives = run.passives.iter().map(|(p, l)| (p.key, *l)).collect();
    for (kind, text) in out {
        log.push(kind, text);
    }
}

/// The window's frame and its pool of rows, bottom left.
fn build(commands: &mut Commands, log: &mut ChatLog, font: &Handle<Font>) {
    let mut rows = Vec::with_capacity(VISIBLE);
    let root = commands
        .spawn((
            ChatRoot,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(20.0),
                width: Val::Px(WIDTH),
                height: Val::Px(LINE_H * VISIBLE as f32 + PAD * 2.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::FlexEnd,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(PAD)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, FILL_ALPHA)),
            GlobalZIndex(Z),
        ))
        .with_children(|p| {
            for _ in 0..VISIBLE {
                rows.push(
                    p.spawn((
                        Text::new(""),
                        TextFont {
                            font: font.clone(),
                            font_size: FONT_SIZE,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        TextLayout::new_with_no_wrap(),
                        TextShadow {
                            offset: Vec2::splat(1.0),
                            color: Color::BLACK,
                        },
                        Node {
                            height: Val::Px(LINE_H),
                            ..default()
                        },
                    ))
                    .id(),
                );
            }
        })
        .id();
    log.root = Some(root);
    log.rows = rows;
    log.dirty = true;
    log.shown_alpha = -1.0;
}

/// Build the window on the first frame of a run, then keep its rows, fade and scroll current.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    mut log: ResMut<ChatLog>,
    mut wheel: MessageReader<MouseWheel>,
    font: Option<Res<HudFont>>,
    roots: Query<&Interaction, With<ChatRoot>>,
    mut fills: Query<&mut BackgroundColor, With<ChatRoot>>,
    mut rows: Query<(&mut Text, &mut TextColor, &mut TextShadow)>,
) {
    let log = &mut *log;
    let Some(root) = log.root else {
        if let Some(font) = font {
            build(&mut commands, log, &font.0);
        }
        return;
    };
    if !log.greeted {
        log.greeted = true;
        log.push(ChatKind::System, "Welcome to Warcraft Survivors.".into());
    }
    let over = roots
        .get(root)
        .is_ok_and(|i| matches!(i, Interaction::Hovered | Interaction::Pressed));
    HOVERED.store(over, Ordering::Relaxed);
    if over {
        log.quiet = 0.0;
    }
    for w in wheel.read() {
        if over && w.y != 0.0 {
            log.scroll(if w.y > 0.0 { 1 } else { -1 });
        }
    }

    let alpha = log.alpha();
    if !log.dirty && (alpha - log.shown_alpha).abs() < 0.004 {
        return;
    }
    log.dirty = false;
    log.shown_alpha = alpha;
    if let Ok(mut bg) = fills.get_mut(root) {
        bg.0 = Color::srgba(0.0, 0.0, 0.0, FILL_ALPHA * alpha);
    }
    // The newest line sits on the last row; rows above the history stay empty.
    let shown: Vec<&(ChatKind, String)> = log.visible().collect();
    let first = log.rows.len().saturating_sub(shown.len());
    for (i, &row) in log.rows.iter().enumerate() {
        let Ok((mut text, mut color, mut shadow)) = rows.get_mut(row) else {
            continue;
        };
        let (kind, line) = match i.checked_sub(first).and_then(|j| shown.get(j)) {
            Some((kind, line)) => (*kind, line.as_str()),
            None => (ChatKind::System, ""),
        };
        if text.0 != line {
            text.0 = line.to_string();
        }
        color.0 = kind.color().with_alpha(alpha);
        shadow.color = Color::BLACK.with_alpha(alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(log: &ChatLog) -> Vec<String> {
        log.lines.iter().map(|(_, t)| t.clone()).collect()
    }

    #[test]
    fn kills_sum_into_one_line_per_window() {
        let mut log = ChatLog::default();
        for _ in 0..100 {
            log.add_xp(1.2);
            log.add_gold(1);
            log.tick(0.01);
        }
        assert!(log.lines.is_empty(), "nothing prints inside the window");
        log.tick(AGG_SECS);
        assert_eq!(
            texts(&log),
            ["You gain 120 experience.", "You loot 100 gold."]
        );
        log.tick(AGG_SECS * 2.0);
        assert_eq!(log.lines.len(), 2, "nothing pending, nothing printed");
    }

    #[test]
    fn a_line_of_its_own_prints_the_sum_before_it() {
        let mut log = ChatLog::default();
        log.add_xp(40.0);
        log.push(ChatKind::System, "You have reached level 2.".into());
        assert_eq!(
            texts(&log),
            ["You gain 40 experience.", "You have reached level 2."]
        );
        assert!(!log.pending());
    }

    #[test]
    fn fractions_of_experience_carry_over() {
        let mut log = ChatLog::default();
        log.add_xp(0.4);
        log.tick(AGG_SECS * 2.0);
        assert!(log.lines.is_empty(), "under half a point prints nothing");
        log.add_xp(0.4);
        log.tick(AGG_SECS);
        assert_eq!(texts(&log), ["You gain 1 experience."]);
    }

    #[test]
    fn the_history_is_capped() {
        let mut log = ChatLog::default();
        for i in 0..MAX_LINES + 20 {
            log.push(ChatKind::System, i.to_string());
        }
        assert_eq!(log.lines.len(), MAX_LINES);
        assert_eq!(log.lines.front().unwrap().1, "20");
    }

    #[test]
    fn scrolling_stays_inside_the_history_and_holds_its_place() {
        let mut log = ChatLog::default();
        for i in 0..12 {
            log.push(ChatKind::System, i.to_string());
        }
        let top = |log: &ChatLog| log.visible().last().unwrap().1.clone();
        assert_eq!(top(&log), "11");
        log.scroll(1);
        assert_eq!(top(&log), "9");
        log.scroll(10);
        assert_eq!(log.back, 4, "no further than the first screenful");
        assert_eq!(log.visible().count(), VISIBLE);
        log.push(ChatKind::System, "12".into());
        assert_eq!(top(&log), "7", "a scrolled window does not jump");
        log.scroll(-10);
        assert_eq!(top(&log), "12");
    }

    #[test]
    fn the_window_fades_after_quiet_and_returns_with_a_line() {
        let mut log = ChatLog::default();
        log.push(ChatKind::System, "hello".into());
        assert_eq!(log.alpha(), 1.0);
        log.tick(FADE_AFTER + FADE_SECS / 2.0);
        assert!((log.alpha() - 0.5).abs() < 1e-4);
        log.tick(FADE_SECS);
        assert_eq!(log.alpha(), 0.0);
        log.push(ChatKind::System, "again".into());
        assert_eq!(log.alpha(), 1.0);
    }
}
