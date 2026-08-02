use std::collections::HashMap;

use crate::types::Chunk;

const STRONG_PENALTY: f64 = 0.3;
const MODERATE_PENALTY: f64 = 0.5;
const MILD_PENALTY: f64 = 0.7;

const REEXPORT_FILENAMES: &[&str] = &["__init__.py", "package-info.java"];

// Path segments that mark non-canonical code.
const TEST_DIRS: &[&str] = &["test", "tests", "__tests__", "spec", "testing"];
const COMPAT_DIRS: &[&str] = &["compat", "_compat", "legacy"];
const EXAMPLE_DIRS: &[&str] = &[
    "example", "examples", "_example", "_examples", "doc_src", "docs_src",
];

// File-name markers of test files, across languages.
const TEST_FILE_SUFFIXES: &[&str] = &[
    "_test.py", "_test.go", "_test.rb", "_test.cpp", "_test.c", "_test.dart", "_test.lua",
    "Test.java", "Tests.java", "Test.php", "Test.kt", "Tests.kt", "Spec.kt", "Test.swift",
    "Tests.swift", "Spec.swift", "Test.cs", "Tests.cs", "Spec.scala", "Suite.scala", "Test.scala",
    ".test.js", ".test.jsx", ".test.ts", ".test.tsx", ".spec.js", ".spec.jsx", ".spec.ts",
    ".spec.tsx", "_spec.rb", "_spec.lua",
];
// `test_*.<ext>` prefixed test files.
const TEST_PREFIX_EXTS: &[&str] = &[".py", ".cpp", ".c", ".dart", ".lua"];

const FILE_SATURATION_THRESHOLD: usize = 1;
const FILE_SATURATION_DECAY: f64 = 0.5;

pub fn rerank_topk(
    scores: &HashMap<usize, f64>,
    chunks: &[Chunk],
    top_k: usize,
    penalise_paths: bool,
) -> Vec<(usize, f64)> {
    if scores.is_empty() {
        return Vec::new();
    }

    let mut penalty_cache: HashMap<&str, f64> = HashMap::new();
    let mut penalised: Vec<(usize, f64)> = Vec::with_capacity(scores.len());

    for (&idx, &score) in scores {
        let penalty = if penalise_paths {
            let fp = chunks[idx].file_path.as_str();
            *penalty_cache
                .entry(fp)
                .or_insert_with(|| file_path_penalty(fp))
        } else {
            1.0
        };
        penalised.push((idx, score * penalty));
    }

    penalised.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut file_selected: HashMap<&str, usize> = HashMap::new();
    let mut selected: Vec<(f64, usize)> = Vec::new();
    let mut min_selected = f64::INFINITY;

    for &(idx, pen_score) in &penalised {
        if selected.len() >= top_k && pen_score <= min_selected {
            break;
        }

        let fp = chunks[idx].file_path.as_str();
        let already = *file_selected.get(fp).unwrap_or(&0);
        let mut eff_score = pen_score;

        if already >= FILE_SATURATION_THRESHOLD {
            let excess = (already - FILE_SATURATION_THRESHOLD + 1) as i32;
            eff_score *= FILE_SATURATION_DECAY.powi(excess);
        }

        selected.push((eff_score, idx));
        *file_selected.entry(fp).or_default() += 1;

        if selected.len() >= top_k {
            min_selected = selected
                .iter()
                .map(|(s, _)| *s)
                .fold(f64::INFINITY, f64::min);
        }
    }

    selected.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    selected
        .into_iter()
        .take(top_k)
        .map(|(score, idx)| (idx, score))
        .collect()
}

fn is_test_file(name: &str) -> bool {
    (name.starts_with("test_") && TEST_PREFIX_EXTS.iter().any(|e| name.ends_with(e)))
        || name.starts_with("test_helper")
        || TEST_FILE_SUFFIXES.iter().any(|s| name.ends_with(s))
}

fn file_path_penalty(file_path: &str) -> f64 {
    // Last path segment, tolerating either separator.
    let name = file_path.rsplit(['/', '\\']).next().unwrap_or("");
    let has_segment = |set: &[&str]| file_path.split(['/', '\\']).any(|seg| set.contains(&seg));

    let mut penalty = 1.0;

    if is_test_file(name) || has_segment(TEST_DIRS) {
        penalty *= STRONG_PENALTY;
    }
    if REEXPORT_FILENAMES.contains(&name) {
        penalty *= MODERATE_PENALTY;
    }
    if has_segment(COMPAT_DIRS) {
        penalty *= STRONG_PENALTY;
    }
    if has_segment(EXAMPLE_DIRS) {
        penalty *= STRONG_PENALTY;
    }
    if name.ends_with(".d.ts") {
        penalty *= MILD_PENALTY;
    }

    penalty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_files_are_penalised() {
        assert_eq!(file_path_penalty("src/foo_test.go"), STRONG_PENALTY);
        assert_eq!(file_path_penalty("app/UserTest.java"), STRONG_PENALTY);
        assert_eq!(file_path_penalty("pkg/test_helpers.py"), STRONG_PENALTY);
        assert_eq!(file_path_penalty("web/login.test.ts"), STRONG_PENALTY);
        assert_eq!(file_path_penalty("test_parser.py"), STRONG_PENALTY);
    }

    #[test]
    fn test_dirs_are_penalised() {
        assert_eq!(file_path_penalty("tests/util.rs"), STRONG_PENALTY);
        assert_eq!(file_path_penalty("src/__tests__/a.js"), STRONG_PENALTY);
        assert_eq!(file_path_penalty("legacy/old.py"), STRONG_PENALTY);
        assert_eq!(file_path_penalty("pkg/examples/demo.rs"), STRONG_PENALTY);
    }

    #[test]
    fn reexports_and_declaration_files() {
        assert_eq!(file_path_penalty("pkg/__init__.py"), MODERATE_PENALTY);
        assert_eq!(file_path_penalty("types/index.d.ts"), MILD_PENALTY);
    }

    #[test]
    fn normal_files_are_not_penalised() {
        assert_eq!(file_path_penalty("src/search.rs"), 1.0);
        assert_eq!(file_path_penalty("app/user_controller.py"), 1.0);
    }

    #[test]
    fn windows_separators_are_handled() {
        assert_eq!(file_path_penalty("src\\tests\\util.rs"), STRONG_PENALTY);
    }
}
