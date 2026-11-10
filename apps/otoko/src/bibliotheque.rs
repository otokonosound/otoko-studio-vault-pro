//! Carrière › Bibliothèque (comme l'écran Bibliothèque de Studio Vault) : la navigation, les
//! pastilles et la recherche, le tableau aux colonnes choisies, la sélection (P / X / N), le
//! panneau de détails, les archives, les vues enregistrées, les dossiers surveillés.
//! Les fichiers ne bougent jamais ; tout est écrit dans la Carrière.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use otoko_library::career::library::{self as lib, Chips, Nav, Row};
use otoko_library::career::{NATURES, RELEASE_TYPES, STATUSES, nature_short, release_label, status_label};
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use crate::state::{Done, State};
use crate::{AppWindow, ColUi, LibFieldUi, Lib, LibRowUi, NavUi, VersionUi, settings};

macro_rules! lib_on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.global::<Lib>().$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

/// Les rappels de l'écran.
pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    lib_on!(app, state, on_nav_clicked, |app, st, i| { nav_clicked(&app, st, i.max(0) as usize); });
    lib_on!(app, state, on_nav_remove, |app, st, i| { nav_remove(&app, st, i.max(0) as usize); });
    lib_on!(app, state, on_search, |app, st, q| { search(&app, st, q.as_str()); });
    lib_on!(app, state, on_set_chip, |app, st, kind, value| { set_chip(&app, st, kind.as_str(), value.as_str()); });
    lib_on!(app, state, on_reset, |app, st| { reset(&app, st); });
    lib_on!(app, state, on_save_view, |app, st, name| { save_view(&app, st, name.as_str()); });
    lib_on!(app, state, on_sort, |app, st, key| { sort(&app, st, key.as_str()); });
    lib_on!(app, state, on_cards_changed, |app, st, on| {
        if let Some(c) = st.career.as_ref() {
            if !st.selftest {
                lib::set_flag(c.conn(), "library_cards", on);
            }
        }
        app.global::<Lib>().set_cards(on);
    });
    lib_on!(app, state, on_toggle_col, |app, st, key| { toggle_col(&app, st, key.as_str()); });
    lib_on!(app, state, on_row_clicked, |app, st, i, ctrl, shift| { row_clicked(&app, st, i.max(0) as usize, ctrl, shift); });
    lib_on!(app, state, on_row_double, |app, st, i| { row_double(&app, st, i.max(0) as usize); });
    lib_on!(app, state, on_play, |app, st, i| { play(&app, st, i.max(0) as usize); });
    lib_on!(app, state, on_action, |app, st, what| { action(&app, st, what.as_str()); });
    lib_on!(app, state, on_key, |app, st, k| { key(&app, st, k.as_str()); });
    {
        // Les raccourcis du tableau (lettres seules comprises), modifiables dans Réglages › Carrière.
        let (weak, state) = (app.as_weak(), state.clone());
        app.global::<Lib>().on_shortcut(move |text, ctrl, shift, alt| {
            let app = weak.unwrap();
            let Ok(mut st) = state.try_borrow_mut() else { return false };
            crate::raccourcis::key(&app, &mut st, text.as_str(), ctrl, shift, alt, true)
        });
    }
    lib_on!(app, state, on_scan, |app, st| {
        st.lib.last_scan = None;
        scan(&app, st);
    });
    lib_on!(app, state, on_add_folder, |app, st| { add_folder(&app, st); });
    lib_on!(app, state, on_field_edited, |app, st, key, value| { field_edited(&app, st, key.as_str(), value.as_str()); });
    lib_on!(app, state, on_tags_edited, |app, st, text| { tags_edited(&app, st, text.as_str()); });
    lib_on!(app, state, on_version_clicked, |app, st, id| { version_clicked(&app, st, id as i64); });
    lib_on!(app, state, on_close_details, |app, st| { close_details(&app, st); });
    lib_on!(app, state, on_details_tab_changed, |app, st, tab| {
        app.global::<Lib>().set_details_tab(tab);
        details(&app, st);
    });
    lib_on!(app, state, on_bpm_scale, |app, st, factor| { bpm_scale(&app, st, factor as f64); });
    lib_on!(app, state, on_tap, |app, st| { tap(&app, st); });
    lib_on!(app, state, on_tap_keep, |app, st| { tap_keep(&app, st); });
    lib_on!(app, state, on_unlock, |app, st| { unlock(&app, st); });
    lib_on!(app, state, on_player_notes_edited, |app, st, text| { crate::lecteur::notes_edited(&app, st, text.as_str()); });
    lib_on!(app, state, on_dialog_input_edited, |app, st, text| { crate::outils::input_edited(&app, st, text.as_str()); });
    lib_on!(app, state, on_dialog_toggle, |app, st, i| { crate::outils::toggle(&app, st, i.max(0) as usize); });
    lib_on!(app, state, on_dialog_button, |app, st, id| { crate::outils::button(&app, st, id.as_str()); });
    // Lecteur.
    lib_on!(app, state, on_player_toggle, |app, st| { crate::lecteur::toggle(&app, st); });
    lib_on!(app, state, on_player_stop, |app, st| { crate::lecteur::stop(&app, st); });
    lib_on!(app, state, on_player_seek, |app, st, f| { crate::lecteur::seek(&app, st, f as f64); });
    lib_on!(app, state, on_player_next, |app, st| { crate::lecteur::next(&app, st); });
    lib_on!(app, state, on_player_prev, |app, st| { crate::lecteur::previous(&app, st); });
    lib_on!(app, state, on_player_loop, |app, st, what| { crate::lecteur::set_loop(&app, st, what.as_str()); });
    lib_on!(app, state, on_player_mark, |app, st, f| { crate::lecteur::add_marker(&app, st, f as f64); });
    lib_on!(app, state, on_player_unmark, |app, st, k| { crate::lecteur::remove_marker(&app, st, k.max(0) as usize); });
    lib_on!(app, state, on_player_switch, |app, st| { crate::lecteur::switch_ab(&app, st); });
    lib_on!(app, state, on_player_volume_set, |app, st, v| { crate::lecteur::set_volume(&app, st, v); });
    lib_on!(app, state, on_player_equal_toggle, |app, st| { crate::lecteur::toggle_equal(&app, st); });
    watch_phone(app, state);
    crate::lecteur::wire(app, state);
}

/// L'état de l'écran.
#[derive(Default)]
pub struct View {
    pub nav: Vec<Nav>,
    pub active: usize,
    pub query: String,
    pub chips: Chips,
    /// Colonne de tri et sens (vide : le tri par défaut, par album).
    pub order: (String, bool),
    pub rows: Vec<Row>,
    pub selected: BTreeSet<i64>,
    pub anchor: Option<usize>,
    pub details: Option<i64>,
    pub details_error: String,
    pub covers: HashMap<i64, Option<Image>>,
    pub covers_asked: BTreeSet<i64>,
    pub scanning: bool,
    pub last_scan: Option<Instant>,
    pub scan_text: String,
    pub playing: Option<i64>,
    /// L'outil ouvert (renommer, doublons…).
    pub dlg: crate::outils::Dlg,
    /// Analyse audio en cours : la mettre à vrai l'arrête.
    pub analysing: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// L'analyse en fond des morceaux jamais analysés a été lancée (une fois par session).
    pub idle_done: bool,
    /// Entrée de navigation ajoutée (« Compatibles avec… »), gardée jusqu'à la suivante.
    pub extra: Option<Nav>,
    /// Éditer les tags en lot (Ctrl+E sur plusieurs morceaux) : le panneau de détails vaut pour eux tous.
    pub batch: Vec<i64>,
    /// Les dossiers surveillés introuvables ont été signalés (une fois par session).
    pub missing_checked: bool,
    /// Les mini formes d'onde déjà dessinées.
    pub waves: HashMap<i64, Image>,
    /// Les pointes de ces ondes, lues d'un coup dans la base.
    pub peaks: HashMap<i64, Vec<u8>>,
    /// L'horloge qui guette les changements faits depuis le téléphone.
    pub phone_watch: Option<slint::Timer>,
    /// Les frappes du tempo tapé, et le tempo qu'elles donnent.
    pub taps: Vec<Instant>,
    pub tap_bpm: Option<f64>,
}

