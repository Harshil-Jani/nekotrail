//! Settings, About: the licence texts of every third-party component the app ships with.
//! `scripts/third-party-notices.ts` writes them to `resources/THIRD_PARTY_LICENSES.txt` at build
//! time, and `tauri.conf.json` bundles that file as a resource, so it travels with every copy.

use std::path::PathBuf;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

/// The bundled file's name inside the app's resources folder.
pub const LICENSES_FILE: &str = "THIRD_PARTY_LICENSES.txt";

fn licenses_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().resolve(LICENSES_FILE, BaseDirectory::Resource).map_err(|e| e.to_string())
}

/// The full licence texts, for the "Open source licences" view.
#[tauri::command]
pub async fn third_party_licenses(app: AppHandle) -> Result<String, String> {
    let path = licenses_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read {}: {e}", path.display())))
        .await
        .map_err(|e| e.to_string())?
}

/// Opens the same file in the default text viewer, to search or save it.
#[tauri::command]
pub fn open_third_party_licenses(app: AppHandle) -> Result<(), String> {
    let path = licenses_path(&app)?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}
