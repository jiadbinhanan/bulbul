// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Bulbul contributors

use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::sync::OnceLock;
/// Curated development & coding vocabulary for Banglish code-switching dictation.
/// Biases Gemini models to keep programming terms in proper English script
/// rather than phonetic Bengali.
pub const BANGLISH_CODING_VOCABULARY: &[&str] = &[
    "Next.js", "React", "TypeScript", "JavaScript", "Node.js", "PostgreSQL",
    "Drizzle ORM", "Prisma", "Tailwind CSS", "CSS", "HTML", "component",
    "components", "state", "props", "hook", "hooks", "useEffect", "useState",
    "useMemo", "useCallback", "useRef", "useContext", "API route", "API",
    "APIs", "endpoint", "endpoints", "REST", "REST API", "GraphQL", "function",
    "functions", "async", "await", "promise", "database", "databases", "schema",
    "schemas", "table", "tables", "migration", "migrations", "query", "queries",
    "frontend", "backend", "full-stack", "fullstack", "bug", "bugs", "deploy",
    "deployment", "build", "production", "development", "staging", "Docker",
    "container", "Git", "GitHub", "commit", "push", "pull", "pull request",
    "PR", "branch", "branches", "merge", "rebase", "repo", "repository",
    "import", "export", "interface", "interfaces", "type", "types", "package",
    "npm", "pnpm", "yarn", "bun", "Vite", "Tauri", "Rust", "Python", "SDK",
    "auth", "authentication", "authorization", "session", "token", "JWT",
    "middleware", "route", "routes", "router", "handler", "payload", "request",
    "response", "status code", "JSON", "CLI", "terminal", "localhost", "server",
    "client", "error", "errors", "exception", "debug", "debugging", "refactor",
    "render", "re-render", "DOM", "layout", "page", "modal", "dialog", "button",
    "input", "form", "submit", "boolean", "string", "number", "null", "undefined",
    "array", "object", "map", "filter", "reduce", "loop", "key", "value",
    "params", "cache", "cookie", "header", "headers", "URL", "href", "src"
];

fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// Ordered fallback list for Google Gemini speech-to-text models.
/// Strict fallback order:
/// 1. `gemini-3.5-transcribe` (primary specialized transcription model)
/// 2. `gemini-3.6-flash` (fastest multimodal fallback)
/// 3. `gemini-3.5-flash` (standard multimodal fallback)
/// 4. `gemini-3.5-flash-lite` (lightweight resilient fallback)
pub const GEMINI_FALLBACK_MODELS: &[&str] = &[
    "gemini-3.5-transcribe",
    "gemini-3.6-flash",
    "gemini-3.5-flash",
    "gemini-3.5-flash-lite",
];

/// Returns a human-friendly short model name for display on the overlay pill.
pub fn short_model_name(model: &str) -> &str {
    match model {
        "gemini-3.5-transcribe" => "3.5-transcribe",
        "gemini-3.6-flash" => "3.6-flash",
        "gemini-3.5-flash" => "3.5-flash",
        "gemini-3.5-flash-lite" => "3.5-flash-lite",
        other => other.strip_prefix("gemini-").unwrap_or(other),
    }
}

/// Checks whether an error is transient/overload and should trigger fallback to the next model.
pub fn is_fallback_candidate_error(status: u16, err_msg: &str) -> bool {
    if status == 503 || status == 429 || status == 500 || status == 502 || status == 504 || status == 404 {
        return true;
    }
    let lower = err_msg.to_lowercase();
    lower.contains("unavailable")
        || lower.contains("overloaded")
        || lower.contains("resource has been exhausted")
        || lower.contains("quota")
        || lower.contains("rate limit")
        || lower.contains("high demand")
        || lower.contains("temporarily")
        || lower.contains("not found")
}

#[derive(Debug)]
pub struct GeminiErrorDetail {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for GeminiErrorDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Gemini error ({}): {}", self.status, self.message)
    }
}

impl std::error::Error for GeminiErrorDetail {}

