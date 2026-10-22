// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Bulbul contributors

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./ContextModesView.css";

const PRESET_ICONS = ["⚡", "💻", "✉️", "💬", "🌐", "📝", "🎯", "🛠️", "💡", "🤖"];

export default function ContextModesView() {
  const [modes, setModes] = useState([]);
  const [activeId, setActiveId] = useState(1);
  const [loading, setLoading] = useState(true);
  const [editingMode, setEditingMode] = useState(null); // null = modal closed, object = modal open
  const [isNew, setIsNew] = useState(false);

  useEffect(() => {
    loadData();
  }, []);

  async function loadData() {
    try {
      setLoading(true);
      const [modesList, cfg] = await Promise.all([
        invoke("list_context_modes"),
        invoke("get_config"),
      ]);
      setModes(modesList);
      setActiveId(cfg.active_context_mode_id || 1);
    } catch (e) {
      console.error("Failed to load context modes:", e);
    } finally {
      setLoading(false);
    }
  }

  async function handleSetActive(id) {
    try {
      await invoke("set_active_context_mode", { id });
      setActiveId(id);
    } catch (e) {
      console.error("Failed to set active mode:", e);
    }
  }

  function handleCreateNew() {
    setIsNew(true);
    setEditingMode({
      id: null,
      name: "",
      icon: "💻",
      system_prompt: "You are a professional assistant. Transform the user's spoken thoughts into clear, well-structured text. Preserve technical identifiers and terminology.",
      default_phase1_mode: "smart",
      phase2_enabled: true,
      phase2_provider: "gemini",
      phase2_model: "gemini-3.8-flash",
      thinking_level: "low",
      hotkey: null,
      sort_order: modes.length + 1,
    });
  }

  function handleEdit(mode) {
    setIsNew(false);
    setEditingMode({ ...mode });
  }

  function handleDuplicate(mode) {
    setIsNew(true);
    setEditingMode({
      ...mode,
      id: null,
      name: `${mode.name} (Copy)`,
      sort_order: modes.length + 1,
    });
  }

  async function handleDelete(id) {
    if (modes.length <= 1) {
      alert("At least one context mode must remain.");
      return;
    }
    if (!confirm("Are you sure you want to delete this context mode?")) return;
    try {
      await invoke("delete_context_mode", { id });
      if (activeId === id) {
        const remaining = modes.filter((m) => m.id !== id);
        if (remaining.length > 0) {
          await handleSetActive(remaining[0].id);
        }
      }
      await loadData();
    } catch (e) {
      console.error("Failed to delete context mode:", e);
    }
  }

  async function handleSaveModal(e) {
    e.preventDefault();
    if (!editingMode.name.trim()) {
      alert("Please enter a mode name.");
      return;
    }

    try {
      await invoke("save_context_mode", {
        id: editingMode.id,
        name: editingMode.name.trim(),
        icon: editingMode.icon || "💻",
        systemPrompt: editingMode.system_prompt || "",
        defaultPhase1Mode: editingMode.default_phase1_mode || "smart",
        phase2Enabled: Boolean(editingMode.phase2_enabled),
        phase2Provider: editingMode.phase2_provider || "gemini",
        phase2Model: editingMode.phase2_model || "gemini-3.8-flash",
        thinkingLevel: editingMode.phase2_enabled && editingMode.phase2_provider === "gemini" ? (editingMode.thinking_level || "low") : null,
        hotkey: editingMode.hotkey || null,
        sortOrder: editingMode.sort_order || 0,
      });

      setEditingMode(null);
      await loadData();
    } catch (err) {
      console.error("Failed to save context mode:", err);
      alert(`Error saving mode: ${err}`);
    }
  }

  return (
    <div className="page context-modes-page">
      <header className="page-header">
        <div className="header-text">
          <h1>Context Modes & AI Presets</h1>
          <p className="muted small">
            Tailor how your speech is processed. Dictate directly in Smart Mode, or transform Bengali thoughts into high-accuracy Coding Agent prompts, polished emails, or natural chat.
          </p>
        </div>
        <button type="button" className="btn-primary new-mode-btn" onClick={handleCreateNew}>
          + New Mode
        </button>
      </header>

      {loading ? (
        <div className="modes-loading">Loading context modes...</div>
      ) : (
        <div className="modes-grid">
          {modes.map((mode) => {
            const isActive = activeId === mode.id;
            return (
              <div key={mode.id} className={`mode-card ${isActive ? "active-mode" : ""}`}>
                <div className="mode-card-header">
                  <div className="mode-title-wrap">
                    <span className="mode-icon">{mode.icon}</span>
                    <h3 className="mode-name">{mode.name}</h3>
                  </div>
                  {isActive ? (
                    <span className="badge badge-active-glow">Active Mode</span>
                  ) : (
                    <button
                      type="button"
                      className="btn-activate"
                      onClick={() => handleSetActive(mode.id)}
                    >
                      Set Active
                    </button>
                  )}
                </div>

                <div className="mode-badges-row">
                  <span className="badge badge-phase1">
                    Phase 1: {mode.default_phase1_mode.toUpperCase()}
                  </span>
                  {mode.phase2_enabled ? (
                    <>
                      <span className="badge badge-provider">
                        {mode.phase2_provider === "gemini" ? "Google Gemini" : "Groq"}
                      </span>
                      <span className="badge badge-model">{mode.phase2_model}</span>
                      {mode.phase2_provider === "gemini" && mode.thinking_level && (
                        <span className="badge badge-thinking">
                          Thinking: {mode.thinking_level}
                        </span>
                      )}
                    </>
                  ) : (
                    <span className="badge badge-direct">Direct (Skip Phase 2)</span>
                  )}
                </div>

                <div className="mode-prompt-preview">
                  <span className="prompt-label">System Instruction:</span>
                  <p className="prompt-text">
                    {mode.phase2_enabled && mode.system_prompt
                      ? mode.system_prompt
                      : "Pasted directly from Phase 1 STT at maximum speed."}
                  </p>
                </div>

                <div className="mode-card-actions">
                  <button type="button" className="btn-action" onClick={() => handleEdit(mode)}>
                    Edit
                  </button>
                  <button type="button" className="btn-action" onClick={() => handleDuplicate(mode)}>
                    Duplicate
                  </button>
                  {modes.length > 1 && (
                    <button
                      type="button"
                      className="btn-action btn-danger-action"
                      onClick={() => handleDelete(mode.id)}
                    >
                      Delete
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Edit / Create Modal */}
      {editingMode && (
        <div className="modal-backdrop" onClick={() => setEditingMode(null)}>
          <div className="modal-content mode-edit-modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>{isNew ? "Create Context Mode" : `Edit "${editingMode.name}"`}</h2>
              <button
                type="button"
                className="modal-close-btn"
                onClick={() => setEditingMode(null)}
              >
                ✕
              </button>
            </div>

            <form onSubmit={handleSaveModal}>
              <div className="form-group-row">
                <div className="form-group flex-1">
                  <label>Mode Name</label>
                  <input
                    type="text"
                    required
                    value={editingMode.name}
                    onChange={(e) => setEditingMode({ ...editingMode, name: e.target.value })}
                    placeholder="e.g. Coding Agent Prompt, Email Polish"
                  />
                </div>
                <div className="form-group" style={{ width: "90px" }}>
                  <label>Icon</label>
                  <select
                    value={editingMode.icon}
                    onChange={(e) => setEditingMode({ ...editingMode, icon: e.target.value })}
                  >
                    {PRESET_ICONS.map((ic) => (
                      <option key={ic} value={ic}>
                        {ic}
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              <div className="form-group">
                <label>Phase 1 Transcription Mode</label>
                <select
                  value={editingMode.default_phase1_mode}
                  onChange={(e) =>
                    setEditingMode({ ...editingMode, default_phase1_mode: e.target.value })
                  }
                >
                  <option value="smart">SMART Mode (Auto-punctuated, strips fillers, clean)</option>
                  <option value="verbatim">VERBATIM Mode (Raw word-for-word transcript)</option>
                </select>
              </div>

              <div className="form-group checkbox-group">
                <label className="checkbox-label">
                  <input
                    type="checkbox"
                    checked={editingMode.phase2_enabled}
                    onChange={(e) =>
                      setEditingMode({ ...editingMode, phase2_enabled: e.target.checked })
                    }
                  />
                  <span>Enable Phase 2 LLM Post-Processing</span>
                </label>
                <p className="muted x-small">
                  When enabled, Phase 1 transcription is routed through an LLM to adapt formatting, grammar, or task structuring before pasting.
                </p>
              </div>

              {editingMode.phase2_enabled && (
                <div className="phase2-config-panel">
                  <div className="form-group-row">
                    <div className="form-group flex-1">
                      <label>LLM Provider</label>
                      <select
                        value={editingMode.phase2_provider}
                        onChange={(e) => {
                          const p = e.target.value;
                          setEditingMode({
                            ...editingMode,
                            phase2_provider: p,
                            phase2_model: p === "gemini" ? "gemini-3.8-flash" : "llama-3.3-70b-versatile",
                          });
                        }}
                      >
                        <option value="gemini">Google Gemini</option>
                        <option value="groq">Groq</option>
                      </select>
                    </div>

                    <div className="form-group flex-1">
                      <label>Model Tier</label>
                      {editingMode.phase2_provider === "gemini" ? (
                        <select
                          value={editingMode.phase2_model}
                          onChange={(e) =>
                            setEditingMode({ ...editingMode, phase2_model: e.target.value })
                          }
                        >
                          <option value="gemini-3.8-flash">gemini-3.8-flash (Reasoning / Coding)</option>
                          <option value="gemini-2.5-flash">gemini-2.5-flash (Balanced)</option>
                          <option value="gemini-3.5-flash-lite">gemini-3.5-flash-lite (Ultra-fast)</option>
                        </select>
                      ) : (
                        <select
                          value={editingMode.phase2_model}
                          onChange={(e) =>
                            setEditingMode({ ...editingMode, phase2_model: e.target.value })
                          }
                        >
                          <option value="llama-3.3-70b-versatile">llama-3.3-70b-versatile</option>
                          <option value="qwen-2.5-32b">qwen-2.5-32b</option>
                          <option value="deepseek-r1-distill-llama-70b">deepseek-r1-distill-llama-70b</option>
                        </select>
                      )}
                    </div>

                    {editingMode.phase2_provider === "gemini" && (
                      <div className="form-group flex-1">
                        <label>Thinking / Reasoning Level</label>
                        <select
                          value={editingMode.thinking_level || "low"}
                          onChange={(e) =>
                            setEditingMode({ ...editingMode, thinking_level: e.target.value })
                          }
                        >
                          <option value="minimal">Minimal</option>
                          <option value="low">Low (Fast)</option>
                          <option value="medium">Medium</option>
                          <option value="high">High (Deep reasoning)</option>
                        </select>
                      </div>
                    )}
                  </div>

                  <div className="form-group">
                    <label>System / Formatting Prompt</label>
                    <textarea
                      rows={5}
                      value={editingMode.system_prompt}
                      onChange={(e) =>
                        setEditingMode({ ...editingMode, system_prompt: e.target.value })
                      }
                      placeholder="Instructions guiding how the LLM cleans, formats, or converts the raw transcript..."
                    />
                  </div>
                </div>
              )}

              <div className="modal-actions">
                <button
                  type="button"
                  className="btn-action"
                  onClick={() => setEditingMode(null)}
                >
                  Cancel
                </button>
                <button type="submit" className="btn-primary">
                  {isNew ? "Create Mode" : "Save Changes"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}
