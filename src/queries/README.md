# Vendored tree-sitter tag queries

These `<language>.scm` files are **tag queries** consumed by
[`tree-sitter-tags`](https://docs.rs/tree-sitter-tags) to extract symbol
definitions (see `src/symbols.rs`). Each file is copied **verbatim** from the
`queries/tags.scm` that ships inside the corresponding tree-sitter grammar
crate, at the version semble depends on. They remain under their upstream
grammar's license (MIT).

They are vendored (rather than read from the grammar crate at runtime) because a
crate's source query files are not exposed as constants or included in the
compiled artifact.

## Provenance

Copied from `~/.cargo/registry/src/*/tree-sitter-<lang>-<version>/queries/tags.scm`:

| File | Source crate | Version |
| --- | --- | --- |
| `rust.scm` | tree-sitter-rust | 0.24.2 |
| `python.scm` | tree-sitter-python | 0.23.6 |
| `javascript.scm` | tree-sitter-javascript | 0.23.1 |
| `typescript.scm` | tree-sitter-typescript | 0.23.2 |
| `go.scm` | tree-sitter-go | 0.23.4 |
| `java.scm` | tree-sitter-java | 0.23.5 |
| `c.scm` | tree-sitter-c | 0.23.4 |
| `cpp.scm` | tree-sitter-cpp | 0.23.4 |
| `ruby.scm` | tree-sitter-ruby | 0.23.1 |
| `php.scm` | tree-sitter-php | 0.23.11 |
| `swift.scm` | tree-sitter-swift | 0.7.2 |

To re-sync after bumping a grammar, re-copy that grammar's `queries/tags.scm`
over the matching file here.

## Notes

- **TypeScript** composes `javascript.scm` + `typescript.scm` at load time (in
  `src/symbols.rs`): the TS grammar inherits JavaScript, and its own query only
  covers signatures/interfaces, so the JS query supplies functions/classes/
  arrow-functions/methods.
- These queries define the symbol vocabulary (`@definition.function`,
  `@definition.class`, `@definition.method`, `@definition.interface`, ...).
  Coverage is whatever upstream defines — e.g. TypeScript `type` aliases and
  Rust `struct`-vs-`class` granularity follow the query, not semble.