/// Builds custom vocabulary list merging Banglish coding terms and user's dictionary.
pub fn build_custom_vocab(vocabulary: &[String], is_coding_mode: bool) -> Vec<String> {
    let mut custom_vocab: Vec<String> = if is_coding_mode {
        BANGLISH_CODING_VOCABULARY
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };

    for word in vocabulary {
        let trimmed = word.trim();
        if !trimmed.is_empty() && !custom_vocab.iter().any(|v| v.eq_ignore_ascii_case(trimmed)) {
            custom_vocab.push(trimmed.to_string());
        }
    }

    if custom_vocab.len() > 1000 {
        custom_vocab.truncate(1000);
    }

    custom_vocab
}

/// Builds the system prompt for general multimodal Gemini models (gemini-3.8-flash, etc.).
/// Since non-transcribe Gemini models accept multimodal audio + instructions in a SINGLE call,
/// this instructs the model to perform smart speech-to-text dictation with script fidelity,
/// proper Bengali/Banglish handling, disfluency removal, and developer term preservation.
pub fn build_multimodal_transcription_prompt(
    is_coding_mode: bool,
    mode: Option<&str>,
    custom_vocab: &[String],
) -> String {
    let verbatim = mode.map(|m| m.eq_ignore_ascii_case("verbatim")).unwrap_or(false);

    let mut prompt = String::from(
        "You are an expert, highly accurate speech-to-text dictation engine. \
        Transcribe the user's spoken audio faithfully into clean, natural text.\n\n\
        Rules & Guidelines:\n\
        1. Language & Script Fidelity:\n\
           - The speech contains Bengali (বাংলা), English, or mixed Banglish (Bengali speech with English technical terms).\n\
           - Transcribe standard Bengali words in authentic Bengali script (বাংলা লিপি).\n\
           - Write English technical, programming, and development terms in standard English script (e.g. 'React component', 'API route', 'PR merge', 'state hook', 'bug fix'). Do not transliterate technical words into phonetic Bengali script.\n\
        2. Transcription Mode:\n"
    );

    if verbatim {
        prompt.push_str(
            "   - Mode: VERBATIM. Transcribe every spoken word and sound precisely as uttered, keeping stutters and repetitions.\n"
        );
    } else {
        prompt.push_str(
            "   - Mode: SMART. Format sentences with proper capitalization and punctuation (full stops, commas, question marks). Silently remove stutters, false starts, filler sounds (uh, um, mane, mane ki, arki), and throat clears.\n"
        );
    }

    if is_coding_mode {
        prompt.push_str(
            "3. Developer / Coding Context:\n\
               - The speaker is dictating software engineering workflows. Keep all syntax, code keywords, variable naming patterns, and CLI commands in proper English spelling.\n"
        );
    }

    if !custom_vocab.is_empty() {
        prompt.push_str(&format!(
            "4. Priority Vocabulary Dictionary:\n\
               - Keep the following terms in accurate spelling/casing: {}\n",
            custom_vocab.join(", ")
        ));
    }

    prompt.push_str(
        "\nOutput Rule:\n\
        Return ONLY the final transcribed text. Do NOT wrap in markdown quotes, and do NOT add any conversational preamble, intro, or outro."
    );

    prompt
}

