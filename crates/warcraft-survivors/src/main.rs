//! The `warcraft-survivors` launcher: the command line, the install check, then the build stamp
//! handed to the survivors mode.

use std::path::PathBuf;

use benilla_app::{AppExit, BuildId};
use clap::Parser;

/// Warcraft Survivors: a top-down survival mode on the benilla WoW 1.12.1 client.
///
/// Each flag sets the environment variable the client already reads, so `WOW_DATA` and `WOW_WIN`
/// keep working when no flag is given.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The client's `Data` folder (the one holding the `.MPQ` files), or the WoW folder above it.
    /// Sets `WOW_DATA`. Without it and without `WOW_DATA`, the checkout's `wow-data` folder, where
    /// `make fetch-data` puts the client, is used when it exists.
    #[arg(long, value_name = "PATH")]
    data: Option<PathBuf>,

    /// Run in a window instead of borderless fullscreen, for this session only. Sets `WOW_WIN`.
    #[arg(long, short)]
    windowed: bool,

    /// Window size in logical pixels, e.g. `1280x720`; implies `--windowed`. Sets `WOW_WIN`.
    #[arg(long, value_name = "WxH", value_parser = parse_size)]
    size: Option<(u32, u32)>,

    /// Host a co-op party; friends join with `--join <your address>`. The UDP port is 37373
    /// unless given. Sets `WOW_SURVIVORS_HOST`.
    #[arg(long, value_name = "PORT", num_args = 0..=1, default_missing_value = "")]
    host: Option<String>,

    /// Join a friend's co-op party at `ADDRESS` (`192.168.1.20`, `host.local:37373`). Sets
    /// `WOW_SURVIVORS_JOIN`.
    #[arg(long, value_name = "ADDRESS", conflicts_with = "host")]
    join: Option<String>,

    /// The name the party sees. Sets `WOW_SURVIVORS_NAME`.
    #[arg(long, value_name = "NAME")]
    name: Option<String>,
}

/// The windowed size when `--windowed` comes without `--size`: the client's own default.
const DEFAULT_SIZE: (u32, u32) = (1600, 900);

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| format!("expected WxH, e.g. 1280x720, got `{s}`"))?;
    let w: u32 = w.parse().map_err(|_| format!("bad width `{w}`"))?;
    let h: u32 = h.parse().map_err(|_| format!("bad height `{h}`"))?;
    if w == 0 || h == 0 {
        return Err("width and height must be above zero".into());
    }
    Ok((w, h))
}

/// Whether the client will only print a report and exit before any setup: `WOW_CAPTURE=list`,
/// `WOW_PROBE=list` and `WOW_HOVER_LOG_REPORT=<csv>`, the early exits at the top of
/// `benilla_app`'s launch.
fn prints_and_exits() -> bool {
    let is_list = |name| std::env::var(name).as_deref() == Ok("list");
    std::env::var("WOW_HOVER_LOG_REPORT").is_ok()
        || is_list("WOW_CAPTURE")
        || is_list("WOW_PROBE")
}

fn main() -> AppExit {
    let cli = Cli::parse();

    // Nothing else runs yet, so setting the environment here races no other thread.
    let fetched = PathBuf::from(env!("BENILLA_PROJECT_DIR")).join("wow-data");
    let data = cli.data.or_else(|| {
        (std::env::var_os("WOW_DATA").is_none() && fetched.is_dir()).then_some(fetched)
    });
    if let Some(dir) = data {
        // Accept the WoW folder as well as its `Data` folder below it.
        let data = dir.join("Data");
        std::env::set_var("WOW_DATA", if data.is_dir() { data } else { dir });
    }
    if let Some((w, h)) = cli.size.or(cli.windowed.then_some(DEFAULT_SIZE)) {
        std::env::set_var("WOW_WIN", format!("{w}x{h}"));
    }
    for (var, value) in [
        ("WOW_SURVIVORS_HOST", cli.host),
        ("WOW_SURVIVORS_JOIN", cli.join),
        ("WOW_SURVIVORS_NAME", cli.name),
    ] {
        if let Some(value) = value {
            std::env::set_var(var, value);
        }
    }

    // The client starts without an install and only logs it, which leaves an empty world; the
    // survivors mode has nothing to show without one, so it stops here instead. The dev commands
    // that only print a list and exit before any setup need no install, so they skip the check.
    benilla_formats::set_project_folder(env!("BENILLA_PROJECT_DIR"));
    if !prints_and_exits() && benilla_formats::wow_data().is_none() {
        eprintln!("warcraft-survivors: no WoW 1.12.1 Data folder found. Looked in:");
        for path in benilla_formats::candidates() {
            eprintln!("  {}", path.display());
        }
        eprintln!("Pass --data <PATH>, set WOW_DATA, or run `make fetch-data` to fill wow-data.");
        return AppExit::error();
    }

    benilla_app::run_survivors(
        BuildId {
            version: env!("CARGO_PKG_VERSION"),
            describe: env!("BENILLA_GIT_DESCRIBE"),
            sha: env!("BENILLA_GIT_SHA"),
            short: env!("BENILLA_GIT_SHORT"),
            date: env!("BENILLA_GIT_DATE"),
            profile: env!("BENILLA_PROFILE"),
            project_dir: env!("BENILLA_PROJECT_DIR"),
            ..Default::default()
        },
        survivors_app::plugin,
    )
}
