use tauri::{AppHandle, State};

use crate::{
    library::{self, LibraryData, RecentFileEntry},
    llm::{TranslationJob, TranslationRequest},
    pdf::{PageRenderRequest, PageTextLayer, PdfDocumentInfo},
    settings::{AppSettings, AppSettingsPatch},
    AppState,
};

type CommandResult<T> = Result<T, String>;

#[tauri::command]
pub fn open_pdf(state: State<'_, AppState>, app: AppHandle, path: String) -> CommandResult<PdfDocumentInfo> {
    let info = state.pdf.open_pdf(path).map_err(command_error)?;
    let _ = library::add_recent_file(&app, &info.path, &info.title);
    Ok(info)
}

#[tauri::command]
pub fn close_pdf(state: State<'_, AppState>, doc_id: String) {
    state.pdf.close_pdf(&doc_id);
}

#[tauri::command]
pub fn get_page_text_layer(
    state: State<'_, AppState>,
    doc_id: String,
    page_index: usize,
) -> CommandResult<PageTextLayer> {
    state
        .pdf
        .get_page_text_layer(&doc_id, page_index)
        .map_err(command_error)
}

#[tauri::command]
pub fn get_page_render_url(
    state: State<'_, AppState>,
    request: PageRenderRequest,
) -> CommandResult<String> {
    state
        .pdf
        .get_page_render_url(request)
        .map_err(command_error)
}

#[tauri::command]
pub fn prefetch_page_renders(
    state: State<'_, AppState>,
    requests: Vec<PageRenderRequest>,
) -> CommandResult<()> {
    state
        .pdf
        .prefetch_page_renders(requests)
        .map_err(command_error)
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> CommandResult<AppSettings> {
    crate::settings::get_settings(&app).map_err(command_error)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettingsPatch) -> CommandResult<AppSettings> {
    crate::settings::save_settings(&app, settings).map_err(command_error)
}

#[tauri::command]
pub fn save_api_key(provider_id: String, api_key: String) -> CommandResult<()> {
    crate::settings::save_api_key(&provider_id, &api_key).map_err(command_error)
}

#[tauri::command]
pub fn translate_selection(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: TranslationRequest,
) -> CommandResult<TranslationJob> {
    state
        .translations
        .start_translation(app, payload)
        .map_err(command_error)
}

#[tauri::command]
pub fn cancel_translation(state: State<'_, AppState>, job_id: String) {
    state.translations.cancel_translation(&job_id);
}

#[tauri::command]
pub fn get_recent_files(app: AppHandle) -> CommandResult<Vec<RecentFileEntry>> {
    library::load_recent_files(&app).map_err(command_error)
}

#[tauri::command]
pub fn get_library(app: AppHandle) -> CommandResult<LibraryData> {
    library::load_library(&app).map_err(command_error)
}

#[tauri::command]
pub fn add_library_folder(app: AppHandle, name: String) -> CommandResult<LibraryData> {
    library::add_library_folder(&app, name).map_err(command_error)
}

#[tauri::command]
pub fn remove_library_folder(app: AppHandle, folder_id: String) -> CommandResult<LibraryData> {
    library::remove_library_folder(&app, &folder_id).map_err(command_error)
}

#[tauri::command]
pub fn add_library_document(
    app: AppHandle,
    path: String,
    title: String,
    file_hash: String,
    folder_id: Option<String>,
) -> CommandResult<LibraryData> {
    library::add_library_document(&app, &path, &title, &file_hash, folder_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn remove_library_document(app: AppHandle, doc_id: String) -> CommandResult<LibraryData> {
    library::remove_library_document(&app, &doc_id).map_err(command_error)
}

#[tauri::command]
pub fn move_library_document(
    app: AppHandle,
    doc_id: String,
    folder_id: Option<String>,
) -> CommandResult<LibraryData> {
    library::move_library_document(&app, &doc_id, folder_id).map_err(command_error)
}

fn command_error(error: anyhow::Error) -> String {
    error.to_string()
}
