use std::collections::{HashMap, HashSet};
use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::tokens::split_identifier;
use crate::types::Chunk;

static SYMBOL_QUERY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"^(?:",
        r"[A-Za-z_][A-Za-z0-9_]*(?:(?:::|\\|->|\.)[A-Za-z_][A-Za-z0-9_]*)+",
        r"|_[A-Za-z0-9_]*",
        r"|[A-Za-z][A-Za-z0-9]*[A-Z_][A-Za-z0-9_]*",
        r"|[A-Z][A-Za-z0-9]*",
        r")$",
    ))
    .unwrap()
});

static EMBEDDED_SYMBOL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"\b(?:",
        r"[A-Z][a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]*",
        r"|[a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]+",
        r")\b",
    ))
    .unwrap()
});

static WORD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[a-zA-Z_][a-zA-Z0-9_]*").unwrap());

static STOPWORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    "a an and are as at be by do does for from has have how if in is it not of on or the to was \
     what when where which who why with"
        .split_whitespace()
        .collect()
});

const PATH_MATCH_MIN_RATIO: f64 = 0.10;

pub fn is_symbol_query(query: &str) -> bool {
    SYMBOL_QUERY_RE.is_match(query.trim())
}

fn extract_symbol_name(query: &str) -> &str {
    let q = query.trim();
    for sep in &["::", "\\", "->", "."] {
        if let Some(pos) = q.rfind(sep) {
            return &q[pos + sep.len()..];
        }
    }
    q
}

/// Symbol-shaped names present in the query: a namespaced tail (`Foo::bar` →
/// `bar`, `Foo::bar`) plus any embedded camelCase identifiers.
fn query_symbol_names(query: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let trimmed = query.trim();
    if is_symbol_query(trimmed) {
        names.insert(extract_symbol_name(trimmed).to_string());
        names.insert(trimmed.to_string());
    }
    for m in EMBEDDED_SYMBOL_RE.find_iter(query) {
        names.insert(m.as_str().to_string());
    }
    names
}

/// Whether this chunk defines a symbol with the given name, using the AST
/// symbols attached at index time (see `index::create`).
fn chunk_defines_symbol(chunk: &Chunk, symbol_name: &str) -> bool {
    chunk.symbols.iter().any(|s| s == symbol_name)
}

/// Signal list: candidate chunks that *define* a symbol named in the query,
/// preserving the input (base-ranked) order. Empty when the query names no
/// symbol.
pub fn definition_list(query: &str, chunks: &[Chunk], pool: &[usize]) -> Vec<usize> {
    let names = query_symbol_names(query);
    if names.is_empty() {
        return Vec::new();
    }
    pool.iter()
        .copied()
        .filter(|&idx| names.iter().any(|n| chunk_defines_symbol(&chunks[idx], n)))
        .collect()
}

fn extract_keywords(query: &str) -> HashSet<String> {
    WORD_RE
        .find_iter(query)
        .map(|m| m.as_str().to_lowercase())
        .filter(|w| w.len() > 2 && !STOPWORDS.contains(w.as_str()))
        .collect()
}

/// Identifier stems drawn from a file's stem + parent directory name.
fn path_parts(file_path: &str) -> HashSet<String> {
    let path = Path::new(file_path);
    let mut parts: HashSet<String> =
        split_identifier(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""))
            .into_iter()
            .collect();
    if let Some(parent) = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
    {
        if parent != "." && parent != "/" && parent != ".." {
            parts.extend(split_identifier(parent));
        }
    }
    parts
}

fn count_keyword_matches(keywords: &HashSet<String>, parts: &HashSet<String>) -> usize {
    let exact: HashSet<&String> = keywords.intersection(parts).collect();
    if exact.len() == keywords.len() {
        return exact.len();
    }
    let mut n = exact.len();
    for kw in keywords {
        if exact.contains(kw) {
            continue;
        }
        for part in parts {
            let (shorter, longer) = if kw.len() <= part.len() {
                (kw.as_str(), part.as_str())
            } else {
                (part.as_str(), kw.as_str())
            };
            if shorter.len() >= 3 && longer.starts_with(shorter) {
                n += 1;
                break;
            }
        }
    }
    n
}

/// Signal list: candidate chunks whose file path (stem + parent dir) matches
/// query keywords, ordered by match strength then base order.
pub fn path_affinity_list(query: &str, chunks: &[Chunk], pool: &[usize]) -> Vec<usize> {
    let keywords = extract_keywords(query);
    if keywords.is_empty() {
        return Vec::new();
    }
    let mut cache: HashMap<String, HashSet<String>> = HashMap::new();
    // (match_count, base_position, idx) — higher match_count first, base order ties.
    let mut scored: Vec<(usize, usize, usize)> = Vec::new();
    for (pos, &idx) in pool.iter().enumerate() {
        let fp = chunks[idx].file_path.clone();
        let parts = cache.entry(fp.clone()).or_insert_with(|| path_parts(&fp));
        let n = count_keyword_matches(&keywords, parts);
        if n > 0 && (n as f64 / keywords.len() as f64) >= PATH_MATCH_MIN_RATIO {
            scored.push((n, pos, idx));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, idx)| idx).collect()
}
