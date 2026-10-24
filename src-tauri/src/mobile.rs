// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Romanch Roshan Singh

// Bulbul on Android (and eventually iOS).
//
// Phase 1: Tauri app that boots, opens the WebView, loads dist/index.html.
// Phase 2: stubbed the mount-time invoke commands so the loading screen
// advances to actual UI.
// Phase 3 (this): stub every command the React shell calls when the
// user navigates between dashboard pages, so each view renders its
// empty state cleanly instead of erroring in the console. Read
// commands return empty Vecs / zeroed shapes; mutating commands are
// no-ops; expensive commands (transforms, Groq round-trips) return an
// "not supported on Android yet" error that the UI already handles.
//
// Every stub is replaced with a real implementation as the
// corresponding feature lands on mobile — most of that work is in the
// floating-bubble overlay + AccessibilityService path, not the
// dashboard, so these stubs may stay in place for a while.
//
// What's deliberately NOT here:
//   - cpal-based audio capture (replaced by Kotlin AudioRecord + JNI)
//   - tauri-plugin-global-shortcut (no global hotkeys on Android)
//   - tauri-plugin-single-instance (Android process model handles this)
//   - tray icon / overlay window / hover-watcher (different on mobile)
//   - hotkey orchestrator, dictation pipeline (pulled in once audio
//     bridge is in place)

mod config;
mod db;
mod gemini;
mod groq;
mod model_config;
pub mod opus_encoder;
pub mod webdav;

use std::time::Instant;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use config::Config;
use serde_json::{json, Value};
use tauri::Manager;

/// Where Bulbul keeps mobile config on disk. Tauri's app_data_dir on
/// Android resolves to Context.getFilesDir() (i.e.
/// /data/data/com.bulbul.app/files/), so reading the same path from
/// Kotlin via `filesDir` is the bridge that lets BulbulForegroundService
/// pick up the Groq API key the React Settings UI saves here.
const MOBILE_CONFIG_FILE: &str = "config.json";

fn mobile_config_defaults(mut cfg: Config) -> Config {
    // Privacy is acknowledged implicitly on mobile (no separate privacy
    // modal). Onboarding is deliberately NOT forced here: the React wizard
    // runs once on first launch — after the native permission screen —
    // and persists its completion via complete_onboarding.
    cfg.privacy_acknowledged = true;
    cfg
}

// ---------- Startup commands (App.jsx useEffect on mount) ----------

/// Reads the persisted config from app-private storage, applying the
/// mobile defaults (privacy acknowledged; onboarding driven by the real
/// persisted flag so the first-launch wizard runs exactly once).
#[tauri::command]
fn get_config(app: tauri::AppHandle) -> Config {
    if let Ok(dir) = app.path().app_data_dir() {
        config::set_android_app_dir(dir);
    }
    let cfg = read_config(&app).unwrap_or_default();
    mobile_config_defaults(cfg)
}

/// Writes the config as JSON to <app_data_dir>/config.json. The Kotlin
/// foreground service reads the same file to pick up the Groq API
/// key — no JNI needed because both sides agree on the path
/// (Android's Context.getFilesDir() == Tauri's app_data_dir on this
/// platform).
#[tauri::command]
fn save_config(app: tauri::AppHandle, new_cfg: Config) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    config::set_android_app_dir(dir.clone());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(MOBILE_CONFIG_FILE);
    let json = serde_json::to_string_pretty(&new_cfg).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

fn read_config(app: &tauri::AppHandle) -> Option<Config> {
    let dir = app.path().app_data_dir().ok()?;
    config::set_android_app_dir(dir.clone());
    let path = dir.join(MOBILE_CONFIG_FILE);
    if !path.exists() {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let cfg = serde_json::from_str::<Config>(&text).ok()?;
    // Sweeps a stale/retired chat_model (e.g. a Groq model decommissioned
    // after this config was saved) onto the current default, same as
    // desktop's config::load() and the Kotlin bubble's chatModel(). Without
    // this, the dashboard's Groq calls (Insights voice profile, Transforms)
    // kept leading with a dead model and failing outright.
    Some(config::migrate(cfg))
}

/// Autostart on Android is governed by the BOOT_COMPLETED broadcast,
/// not the Tauri autostart plugin. Always returns false for now.
#[tauri::command]
fn get_autostart() -> Result<bool, String> {
    Ok(false)
}

#[tauri::command]
fn set_autostart(_enabled: bool) -> Result<(), String> {
    Ok(())
}

/// Tray doesn't exist on Android — there's a foreground service
/// notification instead (added with the floating bubble work).
#[tauri::command]
fn set_tray_visible(_visible: bool) -> Result<(), String> {
    Ok(())
}

// ---------- Home + Insights (read-only stats) ----------

/// One JSON object per line, appended by the Kotlin foreground service
/// after each successful transcription (see recordHistory). File-as-IPC,
/// same pattern as config.json.
const HISTORY_FILE: &str = "history.jsonl";

fn history_rows(app: &tauri::AppHandle) -> Vec<Value> {
    if let Ok(db) = db::open() {
        let conn = db.lock();
        let _ = db::sync_legacy_dictations_from_recordings(&conn);
        if let Ok(mut stmt) = conn.prepare(
            "SELECT ts, raw_text, cleaned_text, mode, language, foreground_app, duration_ms, word_count, fix_count
             FROM dictations
             ORDER BY ts ASC",
        ) {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok(json!({
                    "ts": row.get::<_, i64>(0)?,
                    "raw_text": row.get::<_, String>(1)?,
                    "cleaned_text": row.get::<_, String>(2)?,
                    "mode": row.get::<_, String>(3)?,
                    "language": row.get::<_, String>(4)?,
                    "foreground_app": row.get::<_, Option<String>>(5)?,
                    "duration_ms": row.get::<_, i64>(6)?,
                    "word_count": row.get::<_, i64>(7)?,
                    "fix_count": row.get::<_, i64>(8)?,
                }))
            }) {
                let list: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
                if !list.is_empty() {
                    return list;
                }
            }
        }
    }

    let Ok(dir) = app.path().app_data_dir() else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(dir.join(HISTORY_FILE)) else { return Vec::new() };
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

#[tauri::command]
fn get_home_stats() -> Result<db::HomeStats, String> {
    let db = db::open().map_err(|e| format!("{e:#}"))?;
    db::home_stats(&db).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn get_recent_dictations(app: tauri::AppHandle, limit: u32, offset: u32) -> Vec<Value> {
    let rows = history_rows(&app);
    rows.iter()
        .rev() // newest first
        .skip(offset as usize)
        .take(limit as usize)
        .map(|r| {
            json!({
                // ts doubles as the row's stable id — history.jsonl has no
                // dedicated id field (unlike desktop's SQLite autoincrement),
                // and a full dictation pipeline (STT + cleanup round-trip)
                // can't realistically complete twice within the same
                // second, so this is unique in practice. A read-time
                // position (offset+i) would shift under any delete/insert,
                // silently pointing a later delete_dictation at the wrong
                // row once the list had moved.
                "id": r["ts"].as_i64().unwrap_or(0),
                "ts": r["ts"],
                "cleaned_text": r["cleaned_text"],
                "foreground_app": r.get("foreground_app").cloned().unwrap_or(Value::Null),
                "mode": r.get("mode").cloned().unwrap_or(json!("clean")),
                "word_count": r["word_count"],
            })
        })
        .collect()
}

/// Removes the history.jsonl row whose ts matches [id] (see
/// get_recent_dictations for why ts is used as the row id) and rewrites the
/// file without it. Errors if the app_data_dir can't be resolved; a missing
/// id (already deleted, e.g. a stale UI list) is a silent no-op rather than
/// an error, matching desktop's DELETE-affects-zero-rows behavior.
#[tauri::command]
fn delete_dictation(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let rows = history_rows(&app);
    let kept: Vec<Value> = rows
        .into_iter()
        .filter(|r| r["ts"].as_i64() != Some(id))
        .collect();
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut text = kept.iter().map(|r| r.to_string()).collect::<Vec<_>>().join("\n");
    if !kept.is_empty() {
        text.push('\n');
    }
    std::fs::write(dir.join(HISTORY_FILE), text).map_err(|e| e.to_string())
}

// ---------- File-backed stores (transforms, dictionary) ----------
//
// Desktop keeps these in SQLite; the mobile build doesn't pull in rusqlite,
// so each list is a JSON array on disk next to config.json / history.jsonl
// (the same app_data_dir file-as-IPC pattern). Seeded from defaults on
// first run — but only when the file is ABSENT, so deleting every entry
// stays deleted instead of respawning the seeds.

const TRANSFORMS_FILE: &str = "transforms.json";
const DICTIONARY_FILE: &str = "dictionary.json";

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn read_json_array(app: &tauri::AppHandle, file: &str) -> Vec<Value> {
    let Ok(dir) = app.path().app_data_dir() else { return Vec::new() };
    match std::fs::read_to_string(dir.join(file)) {
        Ok(text) => serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn write_json_array(app: &tauri::AppHandle, file: &str, rows: &[Value]) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(rows).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(file), json).map_err(|e| e.to_string())
}

fn next_id(rows: &[Value]) -> i64 {
    rows.iter().filter_map(|r| r["id"].as_i64()).max().unwrap_or(0) + 1
}

const VOICE_FILE: &str = "voice.json";

fn read_json_object(app: &tauri::AppHandle, file: &str) -> Option<Value> {
    let dir = app.path().app_data_dir().ok()?;
    let text = std::fs::read_to_string(dir.join(file)).ok()?;
    serde_json::from_str::<Value>(&text).ok()
}

fn write_json_object(app: &tauri::AppHandle, file: &str, obj: &Value) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(obj).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(file), json).map_err(|e| e.to_string())
}

// Overlay runtime state shared with the Kotlin side (file-as-IPC, same as
// history/dictionary). Kept OUT of config.json so it isn't clobbered by the
// React settings save, which overwrites the whole config object. Holds the
// snooze deadline: `snoozed_until` is a unix-seconds timestamp (0 = active).
const OVERLAY_FILE: &str = "overlay.json";

/// Seconds-since-epoch until which the overlay bubble is snoozed (0/none if
/// not snoozed). The Kotlin service writes this when the user drops the bubble
/// on the snooze target; the Settings page reads it to show the state.
#[tauri::command]
fn get_overlay_snoozed_until(app: tauri::AppHandle) -> i64 {
    read_json_object(&app, OVERLAY_FILE)
        .and_then(|v| v.get("snoozed_until").and_then(Value::as_i64))
        .unwrap_or(0)
}

/// Resume the overlay immediately — clears any active snooze so the bubble
/// reappears the next time the user focuses a text field. Wired to the
/// "Resume now" button in Settings → Overlay.
#[tauri::command]
fn resume_overlay(app: tauri::AppHandle) -> Result<(), String> {
    write_json_object(&app, OVERLAY_FILE, &json!({ "snoozed_until": 0 }))
}

/// Groq reasoning models add latency + burn tokens unless disabled. Mirrors
/// desktop's `groq::reasoning_effort_for`, kept inline so the mobile build
/// stays independent of the desktop groq module: qwen fully disables reasoning
/// ("none"); gpt-oss accepts only low/medium/high (use "low"); others omit it.
fn reasoning_effort_for(model: &str) -> Option<&'static str> {
    if model.starts_with("qwen/") {
        Some("none")
    } else if model.starts_with("openai/gpt-oss") {
        Some("low")
    } else {
        None
    }
}

/// One-shot Groq chat completion. Mirrors `groq::chat` on desktop but kept
/// inline so the mobile build doesn't pull the whole desktop groq module.
/// Hard cap on attempts (1 initial + 3 retries) before giving up on a
/// rate-limited or transiently-failing Groq chat request. Mirrors groq.rs
/// (desktop), which mobile.rs otherwise deliberately doesn't import — kept
/// self-contained here for the same reason as the rest of this file.
const MOBILE_CHAT_MAX_ATTEMPTS: u32 = 4;

/// How long to wait before the next attempt: the server's `Retry-After`
/// header (seconds) if present, else exponential backoff (2s, 4s, 8s…)
/// capped at 30s.
fn mobile_retry_wait_secs(resp: &reqwest::Response, attempt: u32) -> u64 {
    if let Some(secs) = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        return secs.clamp(1, 30);
    }
    2u64.saturating_pow(attempt).min(30)
}

async fn groq_chat(
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
    temperature: f32,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))?;
    let mut body = json!({
        "model": model,
        "temperature": temperature,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });
    if let Some(effort) = reasoning_effort_for(model) {
        body["reasoning_effort"] = json!(effort);
    }

    // Retry on 429 (rate limit) and 5xx with backoff, honoring Retry-After —
    // previously this was a single bare attempt, so any rate limit (or a
    // stale/retired model still in the config, see read_config's migrate
    // call) surfaced the raw Groq error straight to the Insights UI.
    let mut attempt = 0u32;
    let resp = loop {
        attempt += 1;
        let resp = client
            .post("https://api.groq.com/openai/v1/chat/completions")
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Network error: {e}"))?;
        let status = resp.status();
        if (status.as_u16() == 429 || status.is_server_error()) && attempt < MOBILE_CHAT_MAX_ATTEMPTS {
            let wait = mobile_retry_wait_secs(&resp, attempt);
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            continue;
        }
        break resp;
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status.as_u16() == 429 {
            return Err(
                "The API is rate-limited right now. Wait a few seconds and try again.".to_string(),
            );
        }
        return Err(format!("Groq error ({status}): {text}"));
    }
    let v: Value = resp.json().await.map_err(|e| format!("parse error: {e}"))?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Groq returned an empty response".to_string())
}

