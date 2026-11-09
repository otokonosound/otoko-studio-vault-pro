//! Carrière › Accueil, Production et Statistiques (comme l'accueil, l'écran Production et les
//! statistiques de Studio Vault) : les chiffres clés et ce qu'il reste à faire, le tableau des
//! morceaux de l'idée à la publication (on glisse une carte d'une colonne à l'autre), les graphiques.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use otoko_library::career::home::{self, Charts};
use otoko_library::career::library as lib;
use otoko_library::career::{backup, board, release_color};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use crate::state::{Done, State};
use crate::{AppWindow, BoardColUi, CardUi, ChartUi, ChipUi, HBarUi, HLinkUi, Home, SliceUi, TileUi, TodayUi};

macro_rules! home_on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.global::<Home>().$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

/// L'état de ces écrans.
#[derive(Default)]
pub struct View {
    /// Production : « mine » (mes morceaux) ou « clients » (les commandes).
    pub mode: String,
    pub filter: String,
    /// Les colonnes montrées (clés), dans l'ordre.
    pub keys: Vec<String>,
    pub covers: HashMap<i64, Option<Image>>,
    pub covers_asked: BTreeSet<i64>,
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

fn hex(c: &str) -> slint::Color {
    let v = u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0x8f8f9a);
    slint::Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    home_on!(app, state, on_stat, |app, st, k| { stat(&app, st, k); });
    home_on!(app, state, on_play, |app, st, id| {
        crate::lecteur::play_id(&app, st, id as i64, 0.0);
    });
    home_on!(app, state, on_act, |app, st, what| { act(&app, st, what.as_str()); });
    home_on!(app, state, on_board_mode_changed, |app, st, mode| {
        st.home.mode = mode.to_string();
        fill_board(&app, st);
    });
    home_on!(app, state, on_board_search, |app, st, text| {
        st.home.filter = text.to_string();
        fill_board(&app, st);
    });
    home_on!(app, state, on_board_drop, |app, st, id, col| { drop(&app, st, id as i64, col.max(0) as usize); });
    home_on!(app, state, on_board_menu, |app, st, id, what| { menu(&app, st, id as i64, what.as_str()); });
}

// ---------------------------------------------------------------------- accueil

fn link(id: i64, text: String, icon: &str, action: &str, tone: i32, cover: Option<Image>) -> HLinkUi {
    HLinkUi { id: id as i32, text: text.into(), icon: icon.into(), action: action.into(), tone, has_cover: cover.is_some(), cover: cover.unwrap_or_default() }
}

fn initials(title: &str) -> String {
    title.split_whitespace().filter_map(|w| w.chars().find(|c| c.is_alphanumeric())).take(2).flat_map(|c| c.to_uppercase()).collect()
}

const TODO_ICONS: [(&str, &str); 8] =
    [("backup", "alert"), ("to_publish", "megaphone"), ("analyze", "flask"), ("complete", "tag"), ("nature", "wand"), ("sync", "disc"), ("brand", "megaphone"), ("missing", "alert")];

