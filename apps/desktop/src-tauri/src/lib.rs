//! Guscio dell'app desktop: crea la finestra e inoltra i comandi al nucleo.
//! Qui non va logica propria: sta tutta in `app-core`.

use std::sync::Mutex;

use app_core::store::Store;
use tauri::Manager;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CoreInfo {
    version: &'static str,
    schema_version: i64,
}

/// Informazioni sul nucleo, mostrate nell'interfaccia.
#[tauri::command]
fn core_info(store: tauri::State<'_, Mutex<Store>>) -> Result<CoreInfo, String> {
    let store = store.lock().map_err(|e| e.to_string())?;
    Ok(CoreInfo {
        version: app_core::version(),
        schema_version: store.schema_version().map_err(|e| e.to_string())?,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = Store::open(&data_dir.join("data.sqlite"))?;
            app.manage(Mutex::new(store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![core_info])
        .run(tauri::generate_context!())
        .expect("errore durante l'avvio dell'app");
}
