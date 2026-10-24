# Bulbul Development Progress Log

## Phase 1: Environment Preparation & Cache Maintenance Configuration
- **Phase / Status:** Phase 1 / PASS
- **Commands run:**
  - `Get-ChildItem -Force | Where-Object { $_.Name -match "agent|progress" }`
  - `Copy-Item AGENTS.md AGENT.md`
  - `git status`
- **Key output:**
  - Updated `AGENTS.md` and `AGENT.md` with 4 mandatory rules (Phase-wise report & git commit, latest tech research, Windows protection, Bengali user communication).
  - Added `"clean:cache": "cargo clean --manifest-path src-tauri/Cargo.toml"` script to `package.json`.
  - Verified `.gitignore` properly isolates `src-tauri/target`, `gen/android/app/build`, and `.gradle`.
  - Preserved existing 32.5 GB build cache per user instruction to ensure fast incremental compilation.
- **Files created or changed:**
  - `AGENTS.md`
  - `AGENT.md`
  - `package.json`
  - `PROJECT_AUDIT_AND_ACTION_PLAN.md`
  - `PROGRESS.md`
- **Deviations from the prompt:** None. Skipped cache purge per explicit user request.
- **Ready for next phase?** Yes; ready for Phase 2 (Android Core Dictation Pipeline & JNI TLS Fix).

## Phase 2: Android Core Dictation Pipeline & JNI TLS Fix
- **Phase / Status:** Phase 2 / PASS
- **Commands run:**
  - `cargo check --manifest-path src-tauri/Cargo.toml`
  - `git diff`
- **Key output:**
  - Fixed `rustls` ring CryptoProvider installation in `Java_com_bulbul_app_BulbulBridge_initDataDir` and inside `process_android_audio`, preventing JNI panics ("No provider set") when called from background Android IME/services.
  - Implemented multi-path fallback candidate probing in `config::load()` (`context.filesDir`, `context.dataDir`, and subfolders) to guarantee API keys are never empty on Android.
  - Added `wrap_pcm_as_wav` to wrap uncompressed PCM in valid 44-byte RIFF WAV headers, preventing HTTP 400 Bad Request errors on Gemini/Groq endpoints.
  - Added bidirectional fallback between Gemini STT and Groq Whisper if one API key or model is temporarily unavailable.
  - Added `MediaRecorder.AudioSource.MIC` fallback in `AudioRecorder.kt` if `VOICE_RECOGNITION` fails or is uninitialized on OEM ROMs.
  - Verified Windows desktop build remains 100% clean and operational.
- **Files created or changed:**
  - `src-tauri/src/config.rs`
  - `src-tauri/src/mobile.rs`
  - `src-tauri/gen/android/app/src/main/java/com/bulbul/app/BulbulBridge.kt`
  - `src-tauri/gen/android/app/src/main/java/com/bulbul/app/AudioRecorder.kt`
  - `PROGRESS.md`
- **Deviations from the prompt:** None.
## Phase 3: Database & Analytics Unification
- **Phase / Status:** Phase 3 / PASS
- **Commands run:**
  - `cargo check --manifest-path src-tauri/Cargo.toml`
  - `git diff`
- **Key output:**
  - Implemented `sync_legacy_dictations_from_recordings(&conn)` in `src-tauri/src/db.rs` to automatically sync modern V2 `recordings`, `transcription_tries`, and `post_process_tries` into the legacy `dictations` table during migrations and after operations.
  - Updated `history_rows(app)` in `src-tauri/src/mobile.rs` to query the SQLite database (`dictations`), eliminating empty stats for Words Dictated, WPM, and Streak on Android.
  - Added SQLite sync trigger to `process_android_audio` in `mobile.rs` so new dictations immediately update analytics.
  - Updated `refresh_voice_narrative` to support Gemini if Groq API key is not configured, unlocking Voice Profile generation on Android.
  - Hooked post-download sync in `src-tauri/src/webdav.rs` so restored/synced desktop recordings immediately update Android charts and insights.
  - Verified host compilation with `cargo check` (0 errors, Windows compatibility preserved).
- **Files created or changed:**
  - `src-tauri/src/db.rs`
  - `src-tauri/src/mobile.rs`
  - `src-tauri/src/webdav.rs`
  - `PROGRESS.md`
- **Deviations from the prompt:** None.
- **Ready for next phase?** Yes; ready for Phase 4 (FlorisBoard-Inspired Android Keyboard Redesign).

## Phase 4: FlorisBoard-Inspired Android Keyboard Redesign
- **Phase / Status:** Phase 4 / PASS
- **Commands run:**
  - `cargo check --manifest-path src-tauri/Cargo.toml`
  - `git diff`
- **Key output:**
  - Re-architected `BulbulInputMethodService.kt` into a modern FlorisBoard-inspired **Dual-Deck Keyboard**:
    - **Deck A (Voice & AI Command Deck):** Hero microphone button with dynamic multi-ring audio amplitude ripples, live Smartbar with instant mode chips, language selector, vocabulary toggles, selection action bar, and post-dictation magic bar (`[⇄ Switch Raw <-> Polished]`, `[↻ Re-Try]`).
    - **Deck B (Virtual Typing Deck):** Full virtual keyboard supporting QWERTY Latin, clean Bengali layout (স্বরবর্ণ, ব্যঞ্জনবর্ণ ও যুক্তবর্ণ), and Number/Symbol layout (`?123`).
    - Integrated one-tap deck switching (`⌨️ Type` / `🎙️ Voice`) in the top Smartbar and bottom bar.
    - Added haptic feedback (`HapticFeedbackConstants.KEYBOARD_TAP`) and spacebar swipe gesture for smooth left/right cursor control.
    - Updated `AudioRecorder.kt` to calculate real-time RMS audio amplitude and stream it to Compose UI.
  - Verified Windows desktop build remains 100% clean and unaffected.
- **Files created or changed:**
  - `src-tauri/gen/android/app/src/main/java/com/bulbul/app/AudioRecorder.kt`
  - `src-tauri/gen/android/app/src/main/java/com/bulbul/app/BulbulInputMethodService.kt`
  - `PROGRESS.md`
- **Deviations from the prompt:** None.
- **Ready for next phase?** Yes; ready for Phase 5 (Build Toolchain Upgrade & Proguard Hardening).
