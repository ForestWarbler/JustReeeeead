use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::settings::{self, LlmProviderSettings};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationRequest {
    pub selection_text: String,
    pub source_language: String,
    pub target_language: String,
    pub prompt_profile_id: Option<String>,
    pub provider_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationJob {
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationStarted {
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationDelta {
    pub job_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationFinished {
    pub job_id: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationError {
    pub job_id: String,
    pub message: String,
}

#[derive(Clone)]
pub struct TranslationService {
    client: reqwest::Client,
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl TranslationService {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start_translation(
        &self,
        app: AppHandle,
        payload: TranslationRequest,
    ) -> anyhow::Result<TranslationJob> {
        let selection_text = payload.selection_text.trim().to_string();
        if selection_text.is_empty() {
            anyhow::bail!("Select text before starting translation");
        }

        let job_id = Uuid::new_v4().to_string();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.jobs
            .lock()
            .expect("translation jobs poisoned")
            .insert(job_id.clone(), cancel_flag.clone());

        let service = self.clone();
        let task_job_id = job_id.clone();
        tauri::async_runtime::spawn(async move {
            let result = service
                .run_translation(
                    app.clone(),
                    task_job_id.clone(),
                    payload,
                    cancel_flag.clone(),
                )
                .await;

            service
                .jobs
                .lock()
                .expect("translation jobs poisoned")
                .remove(&task_job_id);

            if let Err(error) = result {
                let _ = app.emit(
                    "translation_error",
                    TranslationError {
                        job_id: task_job_id,
                        message: error.to_string(),
                    },
                );
            }
        });

        Ok(TranslationJob { job_id })
    }

    pub fn cancel_translation(&self, job_id: &str) {
        if let Some(flag) = self
            .jobs
            .lock()
            .expect("translation jobs poisoned")
            .get(job_id)
        {
            flag.store(true, Ordering::SeqCst);
        }
    }

    async fn run_translation(
        &self,
        app: AppHandle,
        job_id: String,
        payload: TranslationRequest,
        cancel_flag: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        let settings = settings::get_settings(&app)?;
        let provider = settings
            .llm_providers
            .iter()
            .find(|provider| provider.id == payload.provider_id)
            .or_else(|| settings.llm_providers.first())
            .ok_or_else(|| anyhow::anyhow!("No LLM provider is configured"))?
            .clone();
        let api_key = settings::get_api_key(&provider.id)?;
        if api_key.trim().is_empty() {
            anyhow::bail!("API key is not configured for {}", provider.name);
        }

        let profile = settings::select_prompt_profile(
            &settings,
            payload.prompt_profile_id.as_deref(),
            &payload.source_language,
            &payload.target_language,
        )
        .ok_or_else(|| anyhow::anyhow!("No prompt profile is configured"))?;
        let prompt = settings::render_prompt(
            &profile.prompt_template,
            &payload.source_language,
            &payload.target_language,
            payload.selection_text.trim(),
        );

        app.emit(
            "translation_started",
            TranslationStarted {
                job_id: job_id.clone(),
            },
        )?;

        let response = self
            .client
            .post(chat_completions_endpoint(&provider))
            .bearer_auth(api_key)
            .json(&build_chat_request(&provider, &prompt))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM request failed with {}: {}", status, body);
        }

        let mut stream = response.bytes_stream();
        let mut pending = String::new();

        while let Some(chunk) = stream.next().await {
            if cancel_flag.load(Ordering::SeqCst) {
                app.emit(
                    "translation_finished",
                    TranslationFinished {
                        job_id,
                        cancelled: true,
                    },
                )?;
                return Ok(());
            }

            let chunk = chunk?;
            pending.push_str(std::str::from_utf8(&chunk)?);
            emit_complete_sse_lines(&app, &job_id, &mut pending)?;
        }

        if !pending.trim().is_empty() {
            emit_sse_line(&app, &job_id, pending.trim())?;
        }

        app.emit(
            "translation_finished",
            TranslationFinished {
                job_id,
                cancelled: false,
            },
        )?;
        Ok(())
    }
}

impl Default for TranslationService {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_chat_request(provider: &LlmProviderSettings, prompt: &str) -> Value {
    json!({
        "model": provider.model,
        "stream": true,
        "temperature": 0.2,
        "messages": [
            {
                "role": "user",
                "content": prompt
            }
        ]
    })
}

fn chat_completions_endpoint(provider: &LlmProviderSettings) -> String {
    format!(
        "{}/v1/chat/completions",
        provider.base_url.trim_end_matches('/')
    )
}

fn emit_complete_sse_lines(
    app: &AppHandle,
    job_id: &str,
    pending: &mut String,
) -> anyhow::Result<()> {
    while let Some(newline_index) = pending.find('\n') {
        let line = pending[..newline_index].trim().to_string();
        pending.replace_range(..=newline_index, "");
        emit_sse_line(app, job_id, &line)?;
    }
    Ok(())
}

fn emit_sse_line(app: &AppHandle, job_id: &str, line: &str) -> anyhow::Result<()> {
    let Some(data) = line.strip_prefix("data:").map(str::trim) else {
        return Ok(());
    };
    if data == "[DONE]" || data.is_empty() {
        return Ok(());
    }

    let value: Value = serde_json::from_str(data)?;
    let Some(delta) = value["choices"]
        .get(0)
        .and_then(|choice| choice["delta"]["content"].as_str())
    else {
        return Ok(());
    };

    if !delta.is_empty() {
        app.emit(
            "translation_delta",
            TranslationDelta {
                job_id: job_id.to_string(),
                delta: delta.to_string(),
            },
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::settings::default_settings;

    use super::build_chat_request;

    #[test]
    fn chat_request_uses_streaming_model_and_prompt() {
        let settings = default_settings();
        let provider = settings.llm_providers.first().unwrap();
        let body = build_chat_request(provider, "Translate this");

        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["content"], "Translate this");
    }
}
