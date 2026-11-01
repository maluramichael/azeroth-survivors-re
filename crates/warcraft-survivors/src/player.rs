//! The Windows player build (no `dev`): installed and kept up to date by Velopack, with its state
//! in the user's AppData and the WoW data folder asked for once and remembered.
//!
//! - [`start`] runs first in `main`: Velopack's install, update and uninstall hooks exit the
//!   process early, and an update downloaded by an earlier run is applied here (restart).
//! - [`use_appdata`]: settings, profile and saves live in `%APPDATA%\Warcraft Survivors`, not
//!   beside the exe, because every update replaces the exe's folder.
//! - [`find_data`]: the WoW 1.12.1 `Data` folder from `--data`, `WOW_DATA`, the remembered path,
//!   an earlier download, or else the player's choice: download it (`fetch-data.ps1`, the zip
//!   `scripts/fetch-data.sh` fetches, in a window that shows the progress) or pick a folder. There
//!   is no console to print to in this build.
//! - [`check_for_updates`]: a background thread asks the GitHub releases for a newer version and
//!   downloads it; Velopack installs it at the next start.

use std::path::{Path, PathBuf};

/// Where the releases are published (`.github/workflows/windows-release.yml`).
const REPO: &str = "https://github.com/maluramichael/azeroth-survivors-re";

/// The folder in `%APPDATA%` that holds the state, outside the install so updates keep it.
const APPDATA_DIR: &str = "Warcraft Survivors";

/// The file in that folder remembering the WoW data folder.
const DATA_PATH_FILE: &str = "wow-data-path.txt";

/// The client the game data comes from, as `scripts/fetch-data.sh` downloads it; `WOW_DATA_URL`
/// overrides it.
const DATA_URL: &str = "https://archive.org/download/WoW_1.12.1-5875/1.12.1-5875_enUS.zip";

/// The download and unpack script, run in its own window.
const FETCH_SCRIPT: &str = include_str!("fetch-data.ps1");

/// The dialog's buttons; on a Windows without Common Controls 6 they read Yes, No and Cancel.
const DOWNLOAD: &str = "Download";
const CHOOSE: &str = "Choose folder";
const QUIT: &str = "Quit";

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

/// `%LOCALAPPDATA%\Warcraft Survivors`: where the downloaded game data goes, outside the install
/// so updates keep it, and not in the roaming AppData because it is 5.5 GB.
fn download_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(APPDATA_DIR))
}

/// Use `dir` as the data folder from now on: set `WOW_DATA` and remember it for the next start.
fn adopt(dir: PathBuf, remembered: Option<&Path>) {
    if let Some(file) = remembered {
        let _ = std::fs::write(file, dir.to_string_lossy().as_bytes());
    }
    std::env::set_var("WOW_DATA", dir);
}

/// Make sure `WOW_DATA` names a WoW 1.12.1 `Data` folder: the one already set, the remembered
/// one, an earlier download, or what the player chooses now: a download or a folder they have.
/// `false` when the player quits.
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
    let downloads = download_dir();
    if let Some(dir) = downloads.as_deref().and_then(data_in) {
        adopt(dir, remembered.as_deref());
        return true;
    }
    loop {
        let choice = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Info)
            .set_title("Warcraft Survivors")
            .set_description(
                "Warcraft Survivors runs on the game data of a World of Warcraft 1.12.1 client, \
                 which is not included.\n\nDownload it now (about 5.5 GB, once), or choose the \
                 folder of a 1.12.1 client you already have.",
            )
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                DOWNLOAD.into(),
                CHOOSE.into(),
                QUIT.into(),
            ))
            .show();
        let download = match &choice {
            rfd::MessageDialogResult::Custom(c) if c == DOWNLOAD => true,
            rfd::MessageDialogResult::Yes => true,
            rfd::MessageDialogResult::Custom(c) if c == CHOOSE => false,
            rfd::MessageDialogResult::No => false,
            _ => return false,
        };
        if download {
            let Some(dir) = downloads.clone() else {
                return false; // no %LOCALAPPDATA%: nowhere to put it
            };
            if fetch(&dir) {
                if let Some(data) = data_in(&dir) {
                    adopt(data, remembered.as_deref());
                    return true;
                }
            }
            // A failed or closed download: the script told the player why; ask again.
            continue;
        }
        let Some(picked) = rfd::FileDialog::new()
            .set_title("Where is World of Warcraft 1.12.1? Pick its folder or its Data folder")
            .pick_folder()
        else {
            continue;
        };
        if let Some(dir) = data_in(&picked) {
            adopt(dir, remembered.as_deref());
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

/// Download and unpack the game data into `dir` with `fetch-data.ps1` in a window of its own,
/// waiting for it; `true` when it finished well.
fn fetch(dir: &Path) -> bool {
    use std::os::windows::process::CommandExt;
    /// The child gets a console window of its own, which shows curl's progress.
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let script = std::env::temp_dir().join("warcraft-survivors-fetch-data.ps1");
    if std::fs::write(&script, FETCH_SCRIPT).is_err() {
        return false;
    }
    let url = std::env::var("WOW_DATA_URL").unwrap_or_else(|_| DATA_URL.to_string());
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-Dir")
        .arg(dir)
        .arg("-Url")
        .arg(url)
        .creation_flags(CREATE_NEW_CONSOLE)
        .status()
        .is_ok_and(|s| s.success())
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
