# Project Planning V2 (`planning V2.md`)
**Status:** Active Master Plan (Overrides all previous V1 planning documents)  
**Target Agent:** Anti-Gravity IDE  
**Date:** October 2026

---

## 0. CRITICAL OPERATING DIRECTIVES FOR ANTI-GRAVITY

Before reading the technical specifications below, please strictly follow these two meta-instructions:

1. **MANDATORY LIVE WEB RESEARCH (NO TRAINING CUTOFF ASSUMPTIONS):**
   * **Do NOT rely on your pre-trained knowledge cutoff** for API payloads, model names, SDK versions, or Rust crate documentation.
   * Before writing any integration code for **Google Gemini API** (`gemini-3.5-transcribe`, `gemini-3.5-flash-lite`, `gemini-3.6-flash`, `gemini-3.8-flash`, etc.) or **Groq API**, you **MUST use your web search / scraper tools** to fetch the latest official documentation as of late 2026.
   * Verify exact JSON schemas (such as `audioTranscriptionConfig`, `thinking_level`, `languageCodes`, `customVocabulary`, and MIME type support) from current live documentation before implementing.

2. **COLLABORATIVE ARCHITECTURAL STANCE (OPINION, NOT DICTATE):**
   * Treat the architectural details, database schemas, and audio compression formats in this document as **proposed recommendations and design opinions**, not rigid commands.
   * **Your Task First:** Read this document alongside the existing codebase (`src-tauri` and frontend). Analyze how the current app works, identify any gaps, bottlenecks, or better alternatives in this plan, and **share your technical opinion, critique, and clarifying questions** with me before executing major code changes.

---

## 1. Project Pivot: Standalone Fork (Overriding V1 Contribution Plan)

* **Previous Plan (Deprecated):** Originally, the goal was to minimally modify the `bulbul` codebase to submit an upstream PR/contribution. **Disregard that constraint completely.**
* **New Vision (V2):** We are building our own **standalone, full-featured desktop application** using the downloaded `bulbul` source code solely as a starter foundation.
* We have complete freedom to refactor the SQLite database schema, redesign the UI/UX, overhaul the Rust audio pipeline, and introduce deep customization tailored for **Bengali (India) + English** bilingual power users.

---

## 2. High-Level Workflow & Core Philosophy

The application follows a **Local-First, Two-Phase Modular Pipeline** inspired by the best parts of *Whisperian*, enhanced with full user control at every stage:

1. **Capture & Persist First:** Audio is captured via microphone, processed/compressed locally, and **saved to local storage immediately** before any network request is made. (Ensures zero data loss on network/API failure).
2. **Phase 1 — Transcription (STT):** Sends the audio to the user-selected transcription engine and saves the raw/smart transcript to the local database.
3. **Phase 2 — Post-Processing (Optional LLM Pass):** If enabled by the user, takes the Phase 1 transcript and passes it to a user-selected LLM (Groq or Gemini) using a context-specific prompt (Coding Agent, Email, Chat, Translation, etc.), saving the result as a linked output.
4. **Non-Destructive Multi-Try History:** Users can listen to old recordings, inspect Phase 1 and Phase 2 outputs separately, and **re-run either Phase 1 or Phase 2** with different models, modes, or prompts without overwriting previous attempts.

---

## 3. Detailed Feature Specifications

### 3.1. First-Class Bengali (`bn-IN`) Support & Onboarding
* **Onboarding / Start Screen & Settings:** Create a dedicated, prominent **Bengali Language Configuration** section during initial setup and inside the Settings panel.
* **Provider Enforcement Rule:**
  * When **Bengali** (or **Bengali + English Mix**) is active as the transcription language, **Groq STT must be disabled/unavailable** in the UI (as Whisper models struggle with natural Indo-Bengali code-switching and script fidelity).
  * **Only `gemini-3.5-transcribe`** will be active for Bengali transcription.
  * For pure English or other supported languages, the user can freely switch between Groq STT and Gemini STT.
* **Language Code Strategy for Bilingual Speech:**
  * Default Bengali configuration should pass `languageCodes: ["bn-IN", "en-IN"]` to `gemini-3.5-transcribe` so intra-sentence code-switching (speaking Bengali mixed with English technical terms) works seamlessly.
  * Give advanced users the option in Settings to toggle between `["bn-IN", "en-IN"]`, `["bn-IN"]` only, or `[]` (Auto-detect).

### 3.2. Phase 1: Transcription Engine & Multi-Vocabulary System
Users must have granular control over how `gemini-3.5-transcribe` behaves:

1. **Transcription Mode Selector:**
   * `SMART` Mode: Automatically strips filler words, stuttering, repetitions, and formats numbers/punctuation cleanly during transcription.
   * `VERBATIM` Mode: Raw word-for-word transcription with zero filtering.