fn color(hex: &str) -> slint::Color {
    let h = hex.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0x8f8f9a);
    slint::Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// Les colonnes montrées (Titre toujours, en premier). Un choix enregistré avant l'arrivée de la colonne Onde la gagne
/// une fois, juste après le titre ; on peut ensuite la retirer.
fn shown_cols() -> Vec<String> {
    let mut v: Vec<String> = settings::get("career_columns")
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_else(|| lib::DEFAULT_COLUMNS.iter().map(|s| s.to_string()).collect());
    v.retain(|k| lib::COLUMNS.iter().any(|c| c.0 == k) && k != "title");
    if !v.iter().any(|k| k == "wave") && settings::get("career_columns_wave").is_none() {
        v.insert(0, "wave".into());
    }
    v.insert(0, "title".into());
    v
}

const RIGHT: [&str; 6] = ["bpm", "lufs", "duration_s", "track_no", "camelot", "music_key"];

fn cols_ui(view: &View) -> (Vec<ColUi>, Vec<ColUi>, f32) {
    let shown = shown_cols();
    let mut x = 0.0f32;
    let mut cols = Vec::new();
    for k in &shown {
        let Some(c) = lib::COLUMNS.iter().find(|c| c.0 == k) else { continue };
        let sort = if view.order.0 == c.0 { if view.order.1 { 1 } else { -1 } } else { 0 };
        let w = c.2 as f32;
        cols.push(ColUi { key: c.0.into(), label: c.1.into(), width: w, x: if c.0 == "title" { 0.0 } else { x }, sort, shown: true, right: RIGHT.contains(&c.0) });
        if c.0 != "title" {
            x += w;
        }
    }
    let all = lib::COLUMNS
        .iter()
        .map(|c| ColUi { key: c.0.into(), label: c.1.into(), width: c.2 as f32, x: 0.0, sort: 0, shown: shown.iter().any(|k| k == c.0), right: false })
        .collect();
    (cols, all, x)
}

/// Remplit tout l'écran (navigation, colonnes, lignes, détails).
pub fn fill(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    st.lib.nav = lib::nav(c.conn());
    if let Some(x) = &st.lib.extra {
        st.lib.nav.insert(1, x.clone());
    }
    if st.lib.active >= st.lib.nav.len() {
        st.lib.active = 0;
    }
    let g = app.global::<Lib>();
    g.set_help(otoko_library::career::search::HELP.into());
    g.set_statuses(model(STATUSES.iter().map(|s| SharedString::from(s.0)).collect()));
    g.set_status_labels(model(STATUSES.iter().map(|s| SharedString::from(s.1)).collect()));
    g.set_genres(model(lib::genres(c.conn()).into_iter().map(SharedString::from).collect()));
    push_nav(app, st);
    refresh_rows(app, st);
    // Les dossiers surveillés sont relus à l'ouverture, puis toutes les 10 minutes au plus.
    let due = st.lib.last_scan.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(600));
    if due && !st.lib.scanning && !real_career_in_selftest(st) {
        scan(app, st);
    }
}

/// Pendant l'autotest, la vraie Carrière de l'utilisateur n'est ni relue ni écrite. Relié au PC principal, ce PC n'a pas les
/// dossiers surveillés : c'est le PC principal qui les relit.
fn real_career_in_selftest(st: &State) -> bool {
    (st.selftest && st.career_path == crate::career::default_path()) || st.distant.is_some()
}

fn push_nav(app: &AppWindow, st: &State) {
    let mut out = Vec::new();
    let mut section = String::new();
    for (i, n) in st.lib.nav.iter().enumerate() {
        if n.section != section {
            section = n.section.clone();
            out.push(NavUi { section: section.as_str().into(), header: true, ..Default::default() });
        }
        out.push(NavUi {
            section: n.section.as_str().into(),
            label: n.label.as_str().into(),
            count: n.count as i32,
            color: if n.color.is_empty() { slint::Color::default() } else { color(&n.color) },
            has_color: !n.color.is_empty(),
            depth: n.depth as i32,
            header: false,
            active: i == st.lib.active,
            removable: n.view.is_some() || n.folder.as_ref().is_some_and(|f| f.0 > 0),
        });
    }
    app.global::<Lib>().set_nav(model(out));
}

/// L'entrée de navigation d'une ligne de l'interface (les titres de section n'en ont pas).
fn nav_index(st: &State, ui: usize) -> Option<usize> {
    let mut section = String::new();
    let mut k = 0;
    for (i, n) in st.lib.nav.iter().enumerate() {
        if n.section != section {
            section = n.section.clone();
            if k == ui {
                return None;
            }
            k += 1;
        }
        if k == ui {
            return Some(i);
        }
        k += 1;
    }
    None
}

/// Relit les lignes (navigation + pastilles + recherche + tri) et les montre.
pub fn refresh_rows(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let Some(n) = st.lib.nav.get(st.lib.active).cloned() else { return };
    let (w, p) = lib::full_where(c.conn(), &n.where_sql, &n.params, false, &st.lib.chips, &st.lib.query);
    let order = if st.lib.order.0.is_empty() { lib::DEFAULT_ORDER.to_string() } else { lib::order_sql(&st.lib.order.0, st.lib.order.1) };
    st.lib.rows = lib::rows(c.conn(), &w, &p, &order).unwrap_or_default();
    let ids: BTreeSet<i64> = st.lib.rows.iter().map(|r| r.id).collect();
    st.lib.selected.retain(|id| ids.contains(id));
    let g = app.global::<Lib>();
    let ch = &st.lib.chips;
    g.set_chip_release(ch.release.as_str().into());
    g.set_chip_nature(ch.nature.as_str().into());
    g.set_chip_status(if ch.status.is_empty() { "".into() } else { status_label(&ch.status).into() });
    g.set_chip_genre(ch.genre.as_str().into());
    g.set_chip_keep(ch.keep);
    g.set_chip_unanalyzed(ch.unanalyzed);
    g.set_filtered(*ch != Chips::default() || !st.lib.query.trim().is_empty());
    g.set_title(n.label.as_str().into());
    let total: f64 = st.lib.rows.iter().filter_map(|r| r.duration).sum();
    g.set_count_text(format!("{} · {} morceau{} · {:.1} h", n.label, st.lib.rows.len(), if st.lib.rows.len() > 1 { "x" } else { "" }, total / 3600.0).into());
    g.set_undo_label(otoko_library::career::files::last_batch(c.conn()).map(|b| format!("{} ({} fichier{})", b.1, b.2, if b.2 > 1 { "s" } else { "" })).unwrap_or_default().into());
    let (cols, all, fixed) = cols_ui(&st.lib);
    let wave = cols.iter().find(|c| c.key == "wave");
    g.set_wave_x(wave.map(|c| c.x).unwrap_or(-1.0));
    g.set_wave_w(wave.map(|c| c.width).unwrap_or(0.0));
    g.set_cols(model(cols));
    g.set_all_cols(model(all));
    g.set_fixed_width(fixed);
    push_rows(app, st);
    ask_covers(st);
    details(app, st);
}

fn row_ui(st: &State, i: usize, r: &Row, keys: &[String]) -> LibRowUi {
    let cover = st.lib.covers.get(&r.id).cloned().flatten();
    let wave = st.lib.waves.get(&r.id).cloned();
    LibRowUi {
        id: r.id as i32,
        has_wave: wave.is_some(),
        wave: wave.unwrap_or_default(),
        cells: model(keys.iter().map(|k| SharedString::from(r.cell(k))).collect()),
        pick: r.pick as i32,
        status_color: color(lib::status_color(&r.status)),
        playable: !r.missing && (st.distant.is_some() || std::path::Path::new(&r.path).exists()),
        missing: r.missing,
        archived: r.archived,
        selected: st.lib.selected.contains(&r.id),
        playing: st.lib.playing == Some(r.id),
        children: r.children as i32,
        has_cover: cover.is_some(),
        cover: cover.unwrap_or_default(),
        color: crate::images::color(i % 10),
    }
}

