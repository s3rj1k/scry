use std::cmp::Ordering;
use std::collections::HashMap;

use crate::bm25::Bm25Index;
use crate::encoder::{SemanticIndex, StaticEncoder};
use crate::ranking::{
    definition_list, path_affinity_list, rerank_topk, resolve_alpha, weighted_rrf, Signal,
};
use crate::types::{Chunk, MatchLine, SearchResult};

const RRF_K: f64 = 60.0;
const MIN_SCORE_RATIO: f64 = 0.12;

// Fusion weights for the structural signal lists, relative to the combined
// semantic + lexical weight of 1.0 (alpha + (1 - alpha)). Defining the queried
// symbol is the strongest signal; path affinity is a moderate one.
const W_DEFINITION: f64 = 1.0;
const W_PATH: f64 = 0.5;

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

fn filter_low_scores(results: Vec<SearchResult>) -> Vec<SearchResult> {
    if results.len() <= 1 {
        return results;
    }
    let top_score = results[0].score;
    if top_score <= 0.0 {
        return Vec::new();
    }
    let min = top_score * MIN_SCORE_RATIO;
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

/// Chunk indices ordered by their fused base score (descending), ties by index.
fn order_by_score(scores: &HashMap<usize, f64>) -> Vec<usize> {
    let mut v: Vec<(usize, f64)> = scores.iter().map(|(&i, &s)| (i, s)).collect();
    v.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    v.into_iter().map(|(idx, _)| idx).collect()
}

pub fn search_bm25(
    query: &str,
    bm25_index: &Bm25Index,
    chunks: &[Chunk],
    top_k: usize,
    selector: Option<&[usize]>,
) -> Vec<SearchResult> {
    let ranked = bm25_ranked(query, bm25_index, chunks, selector, top_k);
    let results: Vec<SearchResult> = ranked
        .into_iter()
        .enumerate()
        .map(|(rank, idx)| {
            let match_lines = find_match_lines(&chunks[idx], query);
            SearchResult {
                chunk: chunks[idx].clone(),
                // Descending pseudo-score so ordering/filtering behave.
                score: 1.0 / (RRF_K + rank as f64 + 1.0),
                match_lines,
            }
        })
        .collect();

    filter_low_scores(results)
}

#[allow(clippy::too_many_arguments)]
pub fn search_hybrid(
    query: &str,
    encoder: &StaticEncoder,
    semantic_index: &SemanticIndex,
    bm25_index: &Bm25Index,
    chunks: &[Chunk],
    top_k: usize,
    alpha: Option<f64>,
    selector: Option<&[usize]>,
) -> Vec<SearchResult> {
    let alpha_weight = resolve_alpha(query, alpha);
    let candidate_count = top_k * 5;

    let query_embedding = match encoder.encode_single(query) {
        Ok(e) => e,
        Err(_) => return search_bm25(query, bm25_index, chunks, top_k, selector),
    };

    // --- base ranked lists (natural rankings) ---
    let sem_ranked: Vec<usize> = semantic_index
        .query(&query_embedding, candidate_count, selector)
        .into_iter()
        .map(|(idx, _)| idx)
        .collect();
    let lex_ranked = bm25_ranked(query, bm25_index, chunks, selector, candidate_count);

    if sem_ranked.is_empty() && lex_ranked.is_empty() {
        return Vec::new();
    }

    // Preliminary fusion of the two base lists — used only to order the derived
    // structural signal lists.
    let base = weighted_rrf(
        &[
            Signal::new(alpha_weight, sem_ranked.clone()),
            Signal::new(1.0 - alpha_weight, lex_ranked.clone()),
        ],
        RRF_K,
    );
    let pool = order_by_score(&base);

    // --- structural signal lists ---
    let def_ranked = definition_list(query, chunks, &pool);
    let path_ranked = path_affinity_list(query, chunks, &pool);

    // --- one weighted RRF over every signal list ---
    let fused = weighted_rrf(
        &[
            Signal::new(alpha_weight, sem_ranked),
            Signal::new(1.0 - alpha_weight, lex_ranked),
            Signal::new(W_DEFINITION, def_ranked),
            Signal::new(W_PATH, path_ranked),
        ],
        RRF_K,
    );

    // --- path-noise penalties + per-file diversity + top_k ---
    let ranked = rerank_topk(&fused, chunks, top_k, alpha_weight < 1.0);

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

    filter_low_scores(results)
}
