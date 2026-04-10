use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

const PREFS_FILE: &str = "app_preferences.json";
const MAX_RECENT_PROJECTS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppPreferences {
    pub last_opened_project: Option<String>,
    #[serde(default)]
    pub recent_projects: Vec<String>,
}

fn normalize_path(path: &str) -> String {
    let p = Path::new(path);
    match p.canonicalize() {
        Ok(full) => full.to_string_lossy().to_string(),
        Err(_) => p.to_string_lossy().to_string(),
    }
}

fn prefs_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&config_dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    Ok(config_dir.join(PREFS_FILE))
}

fn load_prefs_internal(app: &AppHandle) -> Result<AppPreferences, String> {
    let path = prefs_path(app)?;
    if !path.exists() {
        return Ok(AppPreferences::default());
    }

    let text = fs::read_to_string(path).map_err(|e| format!("failed to read preferences: {e}"))?;
    serde_json::from_str::<AppPreferences>(&text)
        .map_err(|e| format!("failed to parse preferences: {e}"))
}

fn save_prefs_internal(app: &AppHandle, prefs: &AppPreferences) -> Result<(), String> {
    let path = prefs_path(app)?;
    let text = serde_json::to_string_pretty(prefs)
        .map_err(|e| format!("failed to serialize preferences: {e}"))?;
    fs::write(path, text).map_err(|e| format!("failed to write preferences: {e}"))
}

#[tauri::command]
pub fn get_app_preferences(app: AppHandle) -> Result<AppPreferences, String> {
    load_prefs_internal(&app)
}

#[tauri::command]
pub fn remember_project(app: AppHandle, path: String) -> Result<AppPreferences, String> {
    let normalized = normalize_path(&path);
    let mut prefs = load_prefs_internal(&app)?;
    prefs.last_opened_project = Some(normalized.clone());
    prefs.recent_projects.retain(|p| p != &normalized);
    prefs.recent_projects.insert(0, normalized);
    prefs.recent_projects.truncate(MAX_RECENT_PROJECTS);
    save_prefs_internal(&app, &prefs)?;
    Ok(prefs)
}

#[tauri::command]
pub fn forget_project(app: AppHandle, path: String) -> Result<AppPreferences, String> {
    let normalized = normalize_path(&path);
    let mut prefs = load_prefs_internal(&app)?;
    prefs.recent_projects.retain(|p| p != &normalized);
    if prefs.last_opened_project.as_ref() == Some(&normalized) {
        prefs.last_opened_project = None;
    }
    save_prefs_internal(&app, &prefs)?;
    Ok(prefs)
}

#[tauri::command]
pub fn clear_last_project(app: AppHandle) -> Result<AppPreferences, String> {
    let mut prefs = load_prefs_internal(&app)?;
    prefs.last_opened_project = None;
    save_prefs_internal(&app, &prefs)?;
    Ok(prefs)
}

#[tauri::command]
pub async fn open_project_dialog(app: AppHandle) -> Option<String> {
    let path = app
        .dialog()
        .file()
        .add_filter("rsemu project", &["json"])
        .blocking_pick_file()?;
    Some(path.to_string())
}

#[tauri::command]
pub async fn save_project_dialog(app: AppHandle) -> Option<String> {
    let path = app
        .dialog()
        .file()
        .add_filter("rsemu project", &["json"])
        .blocking_save_file()?;
    Some(path.to_string())
}

#[tauri::command]
pub fn read_project_file(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| format!("failed to read project file: {e}"))
}

#[tauri::command]
pub fn write_project_file(path: String, content: String) -> Result<(), String> {
    let p = Path::new(&path);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("failed to prepare project directory: {e}"))?;
    }
    fs::write(p, content).map_err(|e| format!("failed to write project file: {e}"))
}
