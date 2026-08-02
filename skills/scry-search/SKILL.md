---
name: scry-search
description: Find and navigate code by intent using scry (semantic code search) together with the editor LSP. Use when locating an implementation, understanding how something works, tracing a feature end to end, or exploring an unfamiliar codebase. Prefer over grep, glob, and ls for any semantic or "where is / how does" question.
---

# Scry search + LSP navigation

Two complementary tools, used together:

- **scry** finds code by *intent* (semantic embeddings with a definition boost) and returns JSON chunks with the exact `file_path` and line range. It replaces grep / cat / ls for "where is..." and "how does..." questions.
- **The LSP** walks *structure* from a known location: definitions, references, callers. It is exact, not fuzzy.

Rule of thumb: **scry to find the entry point, LSP to follow the wires.**

## Workflow

1. **Find** — `scry search "<intent>" <path>` returns entry-point chunks, each with a `file:start-end` range.
2. **Judge** — read the returned chunks, not the score. Scores are rank-based (reciprocal-rank fusion), not similarity, so the top hit sits in a fixed band however good or bad the match is. Judge by whether the chunks fit and cluster; if they are scattered across unrelated files or none fit, refine the query and search again.
3. **Read** — output is JSON. Each result carries the chunk `content`, its `file_path` with `start_line` and `end_line`, `language`, and `score`. The line range is the reliable locator; the `match_lines` are a lexical hint and can be sparse for natural-language queries.
4. **Explore** — hand a returned `file:line` and symbol to the LSP tool (`goToDefinition`, `findReferences`, `incomingCalls`). scry finds intent; the LSP walks references.

## scry usage

```bash
scry search "how is auth handled" .          # JSON, the only output
scry search "loginWithEmail" .               # symbol query
scry search "parser" . -k 5                  # cap the result count
scry search "config" . --include-text-files  # also index .md, .yaml, .json
```

Path defaults to the current directory. Useful flags: `-k/--top-k`, `--include-text-files`, `--model`. Run `scry search --help` for the full set of tuning knobs. scry keeps no daemon and rebuilds its index per call, so repeated queries are fine and always current.

## Setup (only if a tool is missing)

Check with `command -v scry` before installing anything.

```bash
# scry itself (needs Rust and git)
cargo install --git https://github.com/s3rj1k/scry

# embedding model, fetched once into the HF cache; scry never downloads at runtime
hf download minishlab/potion-code-16M-v2
```

## LSP (optional, for structural navigation)

Use Claude Code's LSP tool on the `file:line` scry reports. It needs a language server for that file type. Install the project's server if it is absent:

| Language        | Server                     | Install                                          |
| --------------- | -------------------------- | ------------------------------------------------ |
| Rust            | rust-analyzer              | `rustup component add rust-analyzer`             |
| Go              | gopls                      | `go install golang.org/x/tools/gopls@latest`     |
| Python          | pyright                    | `npm i -g pyright`                               |
| TypeScript / JS | typescript-language-server | `npm i -g typescript-language-server typescript` |
| C / C++         | clangd                     | install via the system package manager           |

Then run `goToDefinition`, `findReferences`, or `incomingCalls` on the symbol at that location.

## When not to use scry

- You already know the exact string or regex, so ripgrep is faster.
- You can already name the file or symbol precisely, so open it or go straight to the LSP.
