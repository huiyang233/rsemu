use std::sync::Mutex;

mod commands;
mod emulator;
mod state;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(state::SimState::default()))
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
