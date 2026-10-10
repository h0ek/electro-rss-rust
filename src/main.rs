mod logger;
mod metadata;
mod model;
mod rss_client;
mod rss_sources;
mod storage;

use adw::prelude::*;
use chrono::{Duration, Local, Utc};
use gtk::glib;
use gtk::glib::prelude::IsA;
use model::FeedItem;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration as StdDuration;

use logger::{log_error, log_info};

const APP_ID: &str = "pl.hoek.electrorss";
const DISPLAY_LIMIT: usize = 300;

type PictureRegistry = Rc<RefCell<Vec<(gtk::Picture, String, bool)>>>;

#[derive(Clone)]
struct Ui {
    window: adw::ApplicationWindow,
    items: Rc<RefCell<Vec<FeedItem>>>,
    pictures: PictureRegistry,
    list: gtk::ListBox,
    grid: gtk::FlowBox,
    pages: gtk::Stack,
    search: gtk::SearchEntry,
    year_from: gtk::SpinButton,
    year_to: gtk::SpinButton,
    unknown_year: gtk::CheckButton,
    category: gtk::DropDown,
    audio: gtk::DropDown,
    quality: gtk::DropDown,
    published: gtk::DropDown,
    view: gtk::DropDown,
    info: gtk::Label,
    cache: gtk::Label,
    status: gtk::Label,
}

enum RefreshMessage {
    Data(Vec<FeedItem>, usize),
    Thumbnails(usize, usize),
}

fn year_matches(item: &FeedItem, from: i32, to: i32, unknown: bool) -> bool {
    if let Ok(year) = item.year.parse::<i32>() {
        year >= from && year <= to
    } else {
        unknown
    }
}

fn recognized(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.trim().eq_ignore_ascii_case("Nie")
        && !value.trim().eq_ignore_ascii_case("Film Polski")
}

fn item_matches(item: &FeedItem, ui: &Ui, search: &str) -> bool {
    if !search.is_empty() && !item.title.to_lowercase().contains(search) {
        return false;
    }
    if !year_matches(
        item,
        ui.year_from.value_as_int(),
        ui.year_to.value_as_int(),
        ui.unknown_year.is_active(),
    ) {
        return false;
    }
    match ui.category.selected() {
        1 if item.category != "Film" => return false,
        2 if item.category != "Seriale" => return false,
        _ => {}
    }
    let lektor = recognized(&item.lektor);
    let dubbing = recognized(&item.dubbing);
    let napisy = recognized(&item.napisy);
    match ui.audio.selected() {
        1 if !lektor => return false,
        2 if !dubbing => return false,
        3 if !napisy => return false,
        4 if !item.original_polish => return false,
        5 if lektor || dubbing || napisy || item.original_polish => return false,
        _ => {}
    }
    let quality = item.quality.to_ascii_lowercase();
    match ui.quality.selected() {
        1 if quality != "2160p" => return false,
        2 if quality != "1080p" => return false,
        3 if quality != "720p" => return false,
        4 if !quality.is_empty() => return false,
        _ => {}
    }
    let cutoff_days = match ui.published.selected() {
        1 => Some(7),
        2 => Some(30),
        3 => Some(90),
        _ => None,
    };
    if let Some(days) = cutoff_days
        && item.pub_date < Utc::now() - Duration::days(days)
    {
        return false;
    }
    true
}

fn text_line(container: &gtk::Box, text: &str) {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_selectable(false);
    container.append(&label);
}

fn open_link(url: &str) {
    if (url.starts_with("https://") || url.starts_with("http://"))
        && let Err(e) = webbrowser::open(url)
    {
        log_error(&format!("Browser error: {e}"));
    }
}