/// Transform/cleanup model fallback chain, primary-first — mirrors desktop's
/// groq.rs CLEANUP_FALLBACK. The user's chat_model is tried first, then these,
/// deduped; each has its own Groq rate bucket.
const CHAT_FALLBACK: &[&str] = &["openai/gpt-oss-20b", "openai/gpt-oss-120b"];

/// Chat completion with model fallthrough: try the primary (chat_model), then
/// CHAT_FALLBACK (deduped), returning the first success. If every model fails
/// the last error surfaces. Mirrors desktop `execute_transform`, so a
/// rate-limited or decommissioned cleanup model no longer breaks a transform.
async fn groq_chat_chain(
    api_key: &str,
    primary: &str,
    system: &str,
    user: &str,
    temperature: f32,
) -> Result<String, String> {
    let mut chain: Vec<&str> = Vec::new();
    let p = primary.trim();
    if !p.is_empty() {
        chain.push(p);
    }
    for &m in CHAT_FALLBACK {
        if !chain.contains(&m) {
            chain.push(m);
        }
    }
    let mut last_err = "no models in the fallback chain".to_string();
    for model in chain {
        match groq_chat(api_key, model, system, user, temperature).await {
            Ok(out) => return Ok(out),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// Read a store, or seed + persist it if the file doesn't exist yet.
/// Keyed on file existence (not emptiness) so an emptied list stays empty.
fn load_store(app: &tauri::AppHandle, file: &str, seed: impl FnOnce() -> Vec<Value>) -> Vec<Value> {
    let absent = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| !d.join(file).exists())
        .unwrap_or(true);
    if absent {
        let seeded = seed();
        let _ = write_json_array(app, file, &seeded);
        seeded
    } else {
        read_json_array(app, file)
    }
}

/// Days-since-epoch → (year, month, day). Howard Hinnant's civil-from-days
/// algorithm — lets us bucket history by calendar month/day without pulling
/// a date crate into the mobile build.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Same coarse WPM → percentile buckets the desktop uses (db.rs), so the
/// gauge reads identically across platforms.
fn wpm_percentile_estimate(wpm: f64) -> f64 {
    if wpm >= 200.0 { 0.1 }
    else if wpm >= 150.0 { 1.0 }
    else if wpm >= 120.0 { 5.0 }
    else if wpm >= 100.0 { 20.0 }
    else if wpm >= 80.0 { 50.0 }
    else if wpm >= 60.0 { 75.0 }
    else { 90.0 }
}

/// Real usage stats computed from history.jsonl (the same file Home reads).
/// Matches `db::UsageStats` shape. Fix counts stay 0 — mobile doesn't track
/// per-dictation fixes yet — and app_usage stays empty (the React Insights
/// page hides the "Desktop usage" panel on Android).
#[tauri::command]
fn get_insights_usage(app: tauri::AppHandle) -> Value {
    let rows = history_rows(&app);
    let now = now_secs();
    let today = now.div_euclid(86_400);

    let total_words: i64 = rows.iter().map(|r| r["word_count"].as_i64().unwrap_or(0)).sum();
    // fix_count per row is a raw-vs-final word-level diff (BulbulForegroundService
    // countFixes), same meaning as desktop's count_fixes — it folds in the
    // Cleanup LLM's edits together with dictionary substitutions. Split back
    // out below via dictionary_fixes (summed from dictionary.json hit_count,
    // same shape as desktop's SQL), mirroring db.rs::usage_stats exactly.
    let total_fixes: i64 = rows.iter().map(|r| r["fix_count"].as_i64().unwrap_or(0)).sum();
    let dictionary_fixes: i64 = load_dictionary(&app)
        .iter()
        .map(|e| e["hit_count"].as_i64().unwrap_or(0))
        .sum();
    let ai_fixes = (total_fixes - dictionary_fixes).max(0);

    // WPM over the last 7 days — same window Home uses.
    let cutoff = now - 7 * 86_400;
    let (mut words_7d, mut ms_7d) = (0i64, 0i64);
    for r in &rows {
        if r["ts"].as_i64().unwrap_or(0) >= cutoff {
            words_7d += r["word_count"].as_i64().unwrap_or(0);
            ms_7d += r["duration_ms"].as_i64().unwrap_or(0);
        }
    }
    let wpm = if ms_7d > 0 { words_7d as f64 / (ms_7d as f64 / 60_000.0) } else { 0.0 };

    // Month-over-month words, by real calendar month.
    let (cy, cm, _) = civil_from_days(today);
    let (py, pm) = if cm == 1 { (cy - 1, 12) } else { (cy, cm - 1) };
    let (mut words_this_month, mut words_last_month) = (0i64, 0i64);
    for r in &rows {
        let (y, m, _) = civil_from_days(r["ts"].as_i64().unwrap_or(0).div_euclid(86_400));
        let w = r["word_count"].as_i64().unwrap_or(0);
        if y == cy && m == cm {
            words_this_month += w;
        } else if y == py && m == pm {
            words_last_month += w;
        }
    }
    let mom_change_pct = if words_last_month > 0 {
        Some((words_this_month - words_last_month) as f64 / words_last_month as f64 * 100.0)
    } else if words_this_month > 0 {
        None
    } else {
        Some(0.0)
    };

    // Streaks — distinct UTC days with at least one dictation.
    let mut days: Vec<i64> = rows
        .iter()
        .filter_map(|r| r["ts"].as_i64())
        .map(|ts| ts.div_euclid(86_400))
        .collect();
    days.sort_unstable();
    days.dedup();
    let mut day_streak = 0i64;
    while days.binary_search(&(today - day_streak)).is_ok() {
        day_streak += 1;
    }
    let (mut longest_streak, mut cur, mut prev) = (0i64, 0i64, None::<i64>);
    for &d in &days {
        cur = if prev == Some(d - 1) { cur + 1 } else { 1 };
        if cur > longest_streak {
            longest_streak = cur;
        }
        prev = Some(d);
    }

    // Heatmap: last 90 days as {date, count}. React keys by date string, so
    // order doesn't matter.
    let mut counts: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    for r in &rows {
        *counts
            .entry(r["ts"].as_i64().unwrap_or(0).div_euclid(86_400))
            .or_insert(0) += 1;
    }
    let heatmap: Vec<Value> = (0..90)
        .map(|i| {
            let d = today - i;
            let (y, m, day) = civil_from_days(d);
            json!({
                "date": format!("{:04}-{:02}-{:02}", y, m, day),
                "count": counts.get(&d).copied().unwrap_or(0),
            })
        })
        .collect();

    json!({
        "wpm": wpm,
        "wpm_percentile": wpm_percentile_estimate(wpm),
        "total_words": total_words,
        "words_this_month": words_this_month,
        "words_last_month": words_last_month,
        "mom_change_pct": mom_change_pct,
        "total_fixes": total_fixes,
        "ai_fixes": ai_fixes,
        "dictionary_fixes": dictionary_fixes,
        "day_streak": day_streak,
        "longest_streak": longest_streak,
        "total_apps_used": 0,
        "app_usage": [],
        "heatmap": heatmap,
    })
}

/// Matches `db::VoiceStats` shape. The narrative fields stay null (the
/// Groq-generated voice profile isn't wired on mobile yet), but total_words
/// and has_api_key come from real state so the "Your Voice" tab shows the
/// correct "dictate N more words to unlock" progress instead of a dead 0.
#[tauri::command]
fn voice_stats_value(app: &tauri::AppHandle) -> Value {
    let rows = history_rows(app);
    let total_words: i64 = rows.iter().map(|r| r["word_count"].as_i64().unwrap_or(0)).sum();
    let has_api_key = read_config(app).map(|c| c.has_api_key()).unwrap_or(false);

    // Newest-first, capped at 500 — same recency window desktop's
    // pull_cleaned_texts/pull_text_pairs use (db.rs) for the word-frequency
    // stats below. history.jsonl is written oldest-line-first.
    let recent: Vec<&Value> = rows.iter().rev().take(500).collect();
    let texts: Vec<String> = recent
        .iter()
        .filter_map(|r| r["cleaned_text"].as_str())
        .map(|s| s.to_string())
        .collect();
    let pairs: Vec<(String, String)> = recent
        .iter()
        .filter_map(|r| Some((r["raw_text"].as_str()?.to_string(), r["cleaned_text"].as_str()?.to_string())))
        .collect();

    let most_used_word = top_meaningful_word(&texts);
    let most_corrected_word = top_corrected_word(&pairs);
    let catchphrase = top_catchphrase(&texts);

    // Peak (day-of-week, hour) bucket across all history. Rows written after
    // this fix carry real local "dow"/"hour" (BulbulForegroundService::
    // recordHistory, via the JVM's timezone database); older rows fall back
    // to a UTC-derived bucket from `ts` so they still count rather than
    // vanishing from the stat.
    let bucket_for = |r: &Value| -> Option<(i64, i64)> {
        let ts = r["ts"].as_i64()?;
        let dow = r.get("dow").and_then(|v| v.as_i64())
            .unwrap_or_else(|| (ts.div_euclid(86_400) + 4).rem_euclid(7));
        let hour = r.get("hour").and_then(|v| v.as_i64())
            .unwrap_or_else(|| ts.rem_euclid(86_400) / 3_600);
        Some((dow, hour))
    };
    let mut bucket_counts: std::collections::HashMap<(i64, i64), i64> = std::collections::HashMap::new();
    for r in &rows {
        if let Some(b) = bucket_for(r) {
            *bucket_counts.entry(b).or_insert(0) += 1;
        }
    }
    let peak_bucket = bucket_counts.into_iter().max_by_key(|(_, n)| *n).map(|(b, _)| b);

    let (peak_day_name, peak_hour_label, peak_app, peak_app_category) = match peak_bucket {
        Some((dow, hour)) => {
            let mut app_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
            for r in &rows {
                if bucket_for(r) != Some((dow, hour)) {
                    continue;
                }
                if let Some(a) = r["foreground_app"].as_str() {
                    *app_counts.entry(a.to_string()).or_insert(0) += 1;
                }
            }
            let peak_app = app_counts.into_iter().max_by_key(|(_, n)| *n).map(|(a, _)| a);
            let peak_app_category = peak_app.as_deref().map(|a| categorize_app_mobile(a).to_string());
            (Some(day_name(dow).to_string()), Some(format_hour(hour)), peak_app, peak_app_category)
        }
        None => (None, None, None, None),
    };

    let voice = read_json_object(app, VOICE_FILE);
    let field = |k: &str| {
        voice
            .as_ref()
            .and_then(|v| v[k].as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    let last_generated_at = voice.as_ref().and_then(|v| v["last_generated_at"].as_i64());
    let words_at_gen = voice.as_ref().and_then(|v| v["words_at_gen"].as_i64()).unwrap_or(0);
    let words_since = (total_words - words_at_gen).max(0);

    json!({
        "most_used_word": most_used_word,
        "most_corrected_word": most_corrected_word,
        "catchphrase": catchphrase,
        "peak_day_name": peak_day_name,
        "peak_hour_label": peak_hour_label,
        "peak_app": peak_app,
        "peak_app_category": peak_app_category,
        "voice_narrative": field("voice_narrative"),
        "peak_narrative": field("peak_narrative"),
        "last_generated_at": last_generated_at,
        "words_since_last_gen": words_since,
        "min_words_to_refresh": 200,
        "total_words": total_words,
        "has_api_key": has_api_key,
    })
}

// ---------- Local voice-profile stats (ported from db.rs, JSON instead of
// SQL — mobile has no SQLite dependency; see the module doc comment) ----------

const VOICE_STOP_WORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "is", "are", "was", "were", "be", "been",
    "being", "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "must", "shall", "can", "of", "in", "on", "at", "to",
    "for", "with", "by", "from", "up", "about", "into", "through", "during", "before",
    "after", "above", "below", "between", "i", "you", "he", "she", "it", "we", "they",
    "me", "him", "her", "us", "them", "my", "your", "his", "its", "our", "their",
    "this", "that", "these", "those", "what", "which", "who", "whom", "whose", "where",
    "when", "why", "how", "all", "each", "every", "both", "few", "more", "most",
    "other", "some", "such", "no", "not", "only", "own", "same", "so", "than", "too",
    "very", "just", "as", "if", "any", "yes", "well", "okay", "ok", "yeah", "im",
    "youre", "theyre", "weve", "ive", "dont", "doesnt", "didnt", "wont", "cant",
    "isnt", "arent", "wasnt", "werent", "thats", "theres", "heres", "let", "lets",
    "go", "going", "get", "got", "really", "actually", "basically", "kind", "sort",
];

fn voice_tokenize_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

fn voice_is_meaningful(word: &str) -> bool {
    word.len() > 2 && !VOICE_STOP_WORDS.contains(&word)
}

fn top_meaningful_word(texts: &[String]) -> Option<String> {
    let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for text in texts {
        for tok in voice_tokenize_words(text) {
            if voice_is_meaningful(&tok) {
                *counts.entry(tok).or_insert(0) += 1;
            }
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(w, _)| w)
}

fn top_corrected_word(pairs: &[(String, String)]) -> Option<String> {
    let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for (raw, cleaned) in pairs {
        let raw_set: std::collections::HashSet<String> = voice_tokenize_words(raw).into_iter().collect();
        let cleaned_set: std::collections::HashSet<String> = voice_tokenize_words(cleaned).into_iter().collect();
        for token in raw_set.difference(&cleaned_set) {
            if voice_is_meaningful(token) {
                *counts.entry(token.clone()).or_insert(0) += 1;
            }
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(w, _)| w)
}

fn top_catchphrase(texts: &[String]) -> Option<String> {
    let mut ngrams: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for text in texts {
        let words: Vec<String> = text
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        for n in 3..=5 {
            if words.len() < n {
                continue;
            }
            for window in words.windows(n) {
                let all_stop = window.iter().all(|w| VOICE_STOP_WORDS.contains(&w.as_str()));
                if all_stop {
                    continue;
                }
                let phrase = window.join(" ");
                if phrase.len() < 12 {
                    continue;
                }
                *ngrams.entry(phrase).or_insert(0) += 1;
            }
        }
    }
    ngrams
        .into_iter()
        .filter(|(_, n)| *n >= 2)
        .max_by_key(|(p, n)| (*n, p.len() as i64))
        .map(|(p, _)| p)
}

fn day_name(dow: i64) -> &'static str {
    match dow {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        6 => "Saturday",
        _ => "—",
    }
}

fn format_hour(hour: i64) -> String {
    let (display, suffix) = match hour {
        0 => (12, "a.m."),
        1..=11 => (hour, "a.m."),
        12 => (12, "p.m."),
        _ => (hour - 12, "p.m."),
    };
    format!("{display} {suffix}")
}

/// Friendly app name → the same category vocabulary db.rs's categorize_app
/// uses on desktop. Keyed on the launcher-visible label ("WhatsApp"), NOT
/// the package id ("com.whatsapp") — history.jsonl's foreground_app has
/// always stored the label (BulbulForegroundService resolves it via
/// friendlyAppName before writing), and corrections.json now does the same
/// (CorrectionWatcher). Matching is case-insensitive since PackageManager
/// labels aren't guaranteed to match a fixed case. Deliberately
/// conservative: an unrecognized name falls to "Other" rather than
/// guessing, which is a safe, honest default — the UI just skips the
/// personality-title easter egg in that case.
fn categorize_app_mobile(app: &str) -> &'static str {
    match app.to_lowercase().as_str() {
        "whatsapp" | "whatsapp business" | "telegram" | "signal" | "messenger"
        | "instagram" | "snapchat" | "messages" | "google messages" => "Personal messages",
        "slack" | "teams" | "microsoft teams" | "discord" | "chat" | "google chat" => "Work messages",
        "gmail" | "outlook" | "microsoft outlook" | "yahoo mail" | "proton mail"
        | "protonmail" | "k-9 mail" | "k-9 mail (fork)" => "Emails",
        "chrome" | "google chrome" | "firefox" | "firefox fast & private" | "edge"
        | "microsoft edge" | "brave" | "brave browser" | "opera" | "opera browser"
        | "samsung internet" | "internet" => "Browsing",
        "chatgpt" | "claude" | "perplexity" | "perplexity - ask anything" => "AI Prompts",
        "word" | "microsoft word" | "docs" | "google docs" | "evernote" | "obsidian"
        | "notion" => "Documents",
        _ => "Other",
    }
}

#[tauri::command]
fn get_voice_stats(app: tauri::AppHandle) -> Value {
    voice_stats_value(&app)
}

const VOICE_PROFILE_SYSTEM_PROMPT: &str = "You analyze a person's voice-dictation history to describe their communication style. \
From the dictation samples, write a warm, specific 2-3 sentence \"voice profile\" describing how this person comes across when they dictate — their tone, the kinds of things they say, and any recurring quirks. Address them as \"you\". \
Then write one short sentence (\"peak blurb\") about their dictation habit or rhythm. \
Do not invent facts not supported by the samples. \
Return ONLY a JSON object, no preamble and no markdown fences:\n\
{\"voice_profile\": \"...\", \"peak_blurb\": \"...\"}";

/// Generates the "Your Voice" narrative from the user's dictation history via
/// Groq, persists it to voice.json, and returns the refreshed VoiceStats.
#[tauri::command]
async fn refresh_voice_narrative(app: tauri::AppHandle) -> Result<Value, String> {
    let cfg = read_config(&app).unwrap_or_default();
    if !cfg.has_api_key() && cfg.gemini_api_key.trim().is_empty() {
        return Err("Set your Gemini or Groq API key in Settings first.".to_string());
    }
    let rows = history_rows(&app);
    let total_words: i64 = rows.iter().map(|r| r["word_count"].as_i64().unwrap_or(0)).sum();
    if total_words == 0 {
        return Err("Dictate something first to build a voice profile.".to_string());
    }

    // Most recent ~25 non-empty transcripts as the writing sample.
    let samples: String = rows
        .iter()
        .rev()
        .filter_map(|r| r["cleaned_text"].as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .take(25)
        .collect::<Vec<_>>()
        .join("\n");
    let user = format!(
        "Quick stats:\nTotal words dictated: {total_words}. Total dictations: {}.\n\nDictation samples:\n{samples}",
        rows.len(),
    );

    let content = if !cfg.groq_api_key.trim().is_empty() {
        groq_chat_chain(
            cfg.groq_api_key.trim(),
            &cfg.chat_model,
            VOICE_PROFILE_SYSTEM_PROMPT,
            &user,
            0.4,
        )
        .await?
    } else if !cfg.gemini_api_key.trim().is_empty() {
        gemini::post_process(
            &cfg,
            "gemini-2.5-flash",
            VOICE_PROFILE_SYSTEM_PROMPT,
            &user,
            None,
        )
        .await
        .map_err(|e| e.to_string())?
    } else {
        return Err("Set your Gemini or Groq API key in Settings first.".to_string());
    };

    // Strip any code fences the model added despite instructions.
    let trimmed = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let parsed: Value = serde_json::from_str(trimmed)
        .map_err(|e| format!("Groq returned unparseable JSON: {e}"))?;

    let record = json!({
        "voice_narrative": parsed["voice_profile"].as_str().unwrap_or_default(),
        "peak_narrative": parsed["peak_blurb"].as_str().unwrap_or_default(),
        "last_generated_at": now_secs(),
        "words_at_gen": total_words,
    });
    write_json_object(&app, VOICE_FILE, &record)?;
    Ok(voice_stats_value(&app))
}

// ---------- Dictionary + corrections ----------

// Seeded on first run so the Dictionary page opens with useful entries
// instead of an empty list. Mirrors db.rs DEFAULT_DICTIONARY.
const DEFAULT_DICTIONARY: &[(&str, &str)] = &[
    ("groq", "Groq"),
    ("github", "GitHub"),
    ("gitlab", "GitLab"),
    ("vscode", "VS Code"),
    ("javascript", "JavaScript"),
    ("typescript", "TypeScript"),
    ("nodejs", "Node.js"),
    ("npm", "npm"),
    ("pnpm", "pnpm"),
    ("postgres", "Postgres"),
    ("sqlite", "SQLite"),
    ("ios", "iOS"),
    ("macos", "macOS"),
    ("api", "API"),
    ("ui", "UI"),
    ("ux", "UX"),
    ("css", "CSS"),
    ("html", "HTML"),
    ("json", "JSON"),
    ("ai", "AI"),
    ("llm", "LLM"),
    ("openai", "OpenAI"),
    ("anthropic", "Anthropic"),
];

fn seed_dictionary() -> Vec<Value> {
    DEFAULT_DICTIONARY
        .iter()
        .enumerate()
        .map(|(i, (from, to))| {
            json!({
                "id": i as i64 + 1,
                "from_word": from,
                "to_word": to,
                "case_sensitive": false,
                "hit_count": 0,
                "created_at": now_secs(),
            })
        })
        .collect()
}

fn load_dictionary(app: &tauri::AppHandle) -> Vec<Value> {
    load_store(app, DICTIONARY_FILE, seed_dictionary)
}

#[tauri::command]
fn list_dictionary(app: tauri::AppHandle) -> Vec<Value> {
    load_dictionary(&app)
}

// corrections.json is written by CorrectionWatcher.kt (post-injection field
// watcher — see that file) after a dictation is hand-edited; Rust only ever
// reads it. correction_dismissals.json is the reverse: only Rust writes it,
// from the Dictionary page's "Dismiss" action. Keeping them as two files
// (rather than one shared one) means the always-running Kotlin watcher and
// a UI-triggered Rust write never race on the same file.
const CORRECTIONS_FILE: &str = "corrections.json";
const CORRECTION_DISMISSALS_FILE: &str = "correction_dismissals.json";

/// Newest-first corrections, same shape as db::list_corrections on desktop.
#[tauri::command]
fn list_corrections(app: tauri::AppHandle, limit: Option<u32>) -> Vec<Value> {
    let mut rows = read_json_array(&app, CORRECTIONS_FILE);
    rows.sort_by_key(|r| std::cmp::Reverse(r["ts"].as_i64().unwrap_or(0)));
    rows.truncate(limit.unwrap_or(100) as usize);
    rows
}

/// If `injected` and `corrected` differ by exactly one whitespace-delimited
/// word (ignoring surrounding punctuation), return that (from, to) swap.
/// Ported verbatim from db.rs::extract_word_substitution.
fn extract_word_substitution(injected: &str, corrected: &str) -> Option<(String, String)> {
    let inj: Vec<&str> = injected.split_whitespace().collect();
    let cor: Vec<&str> = corrected.split_whitespace().collect();
    if inj.is_empty() || inj.len() != cor.len() {
        return None;
    }
    fn strip(w: &str) -> &str {
        w.trim_matches(|c: char| !c.is_alphanumeric())
    }
    let mut diff: Option<(String, String)> = None;
    for (a, b) in inj.iter().zip(cor.iter()) {
        let (sa, sb) = (strip(a), strip(b));
        if sa == sb {
            continue;
        }
        if diff.is_some() {
            return None; // more than one word changed — not a clean swap
        }
        diff = Some((sa.to_string(), sb.to_string()));
    }
    let (from, to) = diff?;
    if from.is_empty() || to.is_empty() || from.chars().count() > 40 || to.chars().count() > 40 {
        return None;
    }
    Some((from, to))
}

/// Distinct single-word fixes as dictionary suggestions — ported from
/// db.rs::correction_suggestions (SQL GROUP BY replaced with a HashMap over
/// the same JSON array list_corrections reads).
#[tauri::command]
fn correction_suggestions(app: tauri::AppHandle) -> Vec<Value> {
    let dict: std::collections::HashSet<String> = load_dictionary(&app)
        .iter()
        .filter_map(|e| e["from_word"].as_str())
        .map(|s| s.to_lowercase())
        .collect();
    let dismissed: std::collections::HashSet<(String, String)> =
        read_json_array(&app, CORRECTION_DISMISSALS_FILE)
            .iter()
            .filter_map(|e| Some((e["from_word"].as_str()?.to_string(), e["to_word"].as_str()?.to_string())))
            .collect();

    let mut rows = read_json_array(&app, CORRECTIONS_FILE);
    rows.sort_by_key(|r| std::cmp::Reverse(r["ts"].as_i64().unwrap_or(0)));

    let mut order: Vec<(String, String)> = Vec::new();
    let mut counts: std::collections::HashMap<(String, String), (String, String, i64)> =
        std::collections::HashMap::new();
    for r in &rows {
        let (Some(inj), Some(cor)) = (r["injected"].as_str(), r["corrected"].as_str()) else { continue };
        let Some((from, to)) = extract_word_substitution(inj, cor) else { continue };
        let (from_lower, to_lower) = (from.to_lowercase(), to.to_lowercase());
        if dict.contains(&from_lower) {
            continue;
        }
        let key = (from_lower, to_lower);
        if dismissed.contains(&key) {
            continue;
        }
        match counts.get_mut(&key) {
            Some(entry) => entry.2 += 1,
            None => {
                order.push(key.clone());
                counts.insert(key, (from, to, 1));
            }
        }
    }
    let mut out: Vec<(String, String, i64)> =
        order.into_iter().filter_map(|k| counts.remove(&k)).collect();
    out.sort_by(|a, b| b.2.cmp(&a.2)); // stable: keeps recency within ties
    out.truncate(20);
    out.into_iter()
        .map(|(from_word, to_word, count)| json!({ "from_word": from_word, "to_word": to_word, "count": count }))
        .collect()
}

#[tauri::command]
fn add_dictionary_entry(
    app: tauri::AppHandle,
    from_word: String,
    to_word: String,
    case_sensitive: bool,
) -> Result<(), String> {
    let from = from_word.trim();
    let to = to_word.trim();
    if from.is_empty() || to.is_empty() {
        return Err("Word cannot be empty.".into());
    }
    let mut rows = load_dictionary(&app);
    // Adding a word that already exists just updates its target, so the
    // list doesn't accumulate duplicate rows for the same trigger.
    if let Some(existing) = rows.iter_mut().find(|r| {
        r["from_word"].as_str().map(|s| s.eq_ignore_ascii_case(from)).unwrap_or(false)
    }) {
        existing["to_word"] = json!(to);
        existing["case_sensitive"] = json!(case_sensitive);
    } else {
        let id = next_id(&rows);
        rows.push(json!({
            "id": id,
            "from_word": from,
            "to_word": to,
            "case_sensitive": case_sensitive,
            "hit_count": 0,
            "created_at": now_secs(),
        }));
    }
    write_json_array(&app, DICTIONARY_FILE, &rows)
}

#[tauri::command]
fn update_dictionary_entry(
    app: tauri::AppHandle,
    id: i64,
    from_word: String,
    to_word: String,
    case_sensitive: bool,
) -> Result<(), String> {
    let from = from_word.trim();
    let to = to_word.trim();
    if from.is_empty() || to.is_empty() {
        return Err("Word cannot be empty.".into());
    }
    let mut rows = load_dictionary(&app);
    let mut found = false;
    for r in rows.iter_mut() {
        if r["id"].as_i64() == Some(id) {
            r["from_word"] = json!(from);
            r["to_word"] = json!(to);
            r["case_sensitive"] = json!(case_sensitive);
            found = true;
            break;
        }
    }
    if !found {
        return Err(format!("no dictionary entry with id {id}"));
    }
    write_json_array(&app, DICTIONARY_FILE, &rows)
}

#[tauri::command]
fn delete_dictionary_entry(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let mut rows = load_dictionary(&app);
    let before = rows.len();
    rows.retain(|r| r["id"].as_i64() != Some(id));
    if rows.len() == before {
        return Err(format!("no dictionary entry with id {id}"));
    }
    write_json_array(&app, DICTIONARY_FILE, &rows)
}

#[tauri::command]
fn dismiss_correction_suggestion(app: tauri::AppHandle, from_word: String, to_word: String) -> Result<(), String> {
    let (from, to) = (from_word.trim().to_lowercase(), to_word.trim().to_lowercase());
    if from.is_empty() || to.is_empty() {
        return Ok(());
    }
    let mut rows = read_json_array(&app, CORRECTION_DISMISSALS_FILE);
    let already = rows.iter().any(|r| {
        r["from_word"].as_str() == Some(from.as_str()) && r["to_word"].as_str() == Some(to.as_str())
    });
    if !already {
        rows.push(json!({ "from_word": from, "to_word": to }));
        write_json_array(&app, CORRECTION_DISMISSALS_FILE, &rows)?;
    }
    Ok(())
}

// ---------- Snippets ----------
//
// Trigger phrase → expansion, stored in snippets.json. No defaults (users
// add their own). The Kotlin dictation path applies them after the
// dictionary, mirroring desktop.

const SNIPPETS_FILE: &str = "snippets.json";

fn load_snippets(app: &tauri::AppHandle) -> Vec<Value> {
    read_json_array(app, SNIPPETS_FILE)
}

#[tauri::command]
fn list_snippets(app: tauri::AppHandle) -> Vec<Value> {
    load_snippets(&app)
}

#[tauri::command]
fn add_snippet(app: tauri::AppHandle, trigger: String, expansion: String) -> Result<(), String> {
    let t = trigger.trim();
    let e = expansion.trim();
    if t.is_empty() {
        return Err("Trigger is required.".into());
    }
    if e.is_empty() {
        return Err("Expansion is required.".into());
    }
    let mut rows = load_snippets(&app);
    let id = next_id(&rows);
    rows.push(json!({
        "id": id,
        "trigger": t,
        "expansion": e,
        "hit_count": 0,
        "created_at": now_secs(),
    }));
    write_json_array(&app, SNIPPETS_FILE, &rows)
}

#[tauri::command]
fn update_snippet(
    app: tauri::AppHandle,
    id: i64,
    trigger: String,
    expansion: String,
) -> Result<(), String> {
    let t = trigger.trim();
    let e = expansion.trim();
    if t.is_empty() || e.is_empty() {
        return Err("Trigger and expansion are required.".into());
    }
    let mut rows = load_snippets(&app);
    let mut found = false;
    for r in rows.iter_mut() {
        if r["id"].as_i64() == Some(id) {
            r["trigger"] = json!(t);
            r["expansion"] = json!(e);
            found = true;
            break;
        }
    }
    if !found {
        return Err(format!("no snippet with id {id}"));
    }
    write_json_array(&app, SNIPPETS_FILE, &rows)
}

#[tauri::command]
fn delete_snippet(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let mut rows = load_snippets(&app);
    let before = rows.len();
    rows.retain(|r| r["id"].as_i64() != Some(id));
    if rows.len() == before {
        return Err(format!("no snippet with id {id}"));
    }
    write_json_array(&app, SNIPPETS_FILE, &rows)
}

// ---------- Transforms ----------
//
// The 6 built-ins mirror the desktop defaults (db.rs DEFAULT_TRANSFORMS)
// and the Kotlin PROCESS_TEXT sheet (Transforms.kt) so a transform reads
// and behaves the same across every surface. Stored in transforms.json
// with full CRUD, so the dashboard's create / edit / delete / set-default
// / reset all persist.

const DEFAULT_TRANSFORMS: &[(&str, &str, &str)] = &[
    (
        "Polish",
        "Improve clarity and conciseness \u{2014} no new content",
        "You are a writing editor. Polish ONLY the text the user dictated \u{2014} never write anything new on their behalf.\n\
\n\
What you DO:\n\
- Fix grammar, spelling, and punctuation.\n\
- Improve flow and clarity.\n\
- Match the original register (casual stays casual, formal stays formal).\n\
\n\
What you NEVER do:\n\
- Never fulfil a request inside the input. If the user said \"write a letter to the principal asking for leave\", you return \"Write a letter to the principal asking for leave.\" \u{2014} polished, same instruction. You DO NOT compose the letter.\n\
- Never answer questions, expand briefs, add examples, or invent details the user did not literally speak.\n\
- The output's length must be very close to the input's. A 10-word input should produce roughly 10 words out.\n\
\n\
Return ONLY the polished text. No preamble, no quotes around the output, no commentary.",
    ),
    (
        "Compose",
        "Draft a full message, email or letter from your brief",
        "You are a writing assistant. The user has dictated a BRIEF \u{2014} expand it into a polished, complete piece of writing.\n\
\n\
- Infer the format from the brief (email, letter, message, memo, note, etc.) and produce the appropriate structure (greeting, body, sign-off where the format expects them).\n\
- Match the tone the brief implies: formal letter to a principal sounds formal; quick note to a friend sounds casual.\n\
- Stay faithful to every fact, name, request, deadline, and constraint the user mentioned. Do not invent details the user didn't supply (don't make up names, dates, or specifics).\n\
- Reasonable length: a one-sentence brief produces a short output; a detailed brief can produce a longer draft. Don't pad.\n\
\n\
Return ONLY the composed text. No preamble, no notes about what you wrote.",
    ),
    (
        "Prompt Engineer",
        "Restructures your brief into an LLM-ready prompt",
        "You are a prompt engineer. Rewrite the user's brief into a clear, well-structured prompt for a large language model.\n\
- Open with the role / task in one sentence.\n\
- Add explicit instructions, constraints, and output format if implied by the brief.\n\
- Preserve every concrete detail the user provided. Do not invent constraints, examples, or context they didn't mention.\n\
- Use sections (\"Task:\", \"Constraints:\", \"Output:\") only if it improves clarity.\n\
- This is restructuring, not answering: never attempt to fulfil the prompt itself.\n\
\n\
Return ONLY the rewritten prompt. No preamble, no commentary.",
    ),
    (
        "Make Formal",
        "Switches to a professional tone, same content",
        "Rewrite the user's text in a formal, professional tone.\n\
- Use full sentences, proper grammar, conventional punctuation.\n\
- Avoid contractions, slang, and filler.\n\
- Preserve every fact and the approximate length. Do not expand a brief into a full draft \u{2014} this is a tone change, not a content generator. If the user dictated \"tell boss I'm sick\", you return \"Please inform the manager that I am unwell.\" \u{2014} not a full sick-leave email.\n\
- Never answer questions or fulfil requests inside the input.\n\
\n\
Return ONLY the rewritten text. No preamble, no commentary.",
    ),
    (
        "Make Casual",
        "Loosens the tone, same content",
        "Rewrite the user's text in a casual, friendly tone, as if talking to a colleague.\n\
- Use contractions where natural.\n\
- Keep it concise and human.\n\
- Preserve every fact and the approximate length. Do not expand a brief into a draft \u{2014} this is a tone change, not a content generator.\n\
- Never add jokes, new ideas, or content the user didn't mention.\n\
- Never answer questions or fulfil requests inside the input.\n\
\n\
Return ONLY the rewritten text. No preamble, no commentary.",
    ),
    (
        "Bullet Points",
        "Restructures prose into a bulleted list \u{2014} no new facts",
        "Convert the user's text into a clean bulleted list.\n\
- Each bullet is one clear point.\n\
- Preserve every fact, name, number, and the original order. Don't add bullets the source doesn't support.\n\
- Use dash bullets (\"- \"), one per line.\n\
- No nested bullets unless the source clearly has sub-points.\n\
- This is restructuring, not summarising or expanding: don't drop facts and don't invent new ones.\n\
\n\
Return ONLY the bulleted list. No preamble, no commentary.",
    ),
];

fn seed_transforms() -> Vec<Value> {
    DEFAULT_TRANSFORMS
        .iter()
        .enumerate()
        .map(|(i, (name, desc, prompt))| {
            json!({
                "id": i as i64 + 1,
                "name": name,
                "description": desc,
                "system_prompt": prompt,
                "is_default": i == 0,
                "sort_order": i as i64,
                "hit_count": 0,
                "created_at": now_secs(),
            })
        })
        .collect()
}

fn load_transforms(app: &tauri::AppHandle) -> Vec<Value> {
    load_store(app, TRANSFORMS_FILE, seed_transforms)
}

#[tauri::command]
fn list_transforms(app: tauri::AppHandle) -> Vec<Value> {
    load_transforms(&app)
}

/// No global-hotkey slots on Android, so there's nothing to report — the
/// React Transforms page treats an empty list as "no slot chips, no binding
/// warnings," which is exactly right on mobile.
#[tauri::command]
fn list_transform_slot_statuses() -> Vec<Value> {
    Vec::new()
}

#[tauri::command]
fn add_transform(
    app: tauri::AppHandle,
    name: String,
    description: String,
    system_prompt: String,
) -> Result<Value, String> {
    let name = name.trim();
    let prompt = system_prompt.trim();
    if name.is_empty() {
        return Err("Name is required.".into());
    }
    if prompt.is_empty() {
        return Err("System prompt is required.".into());
    }
    let mut rows = load_transforms(&app);
    let is_first = rows.is_empty();
    let id = next_id(&rows);
    let sort_order = rows.iter().filter_map(|r| r["sort_order"].as_i64()).max().unwrap_or(-1) + 1;
    let entry = json!({
        "id": id,
        "name": name,
        "description": description.trim(),
        "system_prompt": prompt,
        "is_default": is_first,
        "sort_order": sort_order,
        "hit_count": 0,
        "created_at": now_secs(),
    });
    rows.push(entry.clone());
    write_json_array(&app, TRANSFORMS_FILE, &rows)?;
    Ok(entry)
}

#[tauri::command]
fn update_transform(
    app: tauri::AppHandle,
    id: i64,
    name: String,
    description: String,
    system_prompt: String,
) -> Result<(), String> {
    let name = name.trim();
    let prompt = system_prompt.trim();
    if name.is_empty() {
        return Err("Name is required.".into());
    }
    if prompt.is_empty() {
        return Err("System prompt is required.".into());
    }
    let mut rows = load_transforms(&app);
    let mut found = false;
    for r in rows.iter_mut() {
        if r["id"].as_i64() == Some(id) {
            r["name"] = json!(name);
            r["description"] = json!(description.trim());
            r["system_prompt"] = json!(prompt);
            found = true;
            break;
        }
    }
    if !found {
        return Err(format!("no transform with id {id}"));
    }
    write_json_array(&app, TRANSFORMS_FILE, &rows)
}

#[tauri::command]
fn delete_transform(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let mut rows = load_transforms(&app);
    let before = rows.len();
    let deleted_default = rows
        .iter()
        .any(|r| r["id"].as_i64() == Some(id) && r["is_default"].as_bool() == Some(true));
    rows.retain(|r| r["id"].as_i64() != Some(id));
    if rows.len() == before {
        return Err(format!("no transform with id {id}"));
    }
    // Deleting the default promotes the first remaining transform, so the
    // pipeline always has a default to fall back to.
    if deleted_default {
        if let Some(first) = rows.first_mut() {
            first["is_default"] = json!(true);
        }
    }
    write_json_array(&app, TRANSFORMS_FILE, &rows)
}

#[tauri::command]
fn set_default_transform(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let mut rows = load_transforms(&app);
    if !rows.iter().any(|r| r["id"].as_i64() == Some(id)) {
        return Err(format!("no transform with id {id}"));
    }
    for r in rows.iter_mut() {
        let is_it = r["id"].as_i64() == Some(id);
        r["is_default"] = json!(is_it);
    }
    write_json_array(&app, TRANSFORMS_FILE, &rows)
}

#[tauri::command]
fn reset_transforms(app: tauri::AppHandle) -> Result<(), String> {
    let seeded = seed_transforms();
    write_json_array(&app, TRANSFORMS_FILE, &seeded)
}

/// Runs a stored transform against arbitrary text (the Scratchpad's
/// rewrite-selection chips). Looks the transform up in transforms.json and
/// runs its prompt through Groq. Arg is `transform_id` to match the desktop
/// command / the React invoke.
#[tauri::command]
async fn run_transform_on_text(
    app: tauri::AppHandle,
    transform_id: i64,
    text: String,
) -> Result<String, String> {
    let cfg = read_config(&app).unwrap_or_default();
    if !cfg.has_api_key() {
        return Err("Set your Groq API key in Settings first.".to_string());
    }
    if text.trim().is_empty() {
        return Ok(text);
    }
    let prompt = load_transforms(&app)
        .into_iter()
        .find(|t| t["id"].as_i64() == Some(transform_id))
        .and_then(|t| t["system_prompt"].as_str().map(|s| s.to_string()))
        .ok_or_else(|| "Transform not found".to_string())?;
    groq_chat_chain(cfg.groq_api_key.trim(), &cfg.chat_model, &prompt, &text, 0.3).await
}

// ---------- Notes / scratchpad ----------
//
// File-backed notes (notes.json), same pattern as the other stores. The
// desktop scratchpad is a separate window; on mobile it's the in-app
// ScratchpadView, which drives these commands directly.

const NOTES_FILE: &str = "notes.json";

fn load_notes(app: &tauri::AppHandle) -> Vec<Value> {
    read_json_array(app, NOTES_FILE)
}

#[tauri::command]
fn list_notes(app: tauri::AppHandle) -> Vec<Value> {
    let mut rows = load_notes(&app);
    // Newest-updated first, matching how the editor prepends new notes.
    rows.sort_by(|a, b| {
        b["updated_at"].as_i64().unwrap_or(0).cmp(&a["updated_at"].as_i64().unwrap_or(0))
    });
    rows
}

#[tauri::command]
fn create_note(app: tauri::AppHandle, title: String, body: String) -> Result<Value, String> {
    let mut rows = load_notes(&app);
    let now = now_secs();
    let note = json!({
        "id": next_id(&rows),
        "title": title,
        "body": body,
        "created_at": now,
        "updated_at": now,
    });
    rows.push(note.clone());
    write_json_array(&app, NOTES_FILE, &rows)?;
    Ok(note)
}

#[tauri::command]
fn update_note(app: tauri::AppHandle, id: i64, title: String, body: String) -> Result<(), String> {
    let mut rows = load_notes(&app);
    let mut found = false;
    for r in rows.iter_mut() {
        if r["id"].as_i64() == Some(id) {
            r["title"] = json!(title);
            r["body"] = json!(body);
            r["updated_at"] = json!(now_secs());
            found = true;
            break;
        }
    }
    if !found {
        return Err(format!("no note with id {id}"));
    }
    write_json_array(&app, NOTES_FILE, &rows)
}

#[tauri::command]
fn delete_note(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let mut rows = load_notes(&app);
    let before = rows.len();
    rows.retain(|r| r["id"].as_i64() != Some(id));
    if rows.len() == before {
        return Err(format!("no note with id {id}"));
    }
    write_json_array(&app, NOTES_FILE, &rows)
}

// ---------- Settings + updater ----------

/// Pings Groq's `/v1/models` with the user-provided key. 200 → key
/// works. 401 → key is wrong. Anything else → surface the body so the
/// user can see what Groq said (rate-limit, account suspended, etc.).
/// Mirrors `groq::validate_key` on desktop; kept inline here so the
/// mobile build doesn't pull in the entire desktop `groq` module
/// (which depends on `hound`, `tokio` retry helpers, and the shared
/// client cache that has different requirements on mobile).
#[tauri::command]
async fn validate_api_key(api_key: String) -> Result<(), String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("API key is empty.".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))?;
    let resp = client
        .get("https://api.groq.com/openai/v1/models")
        .bearer_auth(key)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("Groq rejected key ({status}): {body}"))
    }
}

