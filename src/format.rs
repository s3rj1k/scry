use crate::types::SearchResult;

pub fn format_results(header: &str, results: &[SearchResult]) -> String {
    let mut lines = vec![header.to_string(), String::new()];
    for (i, r) in results.iter().enumerate() {
        lines.push(format!(
            "## {}. {}  [score={:.3}]",
            i + 1,
            r.chunk.location(),
            r.score
        ));
        if !r.match_lines.is_empty() {
            for ml in &r.match_lines {
                lines.push(format!("  L{}: {}", ml.line, ml.content));
            }
        } else {
            lines.push("```".to_string());
            lines.push(r.chunk.content.trim().to_string());
            lines.push("```".to_string());
        }
        lines.push(String::new());
    }
    lines.join("\n")
}
