use ai_novel_production_system_core::commands::KernelState;
use ai_novel_production_system_core::creative_skills::SkillBundleLoader;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use std::path::PathBuf;
#[cfg(not(debug_assertions))]
use tauri::path::BaseDirectory;
use tauri::Manager;

fn main() {
    #[cfg(test)]
    let context = tauri::generate_context!("tauri.conf.json", test = true);
    #[cfg(not(test))]
    let context = tauri::generate_context!("tauri.conf.json");

    let builder = tauri::Builder::default();

    #[cfg(feature = "e2e-webdriver")]
    let builder = {
        eprintln!("E2E WebDriver 已启用");
        builder.plugin(tauri_plugin_wdio_webdriver::init())
    };

    builder
        .setup(|app| {
            let resource_diagnostic =
                std::env::args_os().any(|argument| argument == "--diagnose-skill-resources");
            #[cfg(debug_assertions)]
            if resource_diagnostic {
                eprintln!("PRE01_NATIVE_DIAGNOSTIC_REQUIRES_RELEASE_BUILD");
                std::process::exit(2);
            }

            #[cfg(debug_assertions)]
            let skill_bundle_loader = SkillBundleLoader::development_package();
            #[cfg(not(debug_assertions))]
            let skill_bundle_loader = SkillBundleLoader::from_application_package_root(
                app.path()
                    .resolve("creative-skills/v1.2.0", BaseDirectory::Resource)?,
            );

            if resource_diagnostic {
                match skill_bundle_loader.diagnostic_report() {
                    Ok(report) => {
                        println!("PRE01_SKILL_RESOURCE_DIAGNOSTIC:{}", report);
                        std::process::exit(0);
                    }
                    Err(error) => {
                        eprintln!("PRE01_SKILL_RESOURCE_ERROR:{error}");
                        std::process::exit(2);
                    }
                }
            }

            let app_data = std::env::var_os("MOHENG_REAL_APP_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            app.manage(
                KernelState::with_skill_bundle_loader(AppPaths::new(app_data), skill_bundle_loader)
                    .with_market_source_adapter(std::sync::Arc::new(
                        ai_novel_production_system_core::market_source::FanqieWriterZoneAdapter,
                    )),
            );
            Ok(())
        })
        .invoke_handler(ai_novel_production_system_core::commands::handler())
        .run(context)
        .expect("error while running tauri application");
}
