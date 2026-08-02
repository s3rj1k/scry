# Scry

**Scry finds code by intent with hybrid semantic + lexical search, returning the exact file and line.**

Single-binary code search for AI coding agents — returns the exact chunks, not whole files. No daemon, keys, or network. Hard fork of [semble_rs](https://github.com/johunsang/semble_rs), a Rust port of [MinishLab/semble](https://github.com/MinishLab/semble).

## How it works

- **Hybrid** — BM25 + [Model2Vec](https://github.com/MinishLab/model2vec) embeddings, fused with RRF and a definition boost.
- **AST aware** — [tree-sitter](https://tree-sitter.github.io/tree-sitter/) chunking and symbol extraction.
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
2. **Gauge** — low or scattered scores mean the query needs work.
3. **Narrow** — `--compact` or `--group` pins the exact lines.
4. **Explore** — feed the `file:line` and symbol to your LSP. Scry finds intent; the LSP walks references.

## License

MIT, retaining upstream copyright.
