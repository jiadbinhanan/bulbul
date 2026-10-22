// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Romanch Roshan Singh & Bulbul contributors

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import FeatureHero from "../components/FeatureHero.jsx";
import ConfirmDialog from "../components/ConfirmDialog.jsx";
import RecordingCardV2, {
  RerunTranscribeModal,
  RerunPostProcessModal,
} from "../components/RecordingCardV2.jsx";
import { OS_NOUN, IS_ANDROID } from "../platform.js";
import featureHomeImg from "../assets/feature-home.png";
import "./HomeHistoryV2.css";

const RECENT_LIMIT = 10;

export default function HomeView({ displayName, onNavigate }) {
  const [stats, setStats] = useState(null);
  const [totalCount, setTotalCount] = useState(0);
  const [recordings, setRecordings] = useState([]);
  const [loading, setLoading] = useState(true);
  const [pendingDeleteId, setPendingDeleteId] = useState(null);

  // Re-run modal states
  const [rerunTransModal, setRerunTransModal] = useState(null); // recording item or null
  const [rerunPostModal, setRerunPostModal] = useState(null); // { recording, transTry } or null
  const [contextModes, setContextModes] = useState([]);

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        setLoading(true);
        const [s, recs, modes, cnt] = await Promise.all([
          invoke("get_home_stats").catch(() => null),
          invoke("list_recordings_v2", { limit: RECENT_LIMIT, offset: 0 }).catch(() => []),
          invoke("list_context_modes").catch(() => []),
          invoke("count_recordings_v2").catch(() => 0),
        ]);
        if (mounted) {
          setStats(s);
          setRecordings(recs);
          setContextModes(modes);
          setTotalCount(cnt);
        }
      } catch (e) {
        console.error("home load failed", e);
      } finally {
        if (mounted) setLoading(false);
      }
    }
    load();

    const un = listen("bulbul-status", (e) => {
      if (e.payload?.state === "done" || e.payload?.state === "idle") {
        invoke("list_recordings_v2", { limit: RECENT_LIMIT, offset: 0 })
          .then((recs) => mounted && setRecordings(recs))
          .catch(() => {});
        invoke("get_home_stats")
          .then((s) => mounted && setStats(s))
          .catch(() => {});
        invoke("count_recordings_v2")
          .then((c) => mounted && setTotalCount(c))
          .catch(() => {});
      }
    });

    return () => {
      mounted = false;
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
      invoke("get_home_stats").then(setStats).catch(() => {});
      invoke("count_recordings_v2").then(setTotalCount).catch(() => {});
    } catch (e) {
      console.error("delete recording failed", e);
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
    } catch (e) {
      console.error("toggle pin failed", e);
    }
  }

  return (
    <div className="page home">
      <header className="page-header">
        <h1>
          Welcome back{displayName && displayName.trim() ? `, ${displayName.trim()}` : ""}
        </h1>
        <p className="muted small">
          Local-first, two-phase dictation. Audio is persisted locally before upload; speech is transcribed via Gemini 3.5 and optionally refined via Phase 2 LLMs.
        </p>
      </header>

      <FeatureHero
        dismissKey="bulbul.home.hero.dismissed"
        title={<>Speak. Edit. <em>Move on.</em></>}
        blurb={
          IS_ANDROID
            ? "Tap the floating bubble in any app, talk, and Bulbul types the cleaned-up text right where your cursor is."
            : `Hold your hotkey anywhere on ${OS_NOUN}, talk, release. Bulbul transcribes, formats with your active Context Mode, and types right at your cursor.`
        }
        image={featureHomeImg}
      />

      <section className="stat-cards">
        <StatCard
          label="Total recordings"
          value={formatNumber(totalCount || recordings.length)}
          subtitle="Saved locally"
        />
        <StatCard
          label="Words per minute"
          value={stats ? Math.round(stats.wpm).toString() : "—"}
          subtitle="Last 7 days"
        />
        <StatCard
          label="Day streak"
          value={stats ? `${stats.day_streak}` : "—"}
          subtitle={stats?.day_streak === 1 ? "day" : "days in a row"}
        />
        <StatCard
          label="Context Modes"
          value={contextModes ? `${contextModes.length}` : "5"}
          subtitle="Ready to use"
        />
      </section>

      <section className="timeline-section">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "16px" }}>
          <div>
            <h3 style={{ margin: 0 }}>Recent Activity</h3>
            <span className="muted x-small">Showing last {Math.min(recordings.length, RECENT_LIMIT)} recordings</span>
          </div>
          {onNavigate && (
            <button
              type="button"
              className="btn-action"
              onClick={() => onNavigate("history")}
              style={{ padding: "6px 14px", fontSize: "12px", cursor: "pointer" }}
            >
              View Full History ({totalCount || recordings.length}) →
            </button>
          )}
        </div>

        {loading ? (
          <div className="empty-state"><p className="muted">Loading activity...</p></div>
        ) : recordings.length === 0 ? (
          <div className="empty-state">
            <p>
              No recordings yet. Hold your hotkey anywhere on {OS_NOUN} (or tap if Tap to Talk is on) and start talking!
            </p>
          </div>
        ) : (
          <div className="timeline-v2">
            {recordings.slice(0, RECENT_LIMIT).map((rec) => (
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

        {recordings.length > 0 && onNavigate && (
          <div style={{ textAlign: "center", marginTop: "20px" }}>
            <button
              type="button"
              className="btn-action"
              onClick={() => onNavigate("history")}
              style={{ padding: "8px 20px", fontSize: "13px" }}
            >
              Browse All Past Recordings in History →
            </button>
          </div>
        )}
      </section>

      {/* Delete Confirmation */}
      <ConfirmDialog
        open={pendingDeleteId !== null}
        title="Delete this recording?"
        message="This permanently deletes the audio file and all transcription attempts from your machine."
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

function StatCard({ label, value, subtitle }) {
  return (
    <div className="stat-card">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      {subtitle && <div className="stat-sub">{subtitle}</div>}
    </div>
  );
}

function formatNumber(n) {
  if (n == null) return "0";
  return n.toLocaleString();
}
