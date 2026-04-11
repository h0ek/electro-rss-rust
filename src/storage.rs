use crate::model::{FeedItem, FeedMeta};
use anyhow::Result;
use dirs::cache_dir;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const APP_ID: &str = "electro_rss_rust";

fn base_dir() -> PathBuf {
    cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(APP_ID)
}

fn ensure_dirs() -> Result<()> {
    fs::create_dir_all(base_dir())?;
    fs::create_dir_all(thumbs_dir())?;
    Ok(())
}

pub fn items_path() -> PathBuf {
    base_dir().join("items.json")
}

pub fn state_path() -> PathBuf {
    base_dir().join("state.json")
}

pub fn thumbs_dir() -> PathBuf {
    base_dir().join("thumbs")
}

pub fn thumb_path(url: &str) -> PathBuf {
    let mut hasher = Sha1::new();
    hasher.update(url.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    thumbs_dir().join(format!("{hash}.img"))
}

pub fn load_items() -> Vec<FeedItem> {
    let _ = ensure_dirs();
    let path = items_path();
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_items(items: &[FeedItem]) -> Result<()> {
    ensure_dirs()?;
    fs::write(items_path(), serde_json::to_vec_pretty(items)?)?;
    Ok(())
}

pub fn load_state() -> HashMap<String, FeedMeta> {
    let _ = ensure_dirs();
    let path = state_path();
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

pub fn save_state(state: &HashMap<String, FeedMeta>) -> Result<()> {
    ensure_dirs()?;
    fs::write(state_path(), serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

pub fn clear_cache() -> Result<()> {
    if items_path().exists() {
        let _ = fs::remove_file(items_path());
    }
    if state_path().exists() {
        let _ = fs::remove_file(state_path());
    }
    if thumbs_dir().exists() {
        let _ = fs::remove_dir_all(thumbs_dir());
    }
    ensure_dirs()?;
    Ok(())
}

pub fn cache_stats() -> (u64, u64) {
    let _ = ensure_dirs();

    let mut files = 0u64;
    let mut size = 0u64;

    visit_dir(&base_dir(), &mut files, &mut size);

    (files, size)
}

fn visit_dir(dir: &PathBuf, files: &mut u64, size: &mut u64) {
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let meta = match fs::metadata(&path) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if meta.is_file() {
            *files += 1;
            *size += meta.len();
        } else if meta.is_dir() {
            visit_dir(&path, files, size);
        }
    }
}

pub fn human_bytes(size: u64) -> String {
    let mut s = size as f64;
    for unit in ["B", "KB", "MB", "GB", "TB"] {
        if s < 1024.0 {
            return format!("{:.1} {}", s, unit);
        }
        s /= 1024.0;
    }
    format!("{:.1} PB", s)
}
