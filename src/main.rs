mod logger;
mod model;
mod rss_client;
mod storage;

use adw::prelude::*;
use gtk::glib;
use gtk::pango::WrapMode;
use std::cell::RefCell;
use std::rc::Rc;

use logger::{log_error, log_info};
use model::FeedItem;

const APP_ID: &str = "pl.hoek.electrorss";

fn add_line(parent: &gtk::Box, text: &str) {
    let lbl = gtk::Label::new(Some(text));
    lbl.set_xalign(0.0);
    lbl.set_wrap(true);
    lbl.set_wrap_mode(WrapMode::WordChar);
    lbl.set_selectable(false);
    parent.append(&lbl);
}

fn item_matches_filters(
    it: &FeedItem,
    q: &str,
    year_2024: bool,
    year_2025: bool,
    year_2026: bool,
    show_films: bool,
    show_serials: bool,
) -> bool {
    let q_ok = q.trim().is_empty() || it.title.to_lowercase().contains(q);

    let year_ok = (it.year == "2024" && year_2024)
        || (it.year == "2025" && year_2025)
        || (it.year == "2026" && year_2026);

    let category_ok =
        (it.category == "Film" && show_films) || (it.category == "Seriale" && show_serials);

    q_ok && year_ok && category_ok
}

fn rebuild_list(list: &gtk::ListBox, items: &[FeedItem]) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    for it in items {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(8);
        row_box.set_margin_start(8);
        row_box.set_margin_end(8);

        let picture = gtk::Picture::new();
        picture.set_size_request(140, 210);
        picture.set_can_shrink(true);
        picture.set_halign(gtk::Align::Start);
        picture.set_valign(gtk::Align::Start);

        if !it.thumb.is_empty() {
            let path = storage::thumb_path(&it.thumb);
            if path.exists() {
                picture.set_filename(Some(path));
            }
        }

        let image_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        image_box.set_size_request(140, 210);
        image_box.append(&picture);

        row_box.append(&image_box);

        let right_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        right_box.set_hexpand(true);

        let title = gtk::Label::new(None);
        title.set_xalign(0.0);
        title.set_use_markup(true);
        title.set_wrap(true);
        title.set_wrap_mode(WrapMode::WordChar);
        title.set_max_width_chars(60);
        title.set_markup(&format!(
            "<b>{} ({})</b>",
            glib::markup_escape_text(&it.title),
            glib::markup_escape_text(&it.year)
        ));

        right_box.append(&title);
        let category_label = gtk::Label::new(None);
        category_label.set_xalign(0.0);
        category_label.set_use_markup(true);

        let category_markup = if it.category == "Film" {
            format!(
                "<span foreground='darkblue'><b>Kategoria: {}</b></span>",
                glib::markup_escape_text(&it.category)
            )
        } else {
            format!(
                "<span foreground='deepskyblue'><b>Kategoria: {}</b></span>",
                glib::markup_escape_text(&it.category)
            )
        };

        category_label.set_markup(&category_markup);
        right_box.append(&category_label);
        add_line(&right_box, &format!("Jakość: {}", it.quality));
        add_line(&right_box, &format!("Lektor: {}", it.lektor));
        add_line(&right_box, &format!("Napisy: {}", it.napisy));
        add_line(&right_box, &format!("Dubbing: {}", it.dubbing));

        if !it.season.is_empty() {
            if !it.episode.is_empty() {
                add_line(
                    &right_box,
                    &format!("Sezon: {} | Odcinek: {}", it.season, it.episode),
                );
            } else {
                add_line(&right_box, &format!("Sezon: {}", it.season));
            }
        }

        add_line(
            &right_box,
            &format!(
                "Data dodania: {}",
                it.pub_date
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
            ),
        );

        let btn = gtk::Button::with_label("Otwórz");
        let url = it.link.clone();
        btn.connect_clicked(move |_| {
            let _ = webbrowser::open(&url);
        });

        btn.set_halign(gtk::Align::Start);
        btn.set_hexpand(false);
        right_box.append(&btn);

        row_box.append(&right_box);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_box));
        list.append(&row);
    }
}

fn refresh_stats(cache_label: &gtk::Label, info_label: &gtk::Label, filtered_count: usize) {
    let (files, bytes) = storage::cache_stats();
    cache_label.set_text(&format!(
        "Cache: {} plików, {}",
        files,
        storage::human_bytes(bytes)
    ));
    info_label.set_text(&format!("Pozycji: {}", filtered_count));
}

