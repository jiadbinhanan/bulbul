
# Sync, Storage & Android AI Keyboard Planning (`sync and keyboard planning.md`)
**Companion to:** `planning V2.md` (Standalone V2 Architecture)  
**Target Agent:** Anti-Gravity IDE  
**Date:** October 2026

---

## 0. CRITICAL CONTEXT & INSTRUCTIONS FOR ANTI-GRAVITY (READ FIRST)

1. **WHY YOU MUST CROSS-CHECK & ADAPT THIS PLAN:**
   * This planning document combines architectural ideas drafted with external planners who **do NOT have full visibility into our actual local codebase**.
   * **You (Anti-Gravity) are the only one who knows our complete, real codebase architecture** (`src-tauri`, frontend, database state, and existing implementations).
   * Therefore, **do NOT treat this document as a blind, rigid specification.** Your job is to read this plan, understand the exact product goals, cross-check every detail against our actual codebase, and **adapt the technical implementation** so it integrates cleanly using industry best practices.

2. **MANDATORY LIVE WEB RESEARCH (2026 LATEST DATA ONLY):**
   * **Do NOT rely on your pre-cutoff training knowledge.**
   * Before finalizing your architectural report or writing any code, **use your web search and scraping tools** to verify the latest (late 2026) documentation, API schemas, Rust crates (`crates.io` / `docs.rs`), WebDAV specifications, Gemini 3.5 Transcribe / Flash APIs, and Tauri v2 Android + native Kotlin `InputMethodService` (IME) integration patterns.

3. **YOUR FIRST DELIVERABLE — INITIAL CROSS-CHECK & ADAPTATION REPORT (NO CODE YET):**
   * Inspect our existing codebase first.
   * Compare our current code and `planning V2.md` with this document.
   * Reply with a comprehensive **Initial Technical Report** detailing:
     1. How this plan maps to our actual codebase (what already exists vs. what needs refactoring).
     2. Any architectural gaps, edge cases, or conflicts you found in this plan, along with your superior engineering alternatives based on latest 2026 best practices.
     3. Clarifying questions for me before you begin Milestone 1.

---

## 1. Unified Core Context (Connecting Dictation Engine with Sync & Keyboard)

To ensure the Cloud Sync and Android Keyboard modules fit seamlessly into our broader app architecture, here is how the core system operates:

* **Standalone Application:** We are no longer contributing back to upstream `bulbul`. This is our own standalone cross-platform product (Windows Desktop + Android Mobile).
* **Two-Phase Configurable AI Pipeline:**
  * **Phase 1 (Transcription):**
    * First-class **Bengali (India) + English** bilingual support (`languageCodes: ["bn-IN", "en-IN"]`).
    * When Bengali is active, **Groq STT is disabled/unavailable**; only **`gemini-3.5-transcribe`** is used.
    * User controls Phase 1 Mode (`SMART` vs `VERBATIM`) and toggles **Custom Vocabulary** (selecting from **Multiple Named Vocabulary Profiles** stored by the user).
  * **Phase 2 (Optional Post-Processing):**
    * User can toggle Post-Processing **ON or OFF** globally or per mode.
    * Supports multi-brand model selection: **Google Gemini** (Low/Mid/High tiers, e.g., `gemini-3.5-flash-lite`, `gemini-3.6-flash`, `gemini-3.8-flash` with configurable `thinking_level`) and **Groq** text models.
    * Driven by customizable **Context Modes / Prompt Presets** (*Direct Smart Dictation*, *Coding Agent Prompt*, *Professional Email*, *Natural Chat*, *Translation*, plus user-created modes).
* **Local-First Audio & Non-Destructive Multi-Try History:**
  * Audio is compressed (`.mp3` or `.ogg`/`.opus` mono) and saved to local storage *before* API calls.
  * Every recording can have **multiple Phase 1 Transcription Tries** and **multiple Phase 2 Post-Processing Tries** (using different models, modes, or prompts), and **every single try is saved** in local SQLite (`recordings` -> `transcription_tries` -> `post_process_tries`).

---

## 2. Part A: Serverless BYOK & BYOS Cloud Sync Architecture (WebDAV Framework)