/// Transcribes audio using a specific Google Gemini model.
/// For `gemini-3.5-transcribe`: uses native `audioTranscriptionConfig`.
/// For general multimodal models (e.g. `gemini-3.8-flash`, `gemini-3.6-flash`, `gemini-3.5-flash`, etc.):
/// uses single-step multimodal audio + system prompt dictation.
pub async fn transcribe_model(
    cfg: &Config,
    model: &str,
    wav_bytes: &[u8],
    vocabulary: &[String],
    is_coding_mode: bool,
    mode: Option<&str>,
) -> std::result::Result<String, GeminiErrorDetail> {
    let key = cfg.gemini_api_key.trim();
    if key.is_empty() {
        return Err(GeminiErrorDetail {
            status: 401,
            message: "Google Gemini API key not set. Please add your key in Settings ▸ Account.".to_string(),
        });
    }

    if wav_bytes.is_empty() {
        return Ok(String::new());
    }

    let custom_vocab = build_custom_vocab(vocabulary, is_coding_mode);
    let mime_type = if wav_bytes.starts_with(b"OggS") {
        "audio/ogg"
    } else {
        "audio/wav"
    };
    let b64_audio = BASE64.encode(wav_bytes);
    let client = shared_client();

    let target_model = if model.trim().is_empty() {
        "gemini-3.5-transcribe"
    } else {
        model.trim()
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        target_model, key
    );

    let is_transcribe_specialized = target_model.eq_ignore_ascii_case("gemini-3.5-transcribe")
        || target_model.to_lowercase().contains("transcribe");

    let req_body = if is_transcribe_specialized {
        let selected_mode = match mode {
            Some(m) if m.eq_ignore_ascii_case("verbatim") => "VERBATIM",
            _ => "SMART",
        };
        serde_json::json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {
                            "inlineData": {
                                "mimeType": mime_type,
                                "data": b64_audio
                            }
                        }
                    ]
                }
            ],
            "generationConfig": {
                "audioTranscriptionConfig": {
                    "languageCodes": ["bn-IN", "en-IN"],
                    "mode": selected_mode,
                    "customVocabulary": custom_vocab
                }
            }
        })
    } else {
        // Multimodal model (e.g. gemini-3.8-flash, gemini-3.6-flash, gemini-3.5-flash, gemini-3.5-flash-lite, gemini-2.5-flash)
        let sys_prompt = build_multimodal_transcription_prompt(is_coding_mode, mode, &custom_vocab);
        serde_json::json!({
            "systemInstruction": {
                "parts": [{ "text": sys_prompt }]
            },
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {
                            "inlineData": {
                                "mimeType": mime_type,
                                "data": b64_audio
                            }
                        },
                        {
                            "text": "Transcribe this audio recording accurately."
                        }
                    ]
                }
            ],
            "generationConfig": {
                "temperature": 0.1
            }
        })
    };

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&req_body)
        .send()
        .await
        .map_err(|e| GeminiErrorDetail {
            status: 0,
            message: format!("network error connecting to {target_model}: {e:#}"),
        })?;

    let status = resp.status();
    let status_code = status.as_u16();
    let body = resp.text().await.map_err(|e| GeminiErrorDetail {
        status: status_code,
        message: format!("reading response from {target_model}: {e:#}"),
    })?;

    if !status.is_success() {
        let mut err_msg = body.clone();
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(msg) = parsed["error"]["message"].as_str() {
                err_msg = msg.to_string();
            }
        }
        return Err(GeminiErrorDetail {
            status: status_code,
            message: err_msg,
        });
    }

    let val: serde_json::Value = serde_json::from_str(&body).map_err(|e| GeminiErrorDetail {
        status: status_code,
        message: format!("parsing JSON from {target_model}: {e:#}"),
    })?;

    let mut output = String::new();
    if let Some(candidates) = val["candidates"].as_array() {
        for candidate in candidates {
            if let Some(parts) = candidate["content"]["parts"].as_array() {
                for part in parts {
                    // Skip thinking parts if reasoning model
                    if part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false) {
                        continue;
                    }
                    // For gemini-3.5-transcribe, text might be under audioTranscription.text or text
                    if let Some(at) = part.get("audioTranscription") {
                        if let Some(txt) = at.get("text").and_then(|v| v.as_str()) {
                            if !txt.is_empty() {
                                output.push_str(txt);
                                continue;
                            }
                        }
                    }
                    if let Some(txt) = part.get("text").and_then(|v| v.as_str()) {
                        output.push_str(txt);
                    }
                }
            }
        }
    }

    let text = output.trim().to_string();
    tracing::info!(
        "Gemini ({target_model}) transcription extracted text ({} chars): '{text}'",
        text.chars().count()
    );

    Ok(text)
}

