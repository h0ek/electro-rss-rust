use crate::logger::{log_error, log_info};
use crate::model::{FeedItem, FeedMeta};
use crate::storage;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use regex::Regex;
use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, USER_AGENT};
use roxmltree::Document;
use std::collections::HashMap;
use std::time::Duration as StdDuration;

pub fn rss_urls() -> Vec<(&'static str, &'static str)> {
    vec![
        ("x264/1080p", "https://electro-torrent.pl/rss.php?cat=770"),
        ("x265/2160p", "https://electro-torrent.pl/rss.php?cat=1160"),
        ("x265/1080p", "https://electro-torrent.pl/rss.php?cat=1116"),
        ("Seriale", "https://electro-torrent.pl/rss.php?cat=7"),
    ]
}

fn normalize_category(cat: &str) -> String {
    if cat.trim().eq_ignore_ascii_case("seriale") {
        "Seriale".into()
    } else {
        "Film".into()
    }
}

pub async fn fetch_items(
    days: i64,
    mut state: HashMap<String, FeedMeta>,
    old_items: Vec<FeedItem>,
) -> Result<(Vec<FeedItem>, HashMap<String, FeedMeta>)> {
    log_info(&format!("fetch_items() start, days={days}"));

    let cutoff = Utc::now() - Duration::days(days);

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .timeout(StdDuration::from_secs(12))
        .build()?;

    let mut out = Vec::new();

    for (cat, url) in rss_urls() {
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

        let bytes = match resp {
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
                state.insert(url.to_string(), new_meta);
                r.bytes().await?.to_vec()
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

        let mut parsed_items = parse_feed_xml(cat, &bytes, cutoff)?;
        log_info(&format!("Parsed items for {cat}: {}", parsed_items.len()));
        out.append(&mut parsed_items);
    }

    out.sort_by(|a, b| b.pub_date.cmp(&a.pub_date));
    log_info(&format!("fetch_items() done, total items={}", out.len()));
    Ok((out, state))
}

fn parse_feed_xml(cat: &str, bytes: &[u8], cutoff: DateTime<Utc>) -> Result<Vec<FeedItem>> {
    let xml = std::str::from_utf8(bytes).context("feed is not valid utf-8")?;
    let doc = Document::parse(xml).context("xml parse failed")?;

    let title_re = Regex::new(r"^(?P<title>.+?)\s*\((?P<year>\d{4})\)")?;
    let quality_re = Regex::new(r"\b(2160p|1080p|720p)\b")?;
    let lektor_re = Regex::new(r"(Lektor\s*[^\]/&\s]+(?:\s*AI|\s*\(AI\))?)")?;
    let napisy_re = Regex::new(r"(Napisy\s*[^\]/&\s]+(?:\s*AI|\s*\(AI\))?)")?;
    let dubbing_re = Regex::new(r"(Dubbing\s*[^\]/&\s]+)")?;
    let se_re = Regex::new(r"\bs\s*(\d{1,2})\s*e\s*(\d{1,3})\b")?;
    let desc_img_re = Regex::new(r#"(?i)<img[^>]+src=["']([^"']+)["']"#)?;

    let normalized_cat = normalize_category(cat);
    let mut out = Vec::new();

    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let title_raw = child_text(item, "title").unwrap_or_default();
        if title_raw.is_empty() {
            continue;
        }

        let Some(m) = title_re.captures(&title_raw) else {
            continue;
        };

        let title = m
            .name("title")
            .map(|x| x.as_str().trim())
            .unwrap_or("")
            .to_string();

        let year = m.name("year").map(|x| x.as_str()).unwrap_or("").to_string();

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
            .unwrap_or_default();

        let lektor = lektor_re
            .captures(&title_raw)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| {
                if title_raw.to_lowercase().contains("film polski") {
                    "Film Polski".into()
                } else {
                    "Nie".into()
                }
            });

        let napisy = napisy_re
            .captures(&title_raw)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "Nie".into());

        let dubbing = dubbing_re
            .captures(&title_raw)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "Nie".into());

        let link = child_text(item, "link").unwrap_or_default();
        let description = child_text(item, "description").unwrap_or_default();
        let thumb = find_thumb_url(item, &description, &desc_img_re);

        let mut season = String::new();
        let mut episode = String::new();

        if normalized_cat == "Seriale" {
            let low = title_raw.to_lowercase();
            if let Some(c) = se_re.captures(&low) {
                season = c.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                episode = c.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
            }
        }

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

        if name == "thumbnail" {
            if let Some(url) = child.attribute("url") {
                return url.to_string();
            }
        }

        if name == "content" {
            if let Some(url) = child.attribute("url") {
                let medium = child.attribute("medium").unwrap_or_default().to_lowercase();
                let ty = child.attribute("type").unwrap_or_default().to_lowercase();

                if medium == "image" || ty.starts_with("image/") {
                    return url.to_string();
                }
            }
        }
    }

    if let Some(caps) = desc_img_re.captures(description) {
        if let Some(m) = caps.get(1) {
            return m.as_str().to_string();
        }
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
    let client = match reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0")
        .timeout(StdDuration::from_secs(12))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("thumb client build error: {e:#}"));
            return;
        }
    };

    for it in items {
        if it.thumb.is_empty() {
            continue;
        }

        let path = storage::thumb_path(&it.thumb);
        if path.exists() {
            continue;
        }

        match client.get(&it.thumb).send() {
            Ok(resp) if resp.status().is_success() => match resp.bytes() {
                Ok(bytes) => {
                    if let Err(e) = std::fs::write(&path, &bytes) {
                        log_error(&format!("thumb write error {}: {e:#}", path.display()));
                    }
                }
                Err(e) => {
                    log_error(&format!("thumb bytes error {}: {e:#}", it.thumb));
                }
            },
            Ok(resp) => {
                log_error(&format!("thumb http error {}: {}", it.thumb, resp.status()));
            }
            Err(e) => {
                log_error(&format!("thumb request error {}: {e:#}", it.thumb));
            }
        }
    }
}
