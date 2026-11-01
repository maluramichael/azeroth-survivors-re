//! Frame budget on large screens: a frame-rate cap and a render scale, set in the options
//! (ticket #1152).
//!
//! Three independent parts, all read off [`Prefs`] and the engine's `renderScale` CVar:
//!
//! - The cap ([`limit_frame`], native only): a limiter in `Last` that holds the frame period.
//!   It sleeps most of the wait and spins the last [`SPIN`], and it measures from the previous
//!   limiter point ([`Pacer`]), not from the frame's start, so the cost of the frame itself,
//!   the render thread's sync included, is part of the period and the rate holds. It sits on top
//!   of vsync or without it (`WOW_NOVSYNC`); under vsync a cap above the refresh rate changes
//!   nothing and one below it rides on the vblanks. `WOW_FPS_CAP=<n|off>` overrides the saved
//!   cap for one session and is never saved.
//! - The render scale: the engine already renders the world at `window x renderScale` into an
//!   offscreen target the UI camera samples bilinearly, with the UI at native resolution
//!   (`benilla_app`'s `world_backdrop`). The options only write that CVar, whose registry saves
//!   `config.toml`. The ceiling the player picks is [`SCALES`]' index into it.
//! - The dynamic mode ([`Controller`], [`dynamic_scale`]): while a run is on, it steps the
//!   live scale down when frames run long and tries one step up again after a calm stretch,
//!   never above the player's own scale. It writes the engine's `RenderScale` resource directly,
//!   so the CVar (and the file) keep the player's choice.
//!
//! The FPS readout ([`show_fps`]) is a corner label the options toggle ([`Prefs::show_fps`]).

use std::time::Duration;

use bevy::prelude::*;

use super::kit::{self, KitFonts};
use super::prefs::Prefs;
use super::Phase;
use benilla_app::survivors_host::cvars::Cvars;
use benilla_app::survivors_host::world_backdrop::RenderScale;

/// The caps the slider offers, ascending, `0` (no cap) last so the slider reads left to right as
/// "slower to unlimited".
pub(crate) const CAPS: [u32; 7] = [30, 60, 90, 120, 144, 240, 0];

/// The render scales the slider offers, ascending. The lowest is the dynamic mode's floor.
pub(crate) const SCALES: [f32; 5] = [0.5, 0.67, 0.75, 0.85, 1.0];

/// The engine's CVar the scale slider writes; its registry saves it with `config.toml`.
pub(crate) const SCALE_CVAR: &str = "renderScale";

/// The index in [`CAPS`] a saved cap lands on: the exact value, or else the nearest cap (with `0`,
/// no cap, only for `0`), so a hand-edited file still shows a sensible stop.
pub(crate) fn cap_index(cap: u32) -> usize {
    if cap == 0 {
        return CAPS.len() - 1;
    }
    CAPS[..CAPS.len() - 1]
        .iter()
        .enumerate()
        .min_by_key(|(_, c)| c.abs_diff(cap))
        .map_or(0, |(i, _)| i)
}

/// The index in [`SCALES`] nearest `scale`.
pub(crate) fn scale_index(scale: f32) -> usize {
    SCALES
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - scale).abs().total_cmp(&(*b - scale).abs()))
        .map_or(SCALES.len() - 1, |(i, _)| i)
}

/// The caption of a cap: "Off" or the rate.
pub(crate) fn cap_label(cap: u32) -> String {
    if cap == 0 {
        "Off".to_string()
    } else {
        cap.to_string()
    }
}

/// The cap in force: the session's `WOW_FPS_CAP` when set, else the saved one. `0` is no cap. A
/// value outside 10..=1000 is a typo, not a rate, and reads as no cap.
pub(crate) fn effective_cap(saved: u32, session: Option<u32>) -> u32 {
    let cap = session.unwrap_or(saved);
    if (10..=1000).contains(&cap) {
        cap
    } else {
        0
    }
}

/// `WOW_FPS_CAP=<n|off>`: `Some(0)` for `off`/`0`, `Some(n)` for a number, `None` when unset or
/// unreadable.
fn parse_session_cap(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("off") {
        return Some(0);
    }
    value.parse().ok()
}

// --- The cap. ---

/// How long before the deadline the limiter stops sleeping and spins: an OS sleep overshoots by
/// a fraction of a millisecond or more, a spin does not.
const SPIN: Duration = Duration::from_millis(1);

/// How late a frame may land after its deadline and still stay on the grid (the wake-up's own
/// overshoot). A later frame was really long and starts a new grid at once, so a hitch is not
/// followed by a burst of frames that catch up.
const GRID_SLACK: Duration = Duration::from_millis(2);

