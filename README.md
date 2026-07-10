# skimdiff

Lightweight review of agent-made code changes — a single binary that serves a
fast diff UI in your browser. No IDE, no Electron, no LSP.

```
cd your-repo
skimdiff                 # live working-tree diff, auto-refreshes as files change
skimdiff main..feature   # review a branch
skimdiff HEAD~3          # review the last 3 commits
```

## Features

- **Live mode** — watches the working tree (gitignore-aware) and refreshes the
  browser as an agent edits files.
- **Proper diff rendering** — side-by-side or unified, word-level intra-line
  highlights, syntax highlighting, light/dark theme.
- **Review flow** — mark files viewed, leave notes (persisted in
  `.git/skimdiff/`, never touches your working tree), stage or discard
  individual hunks.
- **Code navigation without an LSP** — double-click any symbol for
  declarations and usages via a tree-sitter index (Java, Kotlin, Go, Python,
  JS/TS). Syntax-aware: mentions in strings and comments don't count.
- **Keyboard-first** — `n`/`p` file, `j`/`k` hunk, `v` viewed, `u` toggle
  split, `Esc` close panels.

## Build

Requires Rust and Node (frontend is embedded into the binary at build time):

```bash
cd web && npm install && npm run build && cd ..
cargo build --release        # target/release/skimdiff
cargo install --path .       # or install to ~/.cargo/bin
```

## Flags

- `--port <n>` — listen port (default 4400; falls back to a free port)
- `--no-open` — don't open the browser

The server binds `127.0.0.1` only.

## Development

```bash
cargo test        # behavior contract: diff parsing, watcher, endpoints, index
cd web && npm run dev   # frontend dev server proxying /api to :4400
```

Design notes in `docs/design.md`.
