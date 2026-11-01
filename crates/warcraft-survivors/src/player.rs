//! The Windows player build (no `dev`): installed and kept up to date by Velopack, with its state
//! in the user's AppData and the WoW data folder asked for once and remembered.
//!
//! - [`start`] runs first in `main`: Velopack's install, update and uninstall hooks exit the
//!   process early, and an update downloaded by an earlier run is applied here (restart).
//! - [`use_appdata`]: settings, profile and saves live in `%APPDATA%\Warcraft Survivors`, not
//!   beside the exe, because every update replaces the exe's folder.
//! - [`find_data`]: the WoW 1.12.1 `Data` folder from `--data`, `WOW_DATA`, the remembered path
//!   or a folder picker; there is no console to print to in this build.
//! - [`check_for_updates`]: a background thread asks the GitHub releases for a newer version and
//!   downloads it; Velopack installs it at the next start.

use std::path::{Path, PathBuf};

/// Where the releases are published (`.github/workflows/windows-release.yml`).
const REPO: &str = "https://github.com/maluramichael/azeroth-survivors-re";

/// The folder in `%APPDATA%` that holds the state, outside the install so updates keep it.
const APPDATA_DIR: &str = "Warcraft Survivors";

/// The file in that folder remembering the WoW data folder.
const DATA_PATH_FILE: &str = "wow-data-path.txt";

/// Velopack's hooks and the update downloaded last time; must run before anything else.
pub fn start() {
    velopack::VelopackApp::build().run();
}

/// `%APPDATA%\Warcraft Survivors`, created; `None` without `%APPDATA%`.
fn appdata() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("APPDATA")?).join(APPDATA_DIR);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Point the state folder (`BENILLA_HOME`, `benilla_app`'s local state) into AppData, unless it is
/// set already (`--home`).
pub fn use_appdata() {
    if std::env::var_os("BENILLA_HOME").is_some() {
        return;
    }
    if let Some(dir) = appdata() {
        // Nothing else runs yet, so setting the environment here races no other thread.
        std::env::set_var("BENILLA_HOME", dir.join("benilla-config"));
    }
}

/// The WoW `Data` folder in `dir`: `dir` itself when it holds the MPQs, or its `Data` folder.
fn data_in(dir: &Path) -> Option<PathBuf> {
    let has_mpqs = |d: &Path| d.join("patch.MPQ").is_file() && d.join("terrain.MPQ").is_file();
    let below = dir.join("Data");
    if has_mpqs(dir) {
        Some(dir.to_path_buf())
    } else if has_mpqs(&below) {
        Some(below)
    } else {
        None
    }
}

/// Make sure `WOW_DATA` names a WoW 1.12.1 `Data` folder: the one already set, the remembered
/// one, or one the player picks (remembered for the next start). `false` when the player gives up.
pub fn find_data() -> bool {
    if let Some(dir) = std::env::var_os("WOW_DATA").map(PathBuf::from) {
        if data_in(&dir).is_some() {
            return true;
        }
    }
    let remembered = appdata().map(|d| d.join(DATA_PATH_FILE));
    if let Some(dir) = remembered
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| data_in(Path::new(s.trim())))
    {
        std::env::set_var("WOW_DATA", dir);
        return true;
    }
    loop {
        let picked = rfd::FileDialog::new()
            .set_title("Where is World of Warcraft 1.12.1? Pick its folder or its Data folder")
            .pick_folder();
        let Some(picked) = picked else {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Warcraft Survivors")
                .set_description(
                    "Warcraft Survivors needs the Data folder of a World of Warcraft 1.12.1 \
                     client (the one with patch.MPQ and terrain.MPQ). It is not included. \
                     Start the game again to pick it.",
                )
                .show();
            return false;
        };
        if let Some(dir) = data_in(&picked) {
            if let Some(file) = &remembered {
                let _ = std::fs::write(file, dir.to_string_lossy().as_bytes());
            }
            std::env::set_var("WOW_DATA", dir);
            return true;
        }
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Warcraft Survivors")
            .set_description(format!(
                "{} holds no patch.MPQ and terrain.MPQ, neither does a Data folder in it. \
                 Pick the World of Warcraft 1.12.1 folder or its Data folder.",
                picked.display()
            ))
            .show();
    }
}

/// Look for a newer release in the background and download it; Velopack applies it at the next
/// start ([`start`]). Nothing happens in a copy that was not installed by Velopack.
pub fn check_for_updates() {
    std::thread::spawn(|| {
        let source = velopack::sources::GithubSource::new(REPO, None, false);
        let Ok(manager) = velopack::UpdateManager::new(source, None, None) else {
            return; // not installed (a plain exe), so nothing to update
        };
        match manager.check_for_updates() {
            Ok(velopack::UpdateCheck::UpdateAvailable(update)) => {
                if let Err(e) = manager.download_updates(&update, None) {
                    eprintln!("warcraft-survivors: update download failed: {e}");
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("warcraft-survivors: update check failed: {e}"),
        }
    });
}