2. **Multi-Vocabulary Manager (Selectable Custom Dictionaries):**
   * Toggle: **Send Custom Vocabulary (On / Off)**.
   * **Multiple Named Vocabulary Sets:** Instead of a single flat dictionary list, allow users to create, edit, and manage **multiple named vocabulary profiles** (up to 1,000 terms per request as supported by Gemini), such as:
     * *Full-Stack & Dev Stack* (e.g., `Next.js`, `Drizzle ORM`, `Better Auth`, `PostgreSQL`, `Coolify`, `Panet`, `Anti-Gravity`)
     * *Business & Client Names*
     * *General Bengali-English Slang / Proper Nouns*
   * Users can select **which vocabulary set(s)** to attach globally or bind specific vocabulary sets to specific Post-Processing Modes.
   * *Note for Anti-Gravity:* Ensure the code respects Gemini API constraints (e.g., `customVocabulary` and `SMART` mode compatibility rules vs `diarization`/`wordTimestamp`).

### 3.3. Phase 2: Configurable Post-Processing Engine
Users can toggle Post-Processing **ON or OFF** globally or per mode.
* **When OFF:** The output from Phase 1 (`gemini-3.5-transcribe`) is immediately injected into the active cursor/clipboard.
* **When ON:** The Phase 1 text is sent to the configured Phase 2 LLM.

#### A. Multi-Brand & Tiered Model Selection
Users can choose their post-processing provider and specific model tier:
* **Google Gemini Provider:**
  * Support tiered model options (verify latest 2026 model IDs via web search):
    * **Low / Fast Tier:** e.g., `gemini-3.5-flash-lite` (for ultra-low latency formatting).
    * **Mid / Balanced Tier:** e.g., `gemini-3.6-flash` / `gemini-2.5-flash`.
    * **High / Reasoning Tier:** e.g., `gemini-3.8-flash` (for complex structuring like Coding Agent prompts).
  * **Thinking Level / Reasoning Control:** Allow users to configure `thinking_level` (`MINIMAL`, `LOW`, `MEDIUM`, `HIGH`) or temperature (depending on the model generation) so latency vs. intelligence is fully controllable.
* **Groq Provider:**
  * Allow selecting fast text-generation models hosted on Groq (e.g., Llama 3.3/4, Qwen, DeepSeek, or custom model ID entry) for near-instant post-processing after Gemini finishes Phase 1 transcription.

#### B. Context Modes / Use-Case Presets (Prompt Profiles)
Users should be able to select from built-in presets and **create/edit unlimited custom modes**. Each mode stores:
* **Mode Name & Icon**
* **System / Formatting Prompt** (fully editable by user)
* **Default Phase 1 Mode** (`SMART` vs `VERBATIM`) & **Linked Vocabulary Set(s)**
* **Default Phase 2 Provider & Model** (or "Skip Phase 2")
* **Optional Dedicated Hotkey** (so the user can trigger "Coding Mode" or "Chat Mode" directly from the keyboard)

**Starter Presets to Include:**
1. **Direct Smart Dictation (No Post-Processing):** Runs only Phase 1 (`gemini-3.5-transcribe` in `SMART` mode) and pastes directly.
2. **Coding Agent Prompt Mode:** Converts Bengali+English spoken thoughts into a structured, technically accurate prompt for AI coding IDEs (Cursor, Anti-Gravity), preserving English identifiers and library names.
3. **Professional Email Mode:** Transforms casual Bengali/Benglish speech into a polished, polite email (in English or formal Bengali based on user sub-option).
4. **Natural Chat Mode:** Keeps casual Bengali in clean Bengali Unicode script and natural English chat/tech words in English script without over-formalizing.
5. **Direct Translation Mode:** Translates spoken Bengali/Benglish directly into fluent English (or another target language).

---

## 4. Audio Storage & Compression Architecture (Whisperian Style)

### Proposed Opinion / Recommendation (For Anti-Gravity to Review):
In the original `bulbul` codebase, `cpal` records raw PCM audio in memory. Sending large uncompressed WAV files over Base64 to Gemini increases upload latency and has been reported in Google developer forums to occasionally return empty `parts: [{}]` responses on certain raw 16kHz PCM headers.

