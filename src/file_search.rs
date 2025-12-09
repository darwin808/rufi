use crate::search_mode::SearchResult;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rand::seq::SliceRandom;
use std::fs;
use std::path::{Path, PathBuf};

fn search_recursive(
    dir: &Path,
    query: &str,
    results: &mut Vec<SearchResult>,
    max_results: usize,
    max_depth: usize,
    current_depth: usize,
) {
    if results.len() >= max_results || current_depth > max_depth {
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        if results.len() >= max_results {
            break;
        }

        let Ok(file_name) = entry.file_name().into_string() else {
            continue;
        };

        // Skip hidden files/directories and system directories
        if file_name.starts_with('.')
            || file_name == "Library"
            || file_name == "node_modules"
            || file_name == "target"
        {
            continue;
        }

        // Case-insensitive search
        if file_name.to_lowercase().contains(&query.to_lowercase()) {
            if let Ok(path) = entry.path().canonicalize() {
                results.push(SearchResult::new(
                    file_name.clone(),
                    path.to_string_lossy().to_string(),
                    crate::search_mode::SearchMode::Files,
                ));
            }
        }

        // Recursively search subdirectories
        if let Ok(metadata) = entry.metadata() {
            if metadata.is_dir() {
                search_recursive(
                    &entry.path(),
                    query,
                    results,
                    max_results,
                    max_depth,
                    current_depth + 1,
                );
            }
        }
    }
}

pub fn search_files(query: &str) -> Vec<SearchResult> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();

    // Search recursively through entire home directory
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

    // Search with reduced depth and max results for better performance
    // Depth of 4 is enough for most files while being fast
    search_recursive(&home, query, &mut results, 50, 4, 0);

    // Apply fuzzy matching on results
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<_> = results
        .into_iter()
        .filter_map(|result| {
            matcher
                .fuzzy_match(&result.name, query)
                .map(|score| (result, score))
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored
        .into_iter()
        .map(|(result, _)| result)
        .take(8)
        .collect()
}

pub fn search_files_random(count: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

    // Quick search in common directories only
    let search_dirs = vec![
        home.join("Documents"),
        home.join("Downloads"),
        home.join("Desktop"),
    ];

    for dir in search_dirs {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Ok(file_name) = entry.file_name().into_string() {
                    if file_name.starts_with('.') {
                        continue;
                    }
                    if let Ok(path) = entry.path().canonicalize() {
                        results.push(SearchResult::new(
                            file_name,
                            path.to_string_lossy().to_string(),
                            crate::search_mode::SearchMode::Files,
                        ));
                    }
                }
                if results.len() >= 20 {
                    break;
                }
            }
        }
        if results.len() >= 20 {
            break;
        }
    }

    // Shuffle and return requested count
    let mut rng = rand::thread_rng();
    results.shuffle(&mut rng);
    results.into_iter().take(count).collect()
}