/// Real transcription for the onboarding "Try it yourself" rehearsal
/// (StepDictateTest in OnboardingWizard.jsx) — not hardcoded, not a
/// mock: the same Groq Whisper endpoint and model fallback order the
/// real floating bubble uses (mirrors GroqClient.kt's STT_MODELS). The
/// webview captures the audio itself via the Web Audio API and encodes
/// a real WAV file client-side (there's no way to reach Kotlin's
/// AudioRecord from here), so this just needs to upload exactly what
/// AudioRecorder.kt would have produced. The wizard has no target app
/// to inject into, so the transcript is returned to JS to render in its
/// own preview field instead. Kept inline rather than routed through
/// groq.rs for the same reason validate_api_key above is: the mobile
/// build skips pulling in the whole desktop groq module.
#[tauri::command]
async fn transcribe_dictate_sample(
    app: tauri::AppHandle,
    wav_bytes: Vec<u8>,
) -> Result<String, String> {
    let cfg = read_config(&app).unwrap_or_default();
    let cfg = mobile_config_defaults(cfg);
    let key = cfg.groq_api_key.trim().to_string();
    if key.is_empty() {
        return Err("No Groq API key saved yet.".to_string());
    }
    if wav_bytes.len() < 200 {
        return Err("No audio captured — try holding a little longer.".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))?;

    let lang = cfg.language.trim();
    let lang = if !lang.is_empty() && lang != "auto" {
        Some(lang.to_string())
    } else {
        None
    };

    #[derive(serde::Deserialize)]
    struct TranscriptionResponse {
        text: String,
    }

    // Same primary-then-fallback order as GroqClient.kt's STT_MODELS, so a
    // decommissioned/rate-limited turbo model doesn't dead-end the rehearsal.
    for model in ["whisper-large-v3-turbo", "whisper-large-v3"] {
        let part = match reqwest::multipart::Part::bytes(wav_bytes.clone())
            .file_name("audio.wav")
            .mime_str("audio/wav")
        {
            Ok(p) => p,
            Err(_) => continue,
        };
        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", model)
            .text("response_format", "json");
        if let Some(l) = &lang {
            form = form.text("language", l.clone());
        }
        let resp = match client
            .post("https://api.groq.com/openai/v1/audio/transcriptions")
            .bearer_auth(&key)
            .multipart(form)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !resp.status().is_success() {
            continue;
        }
        let body = resp.text().await.unwrap_or_default();
        if let Ok(parsed) = serde_json::from_str::<TranscriptionResponse>(&body) {
            return Ok(parsed.text.trim().to_string());
        }
    }
    Err("Transcription failed — check your connection and try again.".to_string())
}