/// Dessine les mini formes d'onde des lignes montrées, si la colonne est là.
fn draw_waves(st: &mut State) {
    if !shown_cols().iter().any(|k| k == "wave") {
        return;
    }
    if st.lib.waves.is_empty() {
        st.lib.peaks = st.career.as_ref().map(|c| lib::waves(c.conn())).unwrap_or_default();
    }
    let ids: Vec<i64> = st.lib.rows.iter().map(|r| r.id).filter(|id| !st.lib.waves.contains_key(id)).collect();
    for id in ids {
        if let Some(p) = st.lib.peaks.get(&id).filter(|p| !p.is_empty()) {
            st.lib.waves.insert(id, crate::fond::wave_image(p));
        }
    }
}

/// Les lignes telles qu'elles sont (sélection, lecture, pochettes à jour).
pub fn push_rows(app: &AppWindow, st: &mut State) {
    draw_waves(st);
    let keys = shown_cols();
    let rows: Vec<LibRowUi> = st.lib.rows.iter().enumerate().map(|(i, r)| row_ui(st, i, r, &keys)).collect();
    let g = app.global::<Lib>();
    // Mêmes lignes (pochettes arrivées, sélection) : mises à jour sur place, la liste ne saute pas.
    let current = g.get_rows();
    let same = current.row_count() == rows.len() && rows.iter().enumerate().all(|(i, r)| current.row_data(i).is_some_and(|c| c.id == r.id && c.cells.row_count() == r.cells.row_count()));
    if same {
        if let Some(m) = current.as_any().downcast_ref::<VecModel<LibRowUi>>() {
            for (i, r) in rows.into_iter().enumerate() {
                m.set_row_data(i, r);
            }
        }
    } else {
        g.set_rows(model(rows));
    }
    g.set_selected_count(st.lib.selected.len() as i32);
    g.set_selection_archived(!st.lib.selected.is_empty() && st.lib.rows.iter().filter(|r| st.lib.selected.contains(&r.id)).all(|r| r.archived));
}

/// Met à jour une seule ligne (après une modification), sans tout relire.
/// Le téléphone a changé un morceau (gardé, statut, note) : sa ligne, ses détails et le lecteur se relisent, les autres
/// pages se reliront à la prochaine visite. Le serveur le signale dans un réglage, lu toutes les 3 secondes.
fn watch_phone(app: &AppWindow, state: &Rc<RefCell<State>>) {
    let weak = app.as_weak();
    let s = state.clone();
    let last = std::cell::RefCell::new(None::<String>);
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(3), move || {
        let Some(app) = weak.upgrade() else { return };
        let Ok(mut st) = s.try_borrow_mut() else { return };
        let Some(now) = st.career.as_ref().map(|c| c.setting(otoko_mobile::edits::STAMP).unwrap_or_default()) else { return };
        let before = last.borrow_mut().replace(now.clone());
        if before.is_none_or(|b| b == now) {
            return;
        }
        let Some(id) = now.split(':').next().and_then(|i| i.parse::<i64>().ok()) else { return };
        phone_changed(&app, &mut st, id);
    });
    state.borrow_mut().lib.phone_watch = Some(timer);
}

fn phone_changed(app: &AppWindow, st: &mut State, id: i64) {
    st.pages_lues = [false; 8];
    update_row(app, st, id);
    if st.lib.details == Some(id) {
        details(app, st);
    }
    if st.player.id == Some(id) {
        if let Some(c) = st.career.as_ref() {
            st.player.markers = lib::markers(c.conn(), id);
        }
        crate::lecteur::push(app, st);
    }
    let title = st.career.as_ref().and_then(|c| lib::row_by_id(c.conn(), id).ok().flatten()).map(|r| r.title).unwrap_or_default();
    if !title.is_empty() {
        app.set_message(format!("« {title} » modifié depuis le téléphone.").into());
    }
}

fn update_row(app: &AppWindow, st: &mut State, id: i64) {
    let Some(c) = st.career.as_ref() else { return };
    let Some(fresh) = lib::row_by_id(c.conn(), id).ok().flatten() else { return };
    let Some(i) = st.lib.rows.iter().position(|r| r.id == id) else { return };
    st.lib.rows[i] = fresh;
    let keys = shown_cols();
    let ui = row_ui(st, i, &st.lib.rows[i], &keys);
    let rows = app.global::<Lib>().get_rows();
    if let Some(m) = rows.as_any().downcast_ref::<VecModel<LibRowUi>>() {
        if i < m.row_count() {
            m.set_row_data(i, ui);
        }
    }
}


// ---------------------------------------------------------------------- pochettes

/// Dossier du cache des pochettes : celui du serveur mobile pour la vraie Carrière, un dossier
/// temporaire pour une Carrière d'essai (autotest, vitrine).
fn cover_cache(st: &State) -> PathBuf {
    if st.career_path == crate::career::default_path() { crate::donnees::data_dir().join("mobile") } else { std::env::temp_dir().join("otoko-essai-pochettes") }
}

/// Demande en fond les pochettes des lignes qui n'en ont pas encore.
fn ask_covers(st: &mut State) {
    let todo: Vec<i64> = st.lib.rows.iter().map(|r| r.id).filter(|id| !st.lib.covers.contains_key(id) && !st.lib.covers_asked.contains(id)).take(600).collect();
    let remote = st.distant.as_ref().map(|d| d.url.clone());
    // L'autotest n'écrit pas dans le cache réel ; les captures du film tournent sur des données temporaires (variable posée).
    let captures = std::env::var_os("OTOKO_CAPTURES").is_some();
    if todo.is_empty() || (st.selftest && st.career_path == crate::career::default_path() && !captures) {
        return;
    }
    st.lib.covers_asked.extend(todo.iter().copied());
    let db = st.career_path.clone();
    let cache = cover_cache(st);
    // Par paquets : les premières lignes ont leur pochette tout de suite.
    let tx = st.jobs.clone();
    st.spawn(move || {
        let chunks: Vec<&[i64]> = todo.chunks(24).collect();
        for (k, chunk) in chunks.iter().enumerate() {
            let list = match remote.as_deref() {
                Some(url) => chunk.iter().map(|id| (*id, crate::distant::cover(url, "tracks", *id, 160).and_then(|b| small_cover(&b)))).collect(),
                None => load_covers(&db, &cache, chunk),
            };
            if k + 1 == chunks.len() {
                return Done::Covers(list, true);
            }
            let _ = tx.send(Done::Covers(list, false));
        }
        Done::Covers(Vec::new(), true)
    });
}

/// Une pochette ramenée à 64 × 64 en RGBA.
fn small_cover(jpg: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let small = image::load_from_memory(jpg).ok()?.resize_exact(64, 64, image::imageops::FilterType::Triangle).to_rgba8();
    Some(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(small.as_raw(), 64, 64))
}

/// (morceau, pochette 64 × 64 en RGBA) ; rien pour un morceau sans pochette.
pub fn load_covers(db: &std::path::Path, cache: &std::path::Path, ids: &[i64]) -> Vec<(i64, Option<SharedPixelBuffer<Rgba8Pixel>>)> {
    let Ok(con) = otoko_mobile::library::connect(db) else { return ids.iter().map(|id| (*id, None)).collect() };
    let media = otoko_mobile::media::Cache::new(cache.to_path_buf());
    ids.iter().map(|id| (*id, media.cover_jpeg(&otoko_mobile::library::cover_sources(&con, *id), 160).and_then(|(jpg, _)| small_cover(&jpg)))).collect()
}

pub fn covers_landed(app: &AppWindow, st: &mut State, list: Vec<(i64, Option<SharedPixelBuffer<Rgba8Pixel>>)>, last: bool) {
    if !last {
        // Un paquet de plus est attendu : le travail n'est pas fini.
        st.pending += 1;
    }
    for (id, buf) in list {
        st.lib.covers.insert(id, buf.map(Image::from_rgba8));
    }
    push_rows(app, st);
    details(app, st);
}

// ---------------------------------------------------------------------- dossiers surveillés

/// Met la bibliothèque à jour d'après les dossiers surveillés (en fond).
pub fn scan(app: &AppWindow, st: &mut State) {
    if st.lib.scanning || st.career.is_none() {
        return;
    }
    st.lib.scanning = true;
    app.global::<Lib>().set_scanning(true);
    app.global::<Lib>().set_scan_text("Lecture des dossiers surveillés…".into());
    let path = st.career_path.clone();
    st.spawn(move || {
        let r = otoko_library::career::Career::open(&path).map_err(|e| e.to_string()).and_then(|mut c| {
            let stop = std::sync::atomic::AtomicBool::new(false);
            otoko_library::career::scan::scan(c.conn_mut(), None, &stop, &mut |_| {}).map_err(|e| e.to_string())
        });
        Done::LibScan(r)
    });
}

