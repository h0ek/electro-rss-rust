# Electro RSS (Rust)
Aplikacja desktopowa do listowania najnowszych materiałów z electro-torrent.pl za pomocą RSS.

Jestem leniwy i nie chce mi się przeglądać całej strony w poszukiwaniu materiałów z 2024, 2025 i 2026, a tylko takie mnie interesują. Jeśli też jesteś leniwy jak ja — ta aplikacja zrobi to za Ciebie.

To ulepszona wersja poprzedniego projektu w Pythonie:
- szybsza
- GUI w GTK4 / libadwaita
- cache + miniatury
- filtrowanie po latach i kategorii

## Funkcje
- RSS parser (bez API)
- filtrowanie:
  - po tytule
  - po roku (2024 / 2025 / 2026)
  - po kategorii (Film / Seriale)
- cache danych + miniatur
- statystyki cache (rozmiar / ilość)
- otwieranie linków w przeglądarce

## Wymagania
### Fedora
```bash
sudo dnf install gtk4-devel libadwaita-devel openssl-devel pkgconf-pkg-config gcc clang
```
### Debian / Ubuntu
```bash
sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config build-essential
```
### Arch
```bash
sudo pacman -S gtk4 libadwaita base-devel
```
## Build
```bash
cargo build --release
```
## Uruchamianie
Dev:
```bash
cargo run
```
Release:
```bash
./target/release/electro-rss-rust
```

## Cache
Dane i miniatury zapisują się w:
```bash
~/.cache/electro_rss_rust/
```

## Poprzednia wersja (Python)
Stara wersja projektu (Python + GTK3):
https://github.com/h0ek/electro-rss
