use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const LIBRARY_FILE: &str = "library.json";
const MAX_RECENT_FILES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFolder {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDocument {
    pub id: String,
    pub path: String,
    pub title: String,
    pub file_hash: String,
    pub folder_id: Option<String>,
    pub added_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryData {
    pub folders: Vec<LibraryFolder>,
    pub documents: Vec<LibraryDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFileEntry {
    pub path: String,
    pub title: String,
    pub opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFilesData {
    pub files: Vec<RecentFileEntry>,
}

pub fn load_recent_files(app: &AppHandle) -> anyhow::Result<Vec<RecentFileEntry>> {
    let path = recent_files_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)?;
    let data: RecentFilesData = serde_json::from_str(&raw).unwrap_or(RecentFilesData { files: vec![] });
    Ok(data.files)
}

pub fn add_recent_file(app: &AppHandle, path: &str, title: &str) -> anyhow::Result<Vec<RecentFileEntry>> {
    let file_path = recent_files_path(app)?;
    let mut files = load_recent_files(app)?;

    files.retain(|f| f.path != path);
    files.insert(
        0,
        RecentFileEntry {
            path: path.to_string(),
            title: title.to_string(),
            opened_at: chrono_now(),
        },
    );

    files.truncate(MAX_RECENT_FILES);

    let data = RecentFilesData {
        files: files.clone(),
    };
    fs::write(&file_path, serde_json::to_string_pretty(&data)?)?;

    Ok(files)
}

pub fn load_library(app: &AppHandle) -> anyhow::Result<LibraryData> {
    let path = library_path(app)?;
    if !path.exists() {
        return Ok(LibraryData::default());
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

pub fn save_library(app: &AppHandle, data: &LibraryData) -> anyhow::Result<()> {
    let path = library_path(app)?;
    fs::write(&path, serde_json::to_string_pretty(data)?)?;
    Ok(())
}

pub fn add_library_folder(
    app: &AppHandle,
    name: String,
) -> anyhow::Result<LibraryData> {
    let mut library = load_library(app)?;
    let folder = LibraryFolder {
        id: Uuid::new_v4().to_string(),
        name,
    };
    library.folders.push(folder);
    save_library(app, &library)?;
    Ok(library)
}

pub fn remove_library_folder(app: &AppHandle, folder_id: &str) -> anyhow::Result<LibraryData> {
    let mut library = load_library(app)?;
    library.folders.retain(|f| f.id != folder_id);
    library.documents.retain(|d| d.folder_id.as_deref() != Some(folder_id));
    save_library(app, &library)?;
    Ok(library)
}

pub fn add_library_document(
    app: &AppHandle,
    path: &str,
    title: &str,
    file_hash: &str,
    folder_id: Option<String>,
) -> anyhow::Result<LibraryData> {
    let mut library = load_library(app)?;

    library.documents.retain(|d| d.path != path);

    library.documents.push(LibraryDocument {
        id: Uuid::new_v4().to_string(),
        path: path.to_string(),
        title: title.to_string(),
        file_hash: file_hash.to_string(),
        folder_id,
        added_at: chrono_now(),
    });

    save_library(app, &library)?;
    Ok(library)
}

pub fn remove_library_document(app: &AppHandle, doc_id: &str) -> anyhow::Result<LibraryData> {
    let mut library = load_library(app)?;
    library.documents.retain(|d| d.id != doc_id);
    save_library(app, &library)?;
    Ok(library)
}

pub fn move_library_document(
    app: &AppHandle,
    doc_id: &str,
    folder_id: Option<String>,
) -> anyhow::Result<LibraryData> {
    let mut library = load_library(app)?;

    if let Some(target_folder_id) = folder_id.as_deref() {
        let folder_exists = library.folders.iter().any(|folder| folder.id == target_folder_id);
        if !folder_exists {
            anyhow::bail!("Target folder does not exist");
        }
    }

    let document = library
        .documents
        .iter_mut()
        .find(|document| document.id == doc_id)
        .ok_or_else(|| anyhow::anyhow!("Library document does not exist"))?;
    document.folder_id = folder_id;

    save_library(app, &library)?;
    Ok(library)
}

fn library_path(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let dir = app.path().app_config_dir()?;
    fs::create_dir_all(&dir)?;
    Ok(dir.join(LIBRARY_FILE))
}

fn recent_files_path(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let dir = app.path().app_config_dir()?;
    fs::create_dir_all(&dir)?;
    Ok(dir.join("recent_files.json"))
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", since.as_secs())
}
