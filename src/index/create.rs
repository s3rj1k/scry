use std::collections::HashSet;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::index::bm25::Bm25Index;
use crate::index::chunking::chunk_source_sized;
use crate::index::encoder::{SemanticIndex, StaticEncoder};
use crate::index::file_walker::{filter_extensions, language_for_path, walk_files};
use crate::index::symbols::extract_symbols;
use crate::index::IndexParams;
use crate::types::Chunk;

fn enrich_for_bm25(chunk: &Chunk, params: &IndexParams) -> String {
    let path = Path::new(&chunk.file_path);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let dir_parts: Vec<&str> = path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| {
                    let s = c.as_os_str().to_str()?;
                    if s == "." || s == "/" {
                        None
                    } else {
                        Some(s)
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let dir_text: String = dir_parts
        .iter()
        .rev()
        .take(params.bm25_dir_parts)
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let stem_tokens = vec![stem; params.bm25_stem_repeat].join(" ");
    format!("{} {stem_tokens} {dir_text}", chunk.content)
}

pub fn create_index_from_path(
    path: &Path,
    encoder: &StaticEncoder,
    extensions: Option<&HashSet<String>>,
    ignore: Option<&HashSet<String>>,
    include_text_files: bool,
    display_root: &Path,
    params: &IndexParams,
) -> Result<(Bm25Index, SemanticIndex, Vec<Chunk>)> {
    let exts = filter_extensions(extensions, include_text_files);
    let files = walk_files(path, &exts, ignore);

    let mut chunks: Vec<Chunk> = Vec::new();

    for file_path in &files {
        let metadata = match file_path.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > params.max_file_bytes {
            continue;
        }
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let language = language_for_path(file_path);
        let chunk_path = file_path
            .strip_prefix(display_root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let mut file_chunks = chunk_source_sized(&source, &chunk_path, language, params.chunk_size);

        // Attach the AST symbols each chunk defines (tag based) so ranking can
        // detect definitions structurally.
        if let Some(lang) = language {
            let symbols = extract_symbols(&source, lang);
            for chunk in &mut file_chunks {
                chunk.symbols = symbols
                    .iter()
                    .filter(|s| s.line >= chunk.start_line && s.line <= chunk.end_line)
                    .map(|s| s.name.clone())
                    .collect();
            }
        }

        chunks.extend(file_chunks);
    }

    if chunks.is_empty() {
        bail!("No supported files found under {}", path.display());
    }

    let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let embeddings = encoder
        .encode_batch(&texts)
        .context("Failed to encode chunks")?;
    let semantic_index = SemanticIndex::new(embeddings);

    let bm25_docs: Vec<String> = chunks.iter().map(|c| enrich_for_bm25(c, params)).collect();
    let bm25_index = Bm25Index::new(&bm25_docs);

    Ok((bm25_index, semantic_index, chunks))
}
