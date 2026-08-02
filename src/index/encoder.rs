use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use model2vec_rs::model::StaticModel;
use ndarray::{Array1, Array2, Axis};

pub struct StaticEncoder {
    model: StaticModel,
    dim: usize,
}

impl StaticEncoder {
    /// Load the embedding model named by `model`, a Hugging Face repo id or a
    /// local path resolved locally. A model absent locally is a hard error.
    pub fn load(model: &str) -> Result<Self> {
        let dir = resolve_local_model(model)?;
        let model = StaticModel::from_pretrained(&dir, None, None, None)
            .with_context(|| format!("Failed to load model from {}", dir.display()))?;
        let dim = model.encode_single("a").len();
        Ok(Self { model, dim })
    }

    pub fn embedding_dim(&self) -> usize {
        self.dim
    }

    pub fn encode_single(&self, text: &str) -> Result<Array1<f32>> {
        let v = self.model.encode_single(text);
        Ok(Array1::from_vec(v))
    }

    pub fn encode_batch(&self, texts: &[String]) -> Result<Array2<f32>> {
        if texts.is_empty() {
            return Ok(Array2::zeros((0, self.dim)));
        }
        let vecs = self.model.encode(texts);
        let n = vecs.len();
        let flat: Vec<f32> = vecs.into_iter().flatten().collect();
        Array2::from_shape_vec((n, self.dim), flat).context("Failed to reshape embeddings")
    }
}

/// Resolve `name_or_path` to a local model directory without any network access.
/// It searches a local directory, then `SCRY_MODEL_PATH`, then the HF hub cache.
fn resolve_local_model(name_or_path: &str) -> Result<PathBuf> {
    // A directory the caller points at directly wins and the loader validates it.
    if Path::new(name_or_path).is_dir() {
        return Ok(PathBuf::from(name_or_path));
    }

    let mut searched: Vec<PathBuf> = Vec::new();

    if let Some(dir) = std::env::var_os("SCRY_MODEL_PATH").map(PathBuf::from) {
        if is_model_dir(&dir) {
            return Ok(dir);
        }
        searched.push(dir);
    }

    // Hugging Face hub cache path built from the repo id and a snapshot commit.
    let hub_dir = format!("models--{}", name_or_path.replace('/', "--"));
    for root in hf_cache_roots() {
        let repo = root.join(&hub_dir);
        match newest_model_snapshot(&repo) {
            Some(dir) => return Ok(dir),
            None => searched.push(repo.join("snapshots")),
        }
    }

    let looked = searched
        .iter()
        .map(|p| format!("  {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    bail!(
        "Embedding model {name_or_path:?} not found locally, and scry never \
         downloads from the network.\nLooked in:\n{looked}\n\nFetch it into the \
         Hugging Face cache with:\n  hf download {name_or_path}\nor point at an \
         existing copy with `--model <dir>` or `SCRY_MODEL_PATH=<dir>`."
    )
}

/// A model2vec model directory has these three files side by side.
fn is_model_dir(dir: &Path) -> bool {
    dir.join("config.json").is_file()
        && dir.join("tokenizer.json").is_file()
        && dir.join("model.safetensors").is_file()
}

/// Hugging Face hub cache roots, most specific first.
fn hf_cache_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(c) = std::env::var_os("HF_HUB_CACHE") {
        roots.push(PathBuf::from(c));
    }
    if let Some(h) = std::env::var_os("HF_HOME") {
        roots.push(PathBuf::from(h).join("hub"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".cache/huggingface/hub"));
    }
    roots
}

/// Newest snapshot under a hub repo dir that looks like a model directory.
fn newest_model_snapshot(repo_dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(repo_dir.join("snapshots"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|d| is_model_dir(d))
        .max_by_key(|d| {
            std::fs::metadata(d)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        })
}

pub struct SemanticIndex {
    embeddings: Array2<f32>,
}

impl SemanticIndex {
    pub fn new(mut embeddings: Array2<f32>) -> Self {
        for mut row in embeddings.axis_iter_mut(Axis(0)) {
            let norm: f32 = row.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 1e-12 {
                row.mapv_inplace(|x| x / norm);
            }
        }
        Self { embeddings }
    }

    pub fn query(
        &self,
        query_embedding: &Array1<f32>,
        k: usize,
        selector: Option<&[usize]>,
    ) -> Vec<(usize, f32)> {
        let norm: f32 = query_embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        let query_norm = if norm > 1e-12 {
            query_embedding.mapv(|x| x / norm)
        } else {
            query_embedding.clone()
        };

        if let Some(selector) = selector {
            let mut dists: Vec<(usize, f32)> = selector
                .iter()
                .filter(|&&idx| idx < self.embeddings.nrows())
                .map(|&idx| {
                    let sim: f32 = self.embeddings.row(idx).dot(&query_norm);
                    (idx, 1.0 - sim)
                })
                .collect();
            dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            dists.truncate(k);
            dists
        } else {
            let similarities = self.embeddings.dot(&query_norm);
            let mut dists: Vec<(usize, f32)> = similarities
                .iter()
                .enumerate()
                .map(|(idx, &sim)| (idx, 1.0 - sim))
                .collect();
            if k < dists.len() {
                dists.select_nth_unstable_by(k, |a, b| {
                    a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                });
                dists.truncate(k);
            }
            dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            dists
        }
    }
}
