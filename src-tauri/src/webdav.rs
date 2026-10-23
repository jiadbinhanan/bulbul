use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::{
    aead::{Aead, KeyInit, rand_core::RngCore},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{bail, Context, Result};
use base64::prelude::*;
use futures::stream::{self, StreamExt};
use pbkdf2::pbkdf2_hmac;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::config::Config;
use crate::db::{self, Db, ContextModeRow, VocabularyProfileRow};

const PBKDF2_ROUNDS: u32 = 100_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const MAX_CONCURRENT_REQUESTS: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebdavQuota {
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteItem {
    pub href: String,
    pub name: String,
    pub etag: Option<String>,
    pub content_length: u64,
    pub is_collection: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordSyncPayload {
    pub id: String,
    pub audio_file_name: String,
    pub duration_ms: u64,
    pub audio_bytes: u64,
    pub is_pinned: bool,
    pub foreground_app: Option<String>,
    pub foreground_app_display: Option<String>,
    pub created_at: i64,
    pub deleted_at: Option<i64>,
    pub transcription_tries: Vec<TranscriptionTrySyncPayload>,
    pub post_process_tries: Vec<PostProcessTrySyncPayload>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranscriptionTrySyncPayload {
    pub provider: String,
    pub model: String,
    pub mode: String,
    pub language_codes: Vec<String>,
    pub vocabulary_profile_ids: Vec<i64>,
    pub transcript_text: String,
    pub latency_ms: u64,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PostProcessTrySyncPayload {
    pub transcription_try_index: usize,
    pub mode_name: String,
    pub provider: String,
    pub model: String,
    pub thinking_level: Option<String>,
    pub prompt_snapshot: String,
    pub output_text: String,
    pub latency_ms: u64,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CredentialsBackup {
    pub version: u32,
    pub exported_at: i64,
    pub groq_api_key: String,
    pub gemini_api_key: String,
    pub cerebras_api_key: String,
    pub cerebras_model: String,
    pub language: String,
    pub active_context_mode_id: i64,
    pub vocabulary_profiles: Vec<VocabularyProfileRow>,
    pub context_modes: Vec<ContextModeRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncStatusResult {
    pub connected: bool,
    pub last_sync_at: Option<i64>,
    pub quota: Option<WebdavQuota>,
    pub total_records_synced: usize,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Encryption & Decryption Helpers
// ---------------------------------------------------------------------------

/// Derives a 32-byte AES-256 key from a passphrase and salt using PBKDF2-HMAC-SHA256.
pub fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, PBKDF2_ROUNDS, &mut key);
    key
}

/// Encrypts plaintext bytes using PBKDF2-HMAC-SHA256 and AES-256-GCM.
/// Binary format: [16-byte salt][12-byte nonce][ciphertext + 16-byte tag]
pub fn encrypt_bytes(passphrase: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    aes_gcm::aead::OsRng.fill_bytes(&mut salt);
    aes_gcm::aead::OsRng.fill_bytes(&mut nonce_bytes);

    let key_bytes = derive_key(passphrase, &salt);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("AES-GCM encryption failed: {e}"))?;

    let mut out = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypts ciphertext bytes using PBKDF2-HMAC-SHA256 and AES-256-GCM.
pub fn decrypt_bytes(passphrase: &str, data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < SALT_LEN + NONCE_LEN + 16 {
        bail!("Ciphertext too short (minimum {} bytes required)", SALT_LEN + NONCE_LEN + 16);
    }

    let salt = &data[..SALT_LEN];
    let nonce_bytes = &data[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ciphertext = &data[SALT_LEN + NONCE_LEN..];

    let key_bytes = derive_key(passphrase, salt);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("AES-GCM decryption failed (wrong password or corrupted data): {e}"))?;

    Ok(plaintext)
}

// ---------------------------------------------------------------------------
// WebDAV Client
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct WebdavClient {
    pub base_url: String,
    pub remote_dir: String,
    pub username: String,
    pub password: String,
    pub client: reqwest::Client,
}

impl WebdavClient {
    pub fn from_config(cfg: &Config) -> Result<Self> {
        if !cfg.webdav_enabled {
            bail!("WebDAV is disabled in configuration");
        }
        if cfg.webdav_url.trim().is_empty() {
            bail!("WebDAV URL is empty");
        }
        if cfg.webdav_username.trim().is_empty() {
            bail!("WebDAV username is empty");
        }

        let base_url = cfg.webdav_url.trim().trim_end_matches('/').to_string();
        let remote_dir = if cfg.webdav_remote_dir.trim().is_empty() {
            "/DictationApp".to_string()
        } else {
            let mut d = cfg.webdav_remote_dir.trim().to_string();
            if !d.starts_with('/') {
                d = format!("/{d}");
            }
            d.trim_end_matches('/').to_string()
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("building HTTP client for WebDAV")?;

        Ok(Self {
            base_url,
            remote_dir,
            username: cfg.webdav_username.trim().to_string(),
            password: cfg.webdav_password.clone(),
            client,
        })
    }

    fn auth_header(&self) -> String {
        let cred = format!("{}:{}", self.username, self.password);
        format!("Basic {}", BASE64_STANDARD.encode(cred))
    }

    fn build_url(&self, path: &str) -> String {
        let p = path.trim_start_matches('/');
        if p.is_empty() {
            format!("{}{}", self.base_url, self.remote_dir)
        } else {
            format!("{}{}/{}", self.base_url, self.remote_dir, p)
        }
    }

    /// Tests connection and fetches RFC 4331 storage quota.
    pub async fn test_connection(&self) -> Result<WebdavQuota> {
        self.ensure_remote_dirs().await?;
        self.get_quota().await
    }

    /// Ensures remote directory structure exists: {remote_dir}, {remote_dir}/records, {remote_dir}/audio
    pub async fn ensure_remote_dirs(&self) -> Result<()> {
        let dirs = ["", "records", "audio"];
        for d in dirs {
            let url = self.build_url(d);
            let req = self
                .client
                .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &url)
                .header(reqwest::header::AUTHORIZATION, self.auth_header());
            
            let res = req.send().await;
            match res {
                Ok(r) => {
                    let s = r.status().as_u16();
                    // 201 Created, 405 Method Not Allowed (already exists), 301/302 redirect are all acceptable
                    if s == 201 || s == 405 || s == 301 || s == 302 || s == 200 {
                        continue;
                    }
                    tracing::warn!("MKCOL {url} returned HTTP {s}");
                }
                Err(e) => {
                    tracing::warn!("MKCOL {url} request failed: {e:#}");
                }
            }
        }
        Ok(())
    }

    /// Queries RFC 4331 storage quota via PROPFIND Depth: 0
    pub async fn get_quota(&self) -> Result<WebdavQuota> {
        let body = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:quota-available-bytes/>
    <d:quota-used-bytes/>
  </d:prop>
</d:propfind>"#;

        let res = self
            .client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &self.base_url)
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .header("Depth", "0")
            .header(reqwest::header::CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(body)
            .send()
            .await
            .context("PROPFIND quota request failed")?;

        let status = res.status();
        if !status.is_success() && status.as_u16() != 207 {
            bail!("WebDAV PROPFIND quota returned HTTP {}", status);
        }

        let xml = res.text().await.unwrap_or_default();
        parse_quota_xml(&xml)
    }

    /// Lists items in a directory using PROPFIND Depth: 1
    pub async fn list_directory(&self, subpath: &str) -> Result<HashMap<String, RemoteItem>> {
        let url = self.build_url(subpath);
        let body = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:getetag/>
    <d:getcontentlength/>
    <d:resourcetype/>
  </d:prop>
</d:propfind>"#;

        let res = self
            .client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .header("Depth", "1")
            .header(reqwest::header::CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(body)
            .send()
            .await
            .context("PROPFIND list directory failed")?;

        let status = res.status();
        if !status.is_success() && status.as_u16() != 207 {
            bail!("WebDAV PROPFIND list returned HTTP {}", status);
        }

        let xml = res.text().await.unwrap_or_default();
        parse_multistatus_items(&xml)
    }

    /// Uploads a file (PUT) and returns the ETag if provided by server.
    pub async fn put_file(&self, path: &str, data: Vec<u8>, content_type: &str) -> Result<Option<String>> {
        let url = self.build_url(path);
        let res = self
            .client
            .put(&url)
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(data)
            .send()
            .await
            .with_context(|| format!("PUT failed for {url}"))?;

        let status = res.status();
        if !status.is_success() {
            bail!("PUT {url} returned HTTP {}", status);
        }

        let etag = res
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(clean_etag);

        Ok(etag)
    }

    /// Downloads a file (GET) and returns bytes.
    pub async fn get_file(&self, path: &str) -> Result<Vec<u8>> {
        let url = self.build_url(path);
        let res = self
            .client
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .send()
            .await
            .with_context(|| format!("GET failed for {url}"))?;

        let status = res.status();
        if !status.is_success() {
            bail!("GET {url} returned HTTP {}", status);
        }

        let bytes = res.bytes().await.context("reading response body")?;
        Ok(bytes.to_vec())
    }

    /// Deletes a file (DELETE).
    pub async fn delete_file(&self, path: &str) -> Result<()> {
        let url = self.build_url(path);
        let res = self
            .client
            .delete(&url)
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .send()
            .await
            .with_context(|| format!("DELETE failed for {url}"))?;

        let status = res.status();
        if !status.is_success() && status.as_u16() != 404 {
            bail!("DELETE {url} returned HTTP {}", status);
        }
        Ok(())
    }

    /// Backs up encrypted credentials and settings to `credentials.json.enc`.
    pub async fn backup_credentials(&self, db: &Db, cfg: &Config) -> Result<()> {
        let vocab = db::list_vocabulary_profiles(db).unwrap_or_default();
        let modes = db::list_context_modes(db).unwrap_or_default();

        let backup = CredentialsBackup {
            version: 1,
            exported_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            groq_api_key: cfg.groq_api_key.clone(),
            gemini_api_key: cfg.gemini_api_key.clone(),
            cerebras_api_key: cfg.cerebras_api_key.clone(),
            cerebras_model: cfg.cerebras_model.clone(),
            language: cfg.language.clone(),
            active_context_mode_id: cfg.active_context_mode_id,
            vocabulary_profiles: vocab,
            context_modes: modes,
        };

        let json = serde_json::to_vec(&backup).context("serializing credentials backup")?;
        let encrypted = encrypt_bytes(&self.password, &json).context("encrypting credentials backup")?;

        self.put_file("credentials.json.enc", encrypted, "application/octet-stream").await?;
        tracing::info!("Credentials and settings backed up encrypted to WebDAV");
        Ok(())
    }

    /// Restores credentials and settings from `credentials.json.enc`.
    pub async fn restore_credentials(&self, db: &Db, cfg: &mut Config) -> Result<CredentialsBackup> {
        let encrypted = self.get_file("credentials.json.enc").await.context("fetching credentials.json.enc")?;
        let decrypted = decrypt_bytes(&self.password, &encrypted).context("decrypting credentials")?;
        let backup: CredentialsBackup = serde_json::from_slice(&decrypted).context("parsing credentials backup JSON")?;

        // Update config API keys if empty locally
        if cfg.groq_api_key.trim().is_empty() && !backup.groq_api_key.trim().is_empty() {
            cfg.groq_api_key = backup.groq_api_key.clone();
        }
        if cfg.gemini_api_key.trim().is_empty() && !backup.gemini_api_key.trim().is_empty() {
            cfg.gemini_api_key = backup.gemini_api_key.clone();
        }
        if cfg.cerebras_api_key.trim().is_empty() && !backup.cerebras_api_key.trim().is_empty() {
            cfg.cerebras_api_key = backup.cerebras_api_key.clone();
            cfg.cerebras_model = backup.cerebras_model.clone();
        }

        // Restore vocabulary profiles
        let local_vocab = db::list_vocabulary_profiles(db).unwrap_or_default();
        for r_prof in &backup.vocabulary_profiles {
            let exists = local_vocab.iter().any(|l| l.name == r_prof.name);
            if !exists {
                let _ = db::save_vocabulary_profile(
                    db,
                    None,
                    &r_prof.name,
                    &r_prof.description,
                    &r_prof.terms,
                    r_prof.is_active,
                );
            }
        }

        // Restore context modes
        let local_modes = db::list_context_modes(db).unwrap_or_default();
        for r_mode in &backup.context_modes {
            let exists = local_modes.iter().any(|l| l.name == r_mode.name);
            if !exists {
                let _ = db::save_context_mode(
                    db,
                    None,
                    &r_mode.name,
                    &r_mode.icon,
                    &r_mode.system_prompt,
                    &r_mode.default_phase1_mode,
                    r_mode.phase2_enabled,
                    &r_mode.phase2_provider,
                    &r_mode.phase2_model,
                    r_mode.thinking_level.as_deref(),
                    r_mode.hotkey.as_deref(),
                    r_mode.sort_order,
                );
            }
        }

        Ok(backup)
    }
}

// ---------------------------------------------------------------------------
// Full Bidirectional Sync Engine
// ---------------------------------------------------------------------------

pub async fn run_sync(db: &Db, cfg: &Config, recordings_dir: &Path) -> Result<SyncStatusResult> {
    let client = WebdavClient::from_config(cfg)?;
    client.ensure_remote_dirs().await?;

    let quota = client.get_quota().await.ok();

    // 1. Fetch remote records list (Depth: 1)
    let remote_records = client.list_directory("records").await.unwrap_or_default();

    // 2. Fetch remote audio list (Depth: 1)
    let remote_audio = if cfg.webdav_sync_audio {
        let aud = client.list_directory("audio").await.unwrap_or_default();
        aud.into_keys().collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };

    // 3. Reconcile local records
    let local_recordings = db::list_all_recordings_including_deleted(db).unwrap_or_default();
    let mut local_map = HashMap::new();
    for rec in &local_recordings {
        local_map.insert(rec.id.clone(), rec.clone());
    }

    // (A) Upload local records needing sync to remote
    let mut upload_queue = Vec::new();
    for rec in &local_recordings {
        let remote_key = format!("{}.json", rec.id);
        let needs_upload = match rec.synced_at {
            None => true,
            Some(synced) => {
                if let Some(del) = rec.deleted_at {
                    del > synced
                } else {
                    false
                }
            }
        };

        if needs_upload || !remote_records.contains_key(&remote_key) {
            upload_queue.push(rec.clone());
        }
    }

    // Process uploads with bounded concurrency
    let upload_stream = stream::iter(upload_queue.into_iter().map(|rec| {
        let client = client.clone();
        let db = db.clone();
        let recordings_dir = recordings_dir.to_path_buf();
        let sync_audio = cfg.webdav_sync_audio;
        let remote_audio_exists = remote_audio.contains(&format!("{}.opus", rec.id));

        async move {
            let rec_id = rec.id.clone();
            // Build full record payload
            let tries = db::list_transcription_tries(&db, &rec_id).unwrap_or_default();
            let mut sync_tries = Vec::new();
            let mut sync_p2 = Vec::new();

            for (idx, t) in tries.into_iter().enumerate() {
                let p2_tries = db::list_post_process_tries(&db, &rec_id, t.id).unwrap_or_default();
                for p2 in p2_tries {
                    sync_p2.push(PostProcessTrySyncPayload {
                        transcription_try_index: idx,
                        mode_name: p2.mode_name,
                        provider: p2.provider,
                        model: p2.model,
                        thinking_level: p2.thinking_level,
                        prompt_snapshot: p2.prompt_snapshot,
                        output_text: p2.output_text,
                        latency_ms: p2.latency_ms,
                        created_at: p2.created_at,
                    });
                }
                sync_tries.push(TranscriptionTrySyncPayload {
                    provider: t.provider,
                    model: t.model,
                    mode: t.mode,
                    language_codes: t.language_codes,
                    vocabulary_profile_ids: t.vocabulary_profile_ids,
                    transcript_text: t.transcript_text,
                    latency_ms: t.latency_ms,
                    created_at: t.created_at,
                });
            }

            let payload = RecordSyncPayload {
                id: rec.id.clone(),
                audio_file_name: rec.audio_file_path.clone(),
                duration_ms: rec.duration_ms,
                audio_bytes: rec.file_size_bytes,
                is_pinned: rec.is_pinned,
                foreground_app: rec.foreground_app.clone(),
                foreground_app_display: rec.foreground_app_display.clone(),
                created_at: rec.created_at,
                deleted_at: rec.deleted_at,
                transcription_tries: sync_tries,
                post_process_tries: sync_p2,
            };

            if let Ok(json_bytes) = serde_json::to_vec(&payload) {
                let remote_path = format!("records/{}.json", rec.id);
                if let Ok(etag) = client.put_file(&remote_path, json_bytes, "application/json").await {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let _ = db::update_recording_sync_metadata(&db, &rec.id, etag.as_deref(), now);
                }
            }

            // Upload audio if required and not yet on remote
            let opus_path = recordings_dir.join(format!("{}.opus", rec.id));
            let wav_path = recordings_dir.join(format!("{}.wav", rec.id));
            let local_has_audio = opus_path.exists() || wav_path.exists();

            if sync_audio && !remote_audio_exists && local_has_audio {
                let (audio_data, file_name, mime) = if opus_path.exists() {
                    (std::fs::read(&opus_path).ok(), format!("{}.opus", rec.id), "audio/ogg")
                } else if wav_path.exists() {
                    (std::fs::read(&wav_path).ok(), format!("{}.wav", rec.id), "audio/wav")
                } else {
                    (None, String::new(), "")
                };

                if let Some(bytes) = audio_data {
                    let remote_audio_path = format!("audio/{file_name}");
                    let _ = client.put_file(&remote_audio_path, bytes, mime).await;
                }
            }
        }
    }))
    .buffer_unordered(MAX_CONCURRENT_REQUESTS);

    upload_stream.collect::<Vec<()>>().await;

    // (B) Download remote records that do not exist locally
    let mut download_queue = Vec::new();
    for (filename, item) in &remote_records {
        if !filename.ends_with(".json") {
            continue;
        }
        let rec_id = filename.trim_end_matches(".json").to_string();
        if !local_map.contains_key(&rec_id) {
            download_queue.push((rec_id, filename.clone(), item.etag.clone()));
        }
    }

    let download_stream = stream::iter(download_queue.into_iter().map(|(_rec_id, filename, remote_etag)| {
        let client = client.clone();
        let db = db.clone();
        let recordings_dir = recordings_dir.to_path_buf();
        let sync_audio = cfg.webdav_sync_audio;

        async move {
            let remote_path = format!("records/{filename}");
            if let Ok(bytes) = client.get_file(&remote_path).await {
                if let Ok(payload) = serde_json::from_slice::<RecordSyncPayload>(&bytes) {
                    // Ingest recording into local SQLite
                    let _ = db::ingest_synced_recording(
                        &db,
                        &payload.id,
                        &payload.audio_file_name,
                        payload.duration_ms,
                        payload.audio_bytes,
                        payload.is_pinned,
                        payload.foreground_app.as_deref(),
                        payload.foreground_app_display.as_deref(),
                        payload.created_at,
                        payload.deleted_at,
                        remote_etag.as_deref(),
                    );

                    // Ingest transcription tries and post-process tries
                    let mut created_try_ids = Vec::new();
                    for t in payload.transcription_tries {
                        let try_id = db::save_transcription_try(
                            &db,
                            &payload.id,
                            &t.provider,
                            &t.model,
                            &t.mode,
                            &t.language_codes,
                            &t.vocabulary_profile_ids,
                            &t.transcript_text,
                            t.latency_ms,
                        ).unwrap_or(0);
                        created_try_ids.push(try_id);
                    }

                    for p2 in payload.post_process_tries {
                        let parent_try_id = created_try_ids
                            .get(p2.transcription_try_index)
                            .copied()
                            .unwrap_or(0);
                        let _ = db::save_post_process_try(
                            &db,
                            &payload.id,
                            parent_try_id,
                            &p2.mode_name,
                            &p2.provider,
                            &p2.model,
                            p2.thinking_level.as_deref(),
                            &p2.prompt_snapshot,
                            &p2.output_text,
                            p2.latency_ms,
                        );
                    }

                    // Download audio if audio sync enabled
                    if sync_audio {
                        let local_opus = recordings_dir.join(format!("{}.opus", payload.id));
                        let local_wav = recordings_dir.join(format!("{}.wav", payload.id));
                        if !local_opus.exists() && !local_wav.exists() {
                            let remote_audio_path = format!("audio/{}.opus", payload.id);
                            if let Ok(audio_bytes) = client.get_file(&remote_audio_path).await {
                                let _ = std::fs::write(&local_opus, audio_bytes);
                            }
                        }
                    }
                }
            }
        }
    }))
    .buffer_unordered(MAX_CONCURRENT_REQUESTS);

    download_stream.collect::<Vec<()>>().await;

    // (C) Backup encrypted credentials
    let _ = client.backup_credentials(db, cfg).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let synced_count = remote_records.len();

    Ok(SyncStatusResult {
        connected: true,
        last_sync_at: Some(now),
        quota,
        total_records_synced: synced_count,
        message: format!("Successfully synced with WebDAV ({synced_count} records)"),
    })
}

/// Deletes unpinned audio files from the WebDAV cloud storage to free up quota.
pub async fn prune_unpinned_cloud_audio(db: &Db, cfg: &Config) -> Result<u32> {
    if !cfg.webdav_enabled {
        return Ok(0);
    }
    let client = WebdavClient::from_config(cfg)?;
    let unpinned_ids = {
        let conn = db.lock();
        let mut stmt = conn.prepare("SELECT id FROM recordings WHERE is_pinned = 0 AND deleted_at IS NULL")?;
        let ids: Vec<String> = stmt.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
        ids
    };
    let mut deleted = 0;
    for id in unpinned_ids {
        let remote_audio_path = format!("audio/{id}.opus");
        if client.delete_file(&remote_audio_path).await.is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

// ---------------------------------------------------------------------------
// XML Parsing Helpers for WebDAV Responses
// ---------------------------------------------------------------------------

fn clean_etag(raw: &str) -> String {
    raw.trim().trim_matches('"').to_string()
}

pub fn parse_quota_xml(xml: &str) -> Result<WebdavQuota> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut used_bytes: Option<u64> = None;
    let mut available_bytes: Option<u64> = None;
    let mut current_tag = String::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                current_tag = name.to_lowercase();
            }
            Ok(Event::Text(e)) => {
                let txt = e.unescape().unwrap_or_default();
                if current_tag.ends_with("quota-used-bytes") {
                    used_bytes = txt.parse().ok();
                } else if current_tag.ends_with("quota-available-bytes") {
                    available_bytes = txt.parse().ok();
                }
            }
            Ok(Event::End(_)) => {
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("XML parse error in quota response: {e:#}");
                break;
            }
            _ => (),
        }
        buf.clear();
    }

    let used = used_bytes.unwrap_or(0);
    let avail = available_bytes.unwrap_or(0);
    Ok(WebdavQuota {
        used_bytes: used,
        available_bytes: avail,
        total_bytes: used.saturating_add(avail),
    })
}

pub fn parse_multistatus_items(xml: &str) -> Result<HashMap<String, RemoteItem>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut items = HashMap::new();
    let mut current_tag = String::new();

    let mut in_response = false;
    let mut in_resourcetype = false;

    let mut href = String::new();
    let mut etag: Option<String> = None;
    let mut content_length: u64 = 0;
    let mut is_collection = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string().to_lowercase();
                if name == "response" {
                    in_response = true;
                    href.clear();
                    etag = None;
                    content_length = 0;
                    is_collection = false;
                } else if name == "resourcetype" {
                    in_resourcetype = true;
                } else if in_resourcetype && name == "collection" {
                    is_collection = true;
                }
                current_tag = name;
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string().to_lowercase();
                if in_resourcetype && name == "collection" {
                    is_collection = true;
                }
            }
            Ok(Event::Text(e)) => {
                let txt = e.unescape().unwrap_or_default().trim().to_string();
                if in_response {
                    if current_tag == "href" {
                        href = txt;
                    } else if current_tag == "getetag" {
                        etag = Some(clean_etag(&txt));
                    } else if current_tag == "getcontentlength" {
                        content_length = txt.parse().unwrap_or(0);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string().to_lowercase();
                if name == "response" {
                    if !href.is_empty() && !is_collection {
                        let filename = href
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or_default()
                            .to_string();
                        if !filename.is_empty() {
                            items.insert(
                                filename.clone(),
                                RemoteItem {
                                    href: href.clone(),
                                    name: filename,
                                    etag: etag.clone(),
                                    content_length,
                                    is_collection,
                                },
                            );
                        }
                    }
                    in_response = false;
                } else if name == "resourcetype" {
                    in_resourcetype = false;
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("XML parse error in multistatus response: {e:#}");
                break;
            }
            _ => (),
        }
        buf.clear();
    }

    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_roundtrip() {
        let password = "CorrectHorseBatteryStaple#2026";
        let secret_data = b"{\"groq_api_key\": \"gsk_test12345\", \"modes\": [1, 2, 3]}";

        let encrypted = encrypt_bytes(password, secret_data).expect("encryption succeeds");
        assert_ne!(encrypted, secret_data);
        assert!(encrypted.len() > secret_data.len());

        let decrypted = decrypt_bytes(password, &encrypted).expect("decryption succeeds");
        assert_eq!(decrypted, secret_data);

        // Wrong password must fail with auth error
        let wrong = decrypt_bytes("WrongPassword123", &encrypted);
        assert!(wrong.is_err());
    }

    #[test]
    fn test_quota_xml_parsing() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/</d:href>
    <d:propstat>
      <d:prop>
        <d:quota-available-bytes>10737418240</d:quota-available-bytes>
        <d:quota-used-bytes>5368709120</d:quota-used-bytes>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        let quota = parse_quota_xml(xml).expect("parse quota xml");
        assert_eq!(quota.available_bytes, 10737418240);
        assert_eq!(quota.used_bytes, 5368709120);
        assert_eq!(quota.total_bytes, 16106127360);
    }

    #[test]
    fn test_multistatus_parsing() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/Koofr/DictationApp/records/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/Koofr/DictationApp/records/rec-001.json</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"abcd1234efgh"</d:getetag>
        <d:getcontentlength>2048</d:getcontentlength>
        <d:resourcetype/>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        let items = parse_multistatus_items(xml).expect("parse multistatus items");
        assert_eq!(items.len(), 1);
        let rec = items.get("rec-001.json").expect("found rec-001.json");
        assert_eq!(rec.etag, Some("abcd1234efgh".to_string()));
        assert_eq!(rec.content_length, 2048);
        assert!(!rec.is_collection);
    }
}
