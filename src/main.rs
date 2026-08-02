use std::process;

use clap::{Parser, Subcommand};

use scry::format::format_results;
use scry::index::chunking::DESIRED_CHUNK_LENGTH_CHARS;
use scry::index::encoder::StaticEncoder;
use scry::index::{IndexParams, ScryIndex};
use scry::search::SearchParams;
use scry::types::SearchResult;

#[derive(Parser)]
#[command(
    name = "scry",
    version = env!("SCRY_VERSION"),
    about = "Scry finds code by intent, returning the file and line range."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search a codebase with keyword/symbol query
    Search {
        /// Query text
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
        /// Reciprocal rank fusion constant
        #[arg(long, default_value_t = 60.0)]
        rrf_k: f64,
        /// Drop results below this fraction of the top score
        #[arg(long, default_value_t = 0.12)]
        min_score_ratio: f64,
        /// Weight of the definition signal in the fusion
        #[arg(long, default_value_t = 1.0)]
        def_weight: f64,
        /// Candidate pool size as a multiple of top_k
        #[arg(long, default_value_t = 5)]
        candidates: usize,
        /// Target chunk size in characters at index time
        #[arg(long, default_value_t = DESIRED_CHUNK_LENGTH_CHARS)]
        chunk_size: usize,
        /// Skip files larger than this many bytes at index time
        #[arg(long, default_value_t = 1_000_000)]
        max_file_bytes: u64,
        /// Output as JSON (for agent/tool integration)
        #[arg(long)]
        json: bool,
        /// Compact output with file paths, scores, and match lines only
        #[arg(long)]
        compact: bool,
        /// Group results by directory
        #[arg(long)]
        group: bool,
        /// Match lines shown per chunk in grouped output
        #[arg(long, default_value_t = 3)]
        max_match_lines: usize,
        /// Embedding model, a Hugging Face repo id or a local path. Resolved
        /// locally from the HF cache, never downloaded.
        #[arg(long, default_value = "minishlab/potion-code-16M-v2")]
        model: String,
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
            rrf_k,
            min_score_ratio,
            def_weight,
            candidates,
            chunk_size,
            max_file_bytes,
            json,
            compact,
            group,
            max_match_lines,
            model,
        } => {
            if candidates == 0 {
                eprintln!("--candidates must be at least 1.");
                process::exit(1);
            }
            if chunk_size == 0 {
                eprintln!("--chunk-size must be at least 1.");
                process::exit(1);
            }
            let index_params = IndexParams {
                chunk_size,
                max_file_bytes,
            };
            let search_params = SearchParams {
                rrf_k,
                min_score_ratio,
                def_weight,
                candidate_multiplier: candidates,
            };
            let index = build_index(&path, include_text_files, &model, &index_params);

            let results = index.search(query.as_str(), top_k, &search_params, None, None);

            if group {
                print_grouped(&results, max_match_lines);
            } else if compact {
                print_compact(&results);
            } else if json {
                print_json(&results);
            } else if results.is_empty() {
                println!("No results found.");
            } else {
                let header = format!("Search results for: {query:?}");
                println!("{}", format_results(&header, &results));
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

fn print_grouped(results: &[SearchResult], max_match_lines: usize) {
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
            for ml in r.match_lines.iter().take(max_match_lines) {
                println!(
                    "{indent}  L{}: {}",
                    ml.line,
                    truncate_line(&ml.content, 100)
                );
            }
            if total > max_match_lines {
                println!("{indent}  ... (+{})", total - max_match_lines);
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

fn build_index(
    path: &str,
    include_text_files: bool,
    model: &str,
    index_params: &IndexParams,
) -> ScryIndex {
    let encoder = StaticEncoder::load(model).unwrap_or_else(|e| {
        eprintln!("{e:#}");
        process::exit(1);
    });
    match ScryIndex::from_path(path, encoder, None, None, include_text_files, index_params) {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("Error: {e:?}");
            process::exit(1);
        }
    }
}
