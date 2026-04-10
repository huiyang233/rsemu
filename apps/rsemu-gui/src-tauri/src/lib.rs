use std::path::Path;
use std::sync::Mutex;

use rsemu_targets::TargetRegistry;

mod commands;
mod emulator;
mod state;

fn create_registry() -> TargetRegistry {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    // manifest_dir = .../rsemu/apps/rsemu-gui/src-tauri → project root is 3 levels up
    let project_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("cannot resolve project root");
    let configs_dir = project_root.join("configs");
    let svds_dir = project_root.join("svds");
    TargetRegistry::from_dirs(&configs_dir, &svds_dir)
        .expect("failed to create target registry from configs/ and svds/")
}

pub fn run() {
    let registry = create_registry();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(state::SimState::default()))
        .manage(registry)
        .invoke_handler(tauri::generate_handler![
            commands::get_boards,
            commands::start_simulation,
            commands::stop_simulation,
            commands::inject_gpio,
            commands::send_uart,
            commands::open_firmware_dialog,
            commands::get_app_preferences,
            commands::remember_project,
            commands::forget_project,
            commands::clear_last_project,
            commands::open_project_dialog,
            commands::save_project_dialog,
            commands::read_project_file,
            commands::write_project_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