pub fn fill_home(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let h = home::home(c.conn());
    let g = app.global::<Home>();
    let salut = crate::state::salutation();
    g.set_hello(if h.alias.is_empty() { salut.into() } else { format!("{salut} {}", h.alias).into() });
    g.set_date_text(otoko_library::career::releases::long_date(&crate::career::today()).into());
    g.set_empty(h.total == 0);
    let mut actions = home::today_actions(c.conn(), &h);
    if let Some(v) = st.update_ready.as_ref() {
        actions.insert(0, home::Action { title: format!("Version {v} prête"), why: "Elle s'installe en un clic.".into(), label: "Installer".into(), action: "update".into(), tone: 1, ..Default::default() });
    }
    let rest = actions.split_off(actions.len().min(3));
    g.set_today(model(actions.iter().map(|a| TodayUi { title: a.title.as_str().into(), why: a.why.as_str().into(), label: a.label.as_str().into(), action: a.action.as_str().into(), tone: a.tone }).collect()));
    g.set_next_up(model(rest.iter().take(6).map(|a| link(0, a.title.clone(), "plus", &a.action, 0, None)).collect()));
    g.set_stats(model([h.total, h.unfinished, h.released, h.recent].iter().map(|n| SharedString::from(n.to_string())).collect()));
    let cover = |id: i64| st.lib.covers.get(&id).cloned().flatten();
    g.set_priority(model(h.priority.iter().map(|(id, t, s)| link(*id, format!("{t}   —  {s}"), "play", "", 0, cover(*id))).collect()));
    let max = h.months.iter().map(|m| m.1).max().unwrap_or(0).max(1) as f32;
    let n = h.months.len();
    g.set_months(model(h.months.iter().enumerate().map(|(i, (l, v))| HBarUi { label: l.as_str().into(), value: *v as i32, frac: *v as f32 / max, strong: i + 1 == n }).collect()));
    let total: i64 = h.months.iter().map(|m| m.1).sum();
    g.set_production_text(
        if total > 0 {
            format!("{total} morceau(x) sorti(s) ces 12 derniers mois — {} au mieux en un mois", h.months.iter().map(|m| m.1).max().unwrap_or(0))
        } else {
            "Aucun morceau sorti ces 12 derniers mois.".into()
        }
        .into(),
    );
    g.set_news(model(h.news.iter().map(|(id, t)| link(*id, t.clone(), "play", "", 0, None)).collect()));
    let w = home::week(c.conn());
    g.set_week(model([w.added, w.finished, w.soon, w.unfinished].iter().map(|n| SharedString::from(n.to_string())).collect()));
    g.set_todo(model(
        h.todo.iter().map(|(t, a)| link(0, t.clone(), TODO_ICONS.iter().find(|x| x.0 == a).map(|x| x.1).unwrap_or("plus"), a, i32::from(a == "backup"), None)).collect(),
    ));
    g.set_release_alerts(model(h.release_alerts.iter().map(|(id, t)| link(*id, t.clone(), "calendar", &format!("release:{id}"), 1, None)).collect()));
    let tiles: Vec<TileUi> = h
        .releases
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let img = st.home.covers.get(&r.id).cloned().flatten();
            TileUi {
                id: r.id as i32,
                title: r.title.as_str().into(),
                kind: r.kind.as_str().into(),
                when: r.when.as_str().into(),
                soon: r.soon,
                done: r.done as i32,
                total: r.total as i32,
                has_cover: img.is_some(),
                cover: img.unwrap_or_default(),
                color: release_color(&r.rtype).map(hex).unwrap_or_else(|| crate::images::color(i)),
                initials: initials(&r.title).into(),
            }
        })
        .collect();
    g.set_releases(model(tiles));
    g.set_deadlines(model(h.deadlines.iter().map(|d| link(0, d.clone(), "calendar", "production-clients", 0, None)).collect()));
    g.set_unpaid(model(h.unpaid.iter().map(|d| link(0, d.clone(), "", "production-clients", 0, None)).collect()));
    g.set_unpaid_total(if h.unpaid_total > 0.0 { format!("Total à encaisser : {:.0} €", h.unpaid_total).into() } else { "".into() });
    g.set_money(!h.deadlines.is_empty() || !h.unpaid.is_empty());
    let todo: Vec<i64> = h.releases.iter().map(|r| r.id).filter(|id| !st.home.covers_asked.contains(id)).collect();
    ask_covers(st, todo);
}

