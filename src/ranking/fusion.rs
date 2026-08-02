use std::collections::HashMap;

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

/// Weighted Reciprocal Rank Fusion.
///
/// Each chunk accrues `weight / (k + rank + 1)` from every signal list it
/// appears in. Because it operates on rank positions, signals on different
/// score scales (BM25, cosine, boolean structural signals) combine without any
/// normalization — a chunk that ranks well across several signals wins.
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
