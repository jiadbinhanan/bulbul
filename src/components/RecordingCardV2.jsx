// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Romanch Roshan Singh & Bulbul contributors

import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "../views/HomeHistoryV2.css";

export function formatSecs(s) {
  if (!s || isNaN(s)) return "0:00";
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${sec < 10 ? "0" : ""}${sec}`;
}

export function TrashIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M3 6h18" />
      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
      <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
      <line x1="10" y1="11" x2="10" y2="17" />
      <line x1="14" y1="11" x2="14" y2="17" />
    </svg>
  );
}

export default function RecordingCardV2({
  rec,
  onTogglePin,
  onRequestDelete,
  onOpenRerunTrans,
  onOpenRerunPost,
}) {
  const { recording, transcription_tries, post_process_tries } = rec;
  const [activePhase, setActivePhase] = useState(
    post_process_tries && post_process_tries.length > 0 ? "phase2" : "phase1"
  );
  const [transTryIdx, setTransTryIdx] = useState(
    transcription_tries && transcription_tries.length > 0 ? transcription_tries.length - 1 : 0
  );
  const [postTryIdx, setPostTryIdx] = useState(
    post_process_tries && post_process_tries.length > 0 ? post_process_tries.length - 1 : 0
  );
  const [copied, setCopied] = useState(false);

  // Audio player state
  const [audioUrl, setAudioUrl] = useState(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [audioDuration, setAudioDuration] = useState(recording.duration_ms / 1000);
  const audioRef = useRef(null);

  async function togglePlayAudio() {
    if (!audioUrl) {
      try {
        const dataUri = await invoke("get_recording_audio_bytes", {
          recordingId: recording.id,
        });
        setAudioUrl(dataUri);
        setTimeout(() => {
          if (audioRef.current) {
            audioRef.current.play();
            setIsPlaying(true);
          }
        }, 50);
      } catch (err) {
        console.error("Failed to load audio bytes:", err);
      }
      return;
    }

    if (audioRef.current) {
      if (isPlaying) {
        audioRef.current.pause();
        setIsPlaying(false);
      } else {
        audioRef.current.play();
        setIsPlaying(true);
      }
    }
  }

  function handleAudioTimeUpdate() {
    if (audioRef.current) {
      setCurrentTime(audioRef.current.currentTime);
      if (audioRef.current.duration) setAudioDuration(audioRef.current.duration);
    }
  }

  function handleAudioEnded() {
    setIsPlaying(false);
    setCurrentTime(0);
  }

  const currentTransTry = transcription_tries[transTryIdx] || transcription_tries[0];
  const currentPostTry = post_process_tries[postTryIdx] || post_process_tries[0];

  const displayText =
    activePhase === "phase2" && currentPostTry
      ? currentPostTry.output_text
      : currentTransTry?.transcript_text || "";

  async function copyToClipboard() {
    try {
      await navigator.clipboard.writeText(displayText);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch (e) {
      console.error("copy failed", e);
    }
  }

  const dateObj = new Date(recording.created_at * 1000);
  const dateStr = dateObj.toLocaleDateString([], {
    month: "short",
    day: "numeric",
  });
  const timeStr = dateObj.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });

  return (
    <div className={`history-card-v2 ${recording.is_pinned ? "pinned-card" : ""}`}>
      <div className="history-card-header">
        <div className="history-meta-left">
          <span className="history-time">{dateStr} · {timeStr}</span>
          <span className="history-audio-duration">
            {(recording.duration_ms / 1000).toFixed(1)}s Audio
          </span>
          <span className="history-audio-duration">
            {(recording.file_size_bytes / 1024).toFixed(0)} KB
          </span>
        </div>

        <div className="history-actions-right">
          <button
            type="button"
            className={`btn-icon-v2 ${recording.is_pinned ? "pinned" : ""}`}
            onClick={onTogglePin}
            title={recording.is_pinned ? "Unpin recording" : "Pin recording"}
          >
            {recording.is_pinned ? "★" : "☆"}
          </button>
          <button
            type="button"
            className="btn-icon-v2 danger"
            onClick={onRequestDelete}
            title="Delete recording"
          >
            <TrashIcon />
          </button>
        </div>
      </div>

      {/* Built-in Audio Player */}
      <div className="inline-audio-player">
        <button
          type="button"
          className="audio-play-btn"
          onClick={togglePlayAudio}
          title={isPlaying ? "Pause" : "Play recording"}
        >
          {isPlaying ? "⏸" : "▶"}
        </button>

        <input
          type="range"
          min={0}
          max={audioDuration || 1}
          step={0.1}
          value={currentTime}
          onChange={(e) => {
            const t = parseFloat(e.target.value);
            setCurrentTime(t);
            if (audioRef.current) audioRef.current.currentTime = t;
          }}
          className="audio-track-slider"
        />

        <span className="audio-time-label">
          {formatSecs(currentTime)} / {formatSecs(audioDuration)}
        </span>

        {audioUrl && (
          <audio
            ref={audioRef}
            src={audioUrl}
            onTimeUpdate={handleAudioTimeUpdate}
            onEnded={handleAudioEnded}
          />
        )}
      </div>

      {/* Phase selection and Try selector */}
      <div className="phase-selection-bar">
        <div className="phase-tabs">
          <button
            type="button"
            className={`phase-tab-btn ${activePhase === "phase1" ? "active-phase" : ""}`}
            onClick={() => setActivePhase("phase1")}
          >
            Phase 1: Transcribe ({transcription_tries.length})
          </button>
          {post_process_tries.length > 0 && (
            <button
              type="button"
              className={`phase-tab-btn ${activePhase === "phase2" ? "active-phase" : ""}`}
              onClick={() => setActivePhase("phase2")}
            >
              Phase 2: Formatted ({post_process_tries.length})
            </button>
          )}
        </div>

        {/* Multi-Try Version Pill Switcher */}
        {activePhase === "phase1" && transcription_tries.length > 1 && (
          <div className="try-switcher">
            <span className="muted x-small">Tries:</span>
            {transcription_tries.map((t, idx) => (
              <button
                key={t.id}
                type="button"
                className={`try-pill ${transTryIdx === idx ? "active-try" : ""}`}
                onClick={() => setTransTryIdx(idx)}
              >
                #{idx + 1}
              </button>
            ))}
          </div>
        )}

        {activePhase === "phase2" && post_process_tries.length > 1 && (
          <div className="try-switcher">
            <span className="muted x-small">Tries:</span>
            {post_process_tries.map((p, idx) => (
              <button
                key={p.id}
                type="button"
                className={`try-pill ${postTryIdx === idx ? "active-try" : ""}`}
                onClick={() => setPostTryIdx(idx)}
              >
                #{idx + 1}
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Main Content Box */}
      <div className="history-content-box">
        <div className="content-top-badges">
          <div className="badge-info-cluster">
            {activePhase === "phase1" && currentTransTry && (
              <>
                <span className="badge badge-provider">{currentTransTry.provider}</span>
                <span className="badge badge-model">{currentTransTry.model}</span>
                <span className="badge badge-phase1">{currentTransTry.mode.toUpperCase()}</span>
                <span className="badge muted-badge">{currentTransTry.latency_ms}ms</span>
              </>
            )}
            {activePhase === "phase2" && currentPostTry && (
              <>
                <span className="badge badge-active-glow">{currentPostTry.mode_name}</span>
                <span className="badge badge-provider">{currentPostTry.provider}</span>
                <span className="badge badge-model">{currentPostTry.model}</span>
                {currentPostTry.thinking_level && (
                  <span className="badge badge-thinking">
                    Thinking: {currentPostTry.thinking_level}
                  </span>
                )}
                <span className="badge muted-badge">{currentPostTry.latency_ms}ms</span>
              </>
            )}
          </div>

          <button
            type="button"
            className={`btn-action ${copied ? "copied" : ""}`}
            onClick={copyToClipboard}
            style={{ padding: "3px 8px", fontSize: "11px" }}
          >
            {copied ? "✓ Copied" : "Copy"}
          </button>
        </div>

        <div className="text-body-v2">{displayText || "(Empty output)"}</div>

        {activePhase === "phase2" && currentPostTry?.prompt_snapshot && (
          <div className="prompt-snapshot-tag">
            <strong>Prompt Snapshot:</strong> {currentPostTry.prompt_snapshot}
          </div>
        )}
      </div>

      {/* Footer Actions: Re-Run Controls */}
      <div className="history-card-footer">
        <div className="rerun-actions">
          <button
            type="button"
            className="btn-rerun"
            onClick={onOpenRerunTrans}
            title="Re-run Phase 1 speech-to-text on this audio file"
          >
            🔄 Re-Transcribe Audio
          </button>
          {currentTransTry && (
            <button
              type="button"
              className="btn-rerun"
              onClick={() => onOpenRerunPost(currentTransTry)}
              title="Run Phase 2 LLM formatting on this transcript"
            >
              ✨ Re-Run LLM Post-Process
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

export function RerunTranscribeModal({ rec, onClose, onSuccess }) {
  const [provider, setProvider] = useState("gemini");
  const [model, setModel] = useState("gemini-3.5-transcribe");
  const [mode, setMode] = useState("smart");
  const [loading, setLoading] = useState(false);

  async function handleRerun(e) {
    e.preventDefault();
    try {
      setLoading(true);
      const res = await invoke("rerun_transcription", {
        recordingId: rec.recording.id,
        provider,
        model,
        mode,
        customVocabulary: [],
      });
      onSuccess(res);
    } catch (err) {
      console.error("Rerun transcription error:", err);
      alert(`Transcription failed: ${err}`);
      setLoading(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()} style={{ maxWidth: "460px" }}>
        <div className="modal-header">
          <h2>Re-Transcribe Audio</h2>
          <button type="button" className="modal-close-btn" onClick={onClose}>✕</button>
        </div>
        <form onSubmit={handleRerun}>
          <div className="form-group">
            <label>Transcription Provider</label>
            <select
              value={provider}
              onChange={(e) => {
                const p = e.target.value;
                setProvider(p);
                setModel(p === "gemini" ? "gemini-3.5-transcribe" : "whisper-large-v3");
              }}
            >
              <option value="gemini">Google Gemini 3.5 Transcribe (Recommended for Bengali)</option>
              <option value="groq">Groq Whisper (English)</option>
            </select>
          </div>

          <div className="form-group">
            <label>Mode</label>
            <select value={mode} onChange={(e) => setMode(e.target.value)}>
              <option value="smart">SMART Mode (Punctuation, Disfluency removal)</option>
              <option value="verbatim">VERBATIM Mode (Raw word-for-word)</option>
            </select>
          </div>

          <div className="modal-actions">
            <button type="button" className="btn-action" onClick={onClose}>Cancel</button>
            <button type="submit" className="btn-primary" disabled={loading}>
              {loading ? "Transcribing…" : "Run Phase 1 Again"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

export function RerunPostProcessModal({ rec, transTry, modes, onClose, onSuccess }) {
  const [selectedModeId, setSelectedModeId] = useState(modes[0]?.id || null);
  const [provider, setProvider] = useState("gemini");
  const [model, setModel] = useState("gemini-3.8-flash");
  const [thinkingLevel, setThinkingLevel] = useState("low");
  const [prompt, setPrompt] = useState(modes[0]?.system_prompt || "");
  const [loading, setLoading] = useState(false);

  function handleModeChange(id) {
    setSelectedModeId(id);
    const m = modes.find((item) => item.id === parseInt(id, 10));
    if (m) {
      setProvider(m.phase2_provider);
      setModel(m.phase2_model);
      setThinkingLevel(m.thinking_level || "low");
      setPrompt(m.system_prompt);
    }
  }

  async function handleRerun(e) {
    e.preventDefault();
    try {
      setLoading(true);
      const chosenMode = modes.find((m) => m.id === parseInt(selectedModeId, 10));
      const res = await invoke("rerun_post_process", {
        recordingId: rec.recording.id,
        transcriptionTryId: transTry.id,
        modeName: chosenMode ? chosenMode.name : "Custom",
        provider,
        model,
        thinkingLevel: provider === "gemini" ? thinkingLevel : null,
        prompt,
      });
      onSuccess(res);
    } catch (err) {
      console.error("Rerun post-processing error:", err);
      alert(`Post-processing failed: ${err}`);
      setLoading(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()} style={{ maxWidth: "520px" }}>
        <div className="modal-header">
          <h2>Re-Run Phase 2 LLM Post-Processing</h2>
          <button type="button" className="modal-close-btn" onClick={onClose}>✕</button>
        </div>
        <form onSubmit={handleRerun}>
          <div className="form-group">
            <label>Choose Context Preset</label>
            <select
              value={selectedModeId || ""}
              onChange={(e) => handleModeChange(e.target.value)}
            >
              {modes.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.icon} {m.name}
                </option>
              ))}
            </select>
          </div>

          <div className="form-group-row">
            <div className="form-group flex-1">
              <label>Provider</label>
              <select
                value={provider}
                onChange={(e) => {
                  const p = e.target.value;
                  setProvider(p);
                  setModel(p === "gemini" ? "gemini-3.8-flash" : "llama-3.3-70b-versatile");
                }}
              >
                <option value="gemini">Google Gemini</option>
                <option value="groq">Groq</option>
              </select>
            </div>
            <div className="form-group flex-1">
              <label>Model</label>
              <input
                type="text"
                value={model}
                onChange={(e) => setModel(e.target.value)}
              />
            </div>
          </div>

          <div className="form-group">
            <label>Prompt Instruction</label>
            <textarea
              rows={4}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
            />
          </div>

          <div className="modal-actions">
            <button type="button" className="btn-action" onClick={onClose}>Cancel</button>
            <button type="submit" className="btn-primary" disabled={loading}>
              {loading ? "Processing…" : "Run Phase 2 Again"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
