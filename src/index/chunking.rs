use text_splitter::{CodeSplitter, TextSplitter};
use tree_sitter::{Language, Node, Parser};

use crate::types::Chunk;

pub const DESIRED_CHUNK_LENGTH_CHARS: usize = 1500;

/// Recursion cap for descending through nested definitions such as file into
/// type into method. A guard against pathological trees, not a hard limit.
const MAX_DEPTH: usize = 6;

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

/// Character split a slice, code aware when a grammar is available and plain
/// text otherwise. A fallback for oversized atomic leaves and unparsable files.
fn char_split(slice: &str, lang: Option<&Language>, budget: usize) -> Vec<(usize, usize)> {
    let coded = lang
        .cloned()
        .and_then(|l| CodeSplitter::new(l, budget).ok())
        .map(|sp| sp.chunk_indices(slice).map(|(o, t)| (o, t.len())).collect());
    coded.unwrap_or_else(|| {
        TextSplitter::new(budget)
            .chunk_indices(slice)
            .map(|(o, t)| (o, t.len()))
            .collect()
    })
}

/// Line number, counting from one, that holds byte `offset`. Counts on raw
/// bytes so it never slices a multi byte character.
fn line_at(source: &str, offset: usize) -> usize {
    let end = offset.min(source.len());
    source.as_bytes()[..end].iter().filter(|&&b| b == b'\n').count() + 1
}

fn node_size(n: Node) -> usize {
    n.end_byte() - n.start_byte()
}

fn is_comment(n: Node) -> bool {
    n.kind().contains("comment")
}

/// Whether `text` is a short prose doc comment worth gluing to the definition
/// below. At most two lines and free of colon, semicolon and dash punctuation.
fn is_doc_comment_text(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.lines().count() > 2 {
        return false;
    }
    if t.contains([':', ';', '-']) {
        return false;
    }
    t.lines().all(|line| {
        let l = line.trim();
        l.is_empty()
            || l.starts_with("//")
            || l.starts_with('#')
            || l.starts_with("/*")
            || l.starts_with('*')
            || l.starts_with("\"\"\"")
            || l.starts_with("'''")
    })
}

/// A tree sitter comment node that qualifies as an attachable doc comment.
fn is_doc_comment(node: Node, source: &str) -> bool {
    is_comment(node) && is_doc_comment_text(&source[node.start_byte()..node.end_byte()])
}

/// Split `source` into chunks near the default target length. Code uses tree
/// sitter boundaries, anything else falls back to text splitting.
pub fn chunk_source(source: &str, file_path: &str, language: Option<&str>) -> Vec<Chunk> {
    chunk_source_sized(source, file_path, language, DESIRED_CHUNK_LENGTH_CHARS)
}