### 2.1. Core Philosophy: 100% Optional, Client-Side & Zero Developer Hosting Cost
* **Default Local-Only Mode:** Casual users can use the app 100% locally without ever creating an account or connecting cloud storage.
* **Opt-In Cloud Sync (For Technical / Multi-Device Users):** Users who want to sync their data across **Windows and Android** (or back up their history and keys) can connect their own personal cloud storage.
* **No Developer Backend:** We do **NOT** provide any hosted database, object storage, or auth server. Everything runs pure client-side using **BYOK (Bring Your Own AI Keys)** and **BYOS (Bring Your Own Storage)**.
* **Why WebDAV over S3/R2, Supabase, or Google Drive OAuth:**
  * Eliminates Google Cloud / Dropbox OAuth developer console headaches and token expiration issues.
  * Eliminates Supabase's 7-day free-tier project pausing.
  * Eliminates AWS S3 / Cloudflare R2 mandatory credit card barriers.

### 2.2. Decentralized Authentication Gateway (3-Field WebDAV Login)
The user's personal WebDAV credentials act as their cross-device account login:
* **Login UI Inputs (3 Fields):**
  1. `WebDAV Endpoint URL` (Include quick provider presets/guides in UI: **Koofr** [10GB free, no credit card], **Nextcloud**, **Infomaniak kDrive**, **Yandex Disk**, or **Custom WebDAV URL**. *Note: Warn users in UI that pCloud restricts WebDAV to paid accounts*).
  2. `Username / Email`
  3. `App Password / Token`
* **Validation Flow:**
  * The app sends a direct client-side HTTP `PROPFIND` (or `GET`) request to the WebDAV URL.
  * A `200 OK` or `207 Multi-Status` response authenticates the user and ensures the root folder `/DictationApp/` exists (creating it via `MKCOL` if missing).

### 2.3. Remote Cloud Directory & State-Based JSON Sync Structure
To prevent SQLite database corruption from concurrent writes across Windows and Android, **we never sync raw `.db` binary files.** Instead, local SQLite remains the fast query engine on each device, while WebDAV stores structured JSON state files and immutable audio files inside `/DictationApp/`:

#### 1. `/DictationApp/credentials.json` (Keys, Vocabularies & Mode Presets)
Stores the user's active AI API keys alongside their custom configurations so a new device can configure itself in one click:
```json
{
  "updated_at": 1792834700,
  "api_keys": {
    "gemini_api_key": "AIzaSyxxxx...",
    "groq_api_key": "gsk_xxxx..."
  },
  "preferences": {
    "default_language_codes": ["bn-IN", "en-IN"],
    "default_transcription_mode": "SMART",
    "post_processing_enabled": true
  },
  "vocabulary_profiles": [
    {
      "id": "vocab_dev_01",
      "name": "Coding & Stack",
      "terms": ["Next.js", "Drizzle ORM", "Better Auth", "Anti-Gravity", "Coolify"]
    }
  ],
  "custom_modes": [
    {
      "id": "mode_coding_01",
      "name": "Coding Agent Prompt",
      "phase1_mode": "SMART",
      "vocab_ids": ["vocab_dev_01"],
      "phase2_enabled": true,
      "phase2_provider": "gemini",
      "phase2_model": "gemini-3.8-flash",
      "thinking_level": "LOW",
      "prompt": "..."
    }
  ]
}

```

