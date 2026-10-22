// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Romanch Roshan Singh & Bulbul contributors

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ConfirmDialog from "../components/ConfirmDialog.jsx";
import RecordingCardV2, {
  RerunTranscribeModal,
  RerunPostProcessModal,
} from "../components/RecordingCardV2.jsx";
import "./HomeHistoryV2.css";
import "./HistoryView.css";

export default function HistoryView() {
  const [recordings, setRecordings] = useState([]);
  const [contextModes, setContextModes] = useState([]);
  const [totalCount, setTotalCount] = useState(0);
  const [loading, setLoading] = useState(true);

  // Filter & Search states
  const [searchQuery, setSearchQuery] = useState("");
  const [filterTab, setFilterTab] = useState("all"); // 'all' | 'pinned'
  const [filterMode, setFilterMode] = useState("all"); // 'all' | mode_name
  const [pageSize, setPageSize] = useState(25);
  const [currentPage, setCurrentPage] = useState(1);

  // Modals
  const [pendingDeleteId, setPendingDeleteId] = useState(null);
  const [rerunTransModal, setRerunTransModal] = useState(null);
  const [rerunPostModal, setRerunPostModal] = useState(null);

  // Fetch recordings
  async function loadData() {
    try {
      setLoading(true);
      // Fetch up to 200 items for rich in-memory search and multi-criteria filtering
      const [recs, modes, count] = await Promise.all([
        invoke("list_recordings_v2", { limit: 200, offset: 0 }).catch(() => []),
        invoke("list_context_modes").catch(() => []),
        invoke("count_recordings_v2").catch(() => 0),
      ]);
      setRecordings(recs);
      setContextModes(modes);
      setTotalCount(count || recs.length);
    } catch (err) {
      console.error("Failed to load history data:", err);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    loadData();

    const un = listen("bulbul-status", (e) => {
      if (e.payload?.state === "done" || e.payload?.state === "idle") {
        loadData();
      }
    });

    return () => {
      un.then((f) => f());
    };
  }, []);

  async function confirmDelete() {
    const id = pendingDeleteId;
    setPendingDeleteId(null);
    if (!id) return;
    try {
      await invoke("delete_recording_v2", { id });
      setRecordings((prev) => prev.filter((r) => r.recording.id !== id));
      setTotalCount((c) => Math.max(0, c - 1));
    } catch (err) {
      console.error("Delete failed:", err);
    }
  }

  async function handleTogglePin(id) {
    try {
      const nextPinned = await invoke("toggle_recording_pinned", { id });
      setRecordings((prev) =>
        prev.map((r) =>
          r.recording.id === id
            ? { ...r, recording: { ...r.recording, is_pinned: nextPinned } }
            : r
        )
      );
    } catch (err) {
      console.error("Toggle pin failed:", err);
    }
  }

  // Filter & Search computation
  const filteredRecordings = useMemo(() => {
    return recordings.filter((item) => {
      // 1. Tab filter (pinned vs all)
      if (filterTab === "pinned" && !item.recording.is_pinned) {
        return false;
      }

      // 2. Context Mode filter
      if (filterMode !== "all") {
        const matchesMode = item.post_process_tries?.some(
          (p) => p.mode_name?.toLowerCase() === filterMode.toLowerCase()
        );
        if (!matchesMode) return false;
      }

      // 3. Search query filter
      if (searchQuery.trim()) {
        const q = searchQuery.toLowerCase().trim();
        const transMatch = item.transcription_tries?.some((t) =>
          t.transcript_text?.toLowerCase().includes(q)
        );
        const postMatch = item.post_process_tries?.some(
          (p) =>
            p.output_text?.toLowerCase().includes(q) ||
            p.mode_name?.toLowerCase().includes(q) ||
            p.prompt_snapshot?.toLowerCase().includes(q)
        );
        if (!transMatch && !postMatch) return false;
      }

      return true;
    });
  }, [recordings, filterTab, filterMode, searchQuery]);

  // Pagination calculation
  const totalPages = Math.max(1, Math.ceil(filteredRecordings.length / pageSize));
  const safePage = Math.min(currentPage, totalPages);
  const pagedRecordings = useMemo(() => {
    const start = (safePage - 1) * pageSize;
    return filteredRecordings.slice(start, start + pageSize);
  }, [filteredRecordings, safePage, pageSize]);

  const pinnedCount = recordings.filter((r) => r.recording.is_pinned).length;

  return (
    <div className="page history-view-container">
      <header className="page-header">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
          <div>
            <h1>Dictation History</h1>
            <p className="muted small">
              Full archive of your local voice recordings, Phase 1 raw transcripts, and Phase 2 LLM formatted outputs.
            </p>
          </div>
          <div style={{ textAlign: "right" }}>
            <span className="badge badge-active-glow" style={{ fontSize: "12px", padding: "4px 10px" }}>
              {totalCount} Total Audio Recordings
            </span>
          </div>
        </div>
      </header>

      {/* History Toolbar: Search & Filters */}
      <div className="history-toolbar">
        <div className="history-search-row">
          <div className="history-search-input-wrapper">
            <svg
              className="history-search-icon"
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <input
              type="text"
              className="history-search-input"
              placeholder="Search transcript or formatted text..."
              value={searchQuery}
              onChange={(e) => {
                setSearchQuery(e.target.value);
                setCurrentPage(1);
              }}
            />
          </div>
          {searchQuery && (
            <button
              type="button"
              className="btn-action"
              onClick={() => setSearchQuery("")}
              style={{ padding: "8px 12px", fontSize: "12px" }}
            >
              Clear
            </button>
          )}
        </div>

        <div className="history-filter-row">
          <div className="history-filter-tabs">
            <button
              type="button"
              className={`history-filter-tab ${filterTab === "all" ? "active" : ""}`}
              onClick={() => {
                setFilterTab("all");
                setCurrentPage(1);
              }}
            >
              All ({recordings.length})
            </button>
            <button
              type="button"
              className={`history-filter-tab ${filterTab === "pinned" ? "active" : ""}`}
              onClick={() => {
                setFilterTab("pinned");
                setCurrentPage(1);
              }}
            >
              Starred ★ ({pinnedCount})
            </button>
          </div>

          <div className="history-controls-right">
            {contextModes.length > 0 && (
              <select
                className="history-mode-select"
                value={filterMode}
                onChange={(e) => {
                  setFilterMode(e.target.value);
                  setCurrentPage(1);
                }}
              >
                <option value="all">All Context Modes</option>
                {contextModes.map((m) => (
                  <option key={m.id} value={m.name}>
                    {m.icon} {m.name}
                  </option>
                ))}
              </select>
            )}

            <select
              className="history-page-size-select"
              value={pageSize}
              onChange={(e) => {
                setPageSize(Number(e.target.value));
                setCurrentPage(1);
              }}
            >
              <option value={10}>10 / page</option>
              <option value={25}>25 / page</option>
              <option value={50}>50 / page</option>
            </select>
          </div>
        </div>
      </div>

      {/* Main List */}
      {loading ? (
        <div className="history-empty-card">
          <p className="muted">Loading recordings archive...</p>
        </div>
      ) : filteredRecordings.length === 0 ? (
        <div className="history-empty-card">
          <p style={{ margin: "0 0 8px 0", fontSize: "15px", color: "var(--text-main)" }}>
            {searchQuery || filterTab !== "all" || filterMode !== "all"
              ? "No recordings match your search or filter criteria."
              : "No recordings found in history."}
          </p>
          {(searchQuery || filterTab !== "all" || filterMode !== "all") && (
            <button
              type="button"
              className="btn-action"
              onClick={() => {
                setSearchQuery("");
                setFilterTab("all");
                setFilterMode("all");
              }}
              style={{ marginTop: "12px", padding: "6px 14px" }}
            >
              Reset Filters
            </button>
          )}
        </div>
      ) : (
        <div className="timeline-v2">
          {pagedRecordings.map((rec) => (
            <RecordingCardV2
              key={rec.recording.id}
              rec={rec}
              onTogglePin={() => handleTogglePin(rec.recording.id)}
              onRequestDelete={() => setPendingDeleteId(rec.recording.id)}
              onOpenRerunTrans={() => setRerunTransModal(rec)}
              onOpenRerunPost={(transTry) =>
                setRerunPostModal({ recording: rec, transTry })
              }
            />
          ))}
        </div>
      )}

      {/* Pagination Footer */}
      {filteredRecordings.length > pageSize && (
        <div className="history-pagination-bar">
          <div className="muted small">
            Showing {(safePage - 1) * pageSize + 1}–
            {Math.min(safePage * pageSize, filteredRecordings.length)} of{" "}
            {filteredRecordings.length} recordings
          </div>
          <div className="pagination-btn-group">
            <button
              type="button"
              className="pagination-btn"
              disabled={safePage <= 1}
              onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
            >
              ← Previous
            </button>
            <span className="muted small" style={{ margin: "0 8px" }}>
              Page {safePage} of {totalPages}
            </span>
            <button
              type="button"
              className="pagination-btn"
              disabled={safePage >= totalPages}
              onClick={() => setCurrentPage((p) => Math.min(totalPages, p + 1))}
            >
              Next →
            </button>
          </div>
        </div>
      )}

      {/* Delete Confirmation Dialog */}
      <ConfirmDialog
        open={pendingDeleteId !== null}
        title="Delete this recording?"
        message="This will permanently delete the audio recording file and all associated transcription and formatting attempts."
        confirmLabel="Delete"
        danger
        onConfirm={confirmDelete}
        onCancel={() => setPendingDeleteId(null)}
      />

      {/* Re-Transcribe Modal */}
      {rerunTransModal && (
        <RerunTranscribeModal
          rec={rerunTransModal}
          onClose={() => setRerunTransModal(null)}
          onSuccess={(newTry) => {
            setRecordings((prev) =>
              prev.map((r) =>
                r.recording.id === rerunTransModal.recording.id
                  ? { ...r, transcription_tries: [...r.transcription_tries, newTry] }
                  : r
              )
            );
            setRerunTransModal(null);
          }}
        />
      )}

      {/* Re-Post-Process Modal */}
      {rerunPostModal && (
        <RerunPostProcessModal
          rec={rerunPostModal.recording}
          transTry={rerunPostModal.transTry}
          modes={contextModes}
          onClose={() => setRerunPostModal(null)}
          onSuccess={(newPostTry) => {
            setRecordings((prev) =>
              prev.map((r) =>
                r.recording.id === rerunPostModal.recording.recording.id
                  ? { ...r, post_process_tries: [...r.post_process_tries, newPostTry] }
                  : r
              )
            );
            setRerunPostModal(null);
          }}
        />
      )}
    </div>
  );
}
