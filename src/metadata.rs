use regex::Regex;
use std::sync::LazyLock;

static YEAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[\s._\[(*/-])(?P<year>(?:19|20)\d{2})(?:$|[\s._\])*/-])")
        .expect("valid production year pattern")
});
static RANGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:19|20)\d{2}\s*[-–]\s*((?:19|20)\d{2})").expect("valid year range pattern")
});
static SE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[^[:alnum:]])s(?:eason)?[\[\]\s._-]*(\d{1,2})[\[\]\s._-]*e(?:pisode)?[\[\]\s._-]*(\d{1,4})(?:\b|[^0-9])")
        .expect("valid season episode pattern")
});
static EP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[\s*E\s*(\d{1,4})(?:\s*[-–]\s*(\d{1,4}))?\s*\]?")
        .expect("valid episode pattern")
});
static DESC_SEASON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bsezon\s*:\s*(\d{1,2})\b").expect("valid description season pattern")
});
static DESC_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bepizod\s*:\s*(\d{1,4})(?:\s*[-–]\s*(\d{1,4}))?")
        .expect("valid description episode pattern")
});
static POLISH_ORIGINAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:\bserial[\s._-]*polski\b|\bserial[\s._-]*pl\b|\bfilm[\s._-]*polski\b|\boriginal[\s._-]*pl\b)")
        .expect("valid original audio pattern")
});
static VOICE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:lektor(?:\s+pl)?|voice[\s-]*over|pl[\s._-]*lektor)\b")
        .expect("valid voice pattern")
});
static SUBS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:napisy|nap|subtitles|subs)\b").expect("valid subtitle pattern")
});
static DUB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:dubbing|dub(?:bed)?)\b").expect("valid dubbing pattern")
});

pub fn title_and_year(raw: &str) -> (String, String) {
    if let Some(c) = YEAR.captures(raw)
        && let Some(y) = c.name("year")
    {
        let year = RANGE
            .captures(&raw[y.start()..])
            .and_then(|r| r.get(1))
            .map_or_else(|| y.as_str().to_string(), |last| last.as_str().to_string());
        let before =
            raw[..y.start()].trim_end_matches(|c: char| c.is_whitespace() || "([*._-/".contains(c));
        let title = if before.is_empty() { raw } else { before };
        return (title.to_string(), year);
    }
    (raw.to_string(), String::new())
}

pub fn series_episode(title: &str, description: &str) -> (String, String) {
    let mut season = String::new();
    let mut episode = String::new();
    if let Some(c) = SE.captures(title) {
        season = c.get(1).map_or("", |v| v.as_str()).to_string();
        episode = c.get(2).map_or("", |v| v.as_str()).to_string();
    } else if let Some(c) = EP.captures(title) {
        episode = format_episode(&c);
    }
    if season.is_empty() {
        season = DESC_SEASON
            .captures(description)
            .and_then(|c| c.get(1))
            .map_or("", |m| m.as_str())
            .to_string();
    }
    if episode.is_empty() {
        episode = DESC_EPISODE
            .captures(description)
            .map(|c| format_episode(&c))
            .unwrap_or_default();
    }
    (season, episode)
}

fn format_episode(c: &regex::Captures<'_>) -> String {
    let start = c.get(1).map_or("", |v| v.as_str());
    match c.get(2) {
        Some(end) => format!("{start}–{}", end.as_str()),
        None => start.to_string(),
    }
}

pub fn is_polish_original(title: &str) -> bool {
    POLISH_ORIGINAL.is_match(title)
}

pub fn audio_tags(title: &str) -> (String, String, String) {
    let lektor = if VOICE.is_match(title) {
        "Lektor PL"
    } else {
        "Nie"
    };
    let napisy = if SUBS.is_match(title) {
        "Napisy PL"
    } else {
        "Nie"
    };
    let dubbing = if DUB.is_match(title) {
        "Dubbing PL"
    } else {
        "Nie"
    };
    (lektor.into(), napisy.into(), dubbing.into())
}