pub fn scanned(app: &AppWindow, st: &mut State, r: Result<otoko_library::career::scan::ScanStats, String>) {
    st.lib.scanning = false;
    st.lib.last_scan = Some(Instant::now());
    // La base a été écrite par une autre connexion : on la rouvre.
    st.career = crate::career::open(&st.career_path);
    st.lib.scan_text = match &r {
        Ok(s) => {
            let mut bits = vec![format!("{} dossier{} surveillé{}", s.folders, if s.folders > 1 { "s" } else { "" }, if s.folders > 1 { "s" } else { "" })];
            if s.added > 0 {
                bits.push(format!("{} nouveau{}", s.added, if s.added > 1 { "x" } else { "" }));
            }
            if s.updated > 0 {
                bits.push(format!("{} mis à jour", s.updated));
            }
            if s.missing > 0 {
                bits.push(format!("{} introuvable{}", s.missing, if s.missing > 1 { "s" } else { "" }));
            }
            if s.back > 0 {
                bits.push(format!("{} retrouvé{}", s.back, if s.back > 1 { "s" } else { "" }));
            }
            bits.join(" · ")
        }
        Err(e) => format!("Mise à jour impossible : {e}"),
    };
    let g = app.global::<Lib>();
    g.set_scanning(false);
    g.set_scan_text(st.lib.scan_text.as_str().into());
    if r.as_ref().is_ok_and(|s| s.added + s.updated + s.missing + s.back > 0) {
        fill(app, st);
    }
    // Au premier passage : les dossiers surveillés introuvables (disque débranché, dossier déplacé…).
    if !st.lib.missing_checked && !st.selftest && st.career_path == crate::career::default_path() {
        st.lib.missing_checked = true;
        if matches!(st.lib.dlg, crate::outils::Dlg::None) {
            crate::outils::open(app, st, "missing");
        }
    }
    crate::fond::auto_status(app, st);
    crate::fond::waves(st);
    // Puis, comme Studio Vault : les morceaux jamais analysés, en fond (réglage « idle_analyze »),
    // et les nouveaux à chaque mise à jour (réglage « auto_analyze »).
    let idle = !st.lib.idle_done && st.career.as_ref().is_some_and(|c| lib::flag(c.conn(), "idle_analyze", true));
    let fresh = r.as_ref().is_ok_and(|s| s.added > 0) && st.career.as_ref().is_some_and(|c| lib::flag(c.conn(), "auto_analyze", false));
    // Jamais sur une base d'essai (vitrines, autotest) : ses fichiers sont ceux de l'utilisateur.
    if !st.selftest && st.career_path == crate::career::default_path() && (idle || fresh) {
        st.lib.idle_done = true;
        let todo = st.career.as_ref().map(|c| lib::unanalyzed(c.conn())).unwrap_or_default();
        crate::analyse::start(app, st, todo, true);
    }
}

pub fn add_folder(app: &AppWindow, st: &mut State) {
    let Some(dir) = rfd::FileDialog::new().set_title("Ajouter un dossier à la bibliothèque").pick_folder() else { return };
    let Some(c) = st.career.as_ref() else { return };
    match otoko_library::career::scan::add_folder(c.conn(), &dir.to_string_lossy()) {
        Ok(true) => {
            app.set_message(format!("Dossier ajouté : {}. Lecture des morceaux…", dir.display()).into());
            st.lib.last_scan = None;
            scan(app, st);
        }
        Ok(false) => app.set_message("Ce dossier est déjà dans la bibliothèque.".into()),
        Err(e) => app.set_message(format!("Ajout impossible : {e}").into()),
    }
}

// ---------------------------------------------------------------------- actions

pub fn nav_clicked(app: &AppWindow, st: &mut State, ui: usize) {
    let Some(i) = nav_index(st, ui) else { return };
    st.lib.active = i;
    // Une vue enregistrée remet sa recherche.
    if let Some(n) = st.lib.nav.get(i) {
        if n.view.is_some() {
            st.lib.query = n.search.clone();
            app.global::<Lib>().set_query(st.lib.query.as_str().into());
        }
    }
    st.lib.selected.clear();
    st.lib.anchor = None;
    push_nav(app, st);
    refresh_rows(app, st);
}

pub fn nav_remove(app: &AppWindow, st: &mut State, ui: usize) {
    let Some(i) = nav_index(st, ui) else { return };
    let Some(n) = st.lib.nav.get(i).cloned() else { return };
    let Some(c) = st.career.as_ref() else { return };
    let msg = if let Some(v) = n.view {
        let _ = lib::delete_view(c.conn(), v);
        format!("Vue « {} » supprimée.", n.label)
    } else if let Some((id, path)) = n.folder.filter(|f| f.0 > 0) {
        if st.selftest {
            return;
        }
        let ok = rfd::MessageDialog::new()
            .set_title("Retirer le dossier")
            .set_description(format!("Retirer « {path} » de la bibliothèque ?\n\nLes fichiers sur le disque ne sont pas touchés. Les morceaux de ce dossier sortent de la bibliothèque (sauf ceux rangés dans une sortie)."))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if ok != rfd::MessageDialogResult::Yes {
            return;
        }
        match otoko_library::career::scan::remove_folder(c.conn(), id) {
            Ok(n) => format!("Dossier retiré ({n} morceaux sortis de la bibliothèque, fichiers intacts)."),
            Err(e) => format!("Retrait impossible : {e}"),
        }
    } else {
        return;
    };
    if st.lib.active == i {
        st.lib.active = 0;
    }
    app.set_message(msg.into());
    fill(app, st);
}

pub fn search(app: &AppWindow, st: &mut State, text: &str) {
    st.lib.query = text.to_string();
    refresh_rows(app, st);
}

pub fn set_chip(app: &AppWindow, st: &mut State, kind: &str, value: &str) {
    let ch = &mut st.lib.chips;
    match kind {
        "release" => ch.release = value.into(),
        "nature" => ch.nature = value.into(),
        "status" => ch.status = value.into(),
        "genre" => ch.genre = value.into(),
        "keep" => ch.keep = !value.is_empty(),
        "unanalyzed" => ch.unanalyzed = !value.is_empty(),
        _ => return,
    }
    refresh_rows(app, st);
}

pub fn reset(app: &AppWindow, st: &mut State) {
    st.lib.chips = Chips::default();
    st.lib.query.clear();
    app.global::<Lib>().set_query("".into());
    refresh_rows(app, st);
    app.set_message("Filtres réinitialisés.".into());
}

pub fn save_view(app: &AppWindow, st: &mut State, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        app.set_message("Donne un nom à la vue.".into());
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    let Some(n) = st.lib.nav.get(st.lib.active).cloned() else { return };
    let (w, p) = lib::full_where(c.conn(), &n.where_sql, &n.params, false, &st.lib.chips, &st.lib.query);
    match lib::save_view(c.conn(), name, &w, &p, &st.lib.query) {
        Ok(_) => app.set_message(format!("Vue « {name} » enregistrée : elle est dans « Mes vues », ici et sur le téléphone.").into()),
        Err(e) => app.set_message(format!("Vue non enregistrée : {e}").into()),
    }
    st.lib.nav = lib::nav(c.conn());
    push_nav(app, st);
}

pub fn sort(app: &AppWindow, st: &mut State, key: &str) {
    st.lib.order = if st.lib.order.0 == key { (key.into(), !st.lib.order.1) } else { (key.into(), true) };
    refresh_rows(app, st);
}

pub fn toggle_col(app: &AppWindow, st: &mut State, key: &str) {
    if key == "title" {
        return;
    }
    let mut v = shown_cols();
    if let Some(i) = v.iter().position(|k| k == key) {
        v.remove(i);
    } else {
        // Dans l'ordre du tableau de Studio Vault.
        v.push(key.into());
        v.sort_by_key(|k| lib::COLUMNS.iter().position(|c| c.0 == k).unwrap_or(99));
    }
    if !st.selftest {
        settings::set("career_columns", serde_json::json!(v));
        settings::set("career_columns_wave", serde_json::json!(true));
    }
    refresh_rows(app, st);
}

