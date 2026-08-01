# `plan` algorithm (removed)

The `plan` subcommand was removed from `semble_rs`. This records the algorithm it used, for reference.

`plan` never did its own retrieval. `main.rs` ran a normal hybrid `search(task, top_k)` and handed the results to `build_plan`, which re-ranked them and wrapped them in a fixed "recommended flow" template.

## Inputs

- `task` — natural-language query
- `top_k` — candidate count
- `results` — the `SearchResult`s from `index.search(task, top_k)`

## Per-candidate processing

For each search result chunk:

1. **Signature** — `extract_signature_near(content, start_line, match_lines)`; fallback `"(lines a-b)"`.
2. **Evidence line** — best `match_line` after stripping comment markers (`///`, `//`, `#`, `/* */`, `"""`, backticks). A line qualifies only if it shares ≥1 `query_term`; structural lines (`use`/`fn`/`class`/`import`/…) need ≥2. Winner = max by `(overlap, not_structural)`.
3. **Adjusted score** (used only for re-ranking, not shown as the candidate score):

   ```
   plan_score = base_score
              + 0.010 * min(overlap(file_path),  3)
              + 0.008 * min(overlap(signature),  3)
              + 0.014 * min(overlap(evidence),   4)
              - 0.01   if (no evidence AND generic signature)
   ```

   - `overlap(text)` = count of distinct `query_terms` present in `tokenize(text)`.
   - `query_terms` = task tokenized (ASCII-alphanumeric split, lowercased, length ≥ 3), minus a small stop-word list (`the`, `and`, `for`, `with`, `this`, `where`, …).
   - "generic signature" = starts with `fn main` / `pub fn new` / `def __init__` / etc.

Candidates are sorted by `plan_score` descending. The displayed candidate score is the original `base_score`, not `plan_score`.

## Confidence

From the top candidate's `plan_score`:

| top plan_score | label  |
| -------------- | ------ |
| ≥ 0.10         | high   |
| ≥ 0.05         | medium |
| else           | low    |

## Recommended flow (fixed template)

Emitted verbatim, parameterized only by `task`/`path`/`top_k`:

1. `search <task> <path> --outline -k <k>` — start broad
2. `search <task> <path> --group   -k <k>` — group if noisy
3. `search <task> <path> --compact -k <min(k,8)>` — narrow precisely

Then, for the first 3 unique **code** candidate files (by extension):

4. `deps   <file> <path>` — imports / symbols / users
5. `impact <file> <path>` — blast radius

(These `deps`/`impact` steps referenced subcommands that were also later removed.)

Output was plain text (`print_plan`) or `--json` (the full `PlanReport`).