pub fn repair_unescaped_ampersands(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut index = 0;
    let bytes = xml.as_bytes();
    let mut in_cdata = false;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"<![CDATA[") {
            in_cdata = true;
            out.push_str("<![CDATA[");
            index += 9;
            continue;
        }
        if in_cdata && bytes[index..].starts_with(b"]]>") {
            in_cdata = false;
            out.push_str("]]>");
            index += 3;
            continue;
        }
        if !in_cdata && bytes[index] == b'&' {
            let remainder = &xml[index + 1..];
            let valid = ["amp;", "lt;", "gt;", "quot;", "apos;"]
                .iter()
                .any(|name| remainder.starts_with(name))
                || valid_numeric_reference(remainder);
            if valid {
                out.push('&');
            } else {
                out.push_str("&amp;");
            }
            index += 1;
        } else {
            let c = xml[index..].chars().next().expect("valid UTF-8 boundary");
            out.push(c);
            index += c.len_utf8();
        }
    }
    out
}

fn valid_numeric_reference(input: &str) -> bool {
    if let Some(tail) = input
        .strip_prefix("#x")
        .or_else(|| input.strip_prefix("#X"))
    {
        let digits = tail.bytes().take_while(u8::is_ascii_hexdigit).count();
        return (1..=8).contains(&digits) && tail.as_bytes().get(digits) == Some(&b';');
    }
    if let Some(tail) = input.strip_prefix('#') {
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        return (1..=10).contains(&digits) && tail.as_bytes().get(digits) == Some(&b';');
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_polish_year_and_long_episodes() {
        assert_eq!(
            title_and_year("Lombard. Życie pod zastaw *2026* [E1039]"),
            ("Lombard. Życie pod zastaw".into(), "2026".into())
        );
        assert_eq!(
            series_episode(
                "Lombard. Życie pod zastaw *2026* [E1039]",
                "<br>Sezon: 19<br>Epizod: 1039"
            ),
            ("19".into(), "1039".into())
        );
        assert_eq!(
            series_episode("Lombard *2026* [E1036 - 1039]", ""),
            ("".into(), "1036–1039".into())
        );
        assert_eq!(
            series_episode("Lombard *2026* [E1030 - 1035 [HDTVRip]", ""),
            ("".into(), "1030–1035".into())
        );
    }

    #[test]
    fn covers_both_episode_layouts_and_long_episode_counts() {
        assert_eq!(
            series_episode("ZMORA [2026] [S01] [E02]", ""),
            ("01".into(), "02".into())
        );
        assert_eq!(
            series_episode("Last.Seen.2026.S01E06", ""),
            ("01".into(), "06".into())
        );
        assert_eq!(
            series_episode("Szpital św. Anny S04E156", ""),
            ("04".into(), "156".into())
        );
        assert_eq!(
            series_episode("W jedności siła. Żołnierze Kompanii E 2001", ""),
            ("".into(), "".into())
        );
    }

    #[test]
    fn keeps_ranges_and_polish_original_separate_from_dubbing() {
        assert_eq!(
            title_and_year("M3GAN 1-2 (2022-2025)"),
            ("M3GAN 1-2".into(), "2025".into())
        );
        assert!(is_polish_original("ZMORA [SERIAL-PL]"));
        assert!(is_polish_original("Gra z Cieniem [Serial Polski]"));
        assert!(!is_polish_original("Example [PL]"));
        assert_eq!(
            audio_tags("Film [PL]"),
            ("Nie".into(), "Nie".into(), "Nie".into())
        );
        assert_eq!(
            audio_tags("Film [Lektor PL i Napisy PL]"),
            ("Lektor PL".into(), "Napisy PL".into(), "Nie".into())
        );
        assert_eq!(
            audio_tags("Film [Dubbing i napisy PL]"),
            ("Nie".into(), "Napisy PL".into(), "Dubbing PL".into())
        );
    }

    #[test]
    fn repairs_only_unescaped_entities_outside_cdata() {
        let xml = "<rss><item><title>Dungeons & Dragons &amp; more</title><description><![CDATA[Fish & Chips]]></description></item></rss>";
        let repaired = repair_unescaped_ampersands(xml);
        assert!(repaired.contains("Dungeons &amp; Dragons &amp; more"));
        assert!(repaired.contains("<![CDATA[Fish & Chips]]>"));
        assert!(roxmltree::Document::parse(&repaired).is_ok());
    }
}