*(Question for Anti-Gravity: Should we keep `api_keys` in plain-text inside `credentials.json` as drafted for direct readability, or offer an optional/transparent AES-GCM encryption layer using a key derived from the user's WebDAV App Password so zero extra passwords are needed during 1-click recovery? Give us your opinion.)*

#### 2. `/DictationApp/dictations.json` (Multi-Try History Timeline)

Stores the timeline of recordings, including all **Phase 1 Transcription Tries** and **Phase 2 Post-Processing Tries** per recording:

```json
{
  "schema_version": 2,
  "updated_at": 1792834900,
  "records": [
    {
      "id": "0192b8a0-...",
      "audio_file": "audio_1792834700.mp3",
      "duration_ms": 14500,
      "audio_size_bytes": 58000,
      "is_pinned": false,
      "created_at": 1792834700,
      "updated_at": 1792834820,
      "deleted_at": null,
      "transcription_tries": [
        {
          "try_id": "tr_01",
          "provider": "gemini",
          "model": "gemini-3.5-transcribe",
          "mode": "SMART",
          "language_codes": ["bn-IN", "en-IN"],
          "vocab_ids": ["vocab_dev_01"],
          "transcript_text": "...",
          "latency_ms": 840,
          "created_at": 1792834705
        }
      ],
      "post_process_tries": [
        {
          "try_id": "pp_01",
          "source_try_id": "tr_01",
          "mode_name": "Coding Agent Prompt",
          "provider": "gemini",
          "model": "gemini-3.8-flash",
          "thinking_level": "LOW",
          "prompt_snapshot": "...",
          "output_text": "...",
          "latency_ms": 620,
          "created_at": 1792834708
        }
      ]
    }
  ]
}

```

*(Architectural Check for Anti-Gravity: As the user accumulates thousands of recordings and multiple tries across Windows and Android, a single `dictations.json` file might grow large or face concurrent write race conditions. Please evaluate whether we should use record-level/monthly sharded JSON files or `ETag` + UUID-based in-memory array merging before `PUT` uploads so two devices never overwrite each other's new tries.)*

#### 3. `/DictationApp/audio_{timestamp_or_uuid}.mp3` (or `.ogg`)

* Compressed audio recordings are uploaded with unique timestamp/UUID filenames to guarantee zero overwrite conflicts.
* **On-Demand Audio Fetching:** When logging into a second device, text history (`dictations.json`) syncs immediately, while audio files are downloaded on-demand when the user clicks "Play" or "Re-Transcribe" (with an option to "Download All Audio Offline").
* **Selective Sync & Storage Quota Meter:**
* Use WebDAV `PROPFIND` (`<d:quota-used-bytes>`, `<d:quota-available-bytes>`, and file `<d:getcontentlength>`) plus local SQLite byte sums to display a **Storage Consumption Meter** in Settings:
* *Local Storage Used* (Audio MB vs. Text DB KB)
* *Cloud Storage Used* (Cloud Audio MB vs. Cloud JSON MB vs. Available Cloud Quota)


* Allow users to toggle *"Sync Audio Files to Cloud"* ON/OFF (so users with small cloud drives can sync only text/tries + settings) and provide a *"Delete Unpinned Cloud Audio"* button to free up remote space without losing text history.



### 2.4. One-Click New Device Recovery Workflow

1. User installs the app on a brand-new Windows PC or Android phone and enters their 3 WebDAV fields.
2. App connects to `/DictationApp/`, pulls `credentials.json`, and **automatically populates Gemini & Groq API keys, custom vocabularies, and prompt modes**.
3. App pulls `dictations.json` and programmatically rebuilds the local SQLite tables (`recordings`, `transcription_tries`, `post_process_tries`).
4. Audio files appear in History and stream/download on demand when played.

---

## 3. Part B: Platform UI Split & Android-Only "AI Voice & Command Keyboard"

### 3.1. Strict Platform UI Separation

* **Windows (Desktop):**
* **NO On-Screen Keyboard.** Desktop users rely on physical keyboards.
* Windows features the Main Desktop Window (Settings, Multi-Try History, Vocabulary/Prompt Manager, WebDAV Sync) + Global Keyboard Shortcuts + Floating Recording Bar/Overlay.


* **Android (Mobile):**
* **iOS is out of scope.** Focus mobile engineering strictly on **Android**.
* Android includes **two integrated surfaces** sharing the same local database and WebDAV sync engine:
1. **Main Android App:** Full dashboard for managing API keys, WebDAV sync, Storage Meter, Vocabulary Sets, Custom Modes, and browsing/playing full Multi-Try History.
2. **System AI Voice & Command Keyboard (`InputMethodService` / IME):** A next-generation custom Android keyboard that replaces/augments Whisperian's keyboard with far deeper control.





### 3.2. Android Keyboard Feature Specifications (Surpassing Whisperian)

When the user switches to our keyboard in any Android app (WhatsApp, Gmail, Chrome, Notes, AI IDEs, etc.), the keyboard must provide:

1. **Live On-Keyboard Control Ribbon (Full Settings Without Leaving the Target App):**
* **Mode Selector Bar:** One-tap horizontal pills to switch the active Mode (*Direct Smart Dictation*, *Coding Agent*, *Email*, *Chat*, *Translate*, or any custom user mode).
* **Phase 1 Quick Controls:** Instant chips on the keyboard to flip between `SMART` and `VERBATIM` transcription, toggle Vocabulary ON/OFF, or change which **Vocabulary Profile** is active.
* **Phase 2 Quick Controls:** Instant toggle to turn **Post-Processing ON/OFF**, plus a quick selector to switch the Phase 2 Provider/Model (`Gemini Low` / `Mid` / `High` or `Groq`) right from the keyboard.


2. **In-Place Multi-Try & Raw/Processed Switcher (Post-Dictation Magic Bar):**
* Immediately after dictating and inserting text into an Android text field, the keyboard displays a contextual action strip:
* **`[⇄ Switch: Raw Transcript <-> Post-Processed]`**: Toggles the text already pasted inside the target app between Phase 1 (`gemini-3.5-transcribe` output) and Phase 2 (LLM output) in real time!
* **`[↻ Re-Try with Another Mode / Model]`**: Lets the user pick a different mode (e.g., *Email* instead of *Chat*) or a different model (`Gemini 3.8 High` instead of `Groq`), replaces the previously inserted text inside the target app automatically via `InputConnection`, and saves the new output as `Try #2`, `Try #3` in History!




3. **AI Actions on Selected Text (Zero-Voice Text Transformation):**
* If the user highlights/selects existing text in any Android app (`InputConnection.getSelectedText()`), the keyboard surfaces **"Transform Selection"** actions (*Translate*, *Fix Grammar*, *Polish as Email*, *Convert to Coding Prompt*, *Summarize*).
* Tapping an action runs Phase 2 Post-Processing on the selected text and replaces it in place—no microphone input needed.


4. **Voice Command on Selected Text:**
* If text is selected in the target app and the user holds the Mic button to speak an instruction in Bengali/English (e.g., *"এটার একটা ভদ্রভাবে রিপ্লাই লিখে দাও"* or *"এই কোডটাতে টাইপস্ক্রিপ্ট টাইপ বসানোর প্রম্পট বানাও"*), the keyboard combines `Selected Text + Spoken Instruction` and outputs the result directly into the app.


5. **Comprehensive Editing, Clipboard & Navigation Pad:**
* Dedicated macro buttons: `Copy`, `Paste`, `Cut`, `Select All`, `Undo`, `Redo`, plus a quick **Clipboard History** strip.
* Smart deletion & cursor control: `Backspace`, `Delete Word`, `Delete Last Dictation`, swipe-on-bar to move cursor left/right, `Enter / Send`, and `New Line`.
* **Bilingual Punctuation & Utility Row:** Quick keys for Bengali dari (`।`), `,`, `?`, `!`, code backticks (```), user text snippets, and a prominent **Switch Keyboard (`🌐`)** button to jump back to Gboard/standard typing keyboard in one tap.


6. **Built-In Mini History & Audio Drawer Inside Keyboard:**
* Expandable drawer inside the keyboard UI allowing the user to view recent dictations (including dictations recorded on their Windows PC and synced via WebDAV), play the saved audio, switch between saved Tries, and tap to insert any previous try into the current app.



---

## 4. Questions & Tasks for Anti-Gravity's Initial Cross-Check Report

**Before writing or modifying any code**, please analyze our current repository, search the web for the latest 2026 technical references, and send me your **Initial Technical Audit & Adaptation Report** covering:

1. **Codebase Alignment & Architecture Audit:**
* How does our existing `bulbul` codebase currently handle audio recording, SQLite storage, state management, and API calls? What parts can we reuse, and what parts must be cleanly re-architected?


2. **Android App + System IME Keyboard Architecture (Crucial Technical Decision):**
* Since Tauri v2 renders UI inside a WebView—which is great for the Windows Desktop App and the Android Main Dashboard App, but can be heavy/slow for an Android System Keyboard (`InputMethodService` / IME)—what is the best 2026 architecture for our Android build?
* Should we build a **Shared Rust Core Library** (handling SQLite, WebDAV sync, audio encoding, and Gemini/Groq HTTP calls) exposed via **JNI / UniFFI** to a native **Kotlin + Jetpack Compose `InputMethodService**` for the keyboard, while using Tauri v2 for the Desktop & Main Android Dashboard? Or do you recommend a different structure?


3. **WebDAV Sync & Conflict Resolution Strategy:**
* Which Rust crates (`reqwest_dav`, `opendal`, or direct `reqwest` WebDAV `PROPFIND`/`PUT`/`GET`/`MKCOL` requests) are cleanest and most reliable across Windows and Android in 2026?
* How should we structure `dictations.json` (single file with ETag merge vs. sharded/delta JSON files) so that multi-try updates across Windows and Android never overwrite each other and stay fast over thousands of recordings?


4. **Audio Compression Across Windows & Android:**
* Which compressed audio format (`.mp3` vs `.ogg`/`.opus`) and Rust encoder crate will compile smoothly for both Windows (`x86_64`) and Android (`aarch64` NDK) without native C-toolchain headaches?


5. **Gap Analysis & Phased Roadmap:**
* Point out any missing pieces, security/UX improvements, or better ideas you have, and propose a step-by-step **Milestone Plan** so we can approve it and start building.