fn apply_filters_and_refresh_ui(
    list: &gtk::ListBox,
    items_state: &Rc<RefCell<Vec<FeedItem>>>,
    filtered_state: &Rc<RefCell<Vec<FeedItem>>>,
    search: &gtk::SearchEntry,
    year_2024: &gtk::CheckButton,
    year_2025: &gtk::CheckButton,
    year_2026: &gtk::CheckButton,
    film_btn: &gtk::CheckButton,
    serial_btn: &gtk::CheckButton,
    cache_label: &gtk::Label,
    info_label: &gtk::Label,
) {
    let q = search.text().to_string().to_lowercase();

    let y2024 = year_2024.is_active();
    let y2025 = year_2025.is_active();
    let y2026 = year_2026.is_active();
    let show_films = film_btn.is_active();
    let show_serials = serial_btn.is_active();

    let filtered: Vec<FeedItem> = items_state
        .borrow()
        .iter()
        .filter(|it| item_matches_filters(it, &q, y2024, y2025, y2026, show_films, show_serials))
        .cloned()
        .collect();

    *filtered_state.borrow_mut() = filtered.clone();
    rebuild_list(list, &filtered);
    refresh_stats(cache_label, info_label, filtered.len());
}

fn build_ui(app: &adw::Application) {
    let items_state = Rc::new(RefCell::new(storage::load_items()));
    let filtered_state = Rc::new(RefCell::new(storage::load_items()));

    let window = adw::ApplicationWindow::new(app);
    window.set_title(Some("Electro RSS"));
    window.set_default_size(1100, 760);
    window.set_icon_name(Some("torrents"));

    let header = adw::HeaderBar::new();

    let refresh_btn = gtk::Button::with_label("Odśwież");
    let clear_btn = gtk::Button::with_label("Wyczyść");
    let spinner = gtk::Spinner::new();

    let days = gtk::ComboBoxText::new();
    days.append(Some("3"), "3 dni");
    days.append(Some("7"), "7 dni");
    days.append(Some("14"), "14 dni");
    days.append(Some("30"), "30 dni");
    days.set_active_id(Some("7"));

    let year_2024 = gtk::CheckButton::with_label("2024");
    year_2024.set_active(true);

    let year_2025 = gtk::CheckButton::with_label("2025");
    year_2025.set_active(true);

    let year_2026 = gtk::CheckButton::with_label("2026");
    year_2026.set_active(true);

    let film_btn = gtk::CheckButton::with_label("Film");
    film_btn.set_active(true);

    let serial_btn = gtk::CheckButton::with_label("Seriale");
    serial_btn.set_active(true);

    header.pack_start(&refresh_btn);
    header.pack_start(&clear_btn);
    header.pack_start(&days);
    header.pack_start(&year_2024);
    header.pack_start(&year_2025);
    header.pack_start(&year_2026);
    header.pack_start(&film_btn);
    header.pack_start(&serial_btn);
    header.pack_end(&spinner);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Filtr tytułu…"));

    let cache_label = gtk::Label::new(None);
    cache_label.set_xalign(0.0);

    let info_label = gtk::Label::new(None);
    info_label.set_xalign(0.0);

    let stats_box = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    stats_box.append(&cache_label);
    stats_box.append(&info_label);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&list));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&search);
    content.append(&stats_box);
    content.append(&scrolled);

    let root_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root_box.append(&header);
    root_box.append(&content);

    window.set_content(Some(&root_box));

    apply_filters_and_refresh_ui(
        &list,
        &items_state,
        &filtered_state,
        &search,
        &year_2024,
        &year_2025,
        &year_2026,
        &film_btn,
        &serial_btn,
        &cache_label,
        &info_label,
    );

    {
        let list = list.clone();
        let items_state = items_state.clone();
        let filtered_state = filtered_state.clone();
        let search_widget = search.clone();
        let search_for_cb = search.clone();
        let year_2024 = year_2024.clone();
        let year_2025 = year_2025.clone();
        let year_2026 = year_2026.clone();
        let film_btn = film_btn.clone();
        let serial_btn = serial_btn.clone();
        let cache_label = cache_label.clone();
        let info_label = info_label.clone();

        search_widget.connect_search_changed(move |_| {
            apply_filters_and_refresh_ui(
                &list,
                &items_state,
                &filtered_state,
                &search_for_cb,
                &year_2024,
                &year_2025,
                &year_2026,
                &film_btn,
                &serial_btn,
                &cache_label,
                &info_label,
            );
        });
    }

    for year_btn in [&year_2024, &year_2025, &year_2026] {
        let list = list.clone();
        let items_state = items_state.clone();
        let filtered_state = filtered_state.clone();
        let search = search.clone();
        let year_2024 = year_2024.clone();
        let year_2025 = year_2025.clone();
        let year_2026 = year_2026.clone();
        let film_btn = film_btn.clone();
        let serial_btn = serial_btn.clone();
        let cache_label = cache_label.clone();
        let info_label = info_label.clone();

        year_btn.connect_toggled(move |_| {
            apply_filters_and_refresh_ui(
                &list,
                &items_state,
                &filtered_state,
                &search,
                &year_2024,
                &year_2025,
                &year_2026,
                &film_btn,
                &serial_btn,
                &cache_label,
                &info_label,
            );
        });
    }

    for cat_btn in [&film_btn, &serial_btn] {
        let list = list.clone();
        let items_state = items_state.clone();
        let filtered_state = filtered_state.clone();
        let search = search.clone();
        let year_2024 = year_2024.clone();
        let year_2025 = year_2025.clone();
        let year_2026 = year_2026.clone();
        let film_btn = film_btn.clone();
        let serial_btn = serial_btn.clone();
        let cache_label = cache_label.clone();
        let info_label = info_label.clone();

        cat_btn.connect_toggled(move |_| {
            apply_filters_and_refresh_ui(
                &list,
                &items_state,
                &filtered_state,
                &search,
                &year_2024,
                &year_2025,
                &year_2026,
                &film_btn,
                &serial_btn,
                &cache_label,
                &info_label,
            );
        });
    }

    {
        let list = list.clone();
        let items_state = items_state.clone();
        let filtered_state = filtered_state.clone();
        let search = search.clone();
        let year_2024 = year_2024.clone();
        let year_2025 = year_2025.clone();
        let year_2026 = year_2026.clone();
        let film_btn = film_btn.clone();
        let serial_btn = serial_btn.clone();
        let cache_label = cache_label.clone();
        let info_label = info_label.clone();

        clear_btn.connect_clicked(move |_| {
            let _ = storage::clear_cache();
            *items_state.borrow_mut() = Vec::new();
            *filtered_state.borrow_mut() = Vec::new();

            apply_filters_and_refresh_ui(
                &list,
                &items_state,
                &filtered_state,
                &search,
                &year_2024,
                &year_2025,
                &year_2026,
                &film_btn,
                &serial_btn,
                &cache_label,
                &info_label,
            );
        });
    }

    {
        let list = list.clone();
        let spinner = spinner.clone();
        let days = days.clone();
        let items_state = items_state.clone();
        let filtered_state = filtered_state.clone();
        let search = search.clone();
        let year_2024 = year_2024.clone();
        let year_2025 = year_2025.clone();
        let year_2026 = year_2026.clone();
        let film_btn = film_btn.clone();
        let serial_btn = serial_btn.clone();
        let cache_label = cache_label.clone();
        let info_label = info_label.clone();

        refresh_btn.connect_clicked(move |_| {
            spinner.start();

            let selected_days = days
                .active_id()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(7);

            let (tx, rx) = std::sync::mpsc::channel::<Vec<FeedItem>>();

            std::thread::spawn(move || {
                log_info(&format!("THREAD START, days={selected_days}"));

                let _ = storage::clear_cache();

                let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
                let state = storage::load_state();
                let old_items = storage::load_items();

                let items = rt
                    .block_on(async {
                        rss_client::fetch_items(selected_days, state, old_items).await
                    })
                    .map(|(items, state)| {
                        log_info(&format!("FETCH OK, items={}", items.len()));
                        let _ = storage::save_items(&items);
                        let _ = storage::save_state(&state);
                        rss_client::prefetch_thumbs(&items);
                        items
                    })
                    .map_err(|e| {
                        log_error(&format!("FETCH ERROR: {e:#}"));
                        e
                    })
                    .unwrap_or_else(|_| storage::load_items());

                let _ = tx.send(items);
            });

            let list2 = list.clone();
            let spinner2 = spinner.clone();
            let items_state2 = items_state.clone();
            let filtered_state2 = filtered_state.clone();
            let search2 = search.clone();
            let year_2024_2 = year_2024.clone();
            let year_2025_2 = year_2025.clone();
            let year_2026_2 = year_2026.clone();
            let film_btn_2 = film_btn.clone();
            let serial_btn_2 = serial_btn.clone();
            let cache_label2 = cache_label.clone();
            let info_label2 = info_label.clone();

            glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
                match rx.try_recv() {
                    Ok(items) => {
                        log_info(&format!("UI GOT ITEMS={}", items.len()));

                        *items_state2.borrow_mut() = items;
                        apply_filters_and_refresh_ui(
                            &list2,
                            &items_state2,
                            &filtered_state2,
                            &search2,
                            &year_2024_2,
                            &year_2025_2,
                            &year_2026_2,
                            &film_btn_2,
                            &serial_btn_2,
                            &cache_label2,
                            &info_label2,
                        );

                        spinner2.stop();
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        spinner2.stop();
                        glib::ControlFlow::Break
                    }
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
