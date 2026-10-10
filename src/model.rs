use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedItem {
    pub source: String,
    pub category: String,
    pub title: String,
    pub year: String,
    pub quality: String,
    pub lektor: String,
    pub napisy: String,
    pub dubbing: String,
    pub thumb: String,
    pub link: String,
    pub pub_date: DateTime<Utc>,
    pub season: String,
    pub episode: String,
    #[serde(default)]
    pub original_polish: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeedMeta {
    pub etag: Option<String>,
    pub modified: Option<String>,
}