/// Les pochettes des sorties (bandeau de l'accueil, galerie des sorties), en fond.
pub fn ask_covers(st: &mut State, ids: Vec<i64>) {
    // Comme pour la bibliothèque : les captures du film tournent sur des données temporaires (variable posée).
    let captures = std::env::var_os("OTOKO_CAPTURES").is_some();
    if ids.is_empty() || (st.selftest && st.career_path == crate::career::default_path() && !captures) {
        return;
    }
    st.home.covers_asked.extend(ids.iter().copied());
    let db = st.career_path.clone();
    let cache = if st.career_path == crate::career::default_path() { crate::donnees::data_dir().join("mobile") } else { std::env::temp_dir().join("otoko-essai-pochettes") };
    let remote = st.distant.as_ref().map(|d| d.url.clone());
    st.spawn(move || {
        let Ok(con) = otoko_mobile::library::connect(&db) else { return Done::ReleaseCovers(Vec::new()) };
        let media = otoko_mobile::media::Cache::new(cache);
        let list = ids
            .iter()
            .map(|id| {
                let jpg = match remote.as_deref() {
                    Some(url) => crate::distant::cover(url, "releases", *id, 300),
                    None => media.cover_jpeg(&otoko_mobile::library::release_cover_sources(&con, *id), 300).map(|x| x.0),
                };
                let img = jpg.and_then(|jpg| image::load_from_memory(&jpg).ok()).map(|i| {
                    let small = i.resize_to_fill(264, 264, image::imageops::FilterType::Triangle).to_rgba8();
                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(small.as_raw(), 264, 264)
                });
                (*id, img)
            })
            .collect();
        Done::ReleaseCovers(list)
    });
}

pub fn covers_landed(app: &AppWindow, st: &mut State, list: Vec<(i64, Option<SharedPixelBuffer<Rgba8Pixel>>)>) {
    for (id, buf) in list {
        st.home.covers.insert(id, buf.map(Image::from_rgba8));
    }
    fill_home(app, st);
    crate::sorties::push_gallery(app, st);
}

/// Un chiffre clé ouvre la bibliothèque sur les morceaux qu'il compte.
fn stat(app: &AppWindow, st: &mut State, k: i32) {
    let (label, w) = match k {
        0 => ("Toute la bibliothèque", "missing = 0".to_string()),
        1 => ("À terminer", home::UNFINISHED.to_string()),
        2 => ("Publiés", "missing = 0 AND status = 'released'".to_string()),
        _ => ("Nouveaux morceaux", lib::recent_where(14)),
    };
    crate::bibliotheque::open_view(app, st, label, &w);
}

/// Les liens de l'accueil.
pub fn act(app: &AppWindow, st: &mut State, what: &str) {
    match what {
        w if w.starts_with("track:") => {
            if let Ok(id) = w[6..].parse() {
                crate::bibliotheque::reveal(app, st, id);
            }
        }
        "to_publish" => crate::bibliotheque::open_view(app, st, "À publier", lib::TO_PUBLISH_SQL),
        "missing" => crate::bibliotheque::open_view(app, st, "Fichiers manquants", "missing = 1"),
        "triage" => crate::bibliotheque::triage(app, st),
        "to_master" => crate::bibliotheque::open_view(app, st, "À masteriser", "missing = 0 AND status = 'to_master'"),
        "copies" => crate::bibliotheque::open_view(app, st, "Une seule copie", otoko_library::career::copies::SINGLE_SQL),
        "copies-setup" => {
            crate::raccourcis::aller(app, st, 6, 0);
            app.set_prefs_part("analyse".into());
        }
        "batches" => crate::outils::open(app, st, "batches"),
        "links" => {
            crate::sorties::gallery(app, st);
            crate::fond::link(app, st, None);
        }
        "arrivals" => {
            let Some(ids) = st.career.as_ref().map(|c| otoko_library::career::arrivee::todo(c.conn())) else { return };
            let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
            crate::bibliotheque::open_view(app, st, "Exports à ranger", &format!("id IN ({list})"));
        }
        "update" => crate::majs::install(app, st),
        "analyze" => {
            crate::raccourcis::tab(app, st, 1);
            crate::bibliotheque::action(app, st, "analyze-missing");
        }
        // Le dialogue s'ouvre par-dessus l'onglet en cours.
        "complete" | "nature" | "sync" => crate::outils::open(app, st, what),
        "backup" => backup_to_drive(app, st),
        "brand" => {
            crate::raccourcis::tab(app, st, 3);
            app.set_message("Ton univers de marque (nom d'artiste, lien, ville…) sert aux textes de publication.".into());
        }
        "production-clients" => {
            st.home.mode = "clients".into();
            app.global::<Home>().set_board_mode("clients".into());
            crate::raccourcis::tab(app, st, 6);
        }
        "planning" => crate::sorties::planning(app, st),
        "suno" => crate::raccourcis::tab(app, st, 4),
        "releases" => crate::sorties::gallery(app, st),
        w if w.starts_with("release:") => {
            if let Ok(id) = w[8..].parse() {
                crate::sorties::open(app, st, id);
            }
        }
        _ => {}
    }
}

