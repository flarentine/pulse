use std::cmp::Reverse;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::process::ProcessRow;

const NAME_BONUS: i64 = 1_000;
const PID_SCORE: i64 = 500;
const PATH_SCORE: i64 = 100;

#[derive(Default)]
pub struct Filter {
    matcher: SkimMatcherV2,
}

impl Filter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Indices into `rows` that match, best first. A blank query matches all.
    pub fn apply(&self, rows: &[ProcessRow], query: &str) -> Vec<usize> {
        let query = query.trim();
        if query.is_empty() {
            return (0..rows.len()).collect();
        }
        let query_lower = query.to_lowercase();

        let mut scored: Vec<(i64, usize)> = rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| self.score(row, query, &query_lower).map(|score| (score, i)))
            .collect();

        scored.sort_by_key(|&(score, _)| Reverse(score));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    // Names are fuzzy. Paths are plain substrings: fuzzy matching on a long
    // path lets almost any short query hit.
    fn score(&self, row: &ProcessRow, query: &str, query_lower: &str) -> Option<i64> {
        if let Some(score) = self.matcher.fuzzy_match(row.display_name(), query) {
            return Some(score + NAME_BONUS);
        }
        if row.pid.to_string().contains(query) {
            return Some(PID_SCORE);
        }
        row.exe_path
            .as_deref()
            .filter(|path| path.to_lowercase().contains(query_lower))
            .map(|_| PATH_SCORE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::test_row;

    fn names(rows: &[ProcessRow], idxs: &[usize]) -> Vec<String> {
        idxs.iter().map(|&i| rows[i].name.clone()).collect()
    }

    fn sample() -> Vec<ProcessRow> {
        let mut code = test_row(100, None, "Code.exe", 1);
        code.exe_path = Some(r"C:\Program Files\Microsoft VS Code\Code.exe".into());
        let mut chrome = test_row(200, None, "chrome.exe", 1);
        chrome.exe_path = Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".into());
        let mut helper = test_row(4321, None, "helper.exe", 1);
        helper.exe_path = Some(r"C:\Tools\vscode-helper\helper.exe".into());
        vec![code, chrome, helper]
    }

    #[test]
    fn blank_query_keeps_everything_in_order() {
        let rows = sample();
        assert_eq!(Filter::new().apply(&rows, "   "), vec![0, 1, 2]);
    }

    #[test]
    fn name_is_fuzzy() {
        let rows = sample();
        let hits = Filter::new().apply(&rows, "chrm");
        assert_eq!(names(&rows, &hits), ["chrome.exe"]);
    }

    #[test]
    fn pid_substring_matches() {
        let rows = sample();
        let hits = Filter::new().apply(&rows, "432");
        assert_eq!(names(&rows, &hits), ["helper.exe"]);
    }

    #[test]
    fn path_is_substring_not_fuzzy() {
        let rows = sample();
        let hits = Filter::new().apply(&rows, "google");
        assert_eq!(names(&rows, &hits), ["chrome.exe"]);
        // These letters appear in order in "Program Files\Microsoft VS Code",
        // which a fuzzy path match would have accepted.
        assert!(Filter::new().apply(&rows, "pfmcd").is_empty());
    }

    #[test]
    fn path_match_is_case_insensitive() {
        let rows = sample();
        let hits = Filter::new().apply(&rows, "VSCODE");
        assert_eq!(names(&rows, &hits), ["helper.exe"]);
    }

    #[test]
    fn name_hits_rank_above_path_hits() {
        let rows = sample();
        let hits = Filter::new().apply(&rows, "code");
        assert_eq!(names(&rows, &hits)[0], "Code.exe");
    }
}
