// Developer-only evaluation entry point; excluded from ordinary release builds.
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let settings = crate::settings::Settings::open(app.path().app_data_dir()?)
                .map_err(std::io::Error::other)?;
            let config = settings.backend_values();
            let output = app.path().app_cache_dir()?.join("prompt-evaluation.json");
            let app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let report = crate::language::evaluate_prompts(&config).await;
                let _ = crate::storage::write_json(&output, &report);
                app.exit(0);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("prompt evaluation failed to start");
}