fn details(window: &adw::ApplicationWindow, item: &FeedItem) {
    let dialog = gtk::Window::builder()
        .title("Release details")
        .modal(true)
        .transient_for(window)
        .default_width(480)
        .build();
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 8);
    box_.set_margin_start(16);
    box_.set_margin_end(16);
    box_.set_margin_top(16);
    box_.set_margin_bottom(16);
    text_line(&box_, &format!("{} ({})", item.title, item.year));
    text_line(
        &box_,
        &format!("Category: {} | Source: {}", item.category, item.source),
    );
    text_line(&box_, &format!("Quality: {}", item.quality));
    text_line(&box_, &format!("Voice-over: {}", item.lektor));
    text_line(&box_, &format!("Subtitles: {}", item.napisy));
    text_line(&box_, &format!("Dubbing: {}", item.dubbing));
    if !item.season.is_empty() {
        text_line(&box_, &format!("Season: {}", item.season));
    }
    if !item.episode.is_empty() {
        text_line(&box_, &format!("Episode: {}", item.episode));
    }
    if item.original_polish {
        text_line(&box_, "Original audio: Polish");
    }
    text_line(
        &box_,
        &format!(
            "Published: {}",
            item.pub_date.with_timezone(&Local).format("%Y-%m-%d %H:%M")
        ),
    );
    let open = gtk::Button::with_label("Open release in browser");
    open.set_sensitive(item.link.starts_with("https://") || item.link.starts_with("http://"));
    let link = item.link.clone();
    open.connect_clicked(move |_| open_link(&link));
    box_.append(&open);
    let close = gtk::Button::with_label("Close");
    let dialog_to_close = dialog.clone();
    close.connect_clicked(move |_| dialog_to_close.close());
    box_.append(&close);
    dialog.set_child(Some(&box_));
    dialog.present();
}

fn poster(item: &FeedItem, width: i32, height: i32, pictures: &PictureRegistry) -> gtk::Picture {
    let picture = gtk::Picture::new();
    picture.set_size_request(width, height);
    picture.set_can_shrink(true);
    if !item.thumb.is_empty() {
        let path = storage::thumb_path(&item.thumb);
        let loaded = path.is_file();
        if loaded {
            picture.set_filename(Some(path));
        }
        pictures
            .borrow_mut()
            .push((picture.clone(), item.thumb.clone(), loaded));
    }
    picture
}

fn list_row(item: &FeedItem, pictures: &PictureRegistry) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    layout.set_margin_top(7);
    layout.set_margin_bottom(7);
    layout.set_margin_start(7);
    layout.set_margin_end(7);
    layout.append(&poster(item, 88, 122, pictures));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 5);
    body.set_hexpand(true);
    let title = gtk::Label::new(Some(&format!("{} ({})", item.title, item.year)));
    title.set_xalign(0.0);
    title.add_css_class("heading");
    title.set_wrap(true);
    body.append(&title);
    text_line(
        &body,
        &format!("{} · {} · {}", item.category, item.quality, item.source),
    );
    text_line(
        &body,
        &format!(
            "Voice-over: {}   Subtitles: {}   Dubbing: {}",
            item.lektor, item.napisy, item.dubbing
        ),
    );
    if item.original_polish {
        text_line(&body, "Original audio: Polish");
    }
    if !item.season.is_empty() && !item.episode.is_empty() {
        text_line(&body, &format!("S{} E{}", item.season, item.episode));
    } else if !item.episode.is_empty() {
        text_line(&body, &format!("E{}", item.episode));
    } else if !item.season.is_empty() {
        text_line(&body, &format!("S{}", item.season));
    }
    text_line(
        &body,
        &format!(
            "Added: {}",
            item.pub_date.with_timezone(&Local).format("%Y-%m-%d %H:%M")
        ),
    );
    let detail_btn = gtk::Button::with_label("Open");
    detail_btn.set_halign(gtk::Align::Start);
    detail_btn.set_sensitive(item.link.starts_with("https://") || item.link.starts_with("http://"));
    let link = item.link.clone();
    detail_btn.connect_clicked(move |_| open_link(&link));
    body.append(&detail_btn);
    layout.append(&body);
    row.set_child(Some(&layout));
    row
}

