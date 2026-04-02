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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
