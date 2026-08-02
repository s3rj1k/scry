pub mod bm25;
pub mod chunking;
pub mod create;
pub mod encoder;
pub mod file_walker;
pub mod symbols;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::index::bm25::Bm25Index;
use crate::index::encoder::{SemanticIndex, StaticEncoder};
use crate::search::{search_hybrid, SearchParams};
use crate::types::{Chunk, IndexStats, SearchResult};
use create::create_index_from_path;

use std::collections::HashSet;

/// Tunable knobs for building the index. Defaults match the built in values.
pub struct IndexParams {
    /// Target chunk size in characters.
    pub chunk_size: usize,
    /// Skip files larger than this many bytes.
    pub max_file_bytes: u64,
    /// Repeat the file stem this many times so names weigh above body tokens.
    pub bm25_stem_repeat: usize,
    /// Keep at most this many trailing directory names as extra BM25 tokens.
    pub bm25_dir_parts: usize,
}

impl Default for IndexParams {
    fn default() -> Self {
        Self {
            chunk_size: chunking::DESIRED_CHUNK_LENGTH_CHARS,
            max_file_bytes: 1_000_000,
            bm25_stem_repeat: 2,
            bm25_dir_parts: 3,
        }
    }
}

pub struct ScryIndex {
    encoder: StaticEncoder,
    bm25_index: Bm25Index,
    semantic_index: SemanticIndex,
    chunks: Vec<Chunk>,
    #[allow(dead_code)]
    root: Option<PathBuf>,
    file_mapping: HashMap<String, Vec<usize>>,
    language_mapping: HashMap<String, Vec<usize>>,
}

impl ScryIndex {
    pub fn from_path(
        path: impl AsRef<Path>,
        encoder: StaticEncoder,
        extensions: Option<&HashSet<String>>,
        ignore: Option<&HashSet<String>>,
        include_text_files: bool,
        index_params: &IndexParams,
    ) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            bail!("Path does not exist: {}", path.display());
        }
        if !path.is_dir() {
            bail!("Path is not a directory: {}", path.display());
        }
        let path = path.canonicalize().context("Failed to resolve path")?;

        let (bm25_index, semantic_index, chunks) = create_index_from_path(
            &path,
            &encoder,
            extensions,
            ignore,
            include_text_files,
            &path,
            index_params,
        )?;

        let (file_mapping, language_mapping) = build_mappings(&chunks);

        Ok(Self {
            encoder,
            bm25_index,
            semantic_index,
            chunks,
            root: Some(path),
            file_mapping,
            language_mapping,
        })
    }

    pub fn search(
        &self,
        query: &str,
        top_k: usize,
        params: &SearchParams,
        filter_languages: Option<&[String]>,
        filter_paths: Option<&[String]>,
    ) -> Vec<SearchResult> {
        if self.chunks.is_empty() || query.trim().is_empty() {
            return Vec::new();
        }

        let selector = self.get_selector(filter_languages, filter_paths);
        let selector_ref = selector.as_deref();

        search_hybrid(
            query,
            &self.encoder,
            &self.semantic_index,
            &self.bm25_index,
            &self.chunks,
            top_k,
            params,
            selector_ref,
        )
    }

    pub fn stats(&self) -> IndexStats {
        let mut language_counts: HashMap<String, usize> = HashMap::new();
        for chunk in &self.chunks {
            if let Some(lang) = &chunk.language {
                *language_counts.entry(lang.clone()).or_default() += 1;
            }
        }
        IndexStats {
            indexed_files: self.file_mapping.len(),
            total_chunks: self.chunks.len(),
            languages: language_counts,
        }
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    fn get_selector(
        &self,
        filter_languages: Option<&[String]>,
        filter_paths: Option<&[String]>,
    ) -> Option<Vec<usize>> {
        let mut indices = Vec::new();
        if let Some(langs) = filter_languages {
            for lang in langs {
                if let Some(ids) = self.language_mapping.get(lang) {
                    indices.extend(ids);
                }
            }
        }
        if let Some(paths) = filter_paths {
            for path in paths {
                if let Some(ids) = self.file_mapping.get(path) {
                    indices.extend(ids);
                }
            }
        }
        if indices.is_empty() {
            None
        } else {
            indices.sort();
            indices.dedup();
            Some(indices)
        }
    }
}

fn build_mappings(chunks: &[Chunk]) -> (HashMap<String, Vec<usize>>, HashMap<String, Vec<usize>>) {
    let mut file_mapping: HashMap<String, Vec<usize>> = HashMap::new();
    let mut language_mapping: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, chunk) in chunks.iter().enumerate() {
        file_mapping
            .entry(chunk.file_path.clone())
            .or_default()
            .push(i);
        if let Some(lang) = &chunk.language {
            language_mapping.entry(lang.clone()).or_default().push(i);
        }
    }
    (file_mapping, language_mapping)
}