pub fn row_clicked(app: &AppWindow, st: &mut State, i: usize, ctrl: bool, shift: bool) {
    let Some(id) = st.lib.rows.get(i).map(|r| r.id) else { return };
    if shift {
        let a = st.lib.anchor.unwrap_or(i);
        let (lo, hi) = (a.min(i), a.max(i));
        if !ctrl {
            st.lib.selected.clear();
        }
        st.lib.selected.extend(st.lib.rows[lo..=hi].iter().map(|r| r.id));
    } else if ctrl {
        if !st.lib.selected.remove(&id) {
            st.lib.selected.insert(id);
        }
        st.lib.anchor = Some(i);
    } else {
        st.lib.selected.clear();
        st.lib.selected.insert(id);
        st.lib.anchor = Some(i);
    }
    if st.lib.details.is_some() && st.lib.selected.len() == 1 {
        st.lib.details = Some(id);
    }
    st.lib.batch.clear();
    push_rows(app, st);
    details(app, st);
}

/// Éditer les tags : le panneau de détails, pour un morceau ou pour toute la sélection (en lot).
pub fn edit_tags(app: &AppWindow, st: &mut State) {
    let ids = selection(st);
    match ids.len() {
        0 => app.set_message("Sélectionne d'abord les morceaux.".into()),
        1 => {
            st.lib.batch.clear();
            st.lib.details = Some(ids[0]);
            details(app, st);
        }
        _ => {
            st.lib.details = Some(ids[0]);
            st.lib.batch = ids;
            details(app, st);
        }
    }
}

/// Le panneau en lot : la valeur commune de chaque champ, sinon « plusieurs valeurs ».
fn batch_details(app: &AppWindow, st: &State) {
    let g = app.global::<Lib>();
    let Some(c) = st.career.as_ref() else { return };
    let ids = &st.lib.batch;
    let common = |k: &str| -> (String, bool) {
        let vals: BTreeSet<String> = ids.iter().map(|id| lib::field(c.conn(), *id, k)).collect();
        if vals.len() == 1 { (vals.into_iter().next().unwrap_or_default(), false) } else { (String::new(), true) }
    };
    let text = |key: &str, label: &str| {
        let (v, mixed) = common(key);
        LibFieldUi { key: key.into(), label: label.into(), value: v.into(), kind: 0, mixed, ..Default::default() }
    };
    let pick = |key: &str, label: &str| {
        let (choices, labels) = choice(key);
        let (v, mixed) = common(key);
        let shown = if mixed { "≠ plusieurs valeurs".to_string() } else { choices.iter().position(|x| *x == v).and_then(|i| labels.get(i).cloned()).unwrap_or(v) };
        LibFieldUi { key: key.into(), label: label.into(), value: shown.into(), kind: 1, mixed, choices: model(choices.into_iter().map(SharedString::from).collect()), labels: model(labels.into_iter().map(SharedString::from).collect()) }
    };
    let fields = vec![
        text("artist", "ARTISTE"),
        text("album", "ALBUM"),
        text("genre", "GENRE"),
        text("year", "ANNÉE"),
        text("bpm", "BPM"),
        text("music_key", "TONALITÉ"),
        pick("status", "STATUT"),
        pick("nature", "NATURE"),
        pick("release_type", "TYPE DE SORTIE"),
        text("version_label", "VERSION"),
        text("notes", "NOTES"),
    ];
    g.set_fields(model(fields));
    g.set_details_batch(ids.len() as i32);
    g.set_details_title(format!("{} morceaux", ids.len()).into());
    g.set_details_sub("Édition en lot : un champ n'est changé que si tu le modifies, et il l'est pour tous.".into());
    g.set_details_tags("".into());
    g.set_versions(model(Vec::new()));
    g.set_details_info("« Écrire les tags dans les fichiers » (menu des outils) reporte ensuite ces valeurs dans les fichiers audio.".into());
    g.set_details_has_cover(false);
    g.set_details_error(st.lib.details_error.as_str().into());
    g.set_details_attached(false);
    g.set_details_open(true);
}

