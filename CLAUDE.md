# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Local MS Translator: a macOS-only menubar app that translates selected English text to Japanese via a global `cmd+J` shortcut. Translation runs entirely locally through Ollama — no text leaves the machine. English→Japanese only, single-user, never distributed publicly. See `docs/requirements.md` for the full spec.

## Commands

Package manager is **pnpm**.

- `pnpm tauri dev` — run the full app (Rust backend + Vite frontend). This is the primary dev loop.
- `pnpm dev` — frontend only on port 1420 (Tauri expects this fixed port; `strictPort` is on).
- `pnpm build` — `tsc && vite build` (typecheck + build the frontend bundle to `dist/`).
- `pnpm tauri build` — produce the macOS bundle.

There is no test suite and no separate lint step; `tsc` (via `pnpm build`) is the typecheck gate.

## Architecture

Two halves communicate over Tauri commands (frontend→backend calls) and events (backend→frontend pushes).

**Backend** (`src-tauri/src/lib.rs`, single file): registers the `cmd+J` global shortcut and a menubar tray icon (no Dock — `ActivationPolicy::Accessory`). On `cmd+J`, `toggle_window` grabs the current selection *before* showing the window, then emits `translate-request`. Exposed commands: `load_config`, `save_config`, `translate`, `warm_model`, `check_accessibility`, `grab_selection`.

**Frontend** (`src/App.tsx`): a single `Translator` component wrapped by `AccessibilityGuide`. It listens for backend events and renders the Spotlight-style window. shadcn/ui components live in `src/components/ui/`.

### Key flows

- **Selection capture** (`grab_selection`): saves the clipboard, sends a synthetic `cmd+C` via `enigo`, polls up to 500ms for the clipboard to *change* (not a fixed sleep — handles slow apps like Teams/browsers), reads it, then restores the original clipboard. Requires macOS Accessibility permission.
- **Accessibility gate**: `check_accessibility` calls macOS `AXIsProcessTrusted`. `AccessibilityGuide` blocks the UI with a setup guide until granted, and re-checks on window `focus` (so returning from System Settings auto-recovers). In dev, the permission must be granted to the terminal/Tauri dev process, not the bundled app.
- **Streaming translation** (`translate`): POSTs to Ollama `/api/chat` with `stream: true`, parses newline-delimited JSON chunks, and emits each token as a `translate-token` event; `translate-done` on completion. The frontend appends tokens live.
- **Model warm-up** (`warm_model`): on startup, POSTs `/api/chat` with empty `messages` to preload the model into memory (`keep_alive: 30m`), killing first-translation latency.

### Backend→frontend events

`translate-request` (selection text on show), `translate-token`, `translate-done`, `translate-warning` (input truncated at 5000 chars), `reset` (window hidden), `open-settings` (tray menu).

## Config

User config lives at `~/.config/local-ms-translator/config.json` (`model`, `endpoint`). Defaults: model `qwen2.5:14b`, endpoint `http://localhost:11434`. Editable via the in-app settings dialog. The `translate` command reads config fresh on each call.

## Constraints & gotchas

- **`time` crate is pinned to `=0.3.47`** in `src-tauri/Cargo.toml` — 0.3.48 breaks the build with an E0119 trait-impl conflict. Do not bump it.
- Window is frameless, always-on-top, 720px wide, `skipTaskbar`, initially `visible: false`. Height auto-grows to content via a `ResizeObserver` in `App.tsx`.
- New backend commands must be added to BOTH the `invoke_handler!` macro list and (if they touch new capabilities) `src-tauri/capabilities/default.json`.
- The `@` import alias maps to `src/` (configured in both `vite.config.ts` and `tsconfig.json`).
- Despite the product spec, `tauri.conf.json` still has `productName: "tauri-app"` and the crate is `tauri-app` — these are scaffold leftovers.
