// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Romanch Roshan Singh

package com.bulbul.app

import android.util.Log
import org.json.JSONArray
import org.json.JSONObject

data class ProcessAudioOptions(
    val language: String = "auto",
    val contextModeId: Long = 1L,
    val phase1Mode: String? = null,
    val vocabEnabled: Boolean = true,
    val vocabProfileId: Long? = null,
    val phase2Enabled: Boolean? = null,
    val phase2Provider: String? = null,
    val phase2Model: String? = null,
    val thinkingLevel: String? = null,
    val selectedText: String? = null
) {
    fun toJson(): String {
        val obj = JSONObject()
        obj.put("language", language)
        obj.put("context_mode_id", contextModeId)
        phase1Mode?.let { obj.put("phase1_mode", it) }
        obj.put("vocab_enabled", vocabEnabled)
        vocabProfileId?.let { obj.put("vocab_profile_id", it) }
        phase2Enabled?.let { obj.put("phase2_enabled", it) }
        phase2Provider?.let { obj.put("phase2_provider", it) }
        phase2Model?.let { obj.put("phase2_model", it) }
        thinkingLevel?.let { obj.put("thinking_level", it) }
        selectedText?.let { obj.put("selected_text", it) }
        return obj.toString()
    }
}

data class ProcessAudioResult(
    val recordingId: String,
    val phase1RawText: String,
    val phase2ProcessedText: String,
    val activeText: String,
    val transcriptionTryId: Long,
    val postProcessTryId: Long?
) {
    companion object {
        fun fromJson(jsonStr: String): ProcessAudioResult {
            val obj = try {
                JSONObject(jsonStr)
            } catch (_: Throwable) {
                JSONObject()
            }
            return ProcessAudioResult(
                recordingId = obj.optString("recording_id", ""),
                phase1RawText = obj.optString("phase1_raw_text", ""),
                phase2ProcessedText = obj.optString("phase2_processed_text", ""),
                activeText = obj.optString("active_text", ""),
                transcriptionTryId = obj.optLong("transcription_try_id", 0L),
                postProcessTryId = if (obj.has("post_process_try_id") && !obj.isNull("post_process_try_id")) {
                    obj.optLong("post_process_try_id")
                } else null
            )
        }
    }
}

data class HistoryItem(
    val id: String,
    val createdAt: String,
    val durationMs: Long,
    val activeText: String,
    val rawText: String,
    val mode: String,
    val isPinned: Boolean
)

data class VocabProfileItem(
    val id: Long,
    val name: String,
    val description: String,
    val isActive: Boolean,
    val termCount: Int
)

data class ContextModeItem(
    val id: Long,
    val name: String,
    val description: String,
    val icon: String,
    val defaultPhase1Mode: String,
    val phase2Enabled: Boolean,
    val phase2Provider: String,
    val phase2Model: String,
    val thinkingLevel: String?
)

/**
 * JNI Bridge connecting Android services (InputMethodService, ForegroundService)
 * directly to the unified Bulbul Rust Core (SQLite, Ogg Opus encoder, Gemini/Groq STT, WebDAV sync).
 */
object BulbulBridge {
    private const val TAG = "BulbulBridge"

    init {
        try {
            System.loadLibrary("bulbul_lib")
            Log.i(TAG, "Native library bulbul_lib loaded successfully")
        } catch (t: Throwable) {
            Log.e(TAG, "Failed to load native library bulbul_lib", t)
        }
    }

    /**
     * Initializes Rust Core data directory to match Android Context.filesDir.
     */
    @JvmStatic
    external fun initDataDir(dirPath: String)

    fun ensureInitialized(context: android.content.Context) {
        try {
            initDataDir(context.filesDir.absolutePath)
        } catch (t: Throwable) {
            Log.e(TAG, "Failed to initDataDir", t)
        }
    }

    /**
     * Legacy audio processor: returns final active text string.
     */
    @JvmStatic
    external fun processVoiceAudio(
        pcmBytes: ByteArray,
        sampleRate: Int,
        language: String,
        contextModeId: Long
    ): String

    /**
     * Modern audio processor with structured runtime overrides.
     * Returns JSON of { recording_id, phase1_raw_text, phase2_processed_text, active_text, ... }
     */
    @JvmStatic
    external fun processVoiceAudioWithOptions(
        pcmBytes: ByteArray,
        sampleRate: Int,
        optionsJson: String
    ): String

