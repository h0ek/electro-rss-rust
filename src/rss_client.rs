use crate::logger::{log_error, log_info};
use crate::metadata;
use crate::model::{FeedItem, FeedMeta};
use crate::rss_sources;
use crate::storage;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use regex::Regex;
use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, USER_AGENT};
use roxmltree::Document;
use std::collections::{HashMap, HashSet};
use std::time::Duration as StdDuration;

fn normalize_category(cat: &str) -> String {
    rss_sources::category_for(cat).to_string()
}

pub async fn fetch_items(
    days: i64,
    mut state: HashMap<String, FeedMeta>,
    old_items: Vec<FeedItem>,
    enabled: Vec<u16>,
) -> Result<(Vec<FeedItem>, HashMap<String, FeedMeta>)> {
    log_info(&format!("fetch_items() start, days={days}"));

    let cutoff = Utc::now() - Duration::days(days);

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .timeout(StdDuration::from_secs(12))
        .build()?;

    let mut out = old_items.clone();
    for entry in &mut out {
        if entry.year.is_empty() {
            let (_, year) = metadata::title_and_year(&entry.title);
            if !year.is_empty() {
                entry.year = year;
            }
        }
        if entry.category == "Seriale" && (entry.season.is_empty() || entry.episode.is_empty()) {
            let (season, episode) = metadata::series_episode(&entry.title, "");
            if entry.season.is_empty() {
                entry.season = season;
            }
            if entry.episode.is_empty() {
                entry.episode = episode;
            }
        }
        if !entry.original_polish {
            entry.original_polish = metadata::is_polish_original(&entry.title);
        }
    }

    for (cat, source_url) in rss_sources::enabled_sources(&enabled) {
        let url = source_url.as_str();
        log_info(&format!("Fetching URL: {url}"));

        let meta = state.get(url).cloned().unwrap_or_default();

        let mut req = client.get(url).header(USER_AGENT, "Mozilla/5.0");
        if let Some(etag) = &meta.etag {
            req = req.header(IF_NONE_MATCH, etag);
        }
        if let Some(modified) = &meta.modified {
            req = req.header(IF_MODIFIED_SINCE, modified);
        }

        let resp = req.send().await;
        let (bytes, incoming_meta) = match resp {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_MODIFIED => {
                log_info(&format!("304 Not Modified: {url}"));
                let cached: Vec<FeedItem> = old_items
                    .iter()
                    .filter(|it| it.source == cat && it.pub_date >= cutoff)
                    .cloned()
                    .collect();
                log_info(&format!("Using cached items for {cat}: {}", cached.len()));
                out.extend(cached);
                continue;
            }
            Ok(r) => {
                log_info(&format!("HTTP status for {url}: {}", r.status()));
                if !r.status().is_success() {
                    log_error(&format!("HTTP error for {url}: {}", r.status()));
                    continue;
                }
                let new_meta = FeedMeta {
                    etag: r
                        .headers()
                        .get(ETAG)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string),
                    modified: r
                        .headers()
                        .get(LAST_MODIFIED)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string),
                };
                match r.bytes().await {
                    Ok(bytes) => (bytes.to_vec(), new_meta),
                    Err(e) => {
                        log_error(&format!("Body read failed for {url}: {e:#}"));
                        continue;
                    }
                }
            }
            Err(e) => {
                log_error(&format!("Request error for {url}: {e:#}"));
                let cached: Vec<FeedItem> = old_items
                    .iter()
                    .filter(|it| it.source == cat && it.pub_date >= cutoff)
                    .cloned()
                    .collect();
                log_info(&format!(
                    "Fallback cached items for {cat}: {}",
                    cached.len()
                ));
                out.extend(cached);
                continue;
            }
        };

        let mut parsed_items = match parse_feed_xml(cat, &bytes, cutoff) {
            Ok(items) => items,
            Err(e) => {
                log_error(&format!("RSS parser error for {url}: {e:#}"));
                continue;
            }
        };
        state.insert(url.to_string(), incoming_meta);
        log_info(&format!("Parsed items for {cat}: {}", parsed_items.len()));
        out.append(&mut parsed_items);
    }

    out.reverse();
    out.sort_by(|a, b| b.pub_date.cmp(&a.pub_date));
    let mut seen = HashSet::new();
    out.retain(|item| {
        let key = if item.link.trim().is_empty() {
            format!("{}|{}|{}", item.source, item.title, item.pub_date)
        } else {
            item.link.clone()
        };
        seen.insert(key)
    });
    log_info(&format!("fetch_items() done, total items={}", out.len()));
    Ok((out, state))
}

