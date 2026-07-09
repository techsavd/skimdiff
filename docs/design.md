# skimdiff — lightweight agent-change review tool

## Context

The user reviews code changes made by AI agents (mostly Java/JVM backend work) and doesn't want to load IntelliJ/VS Code — both are memory hogs on his Mac — just to read a diff. TUIs feel cumbersome. He wants a lightweight but functional reviewer: instant diff viewing with live updates while an agent works, plus basic code navigation (find usages, go-to-definition) that works for JVM and other languages without dragging in an LSP the size of Eclipse.

**Decisions made in brainstorming (approved):**
- Form: local web app — a CLI that starts a server and opens a browser tab.
- Distribution: single static **Rust** binary with the web UI embedded (user chose Rust over Go — tree-sitter is Rust-native, best-in-class bindings); Homebrew tap later.
- Workflow: live working-tree diff by default; arbitrary commit ranges; review actions (viewed marks, notes, stage/discard hunks).
- Code nav: tree-sitter symbol index (syntax-aware, name-based) — deliberately NOT LSP/jdtls.
- Name: **skimdiff**. New project at `~/code/skimdiff`.

## Architecture

Rust binary (edition 2021+), tokio + axum server.

```
~/code/skimdiff/
  src/
    main.rs               CLI entry (clap): arg parsing (range vs live), start server, open browser
    gitx.rs     shell out to git: diff/status parsing, apply, ls-files (unit-tested vs fixture repos)
    diff.rs     patch model: File -> Hunk -> Line, word-level intra-line diff
    watch.rs    notify-crate watcher, gitignore-aware (ignore crate), debounced -> channel
    index/      tree-sitter symbol index: declarations + references, incremental updates
    server.rs   axum: REST + SSE, serves embedded UI (rust-embed)
    state.rs    review state (viewed/notes) persisted to .git/skimdiff/state.json (serde_json)
  web/          Vite + Preact frontend; `npm run build` output embedded via rust-embed
  tests/        integration tests; fixture repos created in tempdirs (tempfile crate)
```

Key crates: `axum`, `tokio`, `clap`, `notify`, `ignore`, `rust-embed`, `serde`/`serde_json`, `tree-sitter` + grammar crates (`tree-sitter-java`, `tree-sitter-kotlin`, `tree-sitter-go`, `tree-sitter-python`, `tree-sitter-javascript`/`-typescript`), `similar` (word-level diff), `open` (launch browser).

- **Diffing:** parse `git diff --patch --no-color` and `git status --porcelain=v2`. Untracked files rendered as all-added. Ranges (`skimdiff main..feat`, `HEAD~3`, `<sha>`) are static; no-arg mode is live.
- **Live updates:** notify → debounce (~300ms) → recompute diff → push over SSE. UI shows a "changed" badge / auto-refreshes.
- **Symbol index:** built lazily on first nav request using native tree-sitter grammars for Java, Kotlin, Go, Python, JS/TS. Index maps symbol name → declarations and reference sites (call expressions, identifiers), skipping strings/comments by node type. Find-usages results grouped by file, changed files ranked first. Incremental re-parse on file-change events.
- **Review actions:** viewed checkmarks + free-text notes in `.git/skimdiff/state.json` (keyed by diff identity); stage hunk = `git apply --cached` with the hunk patch, discard = `git apply -R`.
- **Frontend:** file tree sidebar (change stats, viewed state), side-by-side/unified toggle, word-level highlights, syntax highlighting (highlight.js), symbol click → popover (definition + usages) → read-only file viewer with same nav. Keyboard: j/k hunks, n/p files, v mark viewed.

## Build phases (each independently usable; tests gate each phase)

1. **Static diff viewer** — cargo scaffold, gitx + diff parsing with unit tests, axum server serving embedded UI, CLI with range args, side-by-side/unified rendering, syntax highlighting, file tree, keyboard nav.
2. **Live mode** — watch module + SSE; default no-arg mode becomes live working-tree view.
3. **Review actions** — state persistence, viewed/notes UI, stage/discard hunk endpoints (careful tests: apply against fixture repos, verify no working-tree corruption).
4. **Code nav** — index module, Java + Kotlin grammars first, then Go/Python/JS-TS; usages popover + file viewer in UI.

Also write the approved design into the repo at `docs/design.md` during phase 1, and `git init` + commit per phase.

## Testing / verification

- `cargo test` — fixture git repos built in tempdirs; cover: patch parsing round-trips, untracked handling, rename/binary edge cases, hunk stage/discard leaves `git status` as expected, index finds known usages in fixture Java/Kotlin files and skips strings/comments.
- End-to-end per phase: run `skimdiff` against a real repo (e.g. `~/code/bridgefire`), open browser, verify rendering/live-update/nav manually.
- Frontend stays thin; no separate JS test suite in v1.

## Non-goals (v1)

- No LSP integration, no semantic type resolution (name-based nav is accepted).
- No commenting/PR integration, no multi-repo, no Windows testing (macOS/Linux fine).
- No auth — server binds 127.0.0.1 only.
