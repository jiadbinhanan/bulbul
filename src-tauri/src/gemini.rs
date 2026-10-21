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
    mode: &'static str,
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
struct GeminiPart {
    #[serde(default)]
    text: String,
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
struct GeminiError {
    message: String,
    code: Option<u32>,
}

/// Transcribes audio using Google's `gemini-3.5-transcribe` model in single-pass SMART mode.
/// Returns already-formatted, punctuated Bengali or Banglish text with no filler words.
pub async fn transcribe(
    cfg: &Config,
    wav_bytes: Vec<u8>,
    vocabulary: &[String],
    is_coding_mode: bool,
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
                language_codes: vec!["bn-IN".to_string()],
                mode: "SMART",
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
        .map(|p| p.text)
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string();

    Ok(text)
}