/// The limiter's clock as pure math: times are offsets from some fixed origin.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Pacer {
    /// The previous limiter point: the deadline it waited for, or when it ran if it did not wait.
    anchor: Option<Duration>,
}

impl Pacer {
    /// Called once a frame at `now`. `Some(deadline)` is the time to wait until before the frame
    /// may go on; `None` means no wait (the first call, or the frame is already late). The next
    /// frame's deadline hangs off this call's, so a sleep that overshoots shortens the next one.
    pub(crate) fn plan(&mut self, now: Duration, period: Duration) -> Option<Duration> {
        let Some(anchor) = self.anchor else {
            self.anchor = Some(now);
            return None;
        };
        let deadline = anchor + period;
        if now >= deadline {
            // Late. A hair late stays on the grid; a real hitch re-anchors on `now`.
            self.anchor = Some(if now - deadline <= GRID_SLACK {
                deadline
            } else {
                now
            });
            None
        } else {
            self.anchor = Some(deadline);
            Some(deadline)
        }
    }

    /// Forget the grid (the cap changed or was lifted).
    pub(crate) fn reset(&mut self) {
        self.anchor = None;
    }
}

/// The limiter's state: its clock origin, the grid, and the cap it was set up for.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
struct FrameLimiter {
    origin: std::time::Instant,
    pacer: Pacer,
    /// The cap the grid belongs to; a change starts a new grid.
    cap: u32,
    /// `WOW_FPS_CAP`, read once.
    session: Option<u32>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for FrameLimiter {
    fn default() -> Self {
        Self {
            origin: std::time::Instant::now(),
            pacer: Pacer::default(),
            cap: 0,
            session: std::env::var("WOW_FPS_CAP")
                .ok()
                .and_then(|v| parse_session_cap(&v)),
        }
    }
}

/// Hold the frame period of the cap, at the very end of the main world's frame.
#[cfg(not(target_arch = "wasm32"))]
fn limit_frame(prefs: Res<Prefs>, mut limiter: ResMut<FrameLimiter>) {
    let cap = effective_cap(prefs.frame_cap, limiter.session);
    if cap != limiter.cap {
        limiter.cap = cap;
        limiter.pacer.reset();
    }
    if cap == 0 {
        return;
    }
    let period = Duration::from_secs_f64(1.0 / f64::from(cap));
    let origin = limiter.origin;
    let Some(deadline) = limiter.pacer.plan(origin.elapsed(), period) else {
        return;
    };
    loop {
        let now = origin.elapsed();
        let Some(left) = deadline.checked_sub(now).filter(|d| !d.is_zero()) else {
            break;
        };
        match left.checked_sub(SPIN).filter(|d| !d.is_zero()) {
            Some(nap) => std::thread::sleep(nap),
            None => std::hint::spin_loop(),
        }
    }
}

// --- The dynamic render scale. ---

/// One window of frame times is judged at a time.
const WINDOW_S: f32 = 1.0;
/// A window slower than the target by this factor is a miss: the scale steps down.
const SLOW: f32 = 1.10;
/// A window within this factor of the target is fine and counts towards a step up.
const FINE: f32 = 1.03;
/// How many fine windows in a row before the scale is tried one step higher.
const CALM_WINDOWS: u32 = 4;
/// The wait before another step up after a success, and its doubling after a failed one.
const BASE_BACKOFF_S: f32 = 8.0;
const MAX_BACKOFF_S: f32 = 64.0;

/// The dynamic scale's decision: an index into [`SCALES`], moved by the frame times it is fed.
/// Pure, so its rhythm is tested without a renderer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Controller {
    pub(crate) idx: usize,
    sum: f32,
    frames: u32,
    elapsed: f32,
    calm: u32,
    /// Seconds until a step up is allowed again.
    cooldown: f32,
    /// How long the next cooldown is; it doubles each time a step up fails at once.
    backoff: f32,
    /// The last change was a step up that has not yet survived a window.
    probing: bool,
}

impl Controller {
    pub(crate) fn new(idx: usize) -> Self {
        Self {
            idx,
            sum: 0.0,
            frames: 0,
            elapsed: 0.0,
            calm: 0,
            cooldown: BASE_BACKOFF_S,
            backoff: BASE_BACKOFF_S,
            probing: false,
        }
    }

