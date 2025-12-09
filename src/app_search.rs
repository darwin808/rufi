use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Application {
    pub name: String,
    pub path: String,
    pub is_action: bool,
    pub command: Option<String>,
}

pub fn index_applications() -> Vec<Application> {
    // Check if cache exists and is recent (less than 1 hour old)
    let cache_fresh = if let Ok(metadata) = fs::metadata(cache_path()) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                elapsed.as_secs() < 3600 // Cache valid for 1 hour
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    // Try loading from cache first if it's fresh
    if cache_fresh {
        if let Some(cached) = load_cache() {
            return cached;
        }
    }

    // Scan application directories
    let home_apps = format!("{}/Applications", std::env::var("HOME").unwrap_or_default());
    let app_dirs = vec!["/Applications", home_apps.as_str(), "/System/Applications"];

    let mut scanned_apps = Vec::new();

    for dir in app_dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".app") {
                        let app_name = name.trim_end_matches(".app").to_string();
                        let app_path = entry.path().display().to_string();

                        scanned_apps.push(Application {
                            name: app_name,
                            path: app_path,
                            is_action: false,
                            command: None,
                        });
                    }
                }
            }
        }
    }

    // Sort apps alphabetically
    scanned_apps.sort_by(|a, b| a.name.cmp(&b.name));

    // Save to cache
    save_cache(&scanned_apps);

    scanned_apps
}

fn system_actions() -> Vec<Application> {
    vec![
        Application {
            name: "🔍 Browser".to_string(),
            path: String::new(),
            is_action: true,
            command: Some("open -a Safari".to_string()),
        },
        Application {
            name: "📁 Files".to_string(),
            path: String::new(),
            is_action: true,
            command: Some("open -a Finder".to_string()),
        },
        Application {
            name: "⚡ Terminal".to_string(),
            path: String::new(),
            is_action: true,
            command: Some("open -a Terminal".to_string()),
        },
        Application {
            name: "🔌 Shutdown".to_string(),
            path: String::new(),
            is_action: true,
            command: Some("osascript -e 'tell app \"System Events\" to shut down'".to_string()),
        },
        Application {
            name: "🔄 Reboot".to_string(),
            path: String::new(),
            is_action: true,
            command: Some("osascript -e 'tell app \"System Events\" to restart'".to_string()),
        },
        Application {
            name: "💤 Sleep".to_string(),
            path: String::new(),
            is_action: true,
            command: Some("pmset sleepnow".to_string()),
        },
        Application {
            name: "🔒 Lock Screen".to_string(),
            path: String::new(),
            is_action: true,
            command: Some("pmset displaysleepnow".to_string()),
        },
    ]
}

fn cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap()
        .join("rofi-mac")
        .join("apps.json")
}

fn load_cache() -> Option<Vec<Application>> {
    let path = cache_path();
    if let Ok(contents) = fs::read_to_string(&path) {
        serde_json::from_str(&contents).ok()
    } else {
        None
    }
}

fn save_cache(apps: &[Application]) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(apps) {
        let _ = fs::write(&path, json);
    }
}

pub fn fuzzy_search(apps: &[Application], query: &str) -> Vec<Application> {
    if query.is_empty() {
        return apps.to_vec();
    }

    let matcher = SkimMatcherV2::default();
    let mut results: Vec<(i64, Application)> = apps
        .iter()
        .filter_map(|app| {
            matcher
                .fuzzy_match(&app.name.to_lowercase(), &query.to_lowercase())
                .map(|score| (score, app.clone()))
        })
        .collect();

    results.sort_by(|a, b| b.0.cmp(&a.0));
    results.into_iter().map(|(_, app)| app).collect()
}
