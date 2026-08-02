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
        /// Keyword, symbol, or function name to search for
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
    /// Find code similar to a specific location
    FindRelated {
        /// File path as shown in search results
        file_path: String,
        /// Line number, starting at 1
        line: usize,
        /// Local path, defaults to the current directory
        #[arg(default_value = ".")]
        path: String,
        /// Number of results
        #[arg(short = 'k', long = "top-k", default_value = "10")]
        top_k: usize,
        /// Also index non code text files
        #[arg(long)]
        include_text_files: bool,
        /// Output as JSON (for agent/tool integration)
        #[arg(long)]
        json: bool,
        /// Embedding model (HF repo id or local path).
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
            json,
            compact,
            group,
            model,
        } => {
            let index = build_index(&path, include_text_files, model.as_deref());

            let results = index.search(query.as_str(), top_k, None, None, None);
            if group {
                print_grouped(&results);
            } else if compact {
                print_compact(&results);
            } else if json {
                print_json(&results);
            } else if results.is_empty() {
                println!("No results found.");
            } else {
                println!(
                    "{}",
                    format_results(&format!("Search results for: {query:?}"), &results)
                );
            }
        }
        Commands::FindRelated {
            file_path,
            line,
            path,
            top_k,
            include_text_files,
            json,
            model,
        } => {
            let index = build_index(&path, include_text_files, model.as_deref());

            let chunk = match resolve_chunk(index.chunks(), &file_path, line) {
                Some(c) => c.clone(),
                None => {
                    eprintln!("No chunk found at {file_path}:{line}.");
                    process::exit(1);
                }
            };

            let results = index.find_related(&chunk, top_k);
            if json {
                print_json(&results);
            } else if results.is_empty() {
                println!("No related chunks found for {file_path}:{line}.");
            } else {
                println!(
                    "{}",
                    format_results(&format!("Chunks related to {file_path}:{line}"), &results)
                );
            }
        }
    }
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