    /**
     * Transforms highlighted text in-place using Gemini/Groq (e.g. Polish, Translate, Code, Email).
     */
    @JvmStatic
    external fun transformSelectedText(
        selectedText: String,
        actionType: String,
        customPrompt: String?
    ): String

    /**
     * Re-runs Phase 2 post-processing on an existing transcription try with a new mode/model.
     * Saves new try to SQLite and returns the transformed text.
     */
    @JvmStatic
    external fun rerunPhase2(
        recordingId: String,
        transcriptionTryId: Long,
        modeId: Long,
        provider: String?,
        model: String?,
        thinkingLevel: String?
    ): String

    /**
     * Returns recent recordings formatted as JSON for the Mini History Drawer.
     */
    @JvmStatic
    external fun getRecentRecordings(limit: Int): String

    /**
     * Returns vocabulary profiles loaded directly from SQLite as JSON.
     */
    @JvmStatic
    external fun getVocabularyProfiles(): String

    /**
     * Toggles a vocabulary profile active/inactive in SQLite.
     */
    @JvmStatic
    external fun toggleVocabularyProfile(id: Long, active: Boolean): Boolean

    /**
     * Returns context modes from SQLite as JSON.
     */
    @JvmStatic
    external fun getContextModes(): String

    /**
     * Triggers bidirectional WebDAV cloud sync manually via the Rust core.
     * Returns status message.
     */
    @JvmStatic
    external fun syncWebdav(): String

    // Helper functions for easy Kotlin consumption
    fun fetchRecentRecordings(limit: Int = 20): List<HistoryItem> {
        return try {
            val jsonStr = getRecentRecordings(limit)
            val arr = JSONArray(jsonStr)
            val list = mutableListOf<HistoryItem>()
            for (i in 0 until arr.length()) {
                val o = arr.getJSONObject(i)
                list.add(
                    HistoryItem(
                        id = o.optString("id"),
                        createdAt = o.optString("created_at"),
                        durationMs = o.optLong("duration_ms"),
                        activeText = o.optString("active_text"),
                        rawText = o.optString("raw_text"),
                        mode = o.optString("mode"),
                        isPinned = o.optBoolean("is_pinned", false)
                    )
                )
            }
            list
        } catch (t: Throwable) {
            Log.e(TAG, "fetchRecentRecordings failed", t)
            emptyList()
        }
    }

    fun fetchVocabularyProfiles(): List<VocabProfileItem> {
        return try {
            val jsonStr = getVocabularyProfiles()
            val arr = JSONArray(jsonStr)
            val list = mutableListOf<VocabProfileItem>()
            for (i in 0 until arr.length()) {
                val o = arr.getJSONObject(i)
                list.add(
                    VocabProfileItem(
                        id = o.optLong("id"),
                        name = o.optString("name"),
                        description = o.optString("description"),
                        isActive = o.optBoolean("is_active", false),
                        termCount = o.optInt("term_count", 0)
                    )
                )
            }
            list
        } catch (t: Throwable) {
            Log.e(TAG, "fetchVocabularyProfiles failed", t)
            emptyList()
        }
    }

    fun fetchContextModes(): List<ContextModeItem> {
        return try {
            val jsonStr = getContextModes()
            val arr = JSONArray(jsonStr)
            val list = mutableListOf<ContextModeItem>()
            for (i in 0 until arr.length()) {
                val o = arr.getJSONObject(i)
                list.add(
                    ContextModeItem(
                        id = o.optLong("id"),
                        name = o.optString("name"),
                        description = o.optString("description"),
                        icon = o.optString("icon"),
                        defaultPhase1Mode = o.optString("default_phase1_mode", "smart"),
                        phase2Enabled = o.optBoolean("phase2_enabled", false),
                        phase2Provider = o.optString("phase2_provider", "gemini"),
                        phase2Model = o.optString("phase2_model", "gemini-2.5-flash"),
                        thinkingLevel = if (o.has("thinking_level") && !o.isNull("thinking_level")) o.optString("thinking_level") else null
                    )
                )
            }
            list
        } catch (t: Throwable) {
            Log.e(TAG, "fetchContextModes failed", t)
            emptyList()
        }
    }
}