fn parse_feed_xml(cat: &str, bytes: &[u8], cutoff: DateTime<Utc>) -> Result<Vec<FeedItem>> {
    let xml = std::str::from_utf8(bytes).context("feed is not valid utf-8")?;
    let repaired_xml = metadata::repair_unescaped_ampersands(xml);
    let doc = Document::parse(&repaired_xml).context("xml parse failed")?;

    let quality_re = Regex::new(r"(?i)\b(2160p|1080p|720p)\b")?;
    let desc_img_re = Regex::new(r#"(?i)<img[^>]+src=["']([^"']+)["']"#)?;

    let normalized_cat = normalize_category(cat);
    let mut out = Vec::new();

    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let title_raw = child_text(item, "title").unwrap_or_default();
        if title_raw.is_empty() {
            continue;
        }

        let (title, year) = metadata::title_and_year(&title_raw);

        let pub_date_raw = child_text(item, "pubDate").unwrap_or_default();
        let pub_date = match parse_pub_date(&pub_date_raw) {
            Ok(d) => d,
            Err(_) => continue,
        };

        if pub_date < cutoff {
            continue;
        }

        let quality = quality_re
            .captures(&title_raw)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| {
                rss_sources::quality_for(cat)
                    .unwrap_or_default()
                    .to_string()
            });

        let (lektor, napisy, dubbing) = metadata::audio_tags(&title_raw);
        let link = child_text(item, "link").unwrap_or_default();
        let description = child_text(item, "description").unwrap_or_default();
        let thumb = find_thumb_url(item, &description, &desc_img_re);

        let (season, episode) = if normalized_cat == "Seriale" {
            metadata::series_episode(&title_raw, &description)
        } else {
            (String::new(), String::new())
        };
        out.push(FeedItem {
            source: cat.to_string(),
            category: normalized_cat.clone(),
            title,
            year,
            quality,
            lektor,
            napisy,
            dubbing,
            thumb,
            link,
            pub_date,
            season,
            episode,
            original_polish: metadata::is_polish_original(&title_raw),
        });
    }

    Ok(out)
}

fn child_text<'a, 'input>(node: roxmltree::Node<'a, 'input>, local_name: &str) -> Option<String> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == local_name)
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
}

fn find_thumb_url<'a, 'input>(
    item: roxmltree::Node<'a, 'input>,
    description: &str,
    desc_img_re: &Regex,
) -> String {
    for child in item.children().filter(|n| n.is_element()) {
        let name = child.tag_name().name();

        if name == "thumbnail"
            && let Some(url) = child.attribute("url")
        {
            return url.to_string();
        }

        if name == "content"
            && let Some(url) = child.attribute("url")
        {
            let medium = child.attribute("medium").unwrap_or_default().to_lowercase();
            let ty = child.attribute("type").unwrap_or_default().to_lowercase();

            if medium == "image" || ty.starts_with("image/") {
                return url.to_string();
            }
        }
    }

    if let Some(caps) = desc_img_re.captures(description)
        && let Some(m) = caps.get(1)
    {
        return m.as_str().to_string();
    }

    String::new()
}

fn parse_pub_date(s: &str) -> Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return Ok(dt.with_timezone(&Utc));
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%a, %d %b %Y %H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }

    anyhow::bail!("bad date: {s}")
}

