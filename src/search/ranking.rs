use std::collections::{HashMap, HashSet};

use once_cell::sync::Lazy;
use regex::Regex;

use crate::types::Chunk;

// weighted RRF fusion

/// A ranked list of chunk indices (best first) with a fusion weight.
pub struct Signal {
    pub weight: f64,
    pub ranked: Vec<usize>,
}

impl Signal {
    pub fn new(weight: f64, ranked: Vec<usize>) -> Self {
        Self { weight, ranked }
    }
}

/// Weighted Reciprocal Rank Fusion. Each chunk accrues weight over (k + rank)
/// from every list, so signals on different scales combine without scaling.
pub fn weighted_rrf(signals: &[Signal], k: f64) -> HashMap<usize, f64> {
    let mut fused: HashMap<usize, f64> = HashMap::new();
    for signal in signals {
        if signal.weight == 0.0 {
            continue;
        }
        for (rank, &idx) in signal.ranked.iter().enumerate() {
            *fused.entry(idx).or_insert(0.0) += signal.weight / (k + rank as f64 + 1.0);
        }
    }
    fused
}

// adaptive semantic vs lexical weight

pub const ALPHA_SYMBOL: f64 = 0.3;
pub const ALPHA_NL: f64 = 0.5;

pub fn resolve_alpha(query: &str, alpha: Option<f64>, alpha_symbol: f64, alpha_nl: f64) -> f64 {
    if let Some(a) = alpha {
        return a;
    }
    if is_symbol_query(query) {
        alpha_symbol
    } else {
        alpha_nl
    }
}

// definition signal

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

/// Symbol shaped names present in the query. A namespaced tail plus any
/// embedded camelCase identifiers.
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
/// symbols attached at index time.
fn chunk_defines_symbol(chunk: &Chunk, symbol_name: &str) -> bool {
    chunk.symbols.iter().any(|s| s == symbol_name)
}

/// Signal list of candidate chunks that define a symbol named in the query,
/// preserving the input order. Empty when the query names no symbol.
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