fn grid_cell(
    item: &FeedItem,
    window: &adw::ApplicationWindow,
    pictures: &PictureRegistry,
) -> gtk::FlowBoxChild {
    let cell = gtk::FlowBoxChild::new();
    let button = gtk::Button::new();
    let layout = gtk::Box::new(gtk::Orientation::Vertical, 5);
    layout.set_size_request(170, -1);
    layout.set_halign(gtk::Align::Center);
    layout.append(&poster(item, 165, 210, pictures));
    let title = gtk::Label::new(Some(&format!("{} ({})", item.title, item.year)));
    title.set_wrap(true);
    title.set_max_width_chars(23);
    title.set_lines(2);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("heading");
    layout.append(&title);
    let metadata = gtk::Label::new(Some(&format!("{} · {}", item.category, item.quality)));
    metadata.set_wrap(true);
    layout.append(&metadata);
    let audio = if item.original_polish {
        "Polish original"
    } else if recognized(&item.lektor) {
        "Voice-over"
    } else if recognized(&item.dubbing) {
        "Dubbing"
    } else if recognized(&item.napisy) {
        "Subtitles"
    } else {
        "Unidentified audio"
    };
    layout.append(&gtk::Label::new(Some(audio)));
    button.set_child(Some(&layout));
    let release = item.clone();
    let parent = window.clone();
    button.connect_clicked(move |_| details(&parent, &release));
    cell.set_child(Some(&button));
    cell
}

fn update_visible_thumbnails(ui: &Ui) {
    for (picture, url, loaded) in ui.pictures.borrow_mut().iter_mut() {
        if *loaded {
            continue;
        }
        let path = storage::thumb_path(url);
        if path.is_file() {
            picture.set_filename(Some(path));
            *loaded = true;
        }
    }
}

fn render(ui: &Ui) {
    ui.pictures.borrow_mut().clear();
    let query = ui.search.text().to_string().trim().to_lowercase();
    let mut results: Vec<FeedItem> = ui
        .items
        .borrow()
        .iter()
        .filter(|it| item_matches(it, ui, &query))
        .cloned()
        .collect();
    results.sort_by(|a, b| b.pub_date.cmp(&a.pub_date));
    let count = results.len();
    let visible = count.min(DISPLAY_LIMIT);
    ui.info.set_text(&format!(
        "Matches: {count} / Saved: {} · Showing: {visible}",
        ui.items.borrow().len()
    ));
    let (files, size) = storage::cache_stats();
    ui.cache.set_text(&format!(
        "Cache: {files} files · {}",
        storage::human_bytes(size)
    ));
    while let Some(child) = ui.list.first_child() {
        ui.list.remove(&child);
    }
    while let Some(child) = ui.grid.first_child() {
        ui.grid.remove(&child);
    }
    match ui.view.selected() {
        1 => {
            ui.pages.set_visible_child_name("grid");
            for item in results.iter().take(DISPLAY_LIMIT) {
                ui.grid
                    .insert(&grid_cell(item, &ui.window, &ui.pictures), -1);
            }
        }
        _ => {
            ui.pages.set_visible_child_name("list");
            for item in results.iter().take(DISPLAY_LIMIT) {
                ui.list.append(&list_row(item, &ui.pictures));
            }
        }
    }
}

fn labeled_control<T: IsA<gtk::Widget>>(label: &str, control: &T) -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let title = gtk::Label::new(Some(label));
    title.set_xalign(0.0);
    box_.append(&title);
    box_.append(control);
    box_
}

fn show_source_settings(ui: &Ui) {
    let dialog = gtk::Window::builder()
        .title("RSS sources")
        .modal(true)
        .transient_for(&ui.window)
        .default_width(470)
        .default_height(460)
        .build();
    let layout = gtk::Box::new(gtk::Orientation::Vertical, 12);
    layout.set_margin_start(16);
    layout.set_margin_end(16);
    layout.set_margin_top(16);
    layout.set_margin_bottom(16);
    text_line(
        &layout,
        "Choose which RSS feeds to download. Saved entries are kept when you disable a feed.",
    );
    let chosen: HashSet<u16> = rss_sources::load_selected().into_iter().collect();
    let choices = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let mut buttons = Vec::new();
    for source in rss_sources::SOURCES {
        let check = gtk::CheckButton::with_label(source.label);
        check.set_active(chosen.contains(&source.id));
        choices.append(&check);
        buttons.push((source.id, check));
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&choices));
    layout.append(&scroll);
    let message = gtk::Label::new(None);
    message.set_xalign(0.0);
    message.set_wrap(true);
    layout.append(&message);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let save = gtk::Button::with_label("Save sources");
    save.add_css_class("suggested-action");
    let cancel_dialog = dialog.clone();
    cancel.connect_clicked(move |_| cancel_dialog.close());
    let save_dialog = dialog.clone();
    let state = ui.clone();
    save.connect_clicked(move |_| {
        let ids: Vec<u16> = buttons
            .iter()
            .filter(|(_, check)| check.is_active())
            .map(|(id, _)| *id)
            .collect();
        if ids.is_empty() {
            message.set_text("Select at least one RSS source.");
            return;
        }
        match rss_sources::save_selected(&ids) {
            Ok(()) => {
                state.status.set_text(&format!(
                    "RSS sources saved: {} enabled. Click Refresh RSS to fetch them.",
                    ids.len()
                ));
                save_dialog.close();
            }
            Err(e) => {
                log_error(&format!("Cannot save RSS sources: {e:#}"));
                message.set_text(&format!("Cannot save RSS sources: {e}"));
            }
        }
    });
    actions.append(&cancel);
    actions.append(&save);
    layout.append(&actions);
    dialog.set_child(Some(&layout));
    dialog.present();
}

