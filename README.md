# Scry

**Scry finds code by intent, returning the file and line range.**

Single-binary code search for AI coding agents — returns the exact chunks, not whole files. No daemon, keys, or network. Hard fork of [semble_rs](https://github.com/johunsang/semble_rs), a Rust port of [MinishLab/semble](https://github.com/MinishLab/semble).

## How it works

- **Semantic** — [Model2Vec](https://github.com/MinishLab/model2vec) embeddings ranked by cosine similarity, with a definition boost fused via RRF.
- **AST aware** — [tree-sitter](https://tree-sitter.github.io/tree-sitter/) structure-first chunking (whole functions and types) and symbol extraction.
- **Local only** — static CPU embedder; models resolved locally, never downloaded.

## Install

```bash
cargo install --git https://github.com/s3rj1k/scry
```

## Model

Scry never downloads models. Install the default embedder into the Hugging Face cache once:

```bash
hf download minishlab/potion-code-16M-v2
```

Or point at a local model directory with `--model <dir>` or `SCRY_MODEL_PATH`.

## Flow

Intent to structure:

1. **Find** — `scry search "auth flow" .` surfaces entry-point chunks.
2. **Judge** — read the chunks, not the score. Scores are rank-based, not confidence; if results are scattered or off, refine the query.
3. **Read** — results are JSON: each carries the chunk body, its `file_path`, `start_line`/`end_line`, `language`, and `score`.
4. **Explore** — feed the `file:line` and symbol to your LSP. Scry finds intent; the LSP walks references.

## License

MIT, retaining upstream copyright.