/// Exporte la vue affichée en CSV (toutes les colonnes sauf la sélection P/X), comme Studio Vault :
/// UTF-8 avec BOM et point-virgule, pour qu'Excel l'ouvre tel quel.
pub fn csv_text(rows: &[Row]) -> String {
    let cols: Vec<&(&str, &str, u32, &str)> = lib::COLUMNS.iter().filter(|c| c.0 != "pick" && c.0 != "wave").collect();
    let quote = |s: &str| if s.contains([';', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let mut out = String::from("\u{feff}");
    out.push_str(&cols.iter().map(|c| quote(c.1)).collect::<Vec<_>>().join(";"));
    out.push_str("\r\n");
    for r in rows {
        out.push_str(&cols.iter().map(|c| quote(&r.cell(c.0))).collect::<Vec<_>>().join(";"));
        out.push_str("\r\n");
    }
    out
}

fn export_csv(app: &AppWindow, st: &mut State) {
    if st.selftest {
        return;
    }
    let Some(f) = rfd::FileDialog::new().set_title("Exporter en CSV").set_file_name("catalogue.csv").add_filter("CSV", &["csv"]).save_file() else { return };
    match std::fs::write(&f, csv_text(&st.lib.rows)) {
        Ok(()) => app.set_message(format!("Fichier exporté : {} ({} morceaux).", f.display(), st.lib.rows.len()).into()),
        Err(e) => app.set_message(format!("Export impossible : {e}").into()),
    }
}

pub fn selection(st: &State) -> Vec<i64> {
    st.lib.rows.iter().filter(|r| st.lib.selected.contains(&r.id)).map(|r| r.id).collect()
}

/// Joue la ligne dans le lecteur (ou met en pause si c'est elle qui joue).
pub fn play(app: &AppWindow, st: &mut State, i: usize) {
    let Some(id) = st.lib.rows.get(i).map(|r| r.id) else { return };
    if st.player.id == Some(id) {
        crate::lecteur::toggle(app, st);
    } else {
        crate::lecteur::play_id(app, st, id, 0.0);
    }
    push_rows(app, st);
}

/// Le morceau de référence : celui qui joue, sinon le premier sélectionné.
fn reference(st: &State) -> Option<i64> {
    st.player.id.or_else(|| st.lib.rows.iter().find(|r| st.lib.selected.contains(&r.id)).map(|r| r.id))
}

/// Les morceaux compatibles (tonalité et tempo) avec le morceau de référence.
fn compatible(app: &AppWindow, st: &mut State) {
    let Some(id) = reference(st) else {
        app.set_message("Sélectionne un morceau (ou fais-en jouer un) pour voir ceux qui s'enchaînent avec lui.".into());
        return;
    };
    let Some(n) = st.career.as_ref().and_then(|c| lib::compatible_nav(c.conn(), id)) else {
        app.set_message("Ce morceau n'a ni tonalité ni tempo : analyse-le d'abord (Ctrl+Maj+A).".into());
        return;
    };
    app.set_message(format!("{} : {} morceau(x).", n.label, n.count).into());
    st.lib.extra = Some(n);
    st.lib.query.clear();
    app.global::<Lib>().set_query("".into());
    fill(app, st);
    st.lib.active = 1;
    push_nav(app, st);
    refresh_rows(app, st);
}

/// Va à la page, relue quoi qu'il arrive : la vue vient de changer.
fn voir(app: &AppWindow, st: &mut State) {
    st.pages_lues[1] = false;
    crate::raccourcis::tab(app, st, 1);
}

/// Ouvre la bibliothèque sur une vue venue d'ailleurs (l'accueil : « À terminer »…), sans
/// pastilles ni recherche.
pub fn open_view(app: &AppWindow, st: &mut State, label: &str, where_sql: &str) {
    let Some(n) = st.career.as_ref().map(|c| lib::custom_nav(c.conn(), label, where_sql)) else { return };
    st.lib.extra = Some(n);
    st.lib.query.clear();
    st.lib.chips = Chips::default();
    app.global::<Lib>().set_query("".into());
    voir(app, st);
    st.lib.active = 1;
    push_nav(app, st);
    refresh_rows(app, st);
    app.global::<Lib>().set_table_scroll(0.0);
}

/// Le nom de la vue du tri des idées.
pub const TRIAGE: &str = "Tri des idées";

/// La bibliothèque est-elle sur le tri des idées ?
pub fn in_triage(st: &State) -> bool {
    st.lib.active == 1 && st.lib.extra.as_ref().is_some_and(|n| n.label == TRIAGE)
}

/// Le tri des idées : les idées jamais triées, chacune jouée sur son passage le plus chargé. P garde, X rejette,
/// N passe : la suivante enchaîne aussitôt.
pub fn triage(app: &AppWindow, st: &mut State) {
    open_view(app, st, TRIAGE, otoko_library::career::home::UNTRIAGED);
    let Some(first) = st.lib.rows.iter().position(|r| !r.missing) else {
        app.set_message("Aucune idée à trier : elles sont toutes gardées ou rejetées.".into());
        return;
    };
    row_clicked(app, st, first, false, false);
    let id = st.lib.rows[first].id;
    crate::lecteur::play_id(app, st, id, 0.0);
    app.set_message(format!("Tri des idées ({} à écouter) : P garde, X rejette, N passe. Le morceau suivant démarre tout seul, sur son passage le plus fort.", st.lib.rows.len()).into());
}

/// Garder, rejeter ou passer un seul morceau mène au suivant (et le lance si celui-là jouait) : on trie au clavier.
fn advance_after_pick(app: &AppWindow, st: &mut State, id: i64) {
    let on = in_triage(st) || st.career.as_ref().is_none_or(|c| lib::flag(c.conn(), "pick_advance", true));
    if !on {
        return;
    }
    let Some(i) = st.lib.rows.iter().position(|r| r.id == id) else { return };
    let Some(j) = st.lib.rows.iter().enumerate().skip(i + 1).find(|(_, r)| !r.missing).map(|(j, _)| j) else {
        if in_triage(st) {
            crate::lecteur::stop(app, st);
            app.set_message("Tri des idées terminé. Les gardées sont dans « Gardés », les rejetées restent dans la bibliothèque.".into());
        }
        return;
    };
    let was_playing = st.player.id == Some(id);
    row_clicked(app, st, j, false, false);
    show_row(app, j);
    if was_playing {
        let next = st.lib.rows[j].id;
        crate::lecteur::play_id(app, st, next, 0.0);
    }
}

/// Fait défiler le tableau pour que la ligne `i` soit à l'écran.
fn show_row(app: &AppWindow, i: usize) {
    let g = app.global::<Lib>();
    let row_h = g.get_row_h().max(1.0);
    let (top, h) = (-g.get_table_scroll(), g.get_table_view_h());
    let y = i as f32 * row_h;
    if y < top {
        g.set_table_scroll(-y);
    } else if y + row_h > top + h {
        g.set_table_scroll(-(y + row_h - h));
    }
}

/// Montre un morceau dans la bibliothèque : tous les morceaux, lui sélectionné, ses détails ouverts.
pub fn reveal(app: &AppWindow, st: &mut State, id: i64) {
    st.lib.extra = None;
    st.lib.query.clear();
    st.lib.chips = Chips::default();
    app.global::<Lib>().set_query("".into());
    voir(app, st);
    // « Tous les morceaux » n'a pas les archives : celle qui contient le morceau, sinon la première.
    st.lib.active = 0;
    refresh_rows(app, st);
    if !st.lib.rows.iter().any(|r| r.id == id) {
        if let Some(k) = st.lib.nav.iter().position(|n| n.label == "Archives") {
            st.lib.active = k;
            refresh_rows(app, st);
        }
    }
    push_nav(app, st);
    st.lib.selected.clear();
    st.lib.selected.insert(id);
    st.lib.details = Some(id);
    let i = st.lib.rows.iter().position(|r| r.id == id);
    st.lib.anchor = i;
    push_rows(app, st);
    details(app, st);
    // Le morceau à l'écran, quelques lignes sous le haut du tableau.
    let row_h = app.global::<Lib>().get_row_h();
    app.global::<Lib>().set_table_scroll(-(i.unwrap_or(0).saturating_sub(3) as f32 * row_h));
}

/// La roue de Camelot : combien de morceaux par tonalité, celle du morceau de référence et ses voisines.
fn wheel(app: &AppWindow, st: &mut State) {
    use otoko_library::career::camelot;
    let Some(c) = st.career.as_ref() else { return };
    let counts = lib::camelot_counts(c.conn());
    let key = reference(st).map(|id| lib::field(c.conn(), id, "music_key")).unwrap_or_default();
    let current = camelot::camelot(&key).to_string();
    let compatibles = camelot::compatibles(&current);
    let (cx, cy) = (230.0f32, 300.0f32);
    let mut items = Vec::new();
    for n in 1..=12 {
        let a = (n as f32 - 1.0) * 30.0f32.to_radians();
        for (letter, r) in [("B", 141.0f32), ("A", 80.0f32)] {
            let code = format!("{n}{letter}");
            items.push(crate::WheelUi {
                keys: camelot::keys_for(&code).join(" · ").into(),
                count: counts.get(&code).copied().unwrap_or(0) as i32,
                x: cx + r * a.sin(),
                y: cy - r * a.cos(),
                current: code == current,
                compatible: code != current && compatibles.contains(&code),
                code: code.into(),
            });
        }
    }
    let g = app.global::<Lib>();
    g.set_wheel(ModelRc::new(VecModel::from(items)));
    g.set_wheel_text(
        if current.is_empty() {
            "Extérieur : majeures (B) ; intérieur : mineures (A). Deux voisins sur la roue s'enchaînent bien. Clique une case pour voir ses morceaux.".into()
        } else {
            format!("{key} = {current}. En vert : les tonalités qui s'enchaînent avec lui (même chiffre ou voisines, et la relative). Clique une case pour voir ses morceaux.").into()
        },
    );
    g.set_wheel_open(true);
}

/// Les actions du menu (clic droit) et du panneau de détails, sur la sélection.
pub fn action(app: &AppWindow, st: &mut State, what: &str) {
    // Ceux-là n'ont pas besoin d'une sélection.
    match what {
        "compatible" => return compatible(app, st),
        "wheel" => return wheel(app, st),
        "wheel-close" => {
            app.global::<Lib>().set_wheel_open(false);
            return;
        }
        w if w.starts_with("wheel:") => {
            app.global::<Lib>().set_wheel_open(false);
            let q = format!("camelot:{}", &w[6..]);
            app.global::<Lib>().set_query(q.as_str().into());
            st.lib.active = 0;
            return search(app, st, &q);
        }
        "undo" => return crate::outils::undo(app, st),
        "analyze-missing" => {
            let todo = st.career.as_ref().map(|c| lib::unanalyzed(c.conn())).unwrap_or_default();
            return crate::analyse::start(app, st, todo, false);
        }
        "stop-analysis" => return crate::analyse::stop(st),
        "analyze" => {
            // La sélection, ou tout ce qui est affiché.
            let sel: Vec<(i64, String)> = st.lib.rows.iter().filter(|r| st.lib.selected.is_empty() || st.lib.selected.contains(&r.id)).map(|r| (r.id, r.path.clone())).collect();
            let minutes = (sel.len() * 7).div_ceil(60).max(1);
            if sel.len() > 1 && !st.selftest {
                let ok = rfd::MessageDialog::new()
                    .set_title("Analyse audio")
                    .set_description(format!("Analyser {} morceau(x) ?\n\nTempo, tonalité, niveau sonore, énergie, ambiance, crêtes et silences. Environ {minutes} minute(s), en arrière-plan : tu peux continuer à travailler.", sel.len()))
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if ok != rfd::MessageDialogResult::Yes {
                    return;
                }
            }
            return crate::analyse::start(app, st, sel, false);
        }
        "complete" | "dupes" | "add-to-release" | "nature" | "genre" | "dupe-folders" | "relocate" | "watch" | "queue" | "batches" | "statuses" => return crate::outils::open(app, st, what),
        w if w.starts_with("release:") => {
            if let Ok(id) = w[8..].parse() {
                crate::sorties::open(app, st, id);
            }
            return;
        }
        "edit-tags" => return edit_tags(app, st),
        "export-csv" => return export_csv(app, st),
        "set-builder" => return crate::projets::set_builder(app, st),
        "clients" => return crate::projets::clients_show(app, st, None),
        "project" => {
            let id = st.lib.rows.iter().find(|r| st.lib.selected.contains(&r.id)).map(|r| r.id).or(st.lib.details);
            match id {
                Some(id) => crate::projets::open(app, st, id),
                None => app.set_message("Sélectionne d'abord un morceau.".into()),
            }
            return;
        }
        "rename" | "reorganize" if st.lib.selected.is_empty() => return crate::outils::open(app, st, what),
        _ => {}
    }
    let ids = selection(st);
    let Some(first) = ids.first().copied().or(if what == "detach" || what == "attach" { st.lib.details } else { None }) else { return };
    match what {
        "play" => {
            if let Some(i) = st.lib.rows.iter().position(|r| r.id == first) {
                play(app, st, i);
            }
        }
        "details" => {
            st.lib.details = if st.lib.details == Some(first) { None } else { Some(first) };
            details(app, st);
        }
        "keep" | "reject" | "neutral" => {
            let v = match what {
                "keep" => 1,
                "reject" => -1,
                _ => 0,
            };
            if let Some(c) = st.career.as_ref() {
                if let Err(e) = lib::set_pick(c.conn(), &ids, v) {
                    app.set_message(format!("Choix non enregistré : {e}").into());
                }
            }
            for id in &ids {
                update_row(app, st, *id);
            }
            if let [id] = ids[..] {
                advance_after_pick(app, st, id);
            }
        }
        "archive" => {
            let archive = !st.lib.rows.iter().filter(|r| ids.contains(&r.id)).all(|r| r.archived);
            if let Some(c) = st.career.as_ref() {
                let _ = lib::set_archived(c.conn(), &ids, archive);
            }
            app.set_message(
                if archive { format!("{} morceau(x) archivé(s) — retrouvables dans la vue « Archives ».", ids.len()) } else { format!("{} morceau(x) sorti(s) des archives.", ids.len()) }.into(),
            );
            fill(app, st);
        }
        "publish" => crate::publish::open(app, st, first),
        "master" => crate::mastrage::add_songs(app, st, &ids),
        "open-project" => crate::career::open_project(app, st, first),
        "share" => {
            let ids = selection(st);
            crate::partage::share(app, st, 0, &ids);
        }
        "rename" | "reorganize" | "attach" => crate::outils::open(app, st, what),
        "detach" => crate::outils::detach(app, st),
        "write-tags" => crate::outils::write_tags(app, st),
        "forget" => crate::outils::forget(app, st),
        "folder" => {
            if let Some(r) = st.lib.rows.iter().find(|r| r.id == first) {
                if !st.selftest {
                    let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", r.path)).spawn();
                }
            }
        }
        _ => {}
    }
}

pub fn key(app: &AppWindow, st: &mut State, k: &str) {
    match k {
        "p" | "P" => action(app, st, "keep"),
        "x" | "X" => action(app, st, "reject"),
        "n" | "N" => action(app, st, "neutral"),
        "i" | "I" => action(app, st, "details"),
        "\n" | "\r" => action(app, st, "play"),
        // Espace : pause si un morceau est ouvert, sinon la sélection.
        " " => {
            if st.player.id.is_some() && st.lib.selected.iter().all(|id| Some(*id) == st.player.id) {
                crate::lecteur::toggle(app, st);
            } else {
                action(app, st, "play");
            }
        }
        "q" | "Q" => {
            let ids: Vec<i64> = st.lib.rows.iter().filter(|r| st.lib.selected.contains(&r.id)).map(|r| r.id).collect();
            crate::lecteur::enqueue(app, st, &ids);
        }
        "\t" => crate::lecteur::switch_ab(app, st),
        "compare" => {
            let ids: Vec<i64> = st.lib.rows.iter().filter(|r| st.lib.selected.contains(&r.id)).map(|r| r.id).collect();
            crate::lecteur::compare(app, st, &ids);
        }
        "\u{1b}" => {
            st.lib.details = None;
            details(app, st);
        }
        _ => {
            // Flèches, début, fin, page : la ligne visée, toujours à l'écran.
            let g = app.global::<Lib>();
            let row_h = g.get_row_h().max(1.0);
            let page = ((g.get_table_view_h() / row_h) as usize).max(2) - 1;
            let last = st.lib.rows.len().saturating_sub(1);
            let cur = st.lib.anchor.unwrap_or(0);
            let next = match k {
                "\u{f701}" => (cur + 1).min(last),
                "\u{f700}" => cur.saturating_sub(1),
                "\u{f729}" => 0,
                "\u{f72b}" => last,
                "\u{f72c}" => cur.saturating_sub(page),
                "\u{f72d}" => (cur + page).min(last),
                _ => return,
            };
            row_clicked(app, st, next, false, false);
            show_row(app, next);
        }
    }
}

// ---------------------------------------------------------------------- détails

fn choice(key: &str) -> (Vec<String>, Vec<String>) {
    match key {
        "status" => (STATUSES.iter().map(|s| s.0.into()).collect(), STATUSES.iter().map(|s| s.1.into()).collect()),
        "nature" => (NATURES.iter().map(|s| s.0.into()).collect(), NATURES.iter().map(|s| s.1.into()).collect()),
        "role" => (lib::ROLE_LABELS.iter().map(|s| s.0.into()).collect(), lib::ROLE_LABELS.iter().map(|s| s.1.into()).collect()),
        "release_type" => {
            let mut c: Vec<String> = vec![String::new()];
            let mut l: Vec<String> = vec!["Aucune".into()];
            c.extend(RELEASE_TYPES.iter().map(|s| s.0.to_string()));
            l.extend(RELEASE_TYPES.iter().map(|s| s.1.to_string()));
            (c, l)
        }
        "music_key" => (
            lib::KEYS.iter().map(|k| k.to_string()).collect(),
            lib::KEYS.iter().map(|k| format!("{k}  ·  {}", otoko_library::career::camelot::camelot(k))).collect(),
        ),
        _ => (Vec::new(), Vec::new()),
    }
}

/// Le panneau de détails du morceau ouvert.
pub fn details(app: &AppWindow, st: &mut State) {
    let g = app.global::<Lib>();
    let Some(id) = st.lib.details else {
        st.lib.batch.clear();
        g.set_details_batch(0);
        g.set_details_open(false);
        return;
    };
    if st.lib.batch.len() > 1 {
        return batch_details(app, st);
    }
    g.set_details_batch(0);
    let Some(c) = st.career.as_ref() else { return };
    let Some(r) = lib::row_by_id(c.conn(), id).ok().flatten() else {
        st.lib.details = None;
        g.set_details_open(false);
        return;
    };
    let f = |k: &str| lib::field(c.conn(), id, k);
    let text = |key: &str, label: &str| LibFieldUi { key: key.into(), label: label.into(), value: f(key).into(), kind: 0, ..Default::default() };
    let pick = |key: &str, label: &str, shown: String| {
        let (choices, labels) = choice(key);
        LibFieldUi { key: key.into(), label: label.into(), value: shown.into(), kind: 1, choices: model(choices.into_iter().map(SharedString::from).collect()), labels: model(labels.into_iter().map(SharedString::from).collect()), mixed: false }
    };
    let rt = f("release_type");
    let fields = match g.get_details_tab() {
        1 => vec![text("bpm", "TEMPO (BPM)"), pick("music_key", "TONALITÉ", if r.key.is_empty() { "Pas de tonalité".into() } else { r.key.clone() }), text("mood_auto", "AMBIANCE")],
        2 => vec![
            pick("release_type", "TYPE DE SORTIE", if rt.is_empty() { "Aucune".into() } else { release_label(&rt) }),
            text("sc_url", "LIEN SOUNDCLOUD"),
            text("published_at", "PUBLIÉ LE (AAAA-MM-JJ)"),
        ],
        3 => vec![pick("role", "TYPE DE FICHIER", lib::role_label(&r.role)), text("version_label", "VERSION"), text("daw_project", "PROJET (FL, ABLETON, CUBASE, REAPER…)")],
        _ => vec![
            text("title", "TITRE"),
            text("artist", "ARTISTE"),
            text("album", "ALBUM"),
            text("genre", "GENRE"),
            text("year", "ANNÉE"),
            text("track_no", "N° DE PISTE"),
            pick("status", "STATUT", status_label(&r.status).to_string()),
            pick("nature", "NATURE", NATURES.iter().find(|n| n.0 == r.nature).map(|n| n.1.to_string()).unwrap_or_else(|| nature_short(&r.nature))),
            text("notes", "NOTES"),
        ],
    };
    g.set_fields(model(fields));
    g.set_details_locked(["bpm", "music_key", "mood_auto"].iter().any(|k| lib::locked(c.conn(), id, k)));
    g.set_details_releases(model(lib::releases_of(c.conn(), id).into_iter().map(|(rid, title, sub)| VersionUi { id: rid as i32, title: title.into(), sub: sub.into() }).collect()));
    g.set_details_id(id as i32);
    g.set_details_title(r.title.as_str().into());
    let mut sub = vec![lib::role_label(&r.role)];
    if !r.artist.is_empty() {
        sub.push(r.artist.clone());
    }
    if let Some(d) = r.duration {
        sub.push(lib::duration_text(Some(d)));
    }
    if r.archived {
        sub.push("archivé".into());
    }
    if r.missing {
        sub.push("fichier introuvable".into());
    }
    g.set_details_sub(sub.join(" · ").into());
    g.set_details_tags(lib::tags_of(c.conn(), id).join(", ").into());
    let versions: Vec<VersionUi> = lib::children(c.conn(), id)
        .unwrap_or_default()
        .iter()
        .map(|v| VersionUi {
            id: v.id as i32,
            title: v.title.as_str().into(),
            sub: [lib::role_label(&v.role), v.version_label.clone(), v.format.to_uppercase(), lib::duration_text(v.duration)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ").into(),
        })
        .collect();
    g.set_versions(model(versions));
    let size: f64 = f("file_size").parse().unwrap_or(0.0);
    let mut info = vec![
        r.path.clone(),
        format!(
            "{} · {} Hz · {} canal{} · {:.1} Mo",
            r.format.to_uppercase(),
            f("samplerate"),
            f("channels"),
            if f("channels") == "1" { "" } else { "aux" },
            size / 1_048_576.0
        ),
    ];
    let analysis: Vec<String> = [("lufs", "Sonie (LUFS)"), ("true_peak", "Crête (dBTP)"), ("energy", "Énergie"), ("brightness", "Brillance"), ("dynamics", "Dynamique"), ("mode", "Mode")]
        .iter()
        .filter_map(|(k, l)| {
            let v = f(k);
            (!v.is_empty()).then(|| format!("{l} : {}", v.replace('.', ",")))
        })
        .collect();
    g.set_details_measures(if analysis.is_empty() { "Ce morceau n'a pas encore été analysé.".to_string() } else { analysis.join("\n") }.into());
    info.push(format!("Ajouté le {} · écouté {} fois", f("added_at").chars().take(10).collect::<String>(), f("play_count").parse::<i64>().unwrap_or(0)));
    g.set_details_info(info.join("\n").into());
    let cover = st.lib.covers.get(&id).cloned().flatten();
    g.set_details_has_cover(cover.is_some());
    g.set_details_cover(cover.unwrap_or_default());
    g.set_details_error(st.lib.details_error.as_str().into());
    g.set_details_attached(r.parent_id.is_some());
    g.set_details_open(true);
}

pub fn field_edited(app: &AppWindow, st: &mut State, key: &str, value: &str) {
    let Some(id) = st.lib.details else { return };
    let Some(c) = st.career.as_ref() else { return };
    // En lot : le champ modifié vaut pour toute la sélection.
    if st.lib.batch.len() > 1 {
        let ids = st.lib.batch.clone();
        let errors: Vec<String> = ids.iter().filter_map(|i| lib::update(c.conn(), *i, key, value).err()).collect();
        st.lib.details_error = errors.into_iter().next().unwrap_or_default();
        for i in ids {
            update_row(app, st, i);
        }
        app.global::<Lib>().set_details_error(st.lib.details_error.as_str().into());
        if ["status", "nature", "release_type"].contains(&key) {
            details(app, st);
        }
        return;
    }
    st.lib.details_error = match lib::update(c.conn(), id, key, value) {
        Ok(()) => String::new(),
        Err(e) => e,
    };
    update_row(app, st, id);
    app.global::<Lib>().set_details_error(st.lib.details_error.as_str().into());
    // Un choix dans une liste se voit tout de suite dans le panneau.
    if ["status", "nature", "role", "release_type"].contains(&key) {
        details(app, st);
    }
}

/// Le morceau du panneau, ou la sélection : ce que les corrections rapides visent.
fn targets(st: &State) -> Vec<i64> {
    let ids = selection(st);
    if ids.is_empty() { st.lib.details.into_iter().collect() } else { ids }
}

/// Tempo doublé ou divisé par deux, sur le morceau ou toute la sélection.
fn bpm_scale(app: &AppWindow, st: &mut State, factor: f64) {
    let ids = targets(st);
    let Some(c) = st.career.as_ref() else { return };
    let n = lib::scale_bpm(c.conn(), &ids, factor).unwrap_or(0);
    for id in ids {
        update_row(app, st, id);
    }
    app.set_message(if n == 0 { "Pas de tempo à corriger : analyse d'abord le morceau.".to_string() } else { format!("Tempo corrigé sur {n} morceau{}.", if n > 1 { "x" } else { "" }) }.into());
    details(app, st);
}

/// Le tempo des frappes : le nombre d'intervalles sur le temps écoulé.
pub fn tap_tempo(taps: &[Instant]) -> Option<f64> {
    let (first, last) = (taps.first()?, taps.last()?);
    let secs = last.duration_since(*first).as_secs_f64();
    (taps.len() >= 4 && secs > 0.0).then(|| 60.0 * (taps.len() - 1) as f64 / secs)
}

/// Une frappe : quatre suffisent, une pause de plus de deux secondes recommence.
fn tap(app: &AppWindow, st: &mut State) {
    let now = Instant::now();
    if st.lib.taps.last().is_some_and(|t| now.duration_since(*t).as_secs_f64() > 2.0) {
        st.lib.taps.clear();
    }
    st.lib.taps.push(now);
    if st.lib.taps.len() > 16 {
        st.lib.taps.remove(0);
    }
    st.lib.tap_bpm = tap_tempo(&st.lib.taps);
    let text = match st.lib.tap_bpm {
        Some(b) => format!("{} BPM ({} frappes)", format!("{b:.1}").replace('.', ","), st.lib.taps.len()),
        None => format!("Continue à taper en rythme… ({})", st.lib.taps.len()),
    };
    app.global::<Lib>().set_details_tap(text.into());
}

fn tap_keep(app: &AppWindow, st: &mut State) {
    let (Some(b), Some(id)) = (st.lib.tap_bpm, st.lib.details) else { return };
    let Some(c) = st.career.as_ref() else { return };
    let _ = lib::update(c.conn(), id, "bpm", &format!("{b:.1}"));
    st.lib.taps.clear();
    st.lib.tap_bpm = None;
    app.global::<Lib>().set_details_tap("".into());
    update_row(app, st, id);
    details(app, st);
}

/// Rend tempo, tonalité et ambiance à l'analyse.
fn unlock(app: &AppWindow, st: &mut State) {
    let Some(id) = st.lib.details else { return };
    if let Some(c) = st.career.as_ref() {
        for k in ["bpm", "music_key", "mood_auto"] {
            let _ = lib::lock(c.conn(), id, k, false);
        }
    }
    app.set_message("Rendu à l'analyse : le prochain passage remplacera ces valeurs.".into());
    details(app, st);
}

pub fn tags_edited(app: &AppWindow, st: &mut State, text: &str) {
    let Some(id) = st.lib.details else { return };
    if let Some(c) = st.career.as_ref() {
        let _ = lib::set_tags(c.conn(), id, text);
    }
    let _ = app;
}

pub fn version_clicked(app: &AppWindow, st: &mut State, id: i64) {
    st.lib.details = Some(id);
    st.lib.selected.clear();
    st.lib.selected.insert(id);
    push_rows(app, st);
    details(app, st);
}

pub fn close_details(app: &AppWindow, st: &mut State) {
    st.lib.details = None;
    details(app, st);
}

pub fn row_double(app: &AppWindow, st: &mut State, i: usize) {
    let Some(id) = st.lib.rows.get(i).map(|r| r.id) else { return };
    st.lib.details = Some(id);
    st.lib.selected.clear();
    st.lib.selected.insert(id);
    push_rows(app, st);
    details(app, st);
}
