use text_splitter::{CodeSplitter, TextSplitter};
use tree_sitter::Language;

use crate::types::Chunk;

const DESIRED_CHUNK_LENGTH_CHARS: usize = 1500;

fn get_language(name: &str) -> Option<Language> {
    let lang_fn = match name {
        "rust" => tree_sitter_rust::LANGUAGE,
        "python" => tree_sitter_python::LANGUAGE,
        "javascript" => tree_sitter_javascript::LANGUAGE,
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "go" => tree_sitter_go::LANGUAGE,
        "java" => tree_sitter_java::LANGUAGE,
        "c" => tree_sitter_c::LANGUAGE,
        "cpp" => tree_sitter_cpp::LANGUAGE,
        "ruby" => tree_sitter_ruby::LANGUAGE,
        "php" => tree_sitter_php::LANGUAGE_PHP,
        "swift" => tree_sitter_swift::LANGUAGE,
        _ => return None,
    };
    Some(Language::from(lang_fn))
}

fn collect_indices<'a>(it: impl Iterator<Item = (usize, &'a str)>) -> Vec<(usize, String)> {
    it.map(|(offset, text)| (offset, text.to_string()))
        .collect()
}

/// Split `source` into chunks capped near `DESIRED_CHUNK_LENGTH_CHARS`. Code
/// uses tree sitter boundaries, anything else falls back to text splitting.
pub fn chunk_source(source: &str, file_path: &str, language: Option<&str>) -> Vec<Chunk> {
    if source.trim().is_empty() {
        return Vec::new();
    }

    let indexed = language
        .and_then(get_language)
        .and_then(|lang| CodeSplitter::new(lang, DESIRED_CHUNK_LENGTH_CHARS).ok())
        .map(|splitter| collect_indices(splitter.chunk_indices(source)))
        .unwrap_or_else(|| {
            collect_indices(TextSplitter::new(DESIRED_CHUNK_LENGTH_CHARS).chunk_indices(source))
        });

    indexed
        .into_iter()
        .map(|(offset, text)| {
            let end = (offset + text.len()).min(source.len());
            let start_line = source[..offset].matches('\n').count() + 1;
            let end_line = source[..end].matches('\n').count() + 1;
            Chunk::new(
                text,
                file_path.to_string(),
                start_line,
                end_line,
                language.map(String::from),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_tree_sitter_chunking_small() {
        let source = r#"
use std::collections::HashMap;

fn foo() {
    println!("foo");
}

struct MyStruct {
    field: i32,
}
"#;
        let chunks = chunk_source(source, "test.rs", Some("rust"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("fn foo"));
        assert!(all_content.contains("struct MyStruct"));
        assert!(all_content.contains("use std::collections"));
    }

    #[test]
    fn test_rust_tree_sitter_splits_large() {
        let long_body = "    let x = 1;\n".repeat(100);
        let source = format!(
            "fn foo() {{\n{long_body}}}\n\nfn bar() {{\n{long_body}}}\n\nfn baz() {{\n{long_body}}}\n"
        );
        let chunks = chunk_source(&source, "test.rs", Some("rust"));
        assert!(
            chunks.len() >= 2,
            "large source should split: got {} chunks",
            chunks.len()
        );
    }

    #[test]
    fn test_chunks_have_valid_line_ranges() {
        let source = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let chunks = chunk_source(source, "test.rs", Some("rust"));
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(c.start_line >= 1);
            assert!(c.end_line >= c.start_line);
        }
    }

    #[test]
    fn test_python_tree_sitter_chunking() {
        let long_body = "    x = 1\n".repeat(100);
        let source =
            format!("import os\n\nclass MyClass:\n{long_body}\ndef standalone():\n{long_body}\n");
        let chunks = chunk_source(&source, "test.py", Some("python"));
        assert!(
            chunks.len() >= 2,
            "large python source should split: got {} chunks",
            chunks.len()
        );
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("class MyClass"));
        assert!(all_content.contains("def standalone"));
    }

    #[test]
    fn test_fallback_for_unknown_language() {
        let source = "line1\nline2\nline3\n";
        let chunks = chunk_source(source, "test.xyz", None);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_javascript_tree_sitter_chunking() {
        let source = r#"
const x = require('something');

function hello() {
    console.log("hello");
}

class Greeter {
    greet() {
        return "hi";
    }
}
"#;
        let chunks = chunk_source(source, "test.js", Some("javascript"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("function hello"));
        assert!(all_content.contains("class Greeter"));
    }

    #[test]
    fn test_go_tree_sitter_chunking() {
        let source = r#"
package main

import "fmt"

func main() {
    fmt.Println("hello")
}

func helper() int {
    return 42
}
"#;
        let chunks = chunk_source(source, "test.go", Some("go"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("func main"));
        assert!(all_content.contains("func helper"));
    }

    #[test]
    fn test_large_source_splits_and_preserves_content() {
        let body = "    let _x = 1;\n".repeat(80);
        let source = format!("fn a() {{\n{body}}}\n\nfn b() {{\n{body}}}\n\nfn c() {{\n{body}}}\n");
        let chunks = chunk_source(&source, "big.rs", Some("rust"));
        assert!(
            chunks.len() >= 2,
            "large source should split: got {} chunks",
            chunks.len()
        );
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("fn a"));
        assert!(all_content.contains("fn b"));
        assert!(all_content.contains("fn c"));
        // The whole file should not collapse into a single chunk.
        assert!(!chunks.iter().any(|c| c.content.contains("fn a")
            && c.content.contains("fn b")
            && c.content.contains("fn c")));
    }

    #[test]
    fn test_ruby_tree_sitter_chunking() {
        let source = r#"
class Greeter
  def initialize(name)
    @name = name
  end

  def hello
    "Hello, #{@name}!"
  end
end

module Utils
  def self.upcase(s)
    s.upcase
  end
end

def standalone
  42
end
"#;
        let chunks = chunk_source(source, "test.rb", Some("ruby"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("class Greeter"));
        assert!(all_content.contains("module Utils"));
        assert!(all_content.contains("def standalone"));
    }

    #[test]
    fn test_php_tree_sitter_chunking() {
        let source = r#"<?php
namespace App\Controller;

class UserController {
    public function index() {
        return 'list users';
    }

    public function show(int $id) {
        return "user $id";
    }
}

function helper() {
    return 1;
}
"#;
        let chunks = chunk_source(source, "test.php", Some("php"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("class UserController"));
        assert!(all_content.contains("function helper"));
    }

    #[test]
    fn test_swift_tree_sitter_chunking() {
        let source = r#"
import Foundation

struct User {
    let name: String
    let age: Int
}

class Greeter {
    func hello(to user: User) -> String {
        return "Hello, \(user.name)"
    }
}

func standalone() -> Int {
    return 42
}
"#;
        let chunks = chunk_source(source, "test.swift", Some("swift"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("struct User"));
        assert!(all_content.contains("class Greeter"));
        assert!(all_content.contains("func standalone"));
    }
}
