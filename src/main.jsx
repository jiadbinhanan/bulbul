// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Romanch Roshan Singh

import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./theme.css";
import { initTheme } from "./theme.js";

const root = ReactDOM.createRoot(document.getElementById("root"));

function renderError(label, err) {
  root.render(
    <div style={{ padding: 24, color: "#e6e6e6", background: "#0e0e0e", height: "100vh", fontFamily: "system-ui", fontSize: 13 }}>
      <h3 style={{ marginTop: 0 }}>Bulbul failed to load ({label})</h3>
      <pre style={{ color: "#d46b6b", whiteSpace: "pre-wrap" }}>{err?.stack || String(err)}</pre>
    </div>
  );
}

(async () => {
  if (typeof window !== "undefined" && !window.__TAURI_INTERNALS__) {
    root.render(
      <div style={{ padding: 40, color: "#e6e6e6", background: "#0e0e0e", minHeight: "100vh", fontFamily: "system-ui", display: "flex", flexDirection: "column", justifyContent: "center", alignItems: "center", textAlign: "center" }}>
        <h2 style={{ margin: "0 0 16px 0", color: "#5EC8C0" }}>Bulbul একটি ডেস্কটপ অ্যাপ্লিকেশন</h2>
        <p style={{ maxWidth: 480, lineHeight: 1.6, color: "#aaa", margin: 0, fontSize: 14 }}>
          আপনি এটি একটি সাধারণ ব্রাউজারে (Microsoft Edge) খুলেছেন। Bulbul উইন্ডোজের নেটিভ অ্যাপ হিসেবে চলে। অনুগ্রহ করে আপনার উইন্ডোজ ডেস্কটপে মূল Bulbul অ্যাপ উইন্ডোটি দেখুন।
        </p>
      </div>
    );
    return;
  }

  let label = "main";
  try {
    label = getCurrentWindow().label;
  } catch {}

  // The overlay pill intentionally stays dark (a neutral HUD over arbitrary
  // app content); every other window follows the user's theme preference.
  if (label !== "overlay") initTheme();

  try {
    if (label === "overlay") {
      const { default: Overlay } = await import("./Overlay.jsx");
      root.render(<React.StrictMode><Overlay /></React.StrictMode>);
    } else if (label === "scratchpad") {
      const { default: ScratchpadWindow } = await import("./ScratchpadWindow.jsx");
      root.render(<React.StrictMode><ScratchpadWindow /></React.StrictMode>);
    } else {
      const { default: App } = await import("./App.jsx");
      root.render(<React.StrictMode><App /></React.StrictMode>);
    }
  } catch (e) {
    renderError(label, e);
  }
})();