pub fn prefetch_thumbs(items: &[FeedItem]) {
    use reqwest::header::CONTENT_TYPE;
    use std::io::Read;

    const MAX_IMAGE_BYTES: u64 = 3 * 1024 * 1024;
    const WORKERS: usize = 4;

    let client = match reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0")
        .timeout(StdDuration::from_secs(9))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            log_error(&format!("Thumbnail client error: {e:#}"));
            return;
        }
    };

    for group in items.chunks(WORKERS) {
        std::thread::scope(|scope| {
            for item in group {
                let client = client.clone();
                scope.spawn(move || {
                    if item.thumb.is_empty() {
                        return;
                    }
                    let path = storage::thumb_path(&item.thumb);
                    if path.is_file() {
                        return;
                    }
                    let response = match client.get(&item.thumb).send() {
                        Ok(response) if response.status().is_success() => response,
                        Ok(response) => {
                            log_error(&format!(
                                "Thumbnail HTTP {}: {}",
                                response.status(),
                                item.thumb
                            ));
                            return;
                        }
                        Err(e) => {
                            log_error(&format!("Thumbnail request {}: {e:#}", item.thumb));
                            return;
                        }
                    };
                    if response
                        .content_length()
                        .is_some_and(|length| length > MAX_IMAGE_BYTES)
                    {
                        log_error(&format!("Thumbnail too large: {}", item.thumb));
                        return;
                    }
                    let media_type = response
                        .headers()
                        .get(CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if !media_type.is_empty()
                        && !media_type.starts_with("image/")
                        && !media_type.starts_with("application/octet-stream")
                    {
                        log_error(&format!(
                            "Unexpected thumbnail MIME type {}: {}",
                            media_type, item.thumb
                        ));
                        return;
                    }
                    let mut bytes = Vec::new();
                    if let Err(e) = response.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes) {
                        log_error(&format!("Thumbnail read {}: {e:#}", item.thumb));
                        return;
                    }
                    if bytes.len() as u64 > MAX_IMAGE_BYTES {
                        log_error(&format!("Thumbnail exceeds size limit: {}", item.thumb));
                        return;
                    }
                    let jpeg = bytes.starts_with(&[0xff, 0xd8, 0xff]);
                    let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
                    let webp = bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(&b"WEBP"[..]);
                    if !jpeg && !png && !webp {
                        log_error(&format!(
                            "Thumbnail has invalid image signature: {}",
                            item.thumb
                        ));
                        return;
                    }
                    if let Err(e) = std::fs::write(&path, bytes) {
                        log_error(&format!("Thumbnail save {}: {e:#}", path.display()));
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod metadata_regression_tests {
    use super::*;

    #[test]
    fn parses_separated_and_compact_season_episode_and_thumbnails() {
        let xml = include_str!("../tests/fixtures/rss-metadata.xml");
        let items = parse_feed_xml(
            "Seriale",
            xml.as_bytes(),
            Utc::now() - Duration::days(36500),
        )
        .unwrap();
        assert_eq!(items.len(), 2);
        let zmora = items
            .iter()
            .find(|item| item.link.ends_with("/668509"))
            .unwrap();
        assert_eq!(
            (&*zmora.year, &*zmora.season, &*zmora.episode),
            ("2026", "01", "02")
        );
        assert_eq!(
            zmora.thumb,
            "https://electro-torrent.pl/torrentimg/668509.jpg"
        );
        let seen = items
            .iter()
            .find(|item| item.link.ends_with("/668475"))
            .unwrap();
        assert_eq!(
            (&*seen.year, &*seen.season, &*seen.episode),
            ("2026", "01", "06")
        );
        assert_eq!(
            seen.thumb,
            "https://electro-torrent.pl/torrentimg/668475.jpg"
        );
        assert_ne!(seen.napisy, "Nie");
    }
}

#[cfg(test)]
mod malformed_rss_tests {
    use super::*;

    #[test]
    fn parses_unescaped_ampersand_and_extended_serial_metadata() {
        let xml = include_str!("../tests/fixtures/rss-unescaped.xml");
        let items = parse_feed_xml(
            "Seriale",
            xml.as_bytes(),
            Utc::now() - Duration::days(36500),
        )
        .expect("recover invalid title entity");
        assert_eq!(items.len(), 2);
        let lombard = items
            .iter()
            .find(|item| item.link.ends_with("/668574"))
            .unwrap();
        assert_eq!(lombard.year, "2026");
        assert_eq!(lombard.season, "19");
        assert_eq!(lombard.episode, "1039");
        let polish = items
            .iter()
            .find(|item| item.link.ends_with("/667204"))
            .unwrap();
        assert!(polish.original_polish);
        assert_eq!(polish.episode, "06");
    }
}