    /// One frame of `dt` seconds against a `target` frame time, with the scale index the player
    /// allows at most. `Some(index)` when the scale should change to it.
    pub(crate) fn feed(&mut self, dt: f32, target: f32, ceiling: usize) -> Option<usize> {
        let before = self.idx;
        self.idx = self.idx.min(ceiling);
        self.cooldown = (self.cooldown - dt).max(0.0);
        // A streaming hitch is not the scene's cost: no frame counts for more than two targets.
        self.sum += dt.min(2.0 * target);
        self.frames += 1;
        self.elapsed += dt;
        if self.elapsed >= WINDOW_S {
            let avg = self.sum / self.frames as f32;
            (self.sum, self.frames, self.elapsed) = (0.0, 0, 0.0);
            if avg > target * SLOW {
                let steps = if avg > target * 1.6 { 2 } else { 1 };
                if self.probing {
                    // The step up did not hold: wait longer before the next try.
                    self.backoff = (self.backoff * 2.0).min(MAX_BACKOFF_S);
                }
                self.probing = false;
                self.calm = 0;
                self.cooldown = self.backoff;
                self.idx = self.idx.saturating_sub(steps);
            } else if avg <= target * FINE {
                if self.probing {
                    self.probing = false;
                    self.backoff = BASE_BACKOFF_S;
                }
                self.calm += 1;
                if self.calm >= CALM_WINDOWS && self.idx < ceiling && self.cooldown <= 0.0 {
                    self.idx += 1;
                    self.probing = true;
                    self.calm = 0;
                }
            } else {
                // Neither a miss nor headroom.
                self.probing = false;
                self.calm = 0;
            }
        }
        (self.idx != before).then_some(self.idx)
    }
}

/// The scale a step means: the player's own CVar value at the ceiling (it may sit between
/// stops), a stop below it.
fn scale_at(idx: usize, ceiling: usize, chosen: f32) -> f32 {
    if idx >= ceiling {
        chosen
    } else {
        SCALES[idx.min(SCALES.len() - 1)]
    }
}

/// The dynamic mode's live state.
#[derive(Resource, Default)]
struct DynamicScale {
    /// `Some` while the mode is on.
    ctl: Option<Controller>,
}

/// The frame time the dynamic mode aims for: the cap, or 60 fps with none.
fn target_seconds(cap: u32) -> f32 {
    1.0 / if cap == 0 { 60.0 } else { cap as f32 }
}

