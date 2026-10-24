# AGENTS.md — rules for every AI agent working in BULBUL

## Project

## Hard rules
1. Stay in scope: do exactly one phase. Never start the next one without committing and reporting.
2. When finished with a phase, append a PHASE REPORT to `PROGRESS.md` and show it.
3. Phase numbering per chat session: Within any single conversation / chat session, all phases must strictly follow session-scoped numbering. The first phase completed in that session establishes the base major phase number (e.g., `Phase 44`). All subsequent phases completed within that same chat session must use alphabetical sub-phase lettering: `44(a)`, `44(b)`, `44(c)`, etc. Never increment to a new major integer within the same chat session.
4. Modern technologies & web research: When designing, implementing, or updating features, always keep the current date / latest modern standards in mind. Actively research the internet and official documentation to identify and adopt the newest, most modern technologies, APIs, and best practices (e.g., modern Tauri, WebDAV, Gemini 3.5 Transcribe / Flash API, Android Kotlin IME, SQLite) rather than legacy patterns.
5. Phase-wise Report & Git Commit Rule: After each phase is complete, write a clear phase report in English in `PROGRESS.md` and immediately commit all changes to Git with a descriptive commit message. Never start the next phase without committing the previous phase.
6. Windows Protection Rule: The Windows version must NEVER break or regress under any circumstances. All changes must be strictly isolated to Android or unified cross-platform layers without impacting Windows desktop operations.
7. Language Rule: All direct conversation, phase explanations, and user updates in chat MUST ALWAYS be delivered in the Bengali language (বাংলা ভাষায়).

## PHASE REPORT format
- Phase / Status (PASS | FAIL | BLOCKED)
- Commands run (one line each)
- Key output (trimmed, no secrets)
- Files created or changed
- Deviations from the prompt
- Ready for next phase? (yes/no and why)

