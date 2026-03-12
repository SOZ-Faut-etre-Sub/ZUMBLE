use std::{env, fs};
use std::path::Path;

/// Read a file and return its trimmed content if it exists
pub fn read_file_trimmed(path: &str) -> Option<String> {
    let p = Path::new(path);
    if p.is_file() {
        fs::read_to_string(p).ok().map(|s| s.trim().to_string())
    }
    else {
        None
    }
}

/// Priority : *_FILE > ENV > flag > défaut
pub fn resolve_credential(
    file_env: &str,
    raw_env: &str,
    flag: Option<String>,
    default: Option<&str>,
) -> Option<String> {
    // 1) *_FILE
    if let Ok(path) = env::var(file_env) {
        if !path.is_empty() {
            if let Some(v) = read_file_trimmed(&path) {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    // 2) Raw ENV
    if let Ok(v) = env::var(raw_env) {
        if !v.is_empty() {
            return Some(v);
        }
    }
    // 3) CLI flag
    if let Some(v) = flag {
        if !v.is_empty() {
            return Some(v);
        }
    }
    // 4) Default
    default.map(|d| d.to_string())
}
