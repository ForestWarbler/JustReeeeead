mod commands;
mod library;
mod llm;
mod pdf;
mod settings;

use llm::TranslationService;
use pdf::PdfService;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::Emitter;

pub struct AppState {
    pub pdf: PdfService,
    pub translations: TranslationService,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let pdf_service = PdfService::new();
    let protocol_pdf_service = pdf_service.clone();
    let state = AppState {
        pdf: pdf_service,
        translations: TranslationService::new(),
    };

    tauri::Builder::default()
        .manage(state)
        .register_uri_scheme_protocol("jrpage", move |_ctx, request| {
            protocol_pdf_service.handle_protocol(request)
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let open_file = MenuItemBuilder::with_id("open_file", "Open File...")
                .accelerator("CmdOrCtrl+O")
                .build(app)?;

            let file_menu = SubmenuBuilder::new(app, "File")
                .item(&open_file)
                .separator()
                .item(&SubmenuBuilder::new(app, "Open Recent").build()?)
                .build()?;

            let menu = MenuBuilder::new(app)
                .item(&file_menu)
                .build()?;

            app.set_menu(menu)?;

            Ok(())
        })
        .on_menu_event(|app, event| {
            let id = event.id().0.as_str();
            match id {
                "open_file" => {
                    let _ = app.emit("menu-open-file", ());
                }
                id if id.starts_with("recent_") => {
                    let _ = app.emit("menu-open-recent", id.strip_prefix("recent_").unwrap_or("").to_string());
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_pdf,
            commands::close_pdf,
            commands::get_page_text_layer,
            commands::get_page_render_url,
            commands::prefetch_page_renders,
            commands::get_settings,
            commands::save_settings,
            commands::save_api_key,
            commands::translate_selection,
            commands::cancel_translation,
            commands::get_recent_files,
            commands::get_library,
            commands::add_library_folder,
            commands::remove_library_folder,
            commands::add_library_document,
            commands::remove_library_document,
            commands::move_library_document,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