/// Split `source` into chunks capped near `chunk_size` characters. Known
/// languages align to whole syntactic units, others fall back to text splitting.
pub fn chunk_source_sized(
    source: &str,
    file_path: &str,
    language: Option<&str>,
    chunk_size: usize,
) -> Vec<Chunk> {
    if source.trim().is_empty() {
        return Vec::new();
    }

    let lang = language.and_then(get_language);
    let indexed: Vec<(usize, String)> = match lang
        .as_ref()
        .and_then(|l| structural_spans(source, l, chunk_size))
    {
        Some(spans) => spans
            .into_iter()
            .map(|(start, end)| (start, source[start..end].to_string()))
            .collect(),
        None => char_split(source, lang.as_ref(), chunk_size)
            .into_iter()
            .map(|(off, len)| (off, source[off..(off + len).min(source.len())].to_string()))
            .collect(),
    };

    indexed
        .into_iter()
        .filter(|(_, text)| !text.trim().is_empty())
        .map(|(offset, text)| {
            let end = (offset + text.len()).min(source.len());
            let start_line = line_at(source, offset);
            // End at the last real byte so a trailing newline does not push the
            // range onto the following line.
            let end_line = line_at(source, end.saturating_sub(1).max(offset));
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

/// Byte ranges of structure aligned chunks for a supported language. Returns
/// None when the parse yields no structure and the caller falls back.
fn structural_spans(source: &str, lang: &Language, budget: usize) -> Option<Vec<(usize, usize)>> {
    let mut parser = Parser::new();
    parser.set_language(lang).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    if root.named_child_count() == 0 {
        return None;
    }

    let mut spans = Vec::new();
    chunk_node(root, source, budget, 0, lang, &mut spans);
    if spans.is_empty() {
        return None;
    }
    Some(coalesce(spans, source, budget))
}

/// Greedily pack a node's named children into chunks under `budget`, descending
/// into oversized children and keeping leading doc comments with their unit.
fn chunk_node(
    node: Node,
    source: &str,
    budget: usize,
    depth: usize,
    lang: &Language,
    out: &mut Vec<(usize, usize)>,
) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.named_children(&mut cursor).collect();
    if children.is_empty() {
        push_leaf(node.start_byte(), node.end_byte(), source, budget, lang, out);
        return;
    }

    // Keep a single unit whole through minor overflow, up to 1.5x the budget.
    // Only genuinely oversized nodes are descended into members or char split.
    let cap = budget + budget / 2;
    let start_len = out.len();
    let mut buf: Vec<Node> = Vec::new();
    let mut bufsz = 0usize;

    for child in children {
        let csz = node_size(child);
        if csz > cap {
            // A single child larger than the cap. Flush what we have, keep its
            // doc comment, then descend it (a type or class) or split it.
            let leading = peel_comments(&mut buf, child, source);
            flush(&mut buf, &mut bufsz, out);
            let region_start = leading
                .first()
                .map(|c| c.start_byte())
                .unwrap_or_else(|| child.start_byte());
            if depth < MAX_DEPTH && child.named_child_count() >= 2 {
                let before = out.len();
                chunk_node(child, source, budget, depth + 1, lang, out);
                match out.get_mut(before) {
                    // Fold the leading comment into the child's first chunk.
                    Some(first) => first.0 = first.0.min(region_start),
                    None => push_leaf(region_start, child.end_byte(), source, budget, lang, out),
                }
            } else {
                push_leaf(region_start, child.end_byte(), source, budget, lang, out);
            }
        } else if !buf.is_empty() && bufsz + csz > budget {
            // Adding this child overflows, so carry any doc comment with it.
            let leading = peel_comments(&mut buf, child, source);
            flush(&mut buf, &mut bufsz, out);
            for c in &leading {
                bufsz += node_size(*c);
            }
            buf.extend(leading);
            buf.push(child);
            bufsz += csz;
        } else {
            buf.push(child);
            bufsz += csz;
        }
    }
    flush(&mut buf, &mut bufsz, out);

    // Extend this node's own output to cover its header (e.g. `impl Foo {`) and
    // closing token, so descending never strands the enclosing signature.
    if out.len() > start_len {
        out[start_len].0 = out[start_len].0.min(node.start_byte());
        let last = out.len() - 1;
        out[last].1 = out[last].1.max(node.end_byte());
    }
}

fn flush(buf: &mut Vec<Node>, bufsz: &mut usize, out: &mut Vec<(usize, usize)>) {
    if let (Some(first), Some(last)) = (buf.first(), buf.last()) {
        out.push((first.start_byte(), last.end_byte()));
    }
    buf.clear();
    *bufsz = 0;
}

/// Pop trailing doc comment nodes off `buf` that sit directly above `next`,
/// returning them in source order so they can lead the chunk `next` starts.
fn peel_comments<'a>(buf: &mut Vec<Node<'a>>, next: Node<'a>, source: &str) -> Vec<Node<'a>> {
    let mut leading: Vec<Node<'a>> = Vec::new();
    while let Some(&last) = buf.last() {
        if !is_doc_comment(last, source) {
            break;
        }
        let follower = leading.first().copied().unwrap_or(next);
        let gap = follower
            .start_position()
            .row
            .saturating_sub(last.end_position().row);
        if gap <= 1 {
            leading.insert(0, last);
            buf.pop();
        } else {
            break;
        }
    }
    leading
}

/// Emit `[start, end)` as one chunk, or char split it (code aware when possible)
/// when it exceeds `budget`, for an atomic oversized leaf with no inner boundary.
fn push_leaf(
    start: usize,
    end: usize,
    source: &str,
    budget: usize,
    lang: &Language,
    out: &mut Vec<(usize, usize)>,
) {
    if end <= start {
        return;
    }
    if end - start <= budget {
        out.push((start, end));
        return;
    }
    let mut any = false;
    for (off, len) in char_split(&source[start..end], Some(lang), budget) {
        let s = start + off;
        let e = (s + len).min(end);
        if e > s {
            out.push((s, e));
            any = true;
        }
    }
    if !any {
        out.push((start, end));
    }
}

/// Merge undersized spans and split doc comments into a neighbour so no chunk
/// is a lone brace, a stray signature or an orphan. Merges stay within the cap.
fn coalesce(spans: Vec<(usize, usize)>, source: &str, budget: usize) -> Vec<(usize, usize)> {
    let min_size = budget / 5;
    let cap = budget + budget / 2;
    let mut result: Vec<(usize, usize)> = Vec::new();
    for span in spans {
        if let Some(&prev) = result.last() {
            let merged = (prev.0, span.1);
            let small = (prev.1 - prev.0) < min_size || (span.1 - span.0) < min_size;
            let comment = is_doc_comment_text(&source[prev.0..prev.1]);
            if (small || comment) && merged.1 - merged.0 <= cap {
                *result.last_mut().unwrap() = merged;
                continue;
            }
        }
        result.push(span);
    }
    result
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
