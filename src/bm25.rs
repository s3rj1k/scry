use bm25::{Embedder, EmbedderBuilder, Scorer, Tokenizer};

use crate::tokens::tokenize;

/// Adapts semble's code-aware tokenizer (camelCase / snake_case splitting) to
/// the `bm25` crate, so BM25 indexes the same tokens as the rest of the engine.
#[derive(Default, Clone)]
struct SembleTokenizer;

impl Tokenizer for SembleTokenizer {
    fn tokenize(&self, input_text: &str) -> Vec<String> {
        tokenize(input_text)
    }
}

/// In-memory BM25 index backed by the `bm25` crate. Document ids are chunk
/// indices, so scores align with the semantic index and the chunk array.
pub struct Bm25Index {
    embedder: Embedder<u32, SembleTokenizer>,
    scorer: Scorer<usize, u32>,
    num_docs: usize,
}

impl Bm25Index {
    /// Build the index from raw (untokenized) document texts; the embedder
    /// tokenizes them via [`SembleTokenizer`].
    pub fn new(documents: &[String]) -> Self {
        let avgdl = if documents.is_empty() {
            1.0
        } else {
            let total: usize = documents.iter().map(|d| tokenize(d).len()).sum();
            (total as f32 / documents.len() as f32).max(1.0)
        };

        let embedder = EmbedderBuilder::<u32, SembleTokenizer>::with_avgdl(avgdl)
            .tokenizer(SembleTokenizer)
            .build();

        let mut scorer = Scorer::<usize, u32>::new();
        for (doc_id, doc) in documents.iter().enumerate() {
            scorer.upsert(&doc_id, embedder.embed(doc.as_str()));
        }

        Self {
            embedder,
            scorer,
            num_docs: documents.len(),
        }
    }

    /// Dense BM25 scores indexed by document id (chunk index). `weight_mask`,
    /// when set, restricts scoring to a subset of documents (language / path
    /// filter); masked-out documents keep a score of 0.
    pub fn get_scores(&self, query: &str, weight_mask: Option<&[bool]>) -> Vec<f32> {
        let mut scores = vec![0.0f32; self.num_docs];
        if query.trim().is_empty() {
            return scores;
        }
        let query_embedding = self.embedder.embed(query);
        for scored in self.scorer.matches(&query_embedding) {
            let id = scored.id;
            if let Some(mask) = weight_mask {
                if id >= mask.len() || !mask[id] {
                    continue;
                }
            }
            if id < scores.len() {
                scores[id] = scored.score;
            }
        }
        scores
    }

    pub fn num_docs(&self) -> usize {
        self.num_docs
    }
}