// ---------- Overlay / scratchpad windows (desktop-only concepts) ----------

#[tauri::command]
fn set_overlay_height(_height: u32) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
fn open_scratchpad() -> Result<(), String> {
    Err("Scratchpad window is not available on Android.".to_string())
}

/// Persists onboarding completion so the first-launch wizard never shows
/// again. Read-modify-write of config.json (keeps the key/language/etc. the
/// wizard just saved). Called by the wizard's final "Open Bulbul" button.
#[tauri::command]
fn complete_onboarding(app: tauri::AppHandle) -> Result<(), String> {
    let mut cfg = read_config(&app).unwrap_or_default();
    cfg.onboarding_completed = true;
    save_config(app, cfg)
}

#[tauri::command]
fn open_keyboard_settings() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = jni_bridge::call_main_activity_static("openKeyboardSettings");
    }
    Ok(())
}

#[tauri::command]
fn show_keyboard_picker() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = jni_bridge::call_main_activity_static("showKeyboardPicker");
    }
    Ok(())
}

// ---------- V2 Multi-Phase Dictation & History (Mobile implementation) ----------

const CONTEXT_MODES_FILE: &str = "context_modes.json";
const VOCABULARY_PROFILES_FILE: &str = "vocabulary_profiles.json";

fn default_context_modes() -> Vec<Value> {
    vec![
        json!({
            "id": 1,
            "name": "Direct Smart Dictation",
            "icon": "⚡",
            "description": "Standard fast speech-to-text with auto punctuation and disfluency cleanup.",
            "system_prompt": "You are a direct dictation assistant. Fix spelling, capitalization, and punctuation. Do not add conversational filler.",
            "phase2_provider": "gemini",
            "phase2_model": "gemini-3.8-flash",
            "thinking_level": "low",
            "is_active": true
        }),
        json!({
            "id": 2,
            "name": "Coding Agent Prompt",
            "icon": "💻",
            "description": "Formats voice descriptions into structured instructions for AI coding assistants.",
            "system_prompt": "You are a prompt engineer for AI coding assistants. Convert speech into concise, structured coding instructions.",
            "phase2_provider": "gemini",
            "phase2_model": "gemini-3.8-flash",
            "thinking_level": "medium",
            "is_active": false
        }),
        json!({
            "id": 3,
            "name": "Professional Email",
            "icon": "✉️",
            "description": "Transforms spoken thoughts into polite, professional email drafts.",
            "system_prompt": "Format this dictated message into a clean, professional email with a polite tone.",
            "phase2_provider": "gemini",
            "phase2_model": "gemini-3.8-flash",
            "thinking_level": "low",
            "is_active": false
        }),
        json!({
            "id": 4,
            "name": "Natural Chat",
            "icon": "💬",
            "description": "Casual, fluent chat messages suitable for Slack, WhatsApp, or Discord.",
            "system_prompt": "Clean up this spoken message into natural, fluent chat dialogue while keeping the tone casual.",
            "phase2_provider": "gemini",
            "phase2_model": "gemini-3.8-flash",
            "thinking_level": "low",
            "is_active": false
        }),
        json!({
            "id": 5,
            "name": "Direct Translation",
            "icon": "🌐",
            "description": "Directly translates Bengali speech to polished English or vice-versa.",
            "system_prompt": "Translate the input text accurately between Bengali and English while preserving full meaning.",
            "phase2_provider": "gemini",
            "phase2_model": "gemini-3.8-flash",
            "thinking_level": "low",
            "is_active": false
        }),
    ]
}

