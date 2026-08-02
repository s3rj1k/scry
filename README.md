<!-- Keywords: code search, semantic code search, AI agent, LLM, BM25, embeddings, tree-sitter, AST, dependency graph, impact analysis, Rust, CLI, Claude Code, Codex, Cursor, grep replacement, token reduction, potion-code, model2vec, hybrid search, RRF -->

<h2 align="center"> semble_rs<br/> Fast and Accurate Code Search for Agents — in Rust<br/> <sub>Returns the exact code chunks an agent needs — replaces grep / cat / read / ls.</sub> </h2>

<div align="center">

<p> <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a> <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.75%2B-orange.svg" alt="Rust"></a> <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue.svg" alt="Platform"> <a href="#benchmarks"><img src="https://img.shields.io/badge/agent%20tokens-up%20to%20--47%25-brightgreen.svg" alt="Token savings"></a> </p>

<p> <a href="#quickstart">Quickstart</a> • <a href="#search">Search</a> • <a href="#how-it-works">How it works</a> • <a href="#benchmarks">Benchmarks</a> </p>

</div>

`semble_rs` is a Rust port and superset of [MinishLab/semble](https://github.com/MinishLab/semble) built for AI coding agents. It returns the exact code chunks an agent needs instead of whole files. One single binary, no daemon, no API keys, no GPU. Hybrid BM25 + [Model2Vec](https://github.com/MinishLab/model2vec) static embeddings with code-aware reranking, AST chunking, and a dependency graph.

## Quickstart

```bash
# Install Rust if needed, then:
git clone https://github.com/johunsang/semble_rs.git && cd semble_rs
cargo install --path .
```

The binary lands at `~/.cargo/bin/semble_rs`. On first run, the default embedding model `minishlab/potion-code-16M` (\~60 MB) is downloaded from HuggingFace.

```bash
# Find code by what it does (replaces grep + cat)
semble_rs search "how is auth handled" ./my-project --outline
```

For agent integration (Claude Code, Codex, Cursor), see [Agent integration](#agent-integration).

## Main Features

- **Fast**: indexes the local repo (22 files) in \~150 ms, \~10 s on 1,600 files. Static embedder — no transformer forward pass at query time.
- **Token-efficient**: `--outline` is **-47%** vs full output; returns chunks, not whole files.
- **Hybrid retrieval**: BM25 + Model2Vec embeddings fused with RRF, then reranked with definition / identifier-stem / file-coherence boosts and noise penalties.
- **Single binary**: no Python, no daemon, no API keys. Runs on CPU.

## Search

```bash
semble_rs search "auth flow" ./my-project --outline    # pass 1: structural overview
semble_rs search "loginWithEmail" ./my-project --compact   # pass 2: matching lines
```

`path` defaults to the current directory.

### Output modes

| Mode | Output | Token cost vs `--compact` | When to use |
| --- | --- | --- | --- |
| `--outline` | One signature line per chunk | **-47%** | First-pass structural scan |
| `--group` | Directory grouping + match lines capped at 3 (`+N` overflow) | \-47% | Many match lines per chunk |
| `--compact` | Score + path + every matching line | baseline | Precision scan |
| `--json --strip` | Chunk bodies (comments stripped) | +800% | Tooling / pipeline integration |
| `--json` | Chunk bodies (raw) | +900% | Tooling / pipeline integration |

**Recommended:** `--outline` to overview → `--compact` to narrow → `--json --strip` only if the chunk body itself is needed.

### `find-related`

Given a `file:line` from a previous search result, returns chunks semantically similar to that location.

```bash
semble_rs find-related src/auth.rs 42 ./my-project
```

### `--model`

All search-side commands accept `--model <hf-repo-or-local-path>` to override the default embedder. Also honours the `SEMBLE_MODEL_PATH` environment variable.

## Agent integration

Append a snippet like the following to your project-root `CLAUDE.md` or `AGENTS.md`. It works for Claude Code, Codex, Cursor (`.cursorrules`), Aider, and OpenHands.

```markdown
## Code search and exploration

Use `semble_rs` instead of `ls -R`, `grep`, `cat`:

​```bash
semble_rs search "<feature or symbol>" . --outline # pass 1
semble_rs search "<feature or symbol>" . --compact # pass 2
​```
```

## How it works

`semble_rs` chunks every file with `tree-sitter` at function / class / module boundaries (line-based fallback for unsupported languages), then scores every query with two complementary retrievers: static [Model2Vec](https://github.com/MinishLab/model2vec) embeddings (default `minishlab/potion-code-16M`) for semantic similarity, and BM25 for lexical matches on identifiers and API names. Score lists are fused with Reciprocal Rank Fusion.

After fusion, results are reranked with code-aware signals:

<details> <summary><b>Ranking signals</b></summary>

- **Adaptive weighting.** Symbol-like queries (`Foo::bar`, `_private`, `getUserById`) get more lexical weight; natural-language queries stay balanced.
- **Definition boosts.** Chunks that define the queried symbol (a `class`, `def`, `func`, etc.) outrank chunks that merely reference it.
- **Identifier stems.** Query tokens are stemmed and matched against identifier stems. Querying `parse config` boosts chunks containing `parseConfig`, `ConfigParser`, or `config_parser`.
- **File coherence.** When multiple chunks of a file match, the file is boosted so the top result reflects file-level relevance.
- **Sibling-chunk boost.** Chunks adjacent to a top hit get a small boost — definitions and their helpers usually cluster.
- **Dependency boost.** Chunks in files imported by a top hit get boosted so call-chain context surfaces.
- **Noise penalties.** Test files, `compat/` / `legacy/` shims, example code, and `.d.ts` declaration stubs are down-ranked so canonical implementations surface first.

</details>

The embedder is fully static (vocab embedding lookup → mean pool → SIF weighting → L2 normalize). All of this runs in milliseconds on CPU.

## Benchmarks

### Retrieval quality — 90-query benchmark (this repo)

90 hand-labelled queries across 4 categories: exact symbol names, natural-language feature descriptions, scenarios, and acronyms. Default model `minishlab/potion-code-16M`.

| Metric | Score |
| --- | --- |
| Recall@1 | 70% |
| Recall@5 | 90% |
| Recall@10 | 95% |
| MRR | 0.78 |
| Median latency | 150 ms / query (cold) |

| Category | n | R@1 | R@5 | R@10 | MRR |
| --- | --- | --- | --- | --- | --- |
| exact_symbol | 30 | 93% | 100% | 100% | 0.96 |
| nl_feature | 40 | 75% | 98% | 100% | 0.83 |
| scenario | 10 | 70% | 100% | 100% | 0.77 |
| acronym | 10 | 50% | 70% | 70% | 0.56 |

### Indexing and query latency by repo size

The index is rebuilt every run (no persistent cache).

| Repo size (code files) | Indexing + first query |
| --- | --- |
| 22 (this repo) | **\~0.15 s** |
| 57–120 | \~0.3–0.7 s |
| 1,600 | \~10 s |

### Token efficiency vs native shell tools

Measured on real projects:

| Operation | `semble_rs` | Native | Reduction |
| --- | --- | --- | --- |
| **Code chunk lookup** (`--outline` vs `--compact`) | \-47% | baseline | \-47% |

> Agents using `grep + cat + ls -R` spend most of their context window on irrelevant code and noise. `semble_rs` returns only what matters.

## Supported languages

| Language | Search | AST chunking | Dependency graph |
| --- | --- | --- | --- |
| Rust | ✓ | ✓ | ✓ |
| Python | ✓ | ✓ | ✓ |
| JavaScript / TypeScript | ✓ | ✓ | ✓ |
| Go | ✓ | ✓ | ✓ |
| Java | ✓ | ✓ | ✓ |
| C / C++ | ✓ | ✓ | ✓ |
| Ruby | ✓ | ✓ | ✓ |
| PHP | ✓ | ✓ | ✓ |
| Swift | ✓ | ✓ | ✓ |
| HTML / CSS / Vue / Svelte | ✓ | line-based | partial |
| Other | ✓ | line-based | — |

## License

MIT

## Acknowledgements

- [MinishLab/semble](https://github.com/MinishLab/semble) — original Python implementation by Stéphan Tulkens and Thomas van Dongen. `semble_rs` is a Rust port + superset of their work.
- [Model2Vec](https://github.com/MinishLab/model2vec) and [model2vec-rs](https://github.com/MinishLab/model2vec-rs) — static distillation framework powering the embedder.
- Embedding model: `minishlab/potion-code-16M`.