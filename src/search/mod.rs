pub mod ranking;

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::index::bm25::Bm25Index;
use crate::index::encoder::{SemanticIndex, StaticEncoder};
use crate::search::ranking::{
    definition_list, resolve_alpha, weighted_rrf, Signal, ALPHA_NL, ALPHA_SYMBOL,
};
use crate::types::{Chunk, MatchLine, SearchResult};

/// Tunable knobs for the search flow. Defaults match the built in values.
pub struct SearchParams {
    /// Semantic weight from 0 to 1. None adapts to the query shape.
    pub alpha: Option<f64>,
    /// Adaptive weight for symbol shaped queries when alpha is None.
    pub alpha_symbol: f64,
    /// Adaptive weight for natural language queries when alpha is None.
    pub alpha_nl: f64,
    /// Reciprocal rank fusion constant.
    pub rrf_k: f64,
    /// Drop results scoring below this fraction of the top result.
    pub min_score_ratio: f64,
    /// Weight of the definition signal, relative to semantic plus lexical of 1.
    pub def_weight: f64,
    /// Candidate pool size as a multiple of top_k.
    pub candidate_multiplier: usize,
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            alpha: None,
            alpha_symbol: ALPHA_SYMBOL,
            alpha_nl: ALPHA_NL,
            rrf_k: 60.0,
            min_score_ratio: 0.12,
            def_weight: 1.0,
            candidate_multiplier: 5,
        }
    }
}

fn selector_to_mask(selector: Option<&[usize]>, size: usize) -> Option<Vec<bool>> {
    let indices = selector?;
    let mut mask = vec![false; size];
    for &idx in indices {
        if idx < size {
            mask[idx] = true;
        }
    }
    Some(mask)
}

fn find_match_lines(chunk: &Chunk, query: &str) -> Vec<MatchLine> {
    let query_lower = query.to_lowercase();
    let keywords: Vec<&str> = query_lower
        .split_whitespace()
        .filter(|w| w.len() >= 2)
        .collect();
    if keywords.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for (i, line) in chunk.content.lines().enumerate() {
        let line_lower = line.to_lowercase();
        if keywords.iter().any(|kw| line_lower.contains(kw)) {
            matches.push(MatchLine {
                line: chunk.start_line + i,
                content: line.trim().to_string(),
            });
        }
    }
    matches
}

fn filter_low_scores(results: Vec<SearchResult>, min_ratio: f64) -> Vec<SearchResult> {
    if results.len() <= 1 {
        return results;
    }
    let top_score = results[0].score;
    if top_score <= 0.0 {
        return Vec::new();
    }
    let min = top_score * min_ratio;
    results.into_iter().filter(|r| r.score >= min).collect()
}

/// BM25 ranked candidate list (chunk indices, best first), capped at `limit`.
fn bm25_ranked(
    query: &str,
    bm25_index: &Bm25Index,
    chunks: &[Chunk],
    selector: Option<&[usize]>,
    limit: usize,
) -> Vec<usize> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mask = selector_to_mask(selector, chunks.len());
    let raw = bm25_index.get_scores(query, mask.as_deref());
    let mut indexed: Vec<(usize, f64)> = raw
        .iter()
        .enumerate()
        .filter(|(_, &s)| s > 0.0)
        .map(|(i, &s)| (i, s as f64))
        .collect();
    indexed.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    indexed.truncate(limit);
    indexed.into_iter().map(|(idx, _)| idx).collect()
}

/// Chunk index and score pairs sorted by score descending, ties by index.
fn sorted_by_score(scores: &HashMap<usize, f64>) -> Vec<(usize, f64)> {
    let mut v: Vec<(usize, f64)> = scores.iter().map(|(&i, &s)| (i, s)).collect();
    v.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    v
}

pub fn search_bm25(
    query: &str,
    bm25_index: &Bm25Index,
    chunks: &[Chunk],
    top_k: usize,
    selector: Option<&[usize]>,
    params: &SearchParams,
) -> Vec<SearchResult> {
    let ranked = bm25_ranked(query, bm25_index, chunks, selector, top_k);
    let results: Vec<SearchResult> = ranked
        .into_iter()
        .enumerate()
        .map(|(rank, idx)| {
            let match_lines = find_match_lines(&chunks[idx], query);
            SearchResult {
                chunk: chunks[idx].clone(),
                // Descending pseudo score so ordering and filtering behave.
                score: 1.0 / (params.rrf_k + rank as f64 + 1.0),
                match_lines,
            }
        })
        .collect();

    filter_low_scores(results, params.min_score_ratio)
}

#[allow(clippy::too_many_arguments)]
pub fn search_hybrid(
    query: &str,
    encoder: &StaticEncoder,
    semantic_index: &SemanticIndex,
    bm25_index: &Bm25Index,
    chunks: &[Chunk],
    top_k: usize,
    params: &SearchParams,
    selector: Option<&[usize]>,
) -> Vec<SearchResult> {
    let alpha_weight = resolve_alpha(query, params.alpha, params.alpha_symbol, params.alpha_nl);
    let candidate_count = top_k * params.candidate_multiplier;

    let query_embedding = match encoder.encode_single(query) {
        Ok(e) => e,
        Err(_) => return search_bm25(query, bm25_index, chunks, top_k, selector, params),
    };

    // base ranked lists (natural rankings)
    let sem_ranked: Vec<usize> = semantic_index
        .query(&query_embedding, candidate_count, selector)
        .into_iter()
        .map(|(idx, _)| idx)
        .collect();
    let lex_ranked = bm25_ranked(query, bm25_index, chunks, selector, candidate_count);

    if sem_ranked.is_empty() && lex_ranked.is_empty() {
        return Vec::new();
    }

    // Preliminary fusion of the two base lists, used only to order the derived
    // structural signal lists.
    let base = weighted_rrf(
        &[
            Signal::new(alpha_weight, sem_ranked.clone()),
            Signal::new(1.0 - alpha_weight, lex_ranked.clone()),
        ],
        params.rrf_k,
    );
    let pool: Vec<usize> = sorted_by_score(&base).into_iter().map(|(i, _)| i).collect();

    // the one structural signal, definitions of the queried symbol
    let def_ranked = definition_list(query, chunks, &pool);

    // one weighted RRF over every signal list
    let fused = weighted_rrf(
        &[
            Signal::new(alpha_weight, sem_ranked),
            Signal::new(1.0 - alpha_weight, lex_ranked),
            Signal::new(params.def_weight, def_ranked),
        ],
        params.rrf_k,
    );

    // take the top_k by fused score
    let mut ranked = sorted_by_score(&fused);
    ranked.truncate(top_k);

    let results: Vec<SearchResult> = ranked
        .into_iter()
        .map(|(idx, score)| {
            let match_lines = find_match_lines(&chunks[idx], query);
            SearchResult {
                chunk: chunks[idx].clone(),
                score,
                match_lines,
            }
        })
        .collect();

    filter_low_scores(results, params.min_score_ratio)
}