fn build_ui(app: &adw::Application) {
    let window = adw::ApplicationWindow::new(app);
    window.set_title(Some("Electro RSS"));
    window.set_default_size(1150, 800);

    let header = adw::HeaderBar::new();
    let refresh = gtk::Button::with_label("Refresh RSS");
    let clear = gtk::Button::with_label("Clear history");
    let source_settings = gtk::Button::with_label("RSS sources");
    let spinner = gtk::Spinner::new();
    header.pack_start(&refresh);
    header.pack_start(&clear);
    header.pack_start(&source_settings);
    header.pack_end(&spinner);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search by title"));
    search.set_hexpand(true);
    let year_from = gtk::SpinButton::with_range(1900.0, 2100.0, 1.0);
    year_from.set_value(2024.0);
    let year_to = gtk::SpinButton::with_range(1900.0, 2100.0, 1.0);
    year_to.set_value(chrono::Datelike::year(&Local::now()) as f64);
    let unknown_year = gtk::CheckButton::with_label("Unknown year");
    unknown_year.set_active(false);
    let category = gtk::DropDown::from_strings(&["All", "Movies", "TV Series"]);
    let audio = gtk::DropDown::from_strings(&[
        "All",
        "Voice-over",
        "Dubbing",
        "Subtitles",
        "Polish original",
        "Unidentified",
    ]);
    let quality = gtk::DropDown::from_strings(&["All", "2160p", "1080p", "720p", "Unknown"]);
    let published = gtk::DropDown::from_strings(&["Any time", "7 days", "30 days", "90 days"]);
    published.set_selected(2);
    let view = gtk::DropDown::from_strings(&["List", "Grid"]);

    let filters = gtk::FlowBox::new();
    filters.set_selection_mode(gtk::SelectionMode::None);
    filters.set_min_children_per_line(1);
    filters.set_max_children_per_line(8);
    filters.set_column_spacing(12);
    filters.set_row_spacing(8);
    filters.set_homogeneous(false);
    filters.set_valign(gtk::Align::Start);
    filters.set_hexpand(true);
    filters.insert(&labeled_control("From year", &year_from), -1);
    filters.insert(&labeled_control("To year", &year_to), -1);
    filters.insert(&unknown_year, -1);
    filters.insert(&labeled_control("Category", &category), -1);
    filters.insert(&labeled_control("Audio / subtitles", &audio), -1);
    filters.insert(&labeled_control("Quality", &quality), -1);
    filters.insert(&labeled_control("Added", &published), -1);
    filters.insert(&labeled_control("View", &view), -1);

    let info = gtk::Label::new(None);
    let cache = gtk::Label::new(None);
    let status = gtk::Label::new(Some("Ready"));
    info.set_xalign(0.0);
    cache.set_xalign(0.0);
    status.set_xalign(0.0);
    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    summary.append(&info);
    summary.append(&cache);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    let list_scroll = gtk::ScrolledWindow::new();
    list_scroll.set_vexpand(true);
    list_scroll.set_child(Some(&list));
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_min_children_per_line(2);
    grid.set_max_children_per_line(6);
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    let grid_scroll = gtk::ScrolledWindow::new();
    grid_scroll.set_vexpand(true);
    grid_scroll.set_child(Some(&grid));
    let pages = gtk::Stack::new();
    pages.add_named(&list_scroll, Some("list"));
    pages.add_named(&grid_scroll, Some("grid"));
    pages.set_vexpand(true);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&search);
    content.append(&filters);
    content.append(&summary);
    content.append(&status);
    content.append(&pages);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&header);
    root.append(&content);
    window.set_content(Some(&root));

    let ui = Ui {
        window: window.clone(),
        items: Rc::new(RefCell::new(storage::load_items())),
        pictures: Rc::new(RefCell::new(Vec::new())),
        list,
        grid,
        pages,
        search,
        year_from,
        year_to,
        unknown_year,
        category,
        audio,
        quality,
        published,
        view,
        info,
        cache,
        status,
    };
    render(&ui);

    {
        let ui = ui.clone();
        let control = ui.search.clone();
        control.connect_search_changed(move |_| render(&ui));
    }
    {
        let ui = ui.clone();
        let control = ui.year_from.clone();
        control.connect_value_changed(move |_| render(&ui));
    }
    {
        let ui = ui.clone();
        let control = ui.year_to.clone();
        control.connect_value_changed(move |_| render(&ui));
    }
    {
        let ui = ui.clone();
        let control = ui.unknown_year.clone();
        control.connect_toggled(move |_| render(&ui));
    }
    for control in [
        &ui.category,
        &ui.audio,
        &ui.quality,
        &ui.published,
        &ui.view,
    ] {
        let state = ui.clone();
        control.connect_selected_notify(move |_| render(&state));
    }
    {
        let ui = ui.clone();
        source_settings.connect_clicked(move |_| show_source_settings(&ui));
    }
    {
        let ui = ui.clone();
        clear.connect_clicked(move |_| {
            let confirm = gtk::Window::builder()
                .title("Delete saved history?")
                .modal(true)
                .transient_for(&ui.window)
                .default_width(330)
                .build();
            let layout = gtk::Box::new(gtk::Orientation::Vertical, 12);
            layout.set_margin_top(16);
            layout.set_margin_bottom(16);
            layout.set_margin_start(16);
            layout.set_margin_end(16);
            text_line(&layout, "This deletes all saved entries and thumbnails.");
            let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let cancel = gtk::Button::with_label("Cancel");
            let delete = gtk::Button::with_label("Delete");
            delete.add_css_class("destructive-action");
            let cancel_window = confirm.clone();
            cancel.connect_clicked(move |_| cancel_window.close());
            let state = ui.clone();
            let delete_window = confirm.clone();
            delete.connect_clicked(move |_| {
                if let Err(e) = storage::clear_cache() {
                    state.status.set_text(&format!("Clear error: {e}"));
                } else {
                    state.items.borrow_mut().clear();
                    state.status.set_text("History cleared");
                    render(&state);
                }
                delete_window.close();
            });
            actions.append(&cancel);
            actions.append(&delete);
            layout.append(&actions);
            confirm.set_child(Some(&layout));
            confirm.present();
        });
    }
    {
        let ui = ui.clone();
        let spinner = spinner.clone();
        let refresh_button = refresh.clone();
        let clear_button = clear.clone();
        refresh.connect_clicked(move |_| {
            spinner.start();
            refresh_button.set_sensitive(false);
            clear_button.set_sensitive(false);
            let enabled = rss_sources::load_selected();
            ui.status
                .set_text(&format!("Updating {} RSS feeds...", enabled.len()));
            let (tx, rx) = std::sync::mpsc::channel::<RefreshMessage>();
            std::thread::spawn(move || {
                let items_before = storage::load_items();
                let previous: HashSet<String> = items_before
                    .iter()
                    .map(|item| {
                        if item.link.is_empty() {
                            format!("{}|{}|{}", item.source, item.title, item.pub_date)
                        } else {
                            item.link.clone()
                        }
                    })
                    .collect();
                let fetched = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt.block_on(rss_client::fetch_items(
                        36500,
                        storage::load_state(),
                        items_before,
                        enabled,
                    )),
                    Err(e) => Err(e.into()),
                };
                let items = match fetched {
                    Ok((items, state)) => {
                        if let Err(e) = storage::save_items(&items) {
                            log_error(&format!("Save items error: {e}"));
                        }
                        if let Err(e) = storage::save_state(&state) {
                            log_error(&format!("Save HTTP state error: {e}"));
                        }
                        items
                    }
                    Err(e) => {
                        log_error(&format!("RSS refresh error: {e:#}"));
                        storage::load_items()
                    }
                };
                let new_count = items
                    .iter()
                    .filter(|item| {
                        let key = if item.link.is_empty() {
                            format!("{}|{}|{}", item.source, item.title, item.pub_date)
                        } else {
                            item.link.clone()
                        };
                        !previous.contains(&key)
                    })
                    .count();
                let candidates: Vec<_> = items
                    .iter()
                    .filter(|it| !it.thumb.is_empty() && !storage::thumb_path(&it.thumb).exists())
                    .take(DISPLAY_LIMIT)
                    .cloned()
                    .collect();
                let _ = tx.send(RefreshMessage::Data(items, new_count));
                if !candidates.is_empty() {
                    let mut seen_thumbs = HashSet::new();
                    let unique: Vec<_> = candidates
                        .into_iter()
                        .filter(|item| seen_thumbs.insert(item.thumb.clone()))
                        .collect();
                    let total = unique.len();
                    for (index, chunk) in unique.chunks(12).enumerate() {
                        rss_client::prefetch_thumbs(chunk);
                        let processed = ((index + 1) * 12).min(total);
                        if tx
                            .send(RefreshMessage::Thumbnails(processed, total))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            });
            let ui = ui.clone();
            let spinner = spinner.clone();
            let refresh_button = refresh_button.clone();
            let clear_button = clear_button.clone();
            glib::timeout_add_local(StdDuration::from_millis(150), move || match rx.try_recv() {
                Ok(RefreshMessage::Data(items, added)) => {
                    log_info(&format!(
                        "GUI refresh complete, {} items, {} new",
                        items.len(),
                        added
                    ));
                    *ui.items.borrow_mut() = items;
                    render(&ui);
                    ui.status
                        .set_text(&format!("RSS refresh finished · {added} new entries"));
                    spinner.stop();
                    refresh_button.set_sensitive(true);
                    clear_button.set_sensitive(true);
                    glib::ControlFlow::Continue
                }
                Ok(RefreshMessage::Thumbnails(done, total)) => {
                    update_visible_thumbnails(&ui);
                    if done == total {
                        ui.status.set_text(&format!(
                            "Thumbnail check complete · {total} images checked"
                        ));
                        glib::ControlFlow::Break
                    } else {
                        ui.status
                            .set_text(&format!("Thumbnails: {done}/{total} checked"));
                        glib::ControlFlow::Continue
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    spinner.stop();
                    refresh_button.set_sensitive(true);
                    clear_button.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            });
        });
    }
    window.present();
}

fn main() {
    let log_path = std::env::temp_dir().join("electro-rss-rust.log");
    logger::init_logger(&log_path);
    adw::init().expect("failed to init libadwaita");
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(year: &str) -> FeedItem {
        FeedItem {
            source: "Test".into(),
            category: "Film".into(),
            title: "Example".into(),
            year: year.into(),
            quality: "1080p".into(),
            lektor: "Nie".into(),
            napisy: "Nie".into(),
            dubbing: "Nie".into(),
            thumb: String::new(),
            link: String::new(),
            pub_date: Utc::now(),
            season: String::new(),
            episode: String::new(),
            original_polish: false,
        }
    }

    #[test]
    fn year_range_is_inclusive() {
        assert!(year_matches(&sample("2024"), 2024, 2026, false));
        assert!(year_matches(&sample("2026"), 2024, 2026, false));
        assert!(!year_matches(&sample("2023"), 2024, 2026, false));
    }

    #[test]
    fn unidentified_year_is_configurable() {
        assert!(!year_matches(&sample(""), 2024, 2026, false));
        assert!(year_matches(&sample(""), 2024, 2026, true));
    }

    #[test]
    fn missing_audio_metadata_is_not_detected_audio() {
        assert!(!recognized("Nie"));
        assert!(!recognized("Film Polski"));
        assert!(!recognized(""));
        assert!(recognized("Lektor PL"));
    }
}
