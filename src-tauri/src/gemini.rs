// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Bulbul contributors

use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.5-transcribe:generateContent";

/// Curated development & coding vocabulary for Banglish code-switching dictation.
/// Biases Gemini 3.5 Transcribe to keep programming terms in proper English script
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

#[derive(Serialize)]
struct InlineData<'a> {
    mime_type: &'a str,
    data: String,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Part<'a> {
    Inline { inline_data: InlineData<'a> },
}

#[derive(Serialize)]
struct Content<'a> {
    role: &'a str,
    parts: Vec<Part<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioTranscriptionConfig {
    language_codes: Vec<String>,
    mode: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    custom_vocabulary: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    audio_transcription_config: AudioTranscriptionConfig,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest<'a> {
    contents: Vec<Content<'a>>,
    generation_config: GenerationConfig,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioTranscription {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPart {
    #[serde(default)]
    text: String,
    #[serde(default)]
    audio_transcription: Option<AudioTranscription>,
}

#[derive(Deserialize)]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    #[serde(default)]
    content: Option<GeminiContent>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    error: Option<GeminiError>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GeminiError {
    message: String,
    code: Option<u32>,
}

/// Transcribes audio using Google's `gemini-3.5-transcribe` model in SMART or VERBATIM mode.
/// Returns formatted, punctuated Bengali or Banglish text with no filler words.
pub async fn transcribe(
    cfg: &Config,
    wav_bytes: Vec<u8>,
    vocabulary: &[String],
    is_coding_mode: bool,
    mode: Option<&str>,
) -> Result<String> {
    let key = cfg.gemini_api_key.trim();
    if key.is_empty() {
        return Err(anyhow!(
            "Google Gemini API key not set. Please add your key in Settings ▸ Account."
        ));
    }

    if wav_bytes.is_empty() {
        return Ok(String::new());
    }

    // Build custom vocabulary list:
    // Coding mode incorporates standard developer terminology so coding terms stay in English.
    let mut custom_vocab: Vec<String> = if is_coding_mode {
        BANGLISH_CODING_VOCABULARY
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };

    // Append user's personal dictionary entries
    for word in vocabulary {
        let trimmed = word.trim();
        if !trimmed.is_empty() && !custom_vocab.iter().any(|v| v.eq_ignore_ascii_case(trimmed)) {
            custom_vocab.push(trimmed.to_string());
        }
    }

    // Cap vocabulary at 1000 items (Gemini API limit)
    if custom_vocab.len() > 1000 {
        custom_vocab.truncate(1000);
    }

    let b64_audio = BASE64.encode(&wav_bytes);

    let selected_mode = match mode {
        Some(m) if m.eq_ignore_ascii_case("verbatim") => "VERBATIM",
        _ => "SMART",
    };

    let request = GeminiRequest {
        contents: vec![Content {
            role: "user",
            parts: vec![Part::Inline {
                inline_data: InlineData {
                    mime_type: "audio/wav",
                    data: b64_audio,
                },
            }],
        }],
        generation_config: GenerationConfig {
            audio_transcription_config: AudioTranscriptionConfig {
                language_codes: vec!["bn-IN".to_string(), "en-IN".to_string()],
                mode: selected_mode.to_string(),
                custom_vocabulary: custom_vocab,
            },
        },
    };

    let url = format!("{GEMINI_API_BASE}?key={key}");
    let client = shared_client();

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .context("sending audio to Gemini 3.5 Transcribe")?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .context("reading Gemini transcription response body")?;

    if !status.is_success() {
        if let Ok(parsed) = serde_json::from_str::<GeminiResponse>(&body) {
            if let Some(err) = parsed.error {
                return Err(anyhow!("Gemini error ({}): {}", status, err.message));
            }
        }
        return Err(anyhow!("Gemini request failed ({status}): {body}"));
    }

    let parsed: GeminiResponse = serde_json::from_str(&body)
        .with_context(|| format!("parsing Gemini transcription response: {body}"))?;

    let text = parsed
        .candidates
        .into_iter()
        .filter_map(|c| c.content)
        .flat_map(|c| c.parts)
        .map(|p| {
            if let Some(at) = p.audio_transcription {
                if !at.text.is_empty() {
                    return at.text;
                }
            }
            p.text
        })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string();

    tracing::info!("Gemini transcription extracted text ({} chars): '{text}'", text.chars().count());

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

