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
    /// Sets `WOW_DATA`.
    #[arg(long, value_name = "PATH")]
    data: Option<PathBuf>,

    /// Run in a window instead of borderless fullscreen, for this session only. Sets `WOW_WIN`.
    #[arg(long, short)]
    windowed: bool,

    /// Window size in logical pixels, e.g. `1280x720`; implies `--windowed`. Sets `WOW_WIN`.
    #[arg(long, value_name = "WxH", value_parser = parse_size)]
    size: Option<(u32, u32)>,
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

fn main() -> AppExit {
    let cli = Cli::parse();

    // Nothing else runs yet, so setting the environment here races no other thread.
    if let Some(dir) = cli.data {
        // Accept the WoW folder as well as its `Data` folder below it.
        let data = dir.join("Data");
        std::env::set_var("WOW_DATA", if data.is_dir() { data } else { dir });
    }
    if let Some((w, h)) = cli.size.or(cli.windowed.then_some(DEFAULT_SIZE)) {
        std::env::set_var("WOW_WIN", format!("{w}x{h}"));
    }

    // The client starts without an install and only logs it, which leaves an empty world; the
    // survivors mode has nothing to show without one, so it stops here instead.
    benilla_formats::set_project_folder(env!("BENILLA_PROJECT_DIR"));
    if benilla_formats::wow_data().is_none() {
        eprintln!("warcraft-survivors: no WoW 1.12.1 Data folder found. Looked in:");
        for path in benilla_formats::candidates() {
            eprintln!("  {}", path.display());
        }
        eprintln!(
            "Pass --data <PATH>, set WOW_DATA, or run `make fetch-data` and use --data wow-data."
        );
        return AppExit::error();
    }

    benilla_app::run_survivors(BuildId {
        version: env!("CARGO_PKG_VERSION"),
        describe: env!("BENILLA_GIT_DESCRIBE"),
        sha: env!("BENILLA_GIT_SHA"),
        short: env!("BENILLA_GIT_SHORT"),
        date: env!("BENILLA_GIT_DATE"),
        profile: env!("BENILLA_PROFILE"),
        project_dir: env!("BENILLA_PROJECT_DIR"),
        ..Default::default()
    })
}
