//! Language-agnostic symbol extraction via tree-sitter tag queries.
//!
//! Instead of a hand-written per-language `match` over AST node kinds, each
//! supported language ships a `tags.scm` query (vendored under `src/queries/`,
//! taken from the grammar's own query set). `tree-sitter-tags` runs the query
//! and reports definitions/references uniformly, so adding a language is just
//! dropping in its query file.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use tree_sitter::Language;
use tree_sitter_tags::{TagsConfiguration, TagsContext};

use crate::graph::Symbol;

fn language_and_query(name: &str) -> Option<(Language, &'static str)> {
    let (lang_fn, query) = match name {
        "rust" => (tree_sitter_rust::LANGUAGE, include_str!("queries/rust.scm")),
        "python" => (
            tree_sitter_python::LANGUAGE,
            include_str!("queries/python.scm"),
        ),
        "javascript" => (
            tree_sitter_javascript::LANGUAGE,
            include_str!("queries/javascript.scm"),
        ),
        // TypeScript inherits JavaScript's grammar, so compose both queries:
        // the JS query captures functions/classes/arrows/methods, the TS query
        // adds interfaces and modules.
        "typescript" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            concat!(
                include_str!("queries/javascript.scm"),
                "\n",
                include_str!("queries/typescript.scm"),
            ),
        ),
        "go" => (tree_sitter_go::LANGUAGE, include_str!("queries/go.scm")),
        "java" => (tree_sitter_java::LANGUAGE, include_str!("queries/java.scm")),
        "c" => (tree_sitter_c::LANGUAGE, include_str!("queries/c.scm")),
        "cpp" => (tree_sitter_cpp::LANGUAGE, include_str!("queries/cpp.scm")),
        "ruby" => (tree_sitter_ruby::LANGUAGE, include_str!("queries/ruby.scm")),
        "php" => (
            tree_sitter_php::LANGUAGE_PHP,
            include_str!("queries/php.scm"),
        ),
        "swift" => (
            tree_sitter_swift::LANGUAGE,
            include_str!("queries/swift.scm"),
        ),
        _ => return None,
    };
    Some((Language::from(lang_fn), query))
}

const LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "java",
    "c",
    "cpp",
    "ruby",
    "php",
    "swift",
];

/// Compiled tag configuration per language, built once. A language whose query
/// fails to compile is simply omitted (its files get no symbols, but still
/// index and search).
static CONFIGS: Lazy<HashMap<&'static str, TagsConfiguration>> = Lazy::new(|| {
    let mut configs = HashMap::new();
    for &lang in LANGUAGES {
        if let Some((language, query)) = language_and_query(lang) {
            match TagsConfiguration::new(language, query, "") {
                Ok(config) => {
                    configs.insert(lang, config);
                }
                Err(e) => log::warn!("tags query for {lang} failed to compile: {e}"),
            }
        }
    }
    configs
});

/// Extract the symbols this source file defines, using its tag query.
pub fn extract_symbols(source: &str, language: &str) -> Vec<Symbol> {
    let Some(config) = CONFIGS.get(language) else {
        return Vec::new();
    };

    let mut context = TagsContext::new();
    let bytes = source.as_bytes();
    let tags = match context.generate_tags(config, bytes, None) {
        Ok((tags, _)) => tags,
        Err(_) => return Vec::new(),
    };

    let mut symbols = Vec::new();
    for tag in tags.flatten() {
        if !tag.is_definition {
            continue;
        }
        let name = String::from_utf8_lossy(&bytes[tag.name_range.clone()]).into_owned();
        if name.is_empty() {
            continue;
        }
        symbols.push(Symbol {
            name,
            kind: config.syntax_type_name(tag.syntax_type_id).to_string(),
            line: tag.span.start.row + 1,
        });
    }
    symbols
}
