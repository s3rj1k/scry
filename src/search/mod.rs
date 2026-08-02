pub mod ranking;

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::index::encoder::{SemanticIndex, StaticEncoder};
use crate::search::ranking::{definition_list, weighted_rrf, Signal};
use crate::types::{Chunk, MatchLine, SearchResult};

/// Tunable knobs for the search flow. Defaults match the built in values.
pub struct SearchParams {
    /// Reciprocal rank fusion constant.
    pub rrf_k: f64,
    /// Drop results scoring below this fraction of the top result.
    pub min_score_ratio: f64,
    /// Weight of the definition signal, relative to the semantic signal of 1.
    pub def_weight: f64,
    /// Candidate pool size as a multiple of `top_k`.
    pub candidate_multiplier: usize,
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            rrf_k: 60.0,
            min_score_ratio: 0.12,
            def_weight: 1.0,
            candidate_multiplier: 5,
        }
    }
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

/// Rank chunks by semantic similarity, boosted by a structural definition
/// signal, and fuse the two with weighted reciprocal rank fusion.
pub fn search_semantic(
    query: &str,
    encoder: &StaticEncoder,
    semantic_index: &SemanticIndex,
    chunks: &[Chunk],
    top_k: usize,
    params: &SearchParams,
    selector: Option<&[usize]>,
) -> Vec<SearchResult> {
    let candidate_count = top_k * params.candidate_multiplier;

    let Ok(query_embedding) = encoder.encode_single(query) else {
        return Vec::new();
    };

    let sem_ranked: Vec<usize> = semantic_index
        .query(&query_embedding, candidate_count, selector)
        .into_iter()
        .map(|(idx, _)| idx)
        .collect();
    if sem_ranked.is_empty() {
        return Vec::new();
    }

    // Structural boost, ordered by the semantic ranking it draws from.
    let def_ranked = definition_list(query, chunks, &sem_ranked);

    let fused = weighted_rrf(
        &[
            Signal::new(1.0, sem_ranked),
            Signal::new(params.def_weight, def_ranked),
        ],
        params.rrf_k,
    );

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
