use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
pub struct RssSource {
    pub id: u16,
    pub label: &'static str,
    pub source: &'static str,
    pub category: &'static str,
    pub default_enabled: bool,
}

pub const SOURCES: &[RssSource] = &[
    RssSource {
        id: 769,
        label: "Movies · 1080p AVC",
        source: "1080p AVC",
        category: "Film",
        default_enabled: true,
    },
    RssSource {
        id: 770,
        label: "Movies · x264/1080p",
        source: "x264/1080p",
        category: "Film",
        default_enabled: true,
    },
    RssSource {
        id: 723,
        label: "Movies · 4K UHD",
        source: "4K UHD",
        category: "Film",
        default_enabled: true,
    },
    RssSource {
        id: 1160,
        label: "Movies · x265/2160p",
        source: "x265/2160p",
        category: "Film",
        default_enabled: true,
    },
    RssSource {
        id: 1116,
        label: "Movies · x265/1080p",
        source: "x265/1080p",
        category: "Film",
        default_enabled: true,
    },
    RssSource {
        id: 7,
        label: "TV Series",
        source: "Seriale",
        category: "Seriale",
        default_enabled: true,
    },
    RssSource {
        id: 1255,
        label: "TV Series · Warezy",
        source: "TV/Seriale - Warezy",
        category: "Seriale",
        default_enabled: false,
    },
    RssSource {
        id: 4,
        label: "Movies · x264/720p",
        source: "x264/720p",
        category: "Film",
        default_enabled: false,
    },
    RssSource {
        id: 1204,
        label: "Movies · x265/720p",
        source: "x265/720p",
        category: "Film",
        default_enabled: false,
    },
];

#[derive(Serialize, Deserialize)]
struct SourceSettings {
    enabled: Vec<u16>,
}

fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("No user configuration directory available")?;
    Ok(base.join("electro_rss_rust").join("rss_sources.json"))
}

pub fn default_selected() -> Vec<u16> {
    SOURCES
        .iter()
        .filter(|source| source.default_enabled)
        .map(|source| source.id)
        .collect()
}

pub fn load_selected() -> Vec<u16> {
    let Ok(path) = config_path() else {
        return default_selected();
    };
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return default_selected(),
        Err(e) => {
            crate::logger::log_error(&format!("Cannot read RSS source settings: {e}"));
            return default_selected();
        }
    };
    let stored: SourceSettings = match serde_json::from_slice(&bytes) {
        Ok(settings) => settings,
        Err(e) => {
            crate::logger::log_error(&format!("Invalid RSS source settings: {e}"));
            return default_selected();
        }
    };
    let ids: HashSet<u16> = stored.enabled.into_iter().collect();
    let result: Vec<u16> = SOURCES
        .iter()
        .filter(|source| ids.contains(&source.id))
        .map(|source| source.id)
        .collect();
    if result.is_empty() {
        crate::logger::log_error(
            "RSS source settings contain no recognized enabled sources; using defaults",
        );
        default_selected()
    } else {
        result
    }
}

pub fn save_selected(ids: &[u16]) -> Result<()> {
    let wanted: HashSet<u16> = ids.iter().copied().collect();
    if wanted.is_empty()
        || wanted
            .iter()
            .any(|id| !SOURCES.iter().any(|source| source.id == *id))
    {
        bail!("Select at least one known RSS source");
    }
    let enabled: Vec<u16> = SOURCES
        .iter()
        .filter(|source| wanted.contains(&source.id))
        .map(|source| source.id)
        .collect();
    let path = config_path()?;
    let directory = path
        .parent()
        .context("Missing RSS settings parent directory")?;
    fs::create_dir_all(directory)?;
    let nano = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temp = directory.join(format!(".rss_sources.{}.{}.tmp", std::process::id(), nano));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(&SourceSettings { enabled })?)?;
        file.sync_all()?;
        fs::rename(&temp, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub fn enabled_sources(ids: &[u16]) -> Vec<(&'static str, String)> {
    let enabled: HashSet<u16> = ids.iter().copied().collect();
    SOURCES
        .iter()
        .filter(|source| enabled.contains(&source.id))
        .map(|source| {
            (
                source.source,
                format!("https://electro-torrent.pl/rss.php?cat={}", source.id),
            )
        })
        .collect()
}

pub fn category_for(source: &str) -> &'static str {
    SOURCES
        .iter()
        .find(|entry| entry.source == source)
        .map_or("Film", |entry| entry.category)
}

pub fn quality_for(source: &str) -> Option<&'static str> {
    let id = SOURCES.iter().find(|entry| entry.source == source)?.id;
    match id {
        723 | 1160 => Some("2160p"),
        769 | 770 | 1116 => Some("1080p"),
        4 | 1204 => Some("720p"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_six_feeds_without_720p() {
        assert_eq!(default_selected(), vec![769, 770, 723, 1160, 1116, 7]);
    }

    #[test]
    fn selection_removes_duplicates_and_uses_known_order() {
        let feeds = enabled_sources(&[7, 4, 4, 769, 9999]);
        assert_eq!(feeds.len(), 3);
        assert!(feeds[0].1.ends_with("cat=769"));
        assert!(feeds[1].1.ends_with("cat=7"));
        assert!(feeds[2].1.ends_with("cat=4"));
    }

    #[test]
    fn category_for_tv_sources_is_series() {
        assert_eq!(category_for("Seriale"), "Seriale");
        assert_eq!(category_for("TV/Seriale - Warezy"), "Seriale");
        assert_eq!(category_for("4K UHD"), "Film");
        assert_eq!(quality_for("4K UHD"), Some("2160p"));
        assert_eq!(quality_for("Seriale"), None);
    }
}
