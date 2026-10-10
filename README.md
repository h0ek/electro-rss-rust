# Electro RSS

A GTK4/libadwaita desktop RSS reader for browsing recent movie and TV-series releases from electro-torrent.pl without navigating the entire website.

I mostly care about recent, good-quality releases. Electro RSS fetches the selected RSS feeds, saves a local history, and lets me filter the results. If you're as lazy as I am, it might save you some browsing too.

![Electro RSS](screenshot.webp)

## Features

- RSS-only integration; no site API required
- Choose enabled movie and TV-series feeds in **RSS sources** (six enabled by default)
- Search by title, select a production-year range, and include or exclude unknown years
- Filter by category, release quality, audio/subtitle metadata, and RSS publication date
- Recognize Polish-original series separately from voice-over, dubbing, and subtitles
- Extract season and episode information where the RSS title or description provides it
- Switch between a detailed list and poster grid
- Open release pages in the default browser
- Persist RSS history, HTTP conditional-request metadata, source preferences, and downloaded thumbnails
- Preserve earlier entries while refreshing; deduplicate by release link

**Year range** refers to the production year identified in the release title, whereas **Added** refers to the RSS publication date. The default **Added** filter is 30 days. Changing filters does not delete saved entries.

Entries not currently published in the available RSS feeds cannot automatically be recovered unless they were previously saved.

## Requirements

Rust and Cargo are required to build from source.

### Fedora

```bash
sudo dnf install gtk4-devel libadwaita-devel openssl-devel pkgconf-pkg-config gcc clang
```

### Debian / Ubuntu

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev libssl-dev pkg-config build-essential
```

### Arch Linux

```bash
sudo pacman -S gtk4 libadwaita openssl base-devel pkgconf
```

## Build

```bash
cargo build --locked --release
```

## Run

```bash
cargo run
```

Or launch the release executable:

```bash
./target/release/electro-rss-rust
```

## Data and cache

- RSS history and thumbnails: `~/.cache/electro_rss_rust/`
- RSS source preferences: `~/.config/electro_rss_rust/rss_sources.json`
- Application log: `/tmp/electro-rss-rust.log` on typical Linux systems

**Refresh RSS** requests the enabled feeds again, using ETag and Last-Modified where supplied by the server. It does not re-download thumbnail files already cached. A first refresh can take longer if the server is slow or the cached validators are not honored.

**Clear history** deletes the locally stored entries and thumbnails. It does not change the selected RSS sources. Clearing saved entries is not needed for routine refreshes.


