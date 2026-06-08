use std::{fs, path::PathBuf};

use keyring::Entry;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "settings.json";
const KEYRING_SERVICE: &str = "com.forestwarbler.justreeeeead";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub version: u32,
    pub reader: ReaderSettings,
    pub layout: LayoutSettings,
    pub llm_providers: Vec<LlmProviderSettings>,
    pub prompt_profiles: Vec<PromptProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReaderSettings {
    pub zoom: f32,
    pub rotation: i32,
    pub prefetch_radius: usize,
    pub page_gap: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSettings {
    pub translation_open: bool,
    pub translation_dock: TranslationDock,
    pub translation_size: u32,
    #[serde(default)]
    pub theme: AppTheme,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TranslationDock {
    Right,
    Bottom,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AppTheme {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderSettings {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub api_key_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PromptProfile {
    pub id: String,
    pub source_language: String,
    pub target_language: String,
    pub prompt_template: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsPatch {
    pub reader: Option<ReaderSettings>,
    pub layout: Option<LayoutSettings>,
    pub llm_providers: Option<Vec<LlmProviderSettings>>,
    pub prompt_profiles: Option<Vec<PromptProfile>>,
}

pub fn get_settings(app: &AppHandle) -> anyhow::Result<AppSettings> {
    let mut settings = load_or_default(settings_path(app)?);
    refresh_api_key_flags(&mut settings);
    Ok(settings)
}

pub fn save_settings(app: &AppHandle, patch: AppSettingsPatch) -> anyhow::Result<AppSettings> {
    let path = settings_path(app)?;
    let mut settings = load_or_default(path.clone());
    apply_patch(&mut settings, patch);
    normalize_settings(&mut settings);
    write_settings(&path, &settings)?;
    refresh_api_key_flags(&mut settings);
    Ok(settings)
}

pub fn save_api_key(provider_id: &str, api_key: &str) -> anyhow::Result<()> {
    let entry = Entry::new(KEYRING_SERVICE, provider_id)?;
    entry.set_password(api_key)?;
    Ok(())
}

pub fn get_api_key(provider_id: &str) -> anyhow::Result<String> {
    let entry = Entry::new(KEYRING_SERVICE, provider_id)?;
    Ok(entry.get_password()?)
}

pub fn default_settings() -> AppSettings {
    AppSettings {
        version: 1,
        reader: ReaderSettings {
            zoom: 1.0,
            rotation: 0,
            prefetch_radius: 2,
            page_gap: 18,
        },
        layout: LayoutSettings {
            translation_open: true,
            translation_dock: TranslationDock::Right,
            translation_size: 380,
            theme: AppTheme::Light,
        },
        llm_providers: vec![LlmProviderSettings {
            id: "openai-compatible".to_string(),
            name: "OpenAI Compatible".to_string(),
            base_url: "https://api.openai.com".to_string(),
            model: "gpt-4o-mini".to_string(),
            api_key_configured: false,
        }],
        prompt_profiles: vec![PromptProfile {
            id: "academic-auto-to-chinese".to_string(),
            source_language: "Auto".to_string(),
            target_language: "Chinese".to_string(),
            prompt_template: default_academic_prompt(),
        }],
    }
}

pub fn select_prompt_profile<'a>(
    settings: &'a AppSettings,
    profile_id: Option<&str>,
    source_language: &str,
    target_language: &str,
) -> Option<&'a PromptProfile> {
    if let Some(profile_id) = profile_id {
        if let Some(profile) = settings
            .prompt_profiles
            .iter()
            .find(|profile| profile.id == profile_id)
        {
            return Some(profile);
        }
    }

    settings
        .prompt_profiles
        .iter()
        .find(|profile| {
            profile.source_language == source_language && profile.target_language == target_language
        })
        .or_else(|| settings.prompt_profiles.first())
}

pub fn render_prompt(
    template: &str,
    source_language: &str,
    target_language: &str,
    text: &str,
) -> String {
    template
        .replace("{{source_language}}", source_language)
        .replace("{{target_language}}", target_language)
        .replace("{{text}}", text)
}

fn settings_path(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let dir = app.path().app_config_dir()?;
    fs::create_dir_all(&dir)?;
    Ok(dir.join(SETTINGS_FILE))
}

fn load_or_default(path: PathBuf) -> AppSettings {
    let Ok(raw) = fs::read_to_string(path) else {
        return default_settings();
    };

    let Ok(mut settings) = serde_json::from_str::<AppSettings>(&raw) else {
        return default_settings();
    };

    normalize_settings(&mut settings);
    settings
}

fn write_settings(path: &PathBuf, settings: &AppSettings) -> anyhow::Result<()> {
    let pretty = serde_json::to_string_pretty(settings)?;
    fs::write(path, pretty)?;
    Ok(())
}

fn apply_patch(settings: &mut AppSettings, patch: AppSettingsPatch) {
    if let Some(reader) = patch.reader {
        settings.reader = reader;
    }
    if let Some(layout) = patch.layout {
        settings.layout = layout;
    }
    if let Some(mut providers) = patch.llm_providers {
        for provider in &mut providers {
            provider.api_key_configured = settings
                .llm_providers
                .iter()
                .find(|existing| existing.id == provider.id)
                .map(|existing| existing.api_key_configured)
                .unwrap_or(false);
        }
        settings.llm_providers = providers;
    }
    if let Some(prompt_profiles) = patch.prompt_profiles {
        settings.prompt_profiles = prompt_profiles;
    }
}

fn normalize_settings(settings: &mut AppSettings) {
    settings.version = 1;
    settings.reader.zoom = settings.reader.zoom.clamp(0.2, 6.0);
    settings.reader.rotation = settings.reader.rotation.rem_euclid(360);
    settings.reader.prefetch_radius = settings.reader.prefetch_radius.min(6);
    settings.reader.page_gap = settings.reader.page_gap.clamp(8, 48);
    settings.layout.translation_size = settings.layout.translation_size.clamp(240, 900);

    if settings.llm_providers.is_empty() {
        settings.llm_providers = default_settings().llm_providers;
    }
    if settings.prompt_profiles.is_empty() {
        settings.prompt_profiles = default_settings().prompt_profiles;
    }

    for provider in &mut settings.llm_providers {
        provider.api_key_configured = false;
    }
}

fn refresh_api_key_flags(settings: &mut AppSettings) {
    for provider in &mut settings.llm_providers {
        provider.api_key_configured = get_api_key(&provider.id)
            .map(|key| !key.is_empty())
            .unwrap_or(false);
    }
}

fn default_academic_prompt() -> String {
    [
        "You are a precise academic paper translation assistant.",
        "Translate the selected text from {{source_language}} to {{target_language}}.",
        "Preserve technical terminology, equations, citations, variable names, and paragraph structure.",
        "Return only the translation without commentary.",
        "",
        "{{text}}",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        default_settings, render_prompt, select_prompt_profile, AppSettingsPatch, ReaderSettings,
    };

    #[test]
    fn prompt_rendering_replaces_all_placeholders() {
        let rendered = render_prompt(
            "{{source_language}} -> {{target_language}}: {{text}}",
            "English",
            "Chinese",
            "hello",
        );

        assert_eq!(rendered, "English -> Chinese: hello");
    }

    #[test]
    fn prompt_selection_prefers_requested_profile() {
        let settings = default_settings();
        let selected = select_prompt_profile(
            &settings,
            Some("academic-auto-to-chinese"),
            "English",
            "Chinese",
        )
        .unwrap();

        assert_eq!(selected.id, "academic-auto-to-chinese");
    }

    #[test]
    fn settings_patch_clamps_reader_values() {
        let mut settings = default_settings();
        super::apply_patch(
            &mut settings,
            AppSettingsPatch {
                reader: Some(ReaderSettings {
                    zoom: 12.0,
                    rotation: -90,
                    prefetch_radius: 99,
                    page_gap: 2,
                }),
                ..AppSettingsPatch::default()
            },
        );
        super::normalize_settings(&mut settings);

        assert_eq!(settings.reader.zoom, 6.0);
        assert_eq!(settings.reader.rotation, 270);
        assert_eq!(settings.reader.prefetch_radius, 6);
        assert_eq!(settings.reader.page_gap, 8);
    }
}