fn default_vocabulary_profiles() -> Vec<Value> {
    vec![
        json!({
            "id": 1,
            "name": "Full-Stack Dev Stack",
            "description": "Programming languages, frameworks, and tools",
            "terms": ["Rust", "Tauri", "React", "TypeScript", "Vite", "TailwindCSS", "Node.js", "Docker", "GraphQL", "PostgreSQL", "SQLite", "GitHub Actions"],
            "is_active": true
        }),
        json!({
            "id": 2,
            "name": "Business & Client",
            "description": "Freelancing and client terminology",
            "terms": ["Upwork", "Fiverr", "Invoice", "Deliverable", "Milestone", "Retainer", "Scope of Work", "SOW"],
            "is_active": true
        }),
        json!({
            "id": 3,
            "name": "Banglish & Proper Nouns",
            "description": "Common Bengali-English transliterated phrases and names",
            "terms": ["Banglish", "Dhaka", "Chittagong", "Bikash", "Nagad", "Khub sundor", "Kemon acho"],
            "is_active": true
        }),
    ]
}

#[tauri::command]
fn list_recordings_v2(limit: Option<u32>, offset: Option<u32>) -> Result<Vec<db::FullRecordingDetail>, String> {
    let db = db::open().map_err(|e| format!("{e:#}"))?;
    db::list_full_recordings(&db, limit.unwrap_or(50), offset.unwrap_or(0))
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn count_recordings_v2() -> Result<u64, String> {
    let db = db::open().map_err(|e| format!("{e:#}"))?;
    let conn = db.lock();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM recordings WHERE deleted_at IS NULL", [], |r| r.get(0))
        .unwrap_or(0);
    Ok(count as u64)
}

#[tauri::command]
fn get_full_recording_v2(id: String) -> Result<Option<db::FullRecordingDetail>, String> {
    let db = db::open().map_err(|e| format!("{e:#}"))?;
    db::get_full_recording(&db, &id).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn delete_recording_v2(id: String) -> Result<(), String> {
    let db = db::open().map_err(|e| format!("{e:#}"))?;
    db::delete_recording(&db, &id).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn toggle_recording_pinned(id: String) -> Result<bool, String> {
    let db = db::open().map_err(|e| format!("{e:#}"))?;
    db::toggle_recording_pinned(&db, &id).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn get_recording_audio_bytes(
    recording_id: String,
) -> Result<String, String> {
    let dir = config::recordings_dir().map_err(|e| format!("{e:#}"))?;
    let opus_path = dir.join(format!("{recording_id}.opus"));
    if opus_path.exists() {
        let bytes = std::fs::read(&opus_path).map_err(|e| format!("reading opus audio file: {e:#}"))?;
        let b64 = BASE64.encode(&bytes);
        return Ok(format!("data:audio/ogg;base64,{b64}"));
    }
    let wav_path = dir.join(format!("{recording_id}.wav"));
    if wav_path.exists() {
        let bytes = std::fs::read(&wav_path).map_err(|e| format!("reading wav audio file: {e:#}"))?;
        let b64 = BASE64.encode(&bytes);
        return Ok(format!("data:audio/wav;base64,{b64}"));
    }

    // On-demand fetch from WebDAV cloud if enabled
    let cfg = config::load();
    if cfg.webdav_enabled {
        if let Ok(client) = webdav::WebdavClient::from_config(&cfg) {
            let remote_audio = format!("audio/{recording_id}.opus");
            if let Ok(bytes) = client.get_file(&remote_audio).await {
                let _ = std::fs::write(&opus_path, &bytes);
                let b64 = BASE64.encode(&bytes);
                return Ok(format!("data:audio/ogg;base64,{b64}"));
            }
        }
    }

    Err("Audio file not found".to_string())
}

#[tauri::command]
fn list_context_modes(app: tauri::AppHandle) -> Result<Vec<Value>, String> {
    let mut modes = read_json_array(&app, CONTEXT_MODES_FILE);
    if modes.is_empty() {
        modes = default_context_modes();
        let _ = write_json_array(&app, CONTEXT_MODES_FILE, &modes);
    }
    Ok(modes)
}

#[tauri::command]
fn get_context_mode(app: tauri::AppHandle, id: i64) -> Result<Option<Value>, String> {
    let modes = list_context_modes(app)?;
    Ok(modes.into_iter().find(|m| m["id"].as_i64() == Some(id)))
}

#[tauri::command]
fn save_context_mode(
    app: tauri::AppHandle,
    id: Option<i64>,
    name: String,
    icon: String,
    description: String,
    system_prompt: String,
    phase2_provider: String,
    phase2_model: String,
    thinking_level: Option<String>,
    is_active: bool,
) -> Result<i64, String> {
    let mut modes = list_context_modes(app.clone())?;
    let target_id = id.unwrap_or_else(|| next_id(&modes));
    if is_active {
        for m in modes.iter_mut() {
            m["is_active"] = json!(false);
        }
    }
    if let Some(pos) = modes.iter().position(|m| m["id"].as_i64() == Some(target_id)) {
        modes[pos] = json!({
            "id": target_id,
            "name": name,
            "icon": icon,
            "description": description,
            "system_prompt": system_prompt,
            "phase2_provider": phase2_provider,
            "phase2_model": phase2_model,
            "thinking_level": thinking_level,
            "is_active": is_active,
        });
    } else {
        modes.push(json!({
            "id": target_id,
            "name": name,
            "icon": icon,
            "description": description,
            "system_prompt": system_prompt,
            "phase2_provider": phase2_provider,
            "phase2_model": phase2_model,
            "thinking_level": thinking_level,
            "is_active": is_active,
        }));
    }
    write_json_array(&app, CONTEXT_MODES_FILE, &modes)?;
    Ok(target_id)
}

#[tauri::command]
fn delete_context_mode(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let mut modes = list_context_modes(app.clone())?;
    modes.retain(|m| m["id"].as_i64() != Some(id));
    write_json_array(&app, CONTEXT_MODES_FILE, &modes)
}

#[tauri::command]
fn set_active_context_mode(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let mut modes = list_context_modes(app.clone())?;
    for m in modes.iter_mut() {
        m["is_active"] = json!(m["id"].as_i64() == Some(id));
    }
    write_json_array(&app, CONTEXT_MODES_FILE, &modes)
}

#[tauri::command]
fn list_vocabulary_profiles(app: tauri::AppHandle) -> Result<Vec<Value>, String> {
    let mut profiles = read_json_array(&app, VOCABULARY_PROFILES_FILE);
    if profiles.is_empty() {
        profiles = default_vocabulary_profiles();
        let _ = write_json_array(&app, VOCABULARY_PROFILES_FILE, &profiles);
    }
    Ok(profiles)
}

#[tauri::command]
fn save_vocabulary_profile(
    app: tauri::AppHandle,
    id: Option<i64>,
    name: String,
    description: String,
    terms: Vec<String>,
    is_active: bool,
) -> Result<i64, String> {
    let mut profiles = list_vocabulary_profiles(app.clone())?;
    let target_id = id.unwrap_or_else(|| next_id(&profiles));
    if let Some(pos) = profiles.iter().position(|p| p["id"].as_i64() == Some(target_id)) {
        profiles[pos] = json!({
            "id": target_id,
            "name": name,
            "description": description,
            "terms": terms,
            "is_active": is_active,
        });
    } else {
        profiles.push(json!({
            "id": target_id,
            "name": name,
            "description": description,
            "terms": terms,
            "is_active": is_active,
        }));
    }
    write_json_array(&app, VOCABULARY_PROFILES_FILE, &profiles)?;
    Ok(target_id)
}

#[tauri::command]
fn toggle_vocabulary_profile(app: tauri::AppHandle, id: i64) -> Result<bool, String> {
    let mut profiles = list_vocabulary_profiles(app.clone())?;
    let mut next = false;
    for p in profiles.iter_mut() {
        if p["id"].as_i64() == Some(id) {
            let cur = p.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false);
            next = !cur;
            p["is_active"] = json!(next);
            break;
        }
    }
    write_json_array(&app, VOCABULARY_PROFILES_FILE, &profiles)?;
    Ok(next)
}

#[tauri::command]
fn delete_vocabulary_profile(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let mut profiles = list_vocabulary_profiles(app.clone())?;
    profiles.retain(|p| p["id"].as_i64() != Some(id));
    write_json_array(&app, VOCABULARY_PROFILES_FILE, &profiles)
}

#[tauri::command]
async fn prune_unpinned_cloud_audio() -> Result<u32, String> {
    let cfg = config::load();
    let db = db::open().map_err(|e| format!("{e:#}"))?;
    webdav::prune_unpinned_cloud_audio(&db, &cfg).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn rerun_transcription(
    recording_id: String,
    provider: String,
    model: String,
    mode: String,
    custom_vocabulary: Option<Vec<String>>,
) -> Result<db::TranscriptionTryRow, String> {
    let cfg = config::load();
    let db = db::open().map_err(|e| format!("{e:#}"))?;

    let dir = config::recordings_dir().map_err(|e| format!("{e:#}"))?;
    let opus_path = dir.join(format!("{recording_id}.opus"));
    let wav_path = dir.join(format!("{recording_id}.wav"));
    let audio_bytes = if opus_path.exists() {
        std::fs::read(&opus_path).map_err(|e| format!("reading opus audio: {e:#}"))?
    } else if wav_path.exists() {
        std::fs::read(&wav_path).map_err(|e| format!("reading wav audio: {e:#}"))?
    } else {
        if cfg.webdav_enabled {
            let client = webdav::WebdavClient::from_config(&cfg).map_err(|e| format!("{e:#}"))?;
            let remote_audio = format!("audio/{recording_id}.opus");
            let bytes = client.get_file(&remote_audio).await.map_err(|e| format!("downloading audio from cloud: {e:#}"))?;
            let _ = std::fs::write(&opus_path, &bytes);
            bytes
        } else {
            return Err("Audio file not found".to_string());
        }
    };

    let vocab = custom_vocabulary.unwrap_or_default();
    let is_coding = mode.to_lowercase().contains("tech") || mode.to_lowercase().contains("code");

    let t_start = Instant::now();
    let (transcript, final_model) = if provider.to_lowercase() == "gemini" {
        if model == "gemini-auto-fallback" || model.trim().is_empty() {
            gemini::transcribe_with_fallback(
                &cfg,
                &audio_bytes,
                &vocab,
                is_coding,
                Some(&mode),
                None,
                |_, _, _| {},
            )
            .await
            .map_err(|e| format!("{e:#}"))?
        } else {
            let text = gemini::transcribe_model(
                &cfg,
                &model,
                &audio_bytes,
                &vocab,
                is_coding,
                Some(&mode),
            )
            .await
            .map_err(|e| format!("{e:#}"))?;
            (text, model.clone())
        }
    } else {
        let text = groq::transcribe(&cfg, audio_bytes, &vocab, None)
            .await
            .map_err(|e| format!("{e:#}"))?;
        (text, model.clone())
    };
    let latency_ms = t_start.elapsed().as_millis() as u64;

    let try_id = db::save_transcription_try(
        &db,
        &recording_id,
        &provider,
        &final_model,
        &mode,
        &[mode.clone()],
        &[],
        &transcript,
        latency_ms,
    )
    .map_err(|e| format!("{e:#}"))?;

    let full = db::get_full_recording(&db, &recording_id)
        .map_err(|e| format!("{e:#}"))?
        .ok_or_else(|| "Recording not found".to_string())?;

    full.transcription_tries
        .into_iter()
        .find(|t| t.id == try_id)
        .ok_or_else(|| "Failed to retrieve saved transcription try".to_string())
}

#[tauri::command]
async fn rerun_post_process(
    recording_id: String,
    transcription_try_id: i64,
    mode_name: String,
    provider: String,
    model: String,
    thinking_level: Option<String>,
    prompt: String,
) -> Result<db::PostProcessTryRow, String> {
    let cfg = config::load();
    let db = db::open().map_err(|e| format!("{e:#}"))?;

    let full = db::get_full_recording(&db, &recording_id)
        .map_err(|e| format!("{e:#}"))?
        .ok_or_else(|| "Recording not found".to_string())?;

    let trans_try = full
        .transcription_tries
        .into_iter()
        .find(|t| t.id == transcription_try_id)
        .ok_or_else(|| "Transcription try not found".to_string())?;

    let t_start = Instant::now();
    let output = if provider.to_lowercase() == "gemini" {
        gemini::post_process(
            &cfg,
            &model,
            &prompt,
            &trans_try.transcript_text,
            thinking_level.as_deref(),
        )
        .await
        .map_err(|e| format!("{e:#}"))?
    } else {
        groq::post_process(&cfg, &model, &prompt, &trans_try.transcript_text)
            .await
            .map_err(|e| format!("{e:#}"))?
    };
    let latency_ms = t_start.elapsed().as_millis() as u64;

    let try_id = db::save_post_process_try(
        &db,
        &recording_id,
        transcription_try_id,
        &mode_name,
        &provider,
        &model,
        thinking_level.as_deref(),
        &prompt,
        &output,
        latency_ms,
    )
    .map_err(|e| format!("{e:#}"))?;

    let updated = db::get_full_recording(&db, &recording_id)
        .map_err(|e| format!("{e:#}"))?
        .ok_or_else(|| "Recording not found".to_string())?;

    updated
        .post_process_tries
        .into_iter()
        .find(|t| t.id == try_id)
        .ok_or_else(|| "Failed to retrieve saved post-process try".to_string())
}

#[tauri::command]
async fn test_webdav_connection(
    url: String,
    username: String,
    password: String,
    remote_dir: Option<String>,
) -> Result<webdav::WebdavQuota, String> {
    let dummy_cfg = Config {
        webdav_enabled: true,
        webdav_url: url,
        webdav_username: username,
        webdav_password: password,
        webdav_remote_dir: remote_dir.unwrap_or_else(|| "/DictationApp".to_string()),
        ..Default::default()
    };
    let client = webdav::WebdavClient::from_config(&dummy_cfg).map_err(|e| format!("{e:#}"))?;
    client.test_connection().await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn sync_webdav_now(
    _app: tauri::AppHandle,
) -> Result<webdav::SyncStatusResult, String> {
    let cfg = config::load();
    let db = db::open().map_err(|e| format!("Failed to open DB: {e:#}"))?;
    let dir = config::recordings_dir().map_err(|e| format!("{e:#}"))?;
    
    let res = webdav::run_sync(&db, &cfg, &dir).await.map_err(|e| format!("{e:#}"))?;
    
    if let Some(ts) = res.last_sync_at {
        let mut c = cfg.clone();
        c.webdav_last_sync_at = Some(ts);
        let _ = config::save(&c);
    }
    
    Ok(res)
}

#[tauri::command]
async fn restore_webdav_backup(
    _app: tauri::AppHandle,
    url: String,
    username: String,
    password: String,
    remote_dir: Option<String>,
) -> Result<webdav::CredentialsBackup, String> {
    let mut cfg = config::load();
    cfg.webdav_enabled = true;
    cfg.webdav_url = url;
    cfg.webdav_username = username;
    cfg.webdav_password = password;
    cfg.webdav_remote_dir = remote_dir.unwrap_or_else(|| "/DictationApp".to_string());
    
    let db = db::open().map_err(|e| format!("Failed to open DB: {e:#}"))?;
    let client = webdav::WebdavClient::from_config(&cfg).map_err(|e| format!("{e:#}"))?;
    let backup = client.restore_credentials(&db, &mut cfg).await.map_err(|e| format!("{e:#}"))?;
    
    config::save(&cfg).map_err(|e| format!("{e:#}"))?;
    
    if let Ok(dir) = config::recordings_dir() {
        let _ = webdav::run_sync(&db, &cfg, &dir).await;
    }
    
    Ok(backup)
}

#[tauri::command]
async fn get_webdav_sync_status(
    _app: tauri::AppHandle,
) -> Result<webdav::SyncStatusResult, String> {
    let cfg = config::load();
    if !cfg.webdav_enabled {
        return Ok(webdav::SyncStatusResult {
            connected: false,
            last_sync_at: cfg.webdav_last_sync_at,
            quota: None,
            total_records_synced: 0,
            message: "WebDAV sync is disabled".to_string(),
        });
    }
    let client = match webdav::WebdavClient::from_config(&cfg) {
        Ok(c) => c,
        Err(e) => return Ok(webdav::SyncStatusResult {
            connected: false,
            last_sync_at: cfg.webdav_last_sync_at,
            quota: None,
            total_records_synced: 0,
            message: format!("WebDAV configuration error: {e:#}"),
        }),
    };
    
    match client.get_quota().await {
        Ok(q) => Ok(webdav::SyncStatusResult {
            connected: true,
            last_sync_at: cfg.webdav_last_sync_at,
            quota: Some(q),
            total_records_synced: 0,
            message: "Connected to WebDAV".to_string(),
        }),
        Err(e) => Ok(webdav::SyncStatusResult {
            connected: false,
            last_sync_at: cfg.webdav_last_sync_at,
            quota: None,
            total_records_synced: 0,
            message: format!("Connection failed: {e:#}"),
        }),
    }
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct ProcessAudioOptions {
    pub language: Option<String>,
    pub context_mode_id: Option<i64>,
    pub phase1_mode: Option<String>,
    pub vocab_enabled: Option<bool>,
    pub vocab_profile_id: Option<i64>,
    pub phase2_enabled: Option<bool>,
    pub phase2_provider: Option<String>,
    pub phase2_model: Option<String>,
    pub thinking_level: Option<String>,
    pub selected_text: Option<String>,
}

#[derive(Debug, serde::Serialize, Default)]
pub struct ProcessAudioResult {
    pub recording_id: String,
    pub phase1_raw_text: String,
    pub phase2_processed_text: String,
    pub active_text: String,
    pub transcription_try_id: i64,
    pub post_process_try_id: Option<i64>,
}

fn wrap_pcm_as_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let num_channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * (num_channels as u32) * (bits_per_sample as u32) / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let data_len = pcm.len() as u32;
    let total_len = data_len + 36;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&total_len.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&num_channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}

async fn process_android_audio(
    pcm_bytes: Vec<u8>,
    sample_rate: u32,
    opts: ProcessAudioOptions,
) -> ProcessAudioResult {
    // Ensure rustls has a crypto provider installed for this process/thread
    let _ = rustls::crypto::ring::default_provider().install_default();

    if pcm_bytes.is_empty() {
        return ProcessAudioResult::default();
    }

    let samples: Vec<i16> = pcm_bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();

    let duration_ms = if sample_rate > 0 {
        ((samples.len() as u64) * 1000) / (sample_rate as u64)
    } else {
        0
    };

    let opus = if sample_rate == 16000 {
        opus_encoder::encode_pcm_to_ogg_opus(&samples).unwrap_or_default()
    } else {
        Vec::new()
    };

    let wav_fallback = if opus.is_empty() {
        wrap_pcm_as_wav(&pcm_bytes, sample_rate)
    } else {
        Vec::new()
    };

    let primary_audio: &[u8] = if !opus.is_empty() {
        &opus
    } else {
        &wav_fallback
    };

    let cfg = config::load();
    let db = match db::open() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("failed to open db on android: {e:#}");
            return ProcessAudioResult::default();
        }
    };

    let recording_id = uuid::Uuid::new_v4().to_string();
    let audio_file_name = if !opus.is_empty() {
        format!("{recording_id}.opus")
    } else {
        format!("{recording_id}.wav")
    };

    if let Ok(dir) = config::recordings_dir() {
        let p = dir.join(&audio_file_name);
        let _ = std::fs::write(&p, primary_audio);
    }

    let _ = db::save_recording(
        &db,
        &recording_id,
        &audio_file_name,
        duration_ms,
        primary_audio.len() as u64,
        false,
        Some("android.ime"),
        Some("Bulbul Keyboard"),
    );

    let active_mode_id = opts.context_mode_id.unwrap_or(cfg.active_context_mode_id);
    let context_mode = db::get_context_mode(&db, active_mode_id).ok().flatten()
        .or_else(|| db::list_context_modes(&db).ok().and_then(|modes| modes.into_iter().next()));

    // Resolve vocabulary based on options
    let (mut vocabulary, active_vocab_ids) = if opts.vocab_enabled.unwrap_or(true) {
        if let Some(prof_id) = opts.vocab_profile_id {
            if prof_id > 0 {
                if let Ok(profiles) = db::list_vocabulary_profiles(&db) {
                    if let Some(p) = profiles.into_iter().find(|p| p.id == prof_id) {
                        (p.terms, vec![prof_id])
                    } else {
                        db::get_active_vocabulary_terms(&db).unwrap_or_default()
                    }
                } else {
                    db::get_active_vocabulary_terms(&db).unwrap_or_default()
                }
            } else {
                db::get_active_vocabulary_terms(&db).unwrap_or_default()
            }
        } else {
            db::get_active_vocabulary_terms(&db).unwrap_or_default()
        }
    } else {
        (Vec::new(), Vec::new())
    };

    if opts.vocab_enabled.unwrap_or(true) {
        if let Ok(dict) = db::list_dictionary(&db) {
            for entry in dict {
                if !vocabulary.contains(&entry.to_word) {
                    vocabulary.push(entry.to_word);
                }
            }
        }
    }

    let lang_req = opts.language.as_deref().unwrap_or("auto");
    let lang = if lang_req != "auto" && !lang_req.trim().is_empty() {
        lang_req
    } else {
        &cfg.language
    };

    let is_bengali_gemini = lang == "bn-IN" || lang == "bn-IN-tech";
    let is_coding = lang == "bn-IN-tech" || context_mode.as_ref().map(|m| m.name.to_lowercase().contains("code")).unwrap_or(false);
    let phase1_mode = opts.phase1_mode.as_deref()
        .unwrap_or_else(|| context_mode.as_ref().map(|m| m.default_phase1_mode.as_str()).unwrap_or("smart"));

    let has_gemini = !cfg.gemini_api_key.trim().is_empty();
    let has_groq = !cfg.groq_api_key.trim().is_empty();
    let use_gemini = is_bengali_gemini || (!has_groq && has_gemini);

    let t_stt_start = Instant::now();
    let mut model_used = if use_gemini {
        "gemini-3.5-transcribe".to_string()
    } else {
        "whisper-large-v3".to_string()
    };

    let transcript = if use_gemini {
        match gemini::transcribe_with_fallback(
            &cfg,
            primary_audio,
            &vocabulary,
            is_coding,
            Some(phase1_mode),
            None,
            |_, _, _| {},
        ).await {
            Ok((t, m)) => {
                model_used = m;
                t
            }
            Err(e) => {
                tracing::warn!("Android Gemini STT failed ({e:#}), trying Groq fallback if available");
                if has_groq {
                    match groq::transcribe(&cfg, primary_audio.to_vec(), &vocabulary, None).await {
                        Ok(t) => {
                            model_used = "whisper-large-v3".to_string();
                            t
                        }
                        Err(ge) => {
                            tracing::error!("Android Groq fallback also failed: {ge:#}");
                            return ProcessAudioResult {
                                recording_id,
                                ..Default::default()
                            };
                        }
                    }
                } else {
                    return ProcessAudioResult {
                        recording_id,
                        ..Default::default()
                    };
                }
            }
        }
    } else {
        match groq::transcribe(&cfg, primary_audio.to_vec(), &vocabulary, None).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Android Groq STT failed ({e:#}), trying Gemini fallback if available");
                if has_gemini {
                    match gemini::transcribe_with_fallback(
                        &cfg,
                        primary_audio,
                        &vocabulary,
                        is_coding,
                        Some(phase1_mode),
                        None,
                        |_, _, _| {},
                    ).await {
                        Ok((t, m)) => {
                            model_used = m;
                            t
                        }
                        Err(ge) => {
                            tracing::error!("Android Gemini fallback also failed: {ge:#}");
                            return ProcessAudioResult {
                                recording_id,
                                ..Default::default()
                            };
                        }
                    }
                } else {
                    return ProcessAudioResult {
                        recording_id,
                        ..Default::default()
                    };
                }
            }
    };

    let t_stt_ms = t_stt_start.elapsed().as_millis() as u64;

    let provider_name = if is_bengali_gemini { "gemini" } else { "groq" };
    let language_codes = if is_bengali_gemini {
        vec!["bn-IN".to_string(), "en-IN".to_string()]
    } else {
        vec![lang.to_string()]
    };

    let transcription_try_id = db::save_transcription_try(
        &db,
        &recording_id,
        provider_name,
        &model_used,
        phase1_mode,
        &language_codes,
        &active_vocab_ids,
        &transcript,
        t_stt_ms,
    ).unwrap_or(0);

    if transcript.trim().is_empty() || (!is_bengali_gemini && groq::is_likely_hallucination(&transcript)) {
        return ProcessAudioResult {
            recording_id,
            transcription_try_id,
            ..Default::default()
        };
    }

    let phase1_raw_text = transcript.clone();
    let mut post_process_try_id = None;
    let mut phase2_processed_text = String::new();

    // Check if user spoke a voice command on selected text
    if let Some(sel) = opts.selected_text.as_deref().filter(|s| !s.trim().is_empty()) {
        let meta_prompt = "You are an expert editor. Follow the user's spoken instruction to transform the input text. Output ONLY the transformed text without preamble or quotes.";
        let input_content = format!("User Instruction: {}\n\nInput Text:\n{}", transcript, sel);
        let provider = opts.phase2_provider.as_deref().unwrap_or("gemini");
        let model = opts.phase2_model.as_deref().unwrap_or("gemini-2.5-flash");
        let t_p2 = Instant::now();

        let p2_res = if provider.to_lowercase() == "gemini" {
            gemini::post_process(&cfg, model, meta_prompt, &input_content, opts.thinking_level.as_deref()).await
        } else {
            groq::post_process(&cfg, model, meta_prompt, &input_content).await
        };
        let p2_lat = t_p2.elapsed().as_millis() as u64;

        if let Ok(out) = p2_res {
            if !out.trim().is_empty() {
                let pid = db::save_post_process_try(
                    &db,
                    &recording_id,
                    transcription_try_id,
                    "Voice Edit Selection",
                    provider,
                    model,
                    opts.thinking_level.as_deref(),
                    meta_prompt,
                    &out,
                    p2_lat,
                ).unwrap_or(0);
                post_process_try_id = Some(pid);
                phase2_processed_text = out;
            }
        }
    } else {
        let should_post_process = opts.phase2_enabled.unwrap_or_else(|| {
            context_mode.as_ref().map(|m| m.phase2_enabled).unwrap_or(false)
        });

        if should_post_process {
            let cm = context_mode.as_ref();
            let provider = opts.phase2_provider.as_deref()
                .or_else(|| cm.map(|m| m.phase2_provider.as_str()))
                .unwrap_or("gemini");
            let model = opts.phase2_model.as_deref()
                .or_else(|| cm.map(|m| m.phase2_model.as_str()))
                .unwrap_or("gemini-2.5-flash");
            let think = opts.thinking_level.as_deref()
                .or_else(|| cm.and_then(|m| m.thinking_level.as_deref()));
            let prompt = cm.map(|m| m.system_prompt.as_str()).unwrap_or("Clean up and format this text nicely.");
            let mode_name = cm.map(|m| m.name.as_str()).unwrap_or("Custom");

            let t_p2 = Instant::now();
            let p2_res = if provider.to_lowercase() == "gemini" {
                gemini::post_process(&cfg, model, prompt, &transcript, think).await
            } else {
                groq::post_process(&cfg, model, prompt, &transcript).await
            };
            let p2_lat = t_p2.elapsed().as_millis() as u64;

            if let Ok(out) = p2_res {
                if !out.trim().is_empty() {
                    let pid = db::save_post_process_try(
                        &db,
                        &recording_id,
                        transcription_try_id,
                        mode_name,
                        provider,
                        model,
                        think,
                        prompt,
                        &out,
                        p2_lat,
                    ).unwrap_or(0);
                    post_process_try_id = Some(pid);
                    phase2_processed_text = out;
                }
            }
        }
    }

    let active_text = if !phase2_processed_text.is_empty() {
        phase2_processed_text.clone()
    } else {
        phase1_raw_text.clone()
    };

    // Synchronize to dictations table so Home/Insights stats update immediately
    {
        let conn = db.lock();
        let _ = db::sync_legacy_dictations_from_recordings(&conn);
    }

    // Auto-sync in background if enabled
    if cfg.webdav_enabled && cfg.webdav_auto_sync {
        let db_sync = db.clone();
        let cfg_sync = cfg.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(dir) = config::recordings_dir() {
                let _ = webdav::run_sync(&db_sync, &cfg_sync, &dir).await;
            }
        });
    }

    ProcessAudioResult {
        recording_id,
        phase1_raw_text,
        phase2_processed_text,
        active_text,
        transcription_try_id,
        post_process_try_id,
    }
}

fn get_action_prompt(action: &str) -> String {
    match action.to_lowercase().as_str() {
        "polish" => "You are an expert editor. Polish and improve this text for clarity, grammar, and natural flow while preserving its original meaning and tone. Output ONLY the polished text without preamble or quotes.".to_string(),
        "translate" => "Translate this text accurately. If it is English, translate into natural Bengali (বাংলা). If it is Bengali or another language, translate into fluent English. Output ONLY the translated text without preamble or quotes.".to_string(),
        "code" => "You are an expert software engineer. Refactor, format, or improve this code snippet. Output ONLY the transformed code directly without markdown code block fences if meant for inline insertion.".to_string(),
        "email" => "You are a professional assistant. Rewrite this text into a polished, clear, and professional email message. Output ONLY the email body without preamble or quotes.".to_string(),
        _ => "Improve and format this text nicely. Output ONLY the result without preamble or quotes.".to_string(),
    }
}

async fn transform_android_selection(
    selected_text: String,
    action_type: String,
    custom_prompt: Option<String>,
) -> String {
    if selected_text.trim().is_empty() {
        return selected_text;
    }
    let cfg = config::load();
    let prompt = if let Some(cp) = custom_prompt {
        if !cp.trim().is_empty() {
            cp
        } else {
            get_action_prompt(&action_type)
        }
    } else {
        get_action_prompt(&action_type)
    };

    let model = if !cfg.gemini_api_key.trim().is_empty() {
        "gemini-2.5-flash"
    } else {
        "llama-3.3-70b-versatile"
    };

    if !cfg.gemini_api_key.trim().is_empty() {
        match gemini::post_process(&cfg, model, &prompt, &selected_text, None).await {
            Ok(out) if !out.trim().is_empty() => out,
            _ => selected_text,
        }
    } else if !cfg.groq_api_key.trim().is_empty() {
        match groq::post_process(&cfg, model, &prompt, &selected_text).await {
            Ok(out) if !out.trim().is_empty() => out,
            _ => selected_text,
        }
    } else {
        selected_text
    }
}

async fn rerun_android_phase2(
    recording_id: String,
    transcription_try_id: i64,
    mode_id: i64,
    provider_override: Option<String>,
    model_override: Option<String>,
    thinking_level: Option<String>,
) -> String {
    let cfg = config::load();
    let db = match db::open() {
        Ok(d) => d,
        Err(_) => return String::new(),
    };

    let full = match db::get_full_recording(&db, &recording_id) {
        Ok(Some(f)) => f,
        _ => return String::new(),
    };

    let trans_try = match full.transcription_tries.into_iter().find(|t| t.id == transcription_try_id) {
        Some(t) => t,
        None => return String::new(),
    };

    let mode = db::get_context_mode(&db, mode_id).ok().flatten()
        .or_else(|| db::list_context_modes(&db).ok().and_then(|ms| ms.into_iter().next()));

    let mode_name = mode.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| "Custom".to_string());
    let provider = provider_override.or_else(|| mode.as_ref().map(|m| m.phase2_provider.clone())).unwrap_or_else(|| "gemini".to_string());
    let model = model_override.or_else(|| mode.as_ref().map(|m| m.phase2_model.clone())).unwrap_or_else(|| "gemini-2.5-flash".to_string());
    let think = thinking_level.or_else(|| mode.as_ref().and_then(|m| m.thinking_level.clone()));
    let prompt = mode.as_ref().map(|m| m.system_prompt.clone()).unwrap_or_else(|| "Clean up and format this text nicely.".to_string());

    let t_start = Instant::now();
    let res = if provider.to_lowercase() == "gemini" {
        gemini::post_process(&cfg, &model, &prompt, &trans_try.transcript_text, think.as_deref()).await
    } else {
        groq::post_process(&cfg, &model, &prompt, &trans_try.transcript_text).await
    };
    let lat = t_start.elapsed().as_millis() as u64;

    match res {
        Ok(out) if !out.trim().is_empty() => {
            let _ = db::save_post_process_try(
                &db,
                &recording_id,
                transcription_try_id,
                &mode_name,
                &provider,
                &model,
                think.as_deref(),
                &prompt,
                &out,
                lat,
            );
            out
        }
        _ => trans_try.transcript_text,
    }
}

fn get_recent_recordings_json(limit: u32) -> String {
    let db = match db::open() {
        Ok(d) => d,
        Err(_) => return "[]".to_string(),
    };
    let recs = db::list_full_recordings(&db, limit, 0).unwrap_or_default();
    let mut list = Vec::new();
    for r in recs {
        let active_text = r.post_process_tries.last().map(|p| p.output_text.clone())
            .or_else(|| r.transcription_tries.last().map(|t| t.transcript_text.clone()))
            .unwrap_or_default();
        let raw_text = r.transcription_tries.last().map(|t| t.transcript_text.clone()).unwrap_or_default();
        let mode = r.post_process_tries.last().map(|p| p.mode_name.clone())
            .or_else(|| r.transcription_tries.last().map(|t| t.mode.clone()))
            .unwrap_or_else(|| "Voice".to_string());

        list.push(json!({
            "id": r.recording.id,
            "created_at": r.recording.created_at,
            "duration_ms": r.recording.duration_ms,
            "active_text": active_text,
            "raw_text": raw_text,
            "mode": mode,
            "is_pinned": r.recording.is_pinned
        }));
    }
    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
}

fn get_vocabulary_profiles_json() -> String {
    let db = match db::open() {
        Ok(d) => d,
        Err(_) => return "[]".to_string(),
    };
    let profiles = db::list_vocabulary_profiles(&db).unwrap_or_default();
    let list: Vec<Value> = profiles.into_iter().map(|p| {
        json!({
            "id": p.id,
            "name": p.name,
            "description": p.description,
            "is_active": p.is_active,
            "term_count": p.terms.len(),
        })
    }).collect();
    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
}

fn toggle_vocabulary_profile_jni(id: i64) -> bool {
    let db = match db::open() {
        Ok(d) => d,
        Err(_) => return false,
    };
    db::toggle_vocabulary_profile(&db, id).is_ok()
}

fn get_context_modes_json() -> String {
    let db = match db::open() {
        Ok(d) => d,
        Err(_) => return "[]".to_string(),
    };
    let modes = db::list_context_modes(&db).unwrap_or_default();
    let list: Vec<Value> = modes.into_iter().map(|m| {
        json!({
            "id": m.id,
            "name": m.name,
            "icon": m.icon,
            "default_phase1_mode": m.default_phase1_mode,
            "phase2_enabled": m.phase2_enabled,
            "phase2_provider": m.phase2_provider,
            "phase2_model": m.phase2_model,
            "thinking_level": m.thinking_level,
        })
    }).collect();
    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(target_os = "android")]
pub mod jni_bridge {
    use super::*;
    use jni::objects::{JByteArray, JClass, JString};
    use jni::sys::{jboolean, jint, jlong, jstring};
    use jni::JNIEnv;

    static JVM: std::sync::OnceLock<jni::JavaVM> = std::sync::OnceLock::new();

    pub fn call_main_activity_static(method: &str) -> Result<(), String> {
        let vm = JVM.get().ok_or_else(|| "JavaVM not initialized yet".to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let class = env.find_class("com/bulbul/app/MainActivity").map_err(|e| e.to_string())?;
        env.call_static_method(class, method, "()V", &[]).map_err(|e| e.to_string())?;
        Ok(())
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_initDataDir(
        mut env: JNIEnv,
        _class: JClass,
        dir_path: JString,
    ) {
        // Guarantee rustls has an active crypto provider for any background Android service
        let _ = rustls::crypto::ring::default_provider().install_default();

        if let Ok(vm) = env.get_java_vm() {
            let _ = JVM.set(vm);
        }
        if let Ok(p) = env.get_string(&dir_path) {
            let s: String = p.into();
            if !s.trim().is_empty() {
                config::set_android_app_dir(std::path::PathBuf::from(s));
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_processVoiceAudio(
        mut env: JNIEnv,
        _class: JClass,
        pcm_bytes: JByteArray,
        sample_rate: jint,
        language: JString,
        context_mode_id: jlong,
    ) -> jstring {
        let pcm_vec = env.convert_byte_array(&pcm_bytes).unwrap_or_default();
        let lang: String = env
            .get_string(&language)
            .map(|s| s.into())
            .unwrap_or_else(|_| "auto".to_string());

        let opts = ProcessAudioOptions {
            language: Some(lang),
            context_mode_id: Some(context_mode_id),
            ..Default::default()
        };

        let res = tauri::async_runtime::block_on(async move {
            process_android_audio(pcm_vec, sample_rate as u32, opts).await
        });

        let out = env.new_string(&res.active_text).unwrap_or_else(|_| env.new_string("").unwrap());
        out.into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_processVoiceAudioWithOptions(
        mut env: JNIEnv,
        _class: JClass,
        pcm_bytes: JByteArray,
        sample_rate: jint,
        options_json: JString,
    ) -> jstring {
        let pcm_vec = env.convert_byte_array(&pcm_bytes).unwrap_or_default();
        let opt_str: String = env
            .get_string(&options_json)
            .map(|s| s.into())
            .unwrap_or_default();

        let opts: ProcessAudioOptions = serde_json::from_str(&opt_str).unwrap_or_default();

        let res = tauri::async_runtime::block_on(async move {
            process_android_audio(pcm_vec, sample_rate as u32, opts).await
        });

        let json_str = serde_json::to_string(&res).unwrap_or_else(|_| "{}".to_string());
        let out = env.new_string(&json_str).unwrap_or_else(|_| env.new_string("").unwrap());
        out.into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_transformSelectedText(
        mut env: JNIEnv,
        _class: JClass,
        selected_text: JString,
        action_type: JString,
        custom_prompt: JString,
    ) -> jstring {
        let sel: String = env.get_string(&selected_text).map(|s| s.into()).unwrap_or_default();
        let act: String = env.get_string(&action_type).map(|s| s.into()).unwrap_or_default();
        let prompt: Option<String> = env.get_string(&custom_prompt).ok().map(|s| s.into()).filter(|s: &String| !s.trim().is_empty());

        let transformed = tauri::async_runtime::block_on(async move {
            transform_android_selection(sel, act, prompt).await
        });

        let out = env.new_string(&transformed).unwrap_or_else(|_| env.new_string("").unwrap());
        out.into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_rerunPhase2(
        mut env: JNIEnv,
        _class: JClass,
        recording_id: JString,
        transcription_try_id: jlong,
        mode_id: jlong,
        provider: JString,
        model: JString,
        thinking_level: JString,
    ) -> jstring {
        let rec_id: String = env.get_string(&recording_id).map(|s| s.into()).unwrap_or_default();
        let prov: Option<String> = env.get_string(&provider).ok().map(|s| s.into()).filter(|s: &String| !s.trim().is_empty());
        let mdl: Option<String> = env.get_string(&model).ok().map(|s| s.into()).filter(|s: &String| !s.trim().is_empty());
        let think: Option<String> = env.get_string(&thinking_level).ok().map(|s| s.into()).filter(|s: &String| !s.trim().is_empty());

        let res = tauri::async_runtime::block_on(async move {
            rerun_android_phase2(rec_id, transcription_try_id, mode_id, prov, mdl, think).await
        });

        let out = env.new_string(&res).unwrap_or_else(|_| env.new_string("").unwrap());
        out.into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_getRecentRecordings(
        env: JNIEnv,
        _class: JClass,
        limit: jint,
    ) -> jstring {
        let json_str = get_recent_recordings_json(limit as u32);
        let out = env.new_string(&json_str).unwrap_or_else(|_| env.new_string("").unwrap());
        out.into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_getVocabularyProfiles(
        env: JNIEnv,
        _class: JClass,
    ) -> jstring {
        let json_str = get_vocabulary_profiles_json();
        let out = env.new_string(&json_str).unwrap_or_else(|_| env.new_string("").unwrap());
        out.into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_toggleVocabularyProfile(
        _env: JNIEnv,
        _class: JClass,
        id: jlong,
        _active: jboolean,
    ) -> jboolean {
        if toggle_vocabulary_profile_jni(id) {
            1
        } else {
            0
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_getContextModes(
        env: JNIEnv,
        _class: JClass,
    ) -> jstring {
        let json_str = get_context_modes_json();
        let out = env.new_string(&json_str).unwrap_or_else(|_| env.new_string("").unwrap());
        out.into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_bulbul_app_BulbulBridge_syncWebdav(
        env: JNIEnv,
        _class: JClass,
    ) -> jstring {
        let status = tauri::async_runtime::block_on(async move {
            let cfg = config::load();
            if !cfg.webdav_enabled {
                return "WebDAV sync disabled".to_string();
            }
            if let Ok(dir) = config::recordings_dir() {
                if let Ok(db) = db::open() {
                    match webdav::run_sync(&db, &cfg, &dir).await {
                        Ok(res) => res.message,
                        Err(e) => format!("Sync error: {e:#}"),
                    }
                } else {
                    "Database init failed".to_string()
                }
            } else {
                "Recordings dir unavailable".to_string()
            }
        });

        let out = env.new_string(&status).unwrap_or_else(|_| env.new_string("").unwrap());
        out.into_raw()
    }
}

// ---------- Entry point ----------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // rustls 0.23 (transitively pulled in by reqwest's rustls-tls
    // feature) refuses to operate until a CryptoProvider is installed
    // in the process-default registry. On Windows + macOS reqwest
    // falls back to a native TLS backend so this never trips, but on
    // Android there is no native backend — the first reqwest call
    // (which Tauri's IPC layer makes internally on the first
    // invoke) panics with "No provider set" and aborts the process.
    // Install ring early; .ok() so a re-init from a hot-reload or a
    // duplicate caller doesn't itself panic.
    let _ = rustls::crypto::ring::default_provider().install_default();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            // Startup
            get_config,
            save_config,
            get_autostart,
            set_autostart,
            set_tray_visible,
            // Home + Insights
            get_home_stats,
            get_recent_dictations,
            delete_dictation,
            get_insights_usage,
            get_voice_stats,
            refresh_voice_narrative,
            // Dictionary + corrections
            list_dictionary,
            correction_suggestions,
            list_corrections,
            add_dictionary_entry,
            update_dictionary_entry,
            delete_dictionary_entry,
            dismiss_correction_suggestion,
            // Snippets
            list_snippets,
            add_snippet,
            update_snippet,
            delete_snippet,
            // Transforms
            list_transforms,
            list_transform_slot_statuses,
            add_transform,
            update_transform,
            delete_transform,
            set_default_transform,
            reset_transforms,
            run_transform_on_text,
            // Notes / scratchpad
            list_notes,
            create_note,
            update_note,
            delete_note,
            // Settings
            validate_api_key,
            transcribe_dictate_sample,
            get_overlay_snoozed_until,
            resume_overlay,
            // Overlay / scratchpad windows
            set_overlay_height,
            open_scratchpad,
            complete_onboarding,
            open_keyboard_settings,
            show_keyboard_picker,
            // V2 Multi-Phase Dictation & History
            list_recordings_v2,
            count_recordings_v2,
            get_full_recording_v2,
            delete_recording_v2,
            toggle_recording_pinned,
            get_recording_audio_bytes,
            list_context_modes,
            get_context_mode,
            save_context_mode,
            delete_context_mode,
            set_active_context_mode,
            list_vocabulary_profiles,
            save_vocabulary_profile,
            toggle_vocabulary_profile,
            delete_vocabulary_profile,
            rerun_transcription,
            rerun_post_process,
            // WebDAV Cloud Sync
            test_webdav_connection,
            sync_webdav_now,
            restore_webdav_backup,
            get_webdav_sync_status,
            prune_unpinned_cloud_audio,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Bulbul on mobile");
}
