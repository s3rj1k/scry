# semble_rs Agent Instructions

Use `semble_rs` to keep code exploration and build logs small.

## Exploration Flow

Prefer the smallest useful command.

```bash
semble_rs search "<feature>" . --outline -k 8  # pass 1: structural overview
semble_rs search "<feature-or-symbol>" . --compact -k 8
```

Start with `search --outline` for a structural overview, then narrow with `search --compact`. Treat weak matches as leads, not facts.

## Token Discipline

- Do not start by opening whole files.
- Do not use `--json` or chunk bodies unless compact results are insufficient.
- Search with natural-language feature descriptions before guessing symbol names.
- Pass a directory path to search commands, not a single file path.
- Use `--model <repo-or-path>` (or `SEMBLE_MODEL_PATH` env) to override the default embedder per-call. Default: `minishlab/potion-code-16M`.
- Fall back to raw `grep`, `cat`, `find`, or `ls` only when `semble_rs` is insufficient.

## Reporting

When summarizing work, keep it short:

- files changed
- key behavior change
- verification command
- remaining risk or low-confidence area

Do not quote fixed whole-session savings unless a workflow benchmark was run. It is okay to cite measured command-level savings, such as byte counts from `wc -c`.