/// Copier chaque jour la sauvegarde de la Carrière sur Google Drive (comme Studio Vault).
fn backup_to_drive(app: &AppWindow, st: &mut State) {
    let Some(dir) = home::suggested_drive_dir() else { return };
    if st.selftest {
        return;
    }
    let ok = rfd::MessageDialog::new()
        .set_title("Sauvegarde sur Google Drive")
        .set_description(format!("Copier chaque jour la sauvegarde de ta base dans :\n{}\n\nLes 10 dernières copies y sont gardées.", dir.display()))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();
    if ok != rfd::MessageDialogResult::Yes {
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    let _ = c.set_setting(backup::EXTRA_SETTING, &dir.to_string_lossy());
    let r = backup::now(c.conn(), &crate::donnees::data_dir().join("sauvegardes"), Some(&dir));
    app.set_message(match r {
        Ok(_) => "Sauvegarde copiée sur Google Drive.".into(),
        Err(e) => format!("Copie impossible ({e}) : vérifie que Google Drive est lancé.").into(),
    });
    fill_home(app, st);
}

// ---------------------------------------------------------------------- production

fn tone(c: &str) -> i32 {
    match c {
        "danger" => 2,
        "warning" => 1,
        "accent" => 3,
        _ => 0,
    }
}

pub fn fill_board(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    if st.home.mode.is_empty() {
        st.home.mode = "mine".into();
    }
    let cols = board::board(c.conn(), &st.home.mode, &st.home.filter);
    st.home.keys = cols.iter().map(|c| c.key.clone()).collect();
    let ui: Vec<BoardColUi> = cols
        .iter()
        .map(|col| BoardColUi {
            key: col.key.as_str().into(),
            title: col.title.as_str().into(),
            count: col.count as i32,
            cards: model(
                col.cards
                    .iter()
                    .map(|k| CardUi {
                        id: k.id as i32,
                        title: k.title.as_str().into(),
                        sub: k.sub.as_str().into(),
                        chips: model(k.chips.iter().map(|(t, c)| ChipUi { text: t.as_str().into(), tone: tone(c) }).collect()),
                        keep: k.keep,
                        to_publish: k.to_publish,
                    })
                    .collect(),
            ),
        })
        .collect();
    let g = app.global::<Home>();
    g.set_board_mode(st.home.mode.as_str().into());
    g.set_board(model(ui));
    let n: usize = cols.iter().map(|c| c.count).sum();
    g.set_board_text(
        if st.home.mode == "clients" {
            format!("{n} commande(s) — glisse une carte pour changer l'étape · clic droit : plus d'actions")
        } else {
            format!("{n} morceau(x) — glisse une carte d'une colonne à l'autre pour changer son statut · clic droit : plus d'actions")
        }
        .into(),
    );
}

/// Une carte déposée dans une colonne.
fn drop(app: &AppWindow, st: &mut State, id: i64, col: usize) {
    let Some(key) = st.home.keys.get(col).cloned() else { return };
    let Some(c) = st.career.as_ref() else { return };
    if let Err(e) = board::drop_card(c.conn(), &st.home.mode, id, &key) {
        app.set_message(format!("Écriture impossible : {e}").into());
        return;
    }
    fill_board(app, st);
    fill_home(app, st);
    crate::bibliotheque::refresh_rows(app, st);
}

fn menu(app: &AppWindow, st: &mut State, id: i64, what: &str) {
    match what {
        "play" => crate::lecteur::play_id(app, st, id, 0.0),
        "reveal" => crate::bibliotheque::reveal(app, st, id),
        "project" => crate::projets::open(app, st, id),
        "master" => crate::mastrage::add_songs(app, st, &[id]),
        "open-project" => crate::career::open_project(app, st, id),
        "publish" | "unpublish" => {
            if let Some(c) = st.career.as_ref() {
                let _ = board::set_to_publish(c.conn(), &[id], what == "publish");
            }
            fill_board(app, st);
            fill_home(app, st);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------- statistiques

fn bars(title: &str, rows: &[(String, i64)], kind: i32, color: slint::Color, empty: &str) -> ChartUi {
    let max = rows.iter().map(|r| r.1).max().unwrap_or(0).max(1) as f32;
    let n = rows.len();
    ChartUi {
        title: title.into(),
        kind,
        bars: model(rows.iter().enumerate().map(|(i, (l, v))| HBarUi { label: l.as_str().into(), value: *v as i32, frac: *v as f32 / max, strong: kind == 0 && i + 1 == n }).collect()),
        slices: model(Vec::new()),
        color,
        empty: if rows.iter().all(|r| r.1 == 0) { empty.into() } else { "".into() },
    }
}

/// Une part d'anneau (viewbox 100 × 100), de `a0` à `a1` (fractions du tour, depuis midi).
pub fn slice_path(a0: f64, a1: f64) -> String {
    let (cx, cy, r_out, r_in) = (50.0, 50.0, 48.0, 48.0 * 0.45);
    let pt = |r: f64, a: f64| {
        let t = (a * 360.0 - 90.0).to_radians();
        (cx + r * t.cos(), cy + r * t.sin())
    };
    let arc = |a0: f64, a1: f64| {
        let large = if a1 - a0 > 0.5 { 1 } else { 0 };
        let (o0, o1, i1, i0) = (pt(r_out, a0), pt(r_out, a1), pt(r_in, a1), pt(r_in, a0));
        format!(
            "M {:.3} {:.3} A {r_out} {r_out} 0 {large} 1 {:.3} {:.3} L {:.3} {:.3} A {r_in:.3} {r_in:.3} 0 {large} 0 {:.3} {:.3} Z",
            o0.0, o0.1, o1.0, o1.1, i1.0, i1.1, i0.0, i0.1
        )
    };
    // Un tour complet ne se dessine pas d'un seul arc : deux moitiés.
    if a1 - a0 > 0.9999 { format!("{} {}", arc(a0, a0 + 0.5), arc(a0 + 0.5, a1)) } else { arc(a0, a1) }
}

fn donut(title: &str, rows: &[(String, i64)], empty: &str) -> ChartUi {
    let total: i64 = rows.iter().map(|r| r.1.max(0)).sum();
    let mut a = 0.0;
    let mut slices = Vec::new();
    for (i, (l, v)) in rows.iter().enumerate() {
        if *v <= 0 || total == 0 {
            continue;
        }
        let f = *v as f64 / total as f64;
        slices.push(SliceUi { label: l.as_str().into(), value: *v as i32, path: slice_path(a, a + f).into(), color: crate::images::color(i) });
        a += f;
    }
    ChartUi {
        title: title.into(),
        kind: 2,
        bars: model(Vec::new()),
        empty: if slices.is_empty() { empty.into() } else { "".into() },
        slices: model(slices),
        color: slint::Color::default(),
    }
}

pub fn fill_charts(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let ch: Charts = home::charts(c.conn());
    let g = app.global::<Home>();
    g.set_summary(model(
        [
            ch.total.to_string(),
            format!("{:.1} h", ch.hours).replace('.', ","),
            ch.keep.to_string(),
            format!("{}/{}", ch.analyzed, ch.total),
            ch.avg_bpm.map(|b| format!("{b:.0}")).unwrap_or_else(|| "—".into()),
        ]
        .into_iter()
        .map(SharedString::from)
        .collect(),
    ));
    let (ember, violet, lagoon, green) = (hex("#ff8a3d"), hex("#c67bff"), hex("#35d9c3"), hex("#59c17a"));
    g.set_creation(model(vec![
        bars("Morceaux ajoutés par mois", &ch.added_by_month, 0, ember, "Rien d'ajouté pour l'instant."),
        donut("Types de sortie", &ch.release_types, "—"),
        donut("Formats", &ch.formats, "—"),
    ]));
    g.set_music(model(vec![
        bars("Tempo (BPM)", &ch.tempo, 0, ember, "Lance l'analyse audio pour voir les tempos."),
        bars("Tonalités les plus utilisées", &ch.keys, 1, violet, "Lance l'analyse audio pour voir les tonalités."),
        donut("Genres", &ch.genres, "Aucun genre renseigné pour l'instant (Bibliothèque › Outils › Compléter genres et artistes)."),
        bars("Ambiances détectées", &ch.moods, 1, lagoon, "Lance l'analyse audio pour voir les ambiances."),
    ]));
    g.set_progress(model(vec![
        bars("Terminés ou publiés par mois", &ch.done_by_month, 0, green, "Aucun morceau passé « terminé » ces douze derniers mois."),
        bars("Sorties datées par mois", &ch.releases_by_month, 0, lagoon, "Aucune sortie datée : Sorties › Nouvelle sortie."),
        bars("Par statut", &ch.by_status, 0, ember, "—"),
        donut("Sélection", &ch.picks, "—"),
        bars("Les plus écoutés", &ch.most_played, 1, green, "Rien d'écouté pour l'instant : le compteur monte à chaque écoute."),
    ]));
    // Parcours : les années, les natures, les sorties (chiffres de la Carrière d'OTOKO).
    let s = c.stats().unwrap_or_default();
    let years: Vec<(String, i64)> = s.by_year.iter().map(|(y, n, _)| (y.clone(), *n as i64)).collect();
    let natures: Vec<(String, i64)> = s.by_nature.iter().map(|(n, k)| (otoko_library::career::nature_short(n), *k as i64)).collect();
    let kinds: Vec<(String, i64)> = s.kinds.iter().map(|(k, n)| (otoko_library::career::release_label(k), *n as i64)).collect();
    let released: Vec<(String, i64)> = s.by_year.iter().map(|(y, _, out)| (y.clone(), *out as i64)).collect();
    g.set_career(model(vec![
        bars("Morceaux par année", &years, 0, ember, "—"),
        bars("Sortis par année", &released, 0, green, "Aucun morceau sorti pour l'instant."),
        donut("Par nature", &natures, "—"),
        donut("Sorties par type", &kinds, "Aucune sortie pour l'instant."),
    ]));
}

#[cfg(test)]
mod tests {
    #[test]
    fn slices_close_the_ring() {
        let p = super::slice_path(0.0, 0.25);
        assert!(p.starts_with("M 50.000 2.000 A 48 48 0 0 1 98.000 50.000"), "{p}");
        // Un seul morceau : deux moitiés.
        assert_eq!(super::slice_path(0.0, 1.0).matches('M').count(), 2);
        assert!(super::slice_path(0.0, 0.7).contains(" 0 1 1 "));
        assert_eq!(super::initials("Nuit blanche à Paris"), "NB");
        assert_eq!(super::initials("(Remix) 2"), "R2");
    }
}
