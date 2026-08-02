//! Language agnostic symbol extraction via tree sitter tag queries. Each
//! language ships a tags.scm query, run uniformly across all languages.

use std::collections::HashMap;

use std::sync::LazyLock;

use tree_sitter::Language;
use tree_sitter_tags::{TagsConfiguration, TagsContext};

/// A symbol definition extracted from a source file.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
}

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
        // TypeScript inherits JavaScript's grammar so compose both queries. The
        // JS query captures functions and classes, the TS query adds interfaces.
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
/// fails to compile is omitted and its files simply get no symbols.
static CONFIGS: LazyLock<HashMap<&'static str, TagsConfiguration>> = LazyLock::new(|| {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn names(source: &str, lang: &str) -> Vec<String> {
        extract_symbols(source, lang)
            .into_iter()
            .map(|s| s.name)
            .collect()
    }

    #[test]
    fn rust_symbols() {
        let src = "pub fn search(q: &str) {}\nstruct Index {}\n";
        let syms = extract_symbols(src, "rust");
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"search"));
        assert!(names.contains(&"Index"));
        // tags map all Rust ADTs to the "class" kind.
        let index = syms.iter().find(|s| s.name == "Index").unwrap();
        assert_eq!(index.kind, "class");
    }

    #[test]
    fn python_captures_nested_methods() {
        let src = "class FileWalker:\n    def walk(self):\n        pass\n\ndef main():\n    pass\n";
        let n = names(src, "python");
        assert!(n.contains(&"FileWalker".to_string()));
        assert!(n.contains(&"walk".to_string())); // nested method is captured
        assert!(n.contains(&"main".to_string()));
    }

    #[test]
    fn typescript_composes_javascript_query() {
        let src = "export async function getUser() {}\nexport const createPage = async () => {};\nexport interface UserProfile {}\n";
        let n = names(src, "typescript");
        assert!(n.contains(&"getUser".to_string()));
        assert!(n.contains(&"createPage".to_string()));
        assert!(n.contains(&"UserProfile".to_string()));
    }

    #[test]
    fn go_symbols() {
        let src = "package main\n\nfunc main() {}\n\nfunc helper() int { return 42 }\n";
        let n = names(src, "go");
        assert!(n.contains(&"main".to_string()));
        assert!(n.contains(&"helper".to_string()));
    }

    #[test]
    fn unsupported_language_yields_nothing() {
        assert!(extract_symbols("class Foo", "kotlin").is_empty());
    }
}