/// Transcribes audio using Google's Gemini models with automatic fallback.
/// If a model is busy (503), rate-limited (429), or unavailable, `on_fallback`
/// is invoked with (failed_model, next_model, status_code) and the next model is tried.
pub async fn transcribe_with_fallback<F>(
    cfg: &Config,
    wav_bytes: &[u8],
    vocabulary: &[String],
    is_coding_mode: bool,
    mode: Option<&str>,
    preferred_model: Option<&str>,
    mut on_fallback: F,
) -> Result<(String, String)>
where
    F: FnMut(&str, &str, u16),
{
    let mut models_to_try: Vec<&str> = Vec::new();

    if let Some(pref) = preferred_model {
        let trimmed = pref.trim();
        if !trimmed.is_empty() && trimmed != "gemini-auto-fallback" {
            models_to_try.push(trimmed);
        }
    }

    for fallback in GEMINI_FALLBACK_MODELS {
        if !models_to_try.contains(fallback) {
            models_to_try.push(fallback);
        }
    }

    let mut last_err = anyhow!("No Gemini models available.");

    for i in 0..models_to_try.len() {
        let current_model = models_to_try[i];
        match transcribe_model(cfg, current_model, wav_bytes, vocabulary, is_coding_mode, mode).await {
            Ok(transcript) => {
                return Ok((transcript, current_model.to_string()));
            }
            Err(err) => {
                tracing::warn!(
                    "Gemini model '{current_model}' failed with status {}: {}",
                    err.status,
                    err.message
                );

                let is_recoverable = is_fallback_candidate_error(err.status, &err.message);
                let has_next = i + 1 < models_to_try.len();

                if is_recoverable && has_next {
                    let next_model = models_to_try[i + 1];
                    on_fallback(current_model, next_model, err.status);
                    last_err = anyhow!("{current_model} failed ({}): {}", err.status, err.message);
                    continue;
                } else {
                    return Err(anyhow!("{current_model} failed ({}): {}", err.status, err.message));
                }
            }
        }
    }

    Err(last_err)
}

/// Transcribes audio using Google's default fallback sequence.
/// Backward-compatible wrapper over `transcribe_with_fallback`.
#[allow(dead_code)]
pub async fn transcribe(
    cfg: &Config,
    wav_bytes: Vec<u8>,
    vocabulary: &[String],
    is_coding_mode: bool,
    mode: Option<&str>,
) -> Result<String> {
    let (text, _) = transcribe_with_fallback(
        cfg,
        &wav_bytes,
        vocabulary,
        is_coding_mode,
        mode,
        None,
        |_, _, _| {},
    )
    .await?;
    Ok(text)
}

/// Runs a Phase 2 LLM post-processing pass using Google Gemini models (e.g. gemini-3.8-flash,
/// gemini-3.5-flash-lite, gemini-2.5-flash) with optional reasoning / thinking_level control.
pub async fn post_process(
    cfg: &Config,
    model: &str,
    system_prompt: &str,
    user_input: &str,
    thinking_level: Option<&str>,
) -> Result<String> {
    let key = cfg.gemini_api_key.trim();
    if key.is_empty() {
        return Err(anyhow!("Google Gemini API key not set for Phase 2 post-processing."));
    }

    let model_name = if model.trim().is_empty() {
        "gemini-3.8-flash"
    } else {
        model.trim()
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model_name, key
    );

    let mut root = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [{ "text": user_input }]
            }
        ]
    });

    if !system_prompt.trim().is_empty() {
        root["systemInstruction"] = serde_json::json!({
            "parts": [{ "text": system_prompt.trim() }]
        });
    }

    let mut gen_config = serde_json::json!({
        "temperature": 0.3
    });

    if let Some(level) = thinking_level {
        let lvl = level.to_lowercase();
        if lvl == "minimal" || lvl == "low" || lvl == "medium" || lvl == "high" {
            gen_config["thinkingConfig"] = serde_json::json!({
                "thinkingLevel": lvl
            });
        }
    }
    root["generationConfig"] = gen_config;

    let client = shared_client();
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&root)
        .send()
        .await
        .context("sending Phase 2 post-processing request to Gemini")?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .context("reading Gemini post-process response")?;

    if !status.is_success() {
        return Err(anyhow!("Gemini post-process failed ({status}): {body}"));
    }

    let val: serde_json::Value = serde_json::from_str(&body)
        .with_context(|| format!("parsing Gemini post-process JSON: {body}"))?;

    let mut output = String::new();
    if let Some(candidates) = val["candidates"].as_array() {
        if let Some(first) = candidates.first() {
            if let Some(parts) = first["content"]["parts"].as_array() {
                for part in parts {
                    if part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false) {
                        continue;
                    }
                    if let Some(txt) = part["text"].as_str() {
                        output.push_str(txt);
                    }
                }
            }
        }
    }

    Ok(output.trim().to_string())
}

