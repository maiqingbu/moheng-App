use ai_novel_production_system_core::commands::KernelState;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            app.manage(KernelState::new(AppPaths::new(app_data)));
            Ok(())
        })
        .invoke_handler(ai_novel_production_system_core::commands::handler())
        .run(tauri::generate_context!("tauri.conf.json", test = true))
        .expect("error while running tauri application");
}