/// Step the world's render scale with the frame times while a run is on.
fn dynamic_scale(
    time: Res<Time<Real>>,
    phase: Res<State<Phase>>,
    prefs: Res<Prefs>,
    cvars: Res<Cvars>,
    #[cfg(not(target_arch = "wasm32"))] limiter: Res<FrameLimiter>,
    mut scale: ResMut<RenderScale>,
    mut dynamic: ResMut<DynamicScale>,
) {
    let chosen = cvars
        .num(SCALE_CVAR)
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(1.0);
    let ceiling = scale_index(chosen);
    if !prefs.dynamic_scale {
        if dynamic.ctl.take().is_some() && scale.0 != chosen {
            // Back to what the player chose.
            scale.0 = chosen;
        }
        return;
    }
    let ctl = dynamic.ctl.get_or_insert_with(|| Controller::new(ceiling));
    if !matches!(phase.get(), Phase::Playing) {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let cap = effective_cap(prefs.frame_cap, limiter.session);
    #[cfg(target_arch = "wasm32")]
    let cap = 0;
    if let Some(idx) = ctl.feed(time.delta_secs(), target_seconds(cap), ceiling) {
        let want = scale_at(idx, ceiling, chosen);
        if scale.0 != want {
            info!(
                "survivors: dynamic render scale {:.2} -> {want:.2}",
                scale.0
            );
            scale.0 = want;
        }
    }
}

// --- The FPS readout. ---

/// The corner label.
#[derive(Component)]
struct FpsText;

/// How often the label is rewritten.
const READOUT_S: f32 = 0.25;

/// Keep the corner label: spawn it once the fonts are in, show it while [`Prefs::show_fps`] is
/// on, and rewrite it four times a second with the mean rate and frame time of that stretch.
fn show_fps(
    mut commands: Commands,
    time: Res<Time<Real>>,
    prefs: Res<Prefs>,
    scale: Res<RenderScale>,
    fonts: Option<Res<KitFonts>>,
    mut label: Query<(&mut Text, &mut Node), With<FpsText>>,
    mut spawned: Local<bool>,
    mut acc: Local<(f32, u32)>,
) {
    let Ok((mut text, mut node)) = label.single_mut() else {
        if let (false, Some(fonts)) = (*spawned, fonts) {
            *spawned = true;
            commands.spawn((
                FpsText,
                kit::body(&fonts, "", 13.0, kit::CREAM),
                Node {
                    // The bottom-left corner under the chat: the top-left holds the
                    // player frame.
                    position_type: PositionType::Absolute,
                    left: Val::Px(8.0),
                    bottom: Val::Px(2.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                GlobalZIndex(402),
                bevy::ui::FocusPolicy::Pass,
            ));
        }
        return;
    };
    let display = if prefs.show_fps {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    if !prefs.show_fps {
        *acc = (0.0, 0);
        return;
    }
    acc.0 += time.delta_secs();
    acc.1 += 1;
    if acc.0 < READOUT_S {
        return;
    }
    let ms = acc.0 / acc.1 as f32 * 1000.0;
    let want = format!(
        "{:.0} fps  {ms:.1} ms  world {:.0}%",
        1000.0 / ms.max(0.001),
        scale.0 * 100.0
    );
    *acc = (0.0, 0);
    if text.0 != want {
        text.0 = want;
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<DynamicScale>()
        .add_systems(Update, (show_fps, dynamic_scale));
    #[cfg(not(target_arch = "wasm32"))]
    app.init_resource::<FrameLimiter>()
        .add_systems(Last, limit_frame);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Run `frames` frames whose own work costs `work` each through the pacer, waiting as told,
    /// and return the time the last one ended at.
    fn run(pacer: &mut Pacer, frames: u32, work: Duration, period: Duration) -> Duration {
        let mut now = Duration::ZERO;
        for _ in 0..frames {
            now += work;
            if let Some(deadline) = pacer.plan(now, period) {
                now = deadline;
            }
        }
        now
    }

    #[test]
    fn the_first_frame_never_waits() {
        let mut p = Pacer::default();
        assert_eq!(p.plan(ms(5), ms(16)), None);
    }

    #[test]
    fn quick_frames_are_held_to_the_period() {
        let mut p = Pacer::default();
        // 600 frames of 3 ms work at 60 fps (a 16 ms-ish period of 1/60 s).
        let period = Duration::from_secs_f64(1.0 / 60.0);
        let end = run(&mut p, 600, ms(3), period);
        let fps = 599.0 / end.as_secs_f64();
        assert!(
            (fps - 60.0).abs() < 0.2,
            "the cap holds the rate, measured {fps}"
        );
    }

    #[test]
    fn a_sleep_that_overshoots_is_paid_back_by_the_next() {
        let mut p = Pacer::default();
        let period = ms(10);
        assert_eq!(p.plan(ms(0), period), None);
        // The frame wakes 1 ms past the deadline of 10: still on the grid.
        assert_eq!(p.plan(ms(11), period), None);
        // So the next deadline is 20, not 21.
        assert_eq!(p.plan(ms(12), period), Some(ms(20)));
    }

    #[test]
    fn a_slow_frame_is_not_chased_by_a_burst() {
        let mut p = Pacer::default();
        let period = ms(10);
        assert_eq!(p.plan(ms(0), period), None);
        // A 50 ms hitch: no wait, and the grid starts over at 50.
        assert_eq!(p.plan(ms(50), period), None);
        assert_eq!(p.plan(ms(52), period), Some(ms(60)));
    }

    #[test]
    fn a_frame_slower_than_the_period_is_left_alone() {
        let mut p = Pacer::default();
        let period = ms(10);
        let end = run(&mut p, 100, ms(25), period);
        assert_eq!(end, ms(2500), "no wait is ever added to a slow frame");
    }

    #[test]
    fn a_reset_forgets_the_grid() {
        let mut p = Pacer::default();
        p.plan(ms(0), ms(10));
        p.reset();
        assert_eq!(p.plan(ms(3), ms(10)), None);
    }

    #[test]
    fn caps_land_on_their_stop() {
        for (i, cap) in CAPS.iter().enumerate() {
            assert_eq!(cap_index(*cap), i);
        }
        assert_eq!(
            cap_index(100),
            2,
            "a hand-edited 100 is nearest the 90 stop"
        );
        assert_eq!(cap_index(10_000), 5, "and a huge one is not 'off'");
        assert_eq!(CAPS[CAPS.len() - 1], 0, "'off' is the last stop");
        assert_eq!(cap_label(0), "Off");
        assert_eq!(cap_label(144), "144");
    }

    #[test]
    fn a_session_cap_beats_the_saved_one() {
        assert_eq!(effective_cap(60, None), 60);
        assert_eq!(effective_cap(60, Some(144)), 144);
        assert_eq!(effective_cap(60, Some(0)), 0, "WOW_FPS_CAP=off lifts it");
        assert_eq!(effective_cap(3, None), 0, "a typo is no cap");
        assert_eq!(parse_session_cap("Off"), Some(0));
        assert_eq!(parse_session_cap(" 90 "), Some(90));
        assert_eq!(parse_session_cap("fast"), None);
    }

    #[test]
    fn scales_are_ascending_and_land_on_their_stop() {
        assert!(SCALES.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(*SCALES.last().unwrap(), 1.0);
        for (i, s) in SCALES.iter().enumerate() {
            assert_eq!(scale_index(*s), i);
        }
        assert_eq!(scale_index(2.0), SCALES.len() - 1);
        assert_eq!(scale_index(0.1), 0);
        assert_eq!(scale_index(0.7), 1);
    }

    /// Feed a steady `frame` for `seconds`, returning the last change.
    fn feed_for(
        c: &mut Controller,
        frame: f32,
        seconds: f32,
        target: f32,
        ceil: usize,
    ) -> Option<usize> {
        let mut last = None;
        let mut t = 0.0;
        while t < seconds {
            if let Some(i) = c.feed(frame, target, ceil) {
                last = Some(i);
            }
            t += frame;
        }
        last
    }

    const T: f32 = 1.0 / 60.0;
    const TOP: usize = 4;

    #[test]
    fn slow_frames_step_the_scale_down() {
        let mut c = Controller::new(TOP);
        let changed = feed_for(&mut c, 0.020, 1.2, T, TOP);
        assert_eq!(changed, Some(TOP - 1), "20 ms against 16.7 is a miss");
        let changed = feed_for(&mut c, 0.040, 1.2, T, TOP);
        assert_eq!(c.idx, TOP - 3, "40 ms is far over and drops two stops");
        assert!(changed.is_some());
    }

    #[test]
    fn the_scale_never_drops_below_the_first_stop() {
        let mut c = Controller::new(1);
        feed_for(&mut c, 0.050, 10.0, T, TOP);
        assert_eq!(c.idx, 0);
    }

    #[test]
    fn a_hitch_does_not_count_as_a_slow_scene() {
        let mut c = Controller::new(TOP);
        // 59 good frames and one 300 ms streaming stall in a second.
        for _ in 0..59 {
            assert_eq!(c.feed(0.0155, T, TOP), None);
        }
        assert_eq!(
            c.feed(0.300, T, TOP),
            None,
            "one hitch is clamped, not a miss"
        );
    }

    #[test]
    fn calm_frames_climb_back_after_the_cooldown_but_never_past_the_ceiling() {
        let mut c = Controller::new(TOP - 2);
        feed_for(&mut c, T, 30.0, T, TOP);
        assert_eq!(c.idx, TOP, "a calm half minute climbs to the ceiling");
        feed_for(&mut c, T, 30.0, T, TOP);
        assert_eq!(c.idx, TOP, "and stops there");
    }

    #[test]
    fn a_failed_step_up_waits_longer_before_the_next_try() {
        let mut c = Controller::new(2);
        // Calm until a step up happens, then it misses at once.
        let mut raised_at = None;
        let mut t = 0.0;
        while raised_at.is_none() {
            if c.feed(T, T, TOP) == Some(3) {
                raised_at = Some(t);
            }
            t += T;
            assert!(t < 60.0, "a step up must come");
        }
        feed_for(&mut c, 0.025, 1.2, T, TOP);
        assert_eq!(c.idx, 2, "back down");
        assert_eq!(c.backoff, BASE_BACKOFF_S * 2.0, "and the wait doubled");
        // The next calm stretch shorter than the doubled wait does not climb.
        feed_for(&mut c, T, BASE_BACKOFF_S * 1.5, T, TOP);
        assert_eq!(c.idx, 2);
    }

    #[test]
    fn a_lowered_ceiling_pulls_the_scale_down() {
        let mut c = Controller::new(TOP);
        assert_eq!(c.feed(T, T, 1), Some(1));
        assert_eq!(c.idx, 1);
    }

    #[test]
    fn the_ceiling_keeps_the_players_own_value() {
        assert_eq!(scale_at(TOP, TOP, 1.0), 1.0);
        assert_eq!(scale_at(2, TOP, 1.0), 0.75);
        assert_eq!(
            scale_at(1, 1, 0.7),
            0.7,
            "a value between stops survives at the top"
        );
    }

    #[test]
    fn the_dynamic_target_is_the_cap_or_sixty() {
        assert!((target_seconds(0) - 1.0 / 60.0).abs() < 1e-6);
        assert!((target_seconds(144) - 1.0 / 144.0).abs() < 1e-6);
    }
}
