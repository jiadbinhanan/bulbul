// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Bulbul contributors

package com.bulbul.app

import android.util.Base64
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.net.HttpURLConnection
import java.net.URL

object GeminiClient {

    private const val TAG = "BulbulGemini"
    private const val ENDPOINT_BASE = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.5-transcribe:generateContent"

    /// Curated development & coding vocabulary for Banglish code-switching dictation.
    /// Biases Gemini 3.5 Transcribe to keep programming terms in proper English script
    /// rather than phonetic Bengali.
    private val BANGLISH_CODING_VOCABULARY = listOf(
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
    )

    /// Transcribes audio using Google's `gemini-3.5-transcribe` model in single-pass SMART mode.
    /// Returns formatted, punctuated Bengali or Banglish text with disfluencies removed.
    fun transcribe(
        apiKey: String,
        wav: ByteArray,
        vocabulary: List<String> = emptyList(),
        isCodingMode: Boolean = false
    ): String? {
        val key = apiKey.trim()
        if (key.isEmpty()) {
            Log.w(TAG, "no Gemini API key set; not transcribing")
            return null
        }
        if (wav.isEmpty()) {
            return ""
        }

        return try {
            val customVocab = LinkedHashSet<String>()
            if (isCodingMode) {
                customVocab.addAll(BANGLISH_CODING_VOCABULARY)
            }
            for (word in vocabulary) {
                val trimmed = word.trim()
                if (trimmed.isNotEmpty()) {
                    customVocab.add(trimmed)
                }
            }

            val vocabArray = JSONArray()
            var count = 0
            for (word in customVocab) {
                if (count >= 1000) break
                vocabArray.put(word)
                count++
            }

            val b64Audio = Base64.encodeToString(wav, Base64.NO_WRAP)

            val inlineData = JSONObject().apply {
                put("mime_type", "audio/wav")
                put("data", b64Audio)
            }
            val part = JSONObject().apply {
                put("inline_data", inlineData)
            }
            val content = JSONObject().apply {
                put("role", "user")
                put("parts", JSONArray().put(part))
            }

            val audioTranscriptionConfig = JSONObject().apply {
                put("languageCodes", JSONArray().put("bn-IN"))
                put("mode", "SMART")
                if (vocabArray.length() > 0) {
                    put("customVocabulary", vocabArray)
                }
            }
            val generationConfig = JSONObject().apply {
                put("audioTranscriptionConfig", audioTranscriptionConfig)
            }

            val requestJson = JSONObject().apply {
                put("contents", JSONArray().put(content))
                put("generationConfig", generationConfig)
            }

            val url = URL("$ENDPOINT_BASE?key=$key")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "POST"
                doOutput = true
                connectTimeout = 15_000
                readTimeout = 45_000
                setRequestProperty("Content-Type", "application/json; charset=utf-8")
            }

            OutputStreamWriter(conn.outputStream, Charsets.UTF_8).use { writer ->
                writer.write(requestJson.toString())
                writer.flush()
            }

            val code = conn.responseCode
            val responseBody = if (code in 200..299) {
                conn.inputStream.bufferedReader(Charsets.UTF_8).use(BufferedReader::readText)
            } else {
                val err = conn.errorStream?.let { InputStreamReader(it, Charsets.UTF_8).buffered().readText() } ?: ""
                Log.w(TAG, "Gemini returned $code: $err")
                return null
            }

            val root = JSONObject(responseBody)
            if (root.has("error")) {
                val errMsg = root.getJSONObject("error").optString("message", "Unknown error")
                Log.w(TAG, "Gemini API error: $errMsg")
                return null
            }

            val candidates = root.optJSONArray("candidates") ?: return ""
            if (candidates.length() == 0) return ""

            val parts = candidates.getJSONObject(0)
                .optJSONObject("content")
                ?.optJSONArray("parts") ?: return ""

            val sb = java.lang.StringBuilder()
            for (i in 0 until parts.length()) {
                val text = parts.getJSONObject(i).optString("text", "")
                sb.append(text)
            }

            sb.toString().trim()
        } catch (t: Throwable) {
            Log.w(TAG, "Gemini transcribe failed", t)
            null
        }
    }
}
