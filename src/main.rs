use std::process;

use clap::{Parser, Subcommand};

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

            print_json(&results);
        }
    }
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