* **Recommended Pipeline:**
  1. Capture audio at native sample rate via `cpal`, resample to **16kHz Mono**.
  2. Apply a subtle safety trim/normalization (and detect pure silence locally so we don't waste API calls on accidental empty key presses).
  3. **Compress Audio:** Encode the recording into a lightweight, high-compatibility format before saving and uploading—**OGG/Opus (`audio/ogg`)** or **MP3/AAC (`audio/mp3` / `audio/m4a`)** at ~32–64 kbps mono.
     * *Why:* A 30-second voice note in Opus/MP3 is only ~120–200 KB (compared to ~1 MB+ in WAV). It saves 80% local disk space, uploads via Base64 `inlineData` almost instantaneously, and avoids the raw WAV header bug on `gemini-3.5-transcribe`.
  4. Save the compressed file to `{app_data_dir}/recordings/{uuid}.{ext}`.

*(Anti-Gravity: Please evaluate which Rust audio encoding crate—such as `opus`, `ogg`, `symphonia`, `mp3lame-encoder`, or keeping an optimized WAV—is easiest and most reliable to compile cross-platform in our current `Cargo.toml` setup without heavy C-dependency headaches, and share your recommendation!)*

---

## 5. Database Schema Redesign: History & Multi-Try Versioning

Unlike standard dictation apps that overwrite history on re-try, our app must **preserve every transcription and post-processing attempt** linked to the original audio file.

### Proposed Relational SQLite Schema:

1. **`recordings` (Parent Table)**
   * `id` (PK, UUID/Integer)
   * `audio_file_path` (Text — relative path to saved local audio)
   * `duration_ms` (Integer)
   * `file_size_bytes` (Integer)
   * `created_at` (Timestamp)
   * `is_pinned` (Boolean — prevents auto-cleanup)

2. **`transcription_tries` (Phase 1 Children — 1-to-Many with `recordings`)**
   * `id` (PK)
   * `recording_id` (FK -> `recordings.id` ON DELETE CASCADE)
   * `provider` (`"gemini"` | `"groq"`)
   * `model` (e.g., `"gemini-3.5-transcribe"`)
   * `mode` (`"SMART"` | `"VERBATIM"`)
   * `language_codes` (JSON array, e.g., `["bn-IN", "en-IN"]`)
   * `vocabulary_profile_ids` (JSON array of used vocab sets)
   * `transcript_text` (Text — the exact output from Phase 1)
   * `latency_ms` (Integer)
   * `created_at` (Timestamp)

3. **`post_process_tries` (Phase 2 Children — 1-to-Many with `transcription_tries`)**
   * `id` (PK)
   * `recording_id` (FK -> `recordings.id` ON DELETE CASCADE)
   * `transcription_try_id` (FK -> `transcription_tries.id` ON DELETE CASCADE)
   * `mode_name` (e.g., `"Coding Agent"`, `"Email"`, `"Custom"`)
   * `provider` (`"gemini"` | `"groq"`)
   * `model` (e.g., `"gemini-3.8-flash"`, `"llama-3.3-70b-versatile"`)
   * `thinking_level` (Text, nullable)
   * `prompt_snapshot` (Text — exact prompt used at the time of execution)
   * `output_text` (Text — the final post-processed output)
   * `latency_ms` (Integer)
   * `created_at` (Timestamp)

### History UI Capabilities:
* **Audio Player:** Play/Pause/Scrub the saved local audio right inside each history card.
* **Split / Tabbed View:** Clearly view **Phase 1 (Transcription)** and **Phase 2 (Post-Processed)** outputs separately, with one-click copy buttons for both.
* **Version Switcher & Re-Try Controls:**
  * *"Re-Transcribe Audio"*: Pick a different mode (`VERBATIM` vs `SMART`) or vocabulary set and run Phase 1 again on the saved audio file. Creates a new entry in `transcription_tries`.
  * *"Re-Run Post-Processing"*: Pick any saved Phase 1 transcript, choose a different Mode (e.g., switch from *Chat* to *Email* or *Coding Prompt*) or a different Model (e.g., compare *Groq* vs *Gemini High*), and run Phase 2 again. Creates a new entry in `post_process_tries`.
  * Dropdown/tabs on the history card let the user flip through **"Try #1", "Try #2", "Try #3"** for both phases seamlessly.

---

## 6. Additional UX Enhancements to Consider (Open Proposals)

1. **Quick Mode Switcher HUD / Tray Menu:**
   * Near the floating recording pill/overlay (or via a shortcut), allow switching the active Mode (e.g., *Smart Dictation* vs *Coding Prompt* vs *Email*) on the fly without opening the main window.
2. **Offline / Network Failure Recovery:**
   * Since audio is saved to disk *first*, if the internet drops or an API rate-limits during dictation, the recording appears in History with a `"Failed / Pending"` badge so the user can click **"Retry"** once back online—never losing a single spoken thought.
3. **Storage Management Settings:**
   * Option to auto-delete unpinned audio files after X days (7, 30, 90, or Never) while keeping the text history intact, plus a visual indicator showing total disk space used by recordings.

---

## 7. Next Steps & Instructions for Anti-Gravity

1. **Step 1 — Research:** Search the web right now for the latest **2026 Google Gemini API documentation** (specifically `gemini-3.5-transcribe` `audioTranscriptionConfig`, `customVocabulary`, and the current Gemini 3.5 / 3.6 / 3.8 Flash model IDs and parameters).
2. **Step 2 — Codebase Audit:** Inspect the current `bulbul` repository structure (`src-tauri` Rust backend and the frontend).
3. **Step 3 — Report & Discuss (DO NOT WRITE CODE YET):**
   * Tell me what you found in the latest online documentation.
   * Point out any gaps, edge cases, or architectural flaws in this `planning V2.md`.
   * Give your opinion on the best Rust crate for audio compression (Opus/OGG vs MP3 vs WAV) on Windows.
   * Propose a phased implementation roadmap (e.g., Milestone 1: DB & Audio Persistence, Milestone 2: Gemini 3.5 Transcribe & Vocab, Milestone 3: Post-Processing & Modes, Milestone 4: Multi-Try History UI) and ask me any clarifying questions before we begin Milestone 1.