use std::process;

use clap::{Parser, Subcommand};

use semble::format::{format_results, resolve_chunk};
use semble::index::encoder::StaticEncoder;
use semble::index::SembleIndex;
use semble::types::SearchResult;

#[derive(Parser)]
#[command(
    name = "semble_rs",
    version,
    about = "Fast and Accurate Code Search for Agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search a codebase with keyword/symbol query
    Search {
        /// Query text, or a file:line location when --related is set
        query: String,
        /// Local path, defaults to the current directory
        #[arg(default_value = ".")]
        path: String,
        /// Number of results
        #[arg(short = 'k', long = "top-k", default_value = "10")]
        top_k: usize,
        /// Also index non code text files like .md, .yaml, .json
        #[arg(long)]
        include_text_files: bool,
        /// Semantic weight from 0.0 (lexical only) to 1.0 (semantic only).
        /// Omitted, it adapts to the query shape.
        #[arg(long)]
        alpha: Option<f64>,
        /// Treat the query as a file:line location and return similar chunks
        #[arg(long)]
        related: bool,
        /// Output as JSON (for agent/tool integration)
        #[arg(long)]
        json: bool,
        /// Compact output with file paths, scores, and match lines only
        #[arg(long)]
        compact: bool,
        /// Group results by directory + cap match lines at 3 per chunk
        #[arg(long)]
        group: bool,
        /// Embedding model (HF repo id or local path).
        /// Overrides SEMBLE_MODEL_PATH and the default embedding model.
        #[arg(long)]
        model: Option<String>,
    },
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Search {
            query,
            path,
            top_k,
            include_text_files,
            alpha,
            related,
            json,
            compact,
            group,
            model,
        } => {
            if let Some(a) = alpha {
                if !(0.0..=1.0).contains(&a) {
                    eprintln!("--alpha must be between 0.0 and 1.0, got {a}.");
                    process::exit(1);
                }
            }
            let index = build_index(&path, include_text_files, model.as_deref());

            let results = if related {
                let (file_path, line) = parse_location(&query).unwrap_or_else(|| {
                    eprintln!("--related expects a file:line location, got {query:?}.");
                    process::exit(1);
                });
                let chunk = resolve_chunk(index.chunks(), file_path, line)
                    .unwrap_or_else(|| {
                        eprintln!("No chunk found at {query}.");
                        process::exit(1);
                    })
                    .clone();
                index.find_related(&chunk, top_k)
            } else {
                index.search(query.as_str(), top_k, alpha, None, None)
            };

            if group {
                print_grouped(&results);
            } else if compact {
                print_compact(&results);
            } else if json {
                print_json(&results);
            } else if results.is_empty() {
                println!("No results found.");
            } else {
                let header = if related {
                    format!("Chunks related to {query}")
                } else {
                    format!("Search results for: {query:?}")
                };
                println!("{}", format_results(&header, &results));
            }
        }
    }
}

/// Parse a `file:line` location, tolerating a trailing line range like `10-20`.
fn parse_location(loc: &str) -> Option<(&str, usize)> {
    let (file, rest) = loc.rsplit_once(':')?;
    let line: usize = rest.split('-').next()?.trim().parse().ok()?;
    Some((file, line))
}

fn print_compact(results: &[SearchResult]) {
    for r in results {
        println!(
            "{:.4}\t{}:{}-{}",
            r.score, r.chunk.file_path, r.chunk.start_line, r.chunk.end_line
        );
        for ml in &r.match_lines {
            println!("  L{}:\t{}", ml.line, truncate_line(&ml.content, 120));
        }
    }
}

fn print_grouped(results: &[SearchResult]) {
    use std::collections::BTreeMap;
    let mut by_dir: BTreeMap<String, (f64, Vec<&SearchResult>)> = BTreeMap::new();
    for r in results {
        let dir = std::path::Path::new(&r.chunk.file_path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();
        let entry = by_dir.entry(dir).or_insert((f64::NEG_INFINITY, Vec::new()));
        if r.score > entry.0 {
            entry.0 = r.score;
        }
        entry.1.push(r);
    }
    let mut dirs: Vec<(&String, &(f64, Vec<&SearchResult>))> = by_dir.iter().collect();
    dirs.sort_by(|a, b| {
        b.1 .0
            .partial_cmp(&a.1 .0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    const MAX_MATCH_LINES: usize = 3;
    for (dir, (_, group)) in dirs {
        let has_dir = !dir.is_empty();
        if has_dir {
            println!("{dir}/");
        }
        for r in group {
            let fname = std::path::Path::new(&r.chunk.file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(r.chunk.file_path.as_str());
            let indent = if has_dir { "  " } else { "" };
            println!(
                "{indent}{:.4} {fname}:{}-{}",
                r.score, r.chunk.start_line, r.chunk.end_line
            );
            let total = r.match_lines.len();
            for ml in r.match_lines.iter().take(MAX_MATCH_LINES) {
                println!(
                    "{indent}  L{}: {}",
                    ml.line,
                    truncate_line(&ml.content, 100)
                );
            }
            if total > MAX_MATCH_LINES {
                println!("{indent}  ... (+{})", total - MAX_MATCH_LINES);
            }
        }
    }
}

fn truncate_line(line: &str, max_len: usize) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= max_len {
        return trimmed.to_string();
    }
    let s: String = trimmed.chars().take(max_len - 3).collect();
    format!("{s}...")
}

fn print_json(results: &[SearchResult]) {
    println!(
        "{}",
        serde_json::to_string(results).unwrap_or_else(|_| "[]".to_string())
    );
}

fn build_index(path: &str, include_text_files: bool, model: Option<&str>) -> SembleIndex {
    let encoder = model.map(|m| {
        StaticEncoder::load(Some(m)).unwrap_or_else(|e| {
            eprintln!("Failed to load model {m:?}: {e}");
            process::exit(1);
        })
    });
    match SembleIndex::from_path(path, encoder, None, None, include_text_files) {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("Error: {e:?}");
            process::exit(1);
        }
    }
}
