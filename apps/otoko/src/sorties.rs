//! Carrière › Sorties (comme l'écran Sorties de Studio Vault) : la galerie des pochettes, la fiche
//! d'une sortie (enregistrée à chaque changement), le planning à rebours depuis la date de sortie,
//! le contrôle avant publication, la pochette écrite dans les fichiers, la discographie (PDF ou
//! page web). La sortie n'est qu'une fiche : supprimer la fiche ne touche ni aux morceaux ni aux
//! fichiers.
//!
//! Une sortie choisit où elle sort (streaming par un distributeur, SoundCloud, Bandcamp, YouTube) : ses étapes, son
//! planning et son contrôle suivent ce choix. Pour un distributeur, la fiche garde le code-barres, le label, la langue, le
//! contenu explicite et l'ISRC de chaque morceau, et prépare le dossier à lui envoyer. Le planning s'exporte vers un
//! agenda (.ics).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use otoko_library::career::releases::{self, Sheet};
use otoko_library::career::{PLATFORMS, RELEASE_TYPES, discography, files, pilotage, platforms_label, steps_for};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use crate::state::{Done, State};
use crate::{AppWindow, DayUi, IsrcUi, PlanRelUi, PlatformUi, QcCellUi, QcRowUi, Rel, RelPointUi, RelStepUi, RelTileUi, RelTrackUi};

macro_rules! rel_on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.global::<Rel>().$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

/// L'état de l'écran.
#[derive(Default)]
pub struct View {
    /// « gallery », « sheet » ou « planning ».
    pub mode: String,
    pub sheet: Option<Sheet>,
    /// Le mois montré par le planning.
    pub month: Option<(i32, u32)>,
    /// Tous les morceaux (pour « Ajouter un morceau ») : (id, titre, album, prod privée).
    pub picker: Vec<(i64, String, String, bool)>,
    /// Le dernier contrôle : (morceau, titre, résultats).
    pub qc: Vec<(i64, String, Vec<(&'static str, &'static str, String)>)>,
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

fn hex(c: &str) -> slint::Color {
    let v = u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0x444441);
    slint::Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Pendant l'autotest, les questions reçoivent « oui » (la Carrière est celle d'essai).
fn confirm(st: &State, title: &str, text: &str) -> bool {
    st.selftest || rfd::MessageDialog::new().set_title(title).set_description(text).set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    rel_on!(app, state, on_open, |app, st, id| { open(&app, st, id as i64); });
    rel_on!(app, state, on_new, |app, st| {
        let _ = st;
        app.global::<Rel>().set_new_title("".into());
        app.global::<Rel>().set_naming(true);
    });
    rel_on!(app, state, on_new_confirm, |app, st, title| { new_confirm(&app, st, title.as_str()); });
    rel_on!(app, state, on_back, |app, st| { gallery(&app, st); });
    rel_on!(app, state, on_planning, |app, st| { planning(&app, st); });
    rel_on!(app, state, on_sync, |app, st| { crate::outils::open(&app, st, "sync"); });
    rel_on!(app, state, on_discography, |app, st| { discography_dialog(&app, st); });
    rel_on!(app, state, on_go_publish, |app, st| { crate::raccourcis::tab(&app, st, 3); });
    rel_on!(app, state, on_delete, |app, st, id| { delete(&app, st, id as i64); });
    rel_on!(app, state, on_edited, |app, st, key, value| { edited(&app, st, key.as_str(), value.as_str()); });
    rel_on!(app, state, on_set_type, |app, st, t| { change(&app, st, |s| s.rtype = t.to_string()); });
    rel_on!(app, state, on_set_has_date, |app, st, on| {
        let today = releases::today().format("%Y-%m-%d").to_string();
        change(&app, st, |s| s.date = if on { if s.date.is_empty() { today } else { s.date.clone() } } else { String::new() });
        let date = st.rel.sheet.as_ref().map(|s| releases::fr_date(&s.date)).unwrap_or_default();
        app.global::<Rel>().set_date_text(date.into());
    });
    rel_on!(app, state, on_toggle_step, |app, st, key| {
        change(&app, st, |s| {
            if !s.steps.iter().any(|k| k == key.as_str()) {
                s.steps.push(key.to_string());
            } else {
                s.steps.retain(|k| k != key.as_str());
            }
        });
    });
    rel_on!(app, state, on_fix, |app, st, what| { fix(&app, st, what.as_str()); });
    rel_on!(app, state, on_toggle_platform, |app, st, key| {
        change(&app, st, |s| {
            if s.platforms.iter().any(|p| p == key.as_str()) {
                s.platforms.retain(|p| p != key.as_str());
            } else {
                s.platforms.push(key.to_string());
                // Dans l'ordre de la liste, quel que soit l'ordre des clics.
                s.platforms.sort_by_key(|p| PLATFORMS.iter().position(|x| x.0 == p));
            }
        });
        // La pochette se juge autrement pour un distributeur.
        push_sheet(&app, st, true);
        push_gallery(&app, st);
    });
    rel_on!(app, state, on_set_explicit, |app, st, on| { change(&app, st, |s| s.explicit = on); });
    rel_on!(app, state, on_isrc_edited, |app, st, id, text| {
        let raw = text.trim();
        let typed = raw.chars().filter(|c| c.is_ascii_alphanumeric()).count();
        // Tant que le code n'est pas complet, rien n'est écrit (ni reproché).
        if !raw.is_empty() && typed < 12 {
            return;
        }
        let Some(c) = st.career.as_ref() else { return };
        let msg = match releases::set_isrc(c.conn(), id as i64, raw) {
            Ok(v) if v.is_empty() => "ISRC effacé.".to_string(),
            Ok(v) => format!("ISRC {v} enregistré."),
            Err(e) => e,
        };
        app.global::<Rel>().set_msg(msg.into());
        push_sheet(&app, st, false);
    });
    rel_on!(app, state, on_delivery, |app, st| { delivery(&app, st); });
    rel_on!(app, state, on_agenda, |app, st| { agenda(&app, st); });
    rel_on!(app, state, on_link, |app, st, id| { crate::fond::link(&app, st, (id >= 0).then_some(id as i64)); });
    rel_on!(app, state, on_pick_cover, |app, st| { pick_cover(&app, st); });
    rel_on!(app, state, on_embed_cover, |app, st| { embed_cover(&app, st); });
    rel_on!(app, state, on_move_track, |app, st, i, dir| {
        change(&app, st, |s| {
            let (i, j) = (i as isize, i as isize + dir as isize);
            if i >= 0 && j >= 0 && (j as usize) < s.tracks.len() && (i as usize) < s.tracks.len() {
                s.tracks.swap(i as usize, j as usize);
            }
        });
    });
    rel_on!(app, state, on_remove_track, |app, st, i| {
        change(&app, st, |s| {
            if (i as usize) < s.tracks.len() {
                s.tracks.remove(i as usize);
            }
        });
    });
    rel_on!(app, state, on_play_track, |app, st, id| { crate::lecteur::play_id(&app, st, id as i64, 0.0); });
    rel_on!(app, state, on_picker_show, |app, st| { picker_show(&app, st); });
    rel_on!(app, state, on_picker_search, |app, st, q| { picker_push(&app, st, q.as_str()); });
    rel_on!(app, state, on_picker_choose, |app, st, id| { picker_choose(&app, st, id as i64); });
    rel_on!(app, state, on_picker_close, |app, st| {
        let _ = st;
        app.global::<Rel>().set_picker_open(false);
    });
    rel_on!(app, state, on_apply_to_tracks, |app, st| {
        let n = match (st.career.as_ref(), st.rel.sheet.as_ref()) {
            (Some(c), Some(s)) => releases::apply_to_tracks(c.conn(), s).unwrap_or(0),
            _ => 0,
        };
        app.global::<Rel>().set_msg(format!("Album, type et numéros appliqués à {n} morceau(x).").into());
        crate::bibliotheque::refresh_rows(&app, st);
    });
    rel_on!(app, state, on_prefill_credits, |app, st| { prefill_credits(&app, st); });
    rel_on!(app, state, on_qc_run, |app, st| { qc_run(&app, st); });
    rel_on!(app, state, on_qc_row, |app, st, i| { qc_detail(&app, st, i.max(0) as usize); });
    rel_on!(app, state, on_qc_analyze, |app, st| {
        let todo: Vec<(i64, String)> = st
            .rel
            .qc
            .iter()
            .filter(|q| q.2.iter().any(|x| x.1 == "—"))
            .filter_map(|q| st.career.as_ref().map(|c| (q.0, otoko_library::career::library::field(c.conn(), q.0, "path"))))
            .collect();
        app.global::<Rel>().set_qc_open(false);
        crate::analyse::start(&app, st, todo, false);
    });
    rel_on!(app, state, on_qc_close, |app, st| {
        let _ = st;
        app.global::<Rel>().set_qc_open(false);
    });
    rel_on!(app, state, on_texts, |app, st| {
        let Some(first) = st.rel.sheet.as_ref().and_then(|s| s.tracks.first().copied()) else {
            app.global::<Rel>().set_msg("Ajoute d'abord des morceaux à la sortie.".into());
            return;
        };
        crate::publish::open(&app, st, first);
    });
    rel_on!(app, state, on_export, |app, st| { export_sheet(&app, st); });
    rel_on!(app, state, on_master, |app, st| { master(&app, st); });
    rel_on!(app, state, on_month, |app, st, delta| {
        let (y, m) = st.rel.month.unwrap_or_else(this_month);
        st.rel.month = Some(releases::shift_month(y, m, delta));
        push_planning(&app, st);
    });
}

fn this_month() -> (i32, u32) {
    let t = releases::today().format("%Y-%m").to_string();
    (t[..4].parse().unwrap_or(2026), t[5..].parse().unwrap_or(1))
}

/// Remplit l'écran tel qu'il est (après un import, une modification ailleurs…).
pub fn fill(app: &AppWindow, st: &mut State) {
    let g = app.global::<Rel>();
    g.set_types(model(RELEASE_TYPES.iter().map(|t| SharedString::from(t.0)).collect()));
    g.set_type_labels(model(RELEASE_TYPES.iter().map(|t| SharedString::from(t.1)).collect()));
    g.set_qc_names(model(releases::QC_NAMES.iter().map(|n| SharedString::from(*n)).collect()));
    if st.rel.mode.is_empty() {
        st.rel.mode = "gallery".into();
    }
    // La fiche ouverte est relue (ou fermée si elle a disparu).
    if let Some(id) = st.rel.sheet.as_ref().map(|s| s.id) {
        st.rel.sheet = st.career.as_ref().and_then(|c| releases::load(c.conn(), id));
        if st.rel.sheet.is_none() && st.rel.mode == "sheet" {
            st.rel.mode = "gallery".into();
        }
    }
    g.set_mode(st.rel.mode.as_str().into());
    push_gallery(app, st);
    if st.rel.mode == "sheet" {
        push_sheet(app, st, true);
    }
    if st.rel.mode == "planning" {
        push_planning(app, st);
    }
}

// ---------------------------------------------------------------------- galerie

pub fn push_gallery(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let items = releases::gallery(c.conn());
    let tiles: Vec<RelTileUi> = items
        .iter()
        .map(|r| {
            let img = st.home.covers.get(&r.id).cloned().flatten();
            RelTileUi {
                id: r.id as i32,
                title: r.title.as_str().into(),
                sub: r.sub.as_str().into(),
                done: r.done as i32,
                total: r.total as i32,
                has_cover: img.is_some(),
                cover: img.unwrap_or_default(),
                color: hex(releases::tile_color(&r.title)),
                initials: releases::initials(&r.title).into(),
            }
        })
        .collect();
    let g = app.global::<Rel>();
    g.set_to_link(otoko_library::career::liens::to_link(c.conn()).len() as i32);
    g.set_gallery_text(
        if items.is_empty() {
            "Tes singles, EP et albums : pochette, morceaux, étapes et textes. Crée ta première sortie, ou range tes albums d'après les fichiers.".to_string()
        } else {
            format!("{} sortie(s) — clic : la fiche · clic droit : supprimer", items.len())
        }
        .into(),
    );
    g.set_tiles(model(tiles));
    let todo: Vec<i64> = items.iter().map(|r| r.id).filter(|id| !st.home.covers_asked.contains(id)).collect();
    crate::accueil::ask_covers(st, todo);
}

pub fn gallery(app: &AppWindow, st: &mut State) {
    st.rel.mode = "gallery".into();
    app.global::<Rel>().set_mode("gallery".into());
    crate::raccourcis::tab(app, st, 2);
    push_gallery(app, st);
}

fn new_confirm(app: &AppWindow, st: &mut State, title: &str) {
    app.global::<Rel>().set_naming(false);
    if title.trim().is_empty() {
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    match releases::create(c.conn(), title, &[]) {
        Ok(id) => {
            open(app, st, id);
            crate::accueil::fill_home(app, st);
        }
        Err(e) => app.set_message(format!("Création impossible : {e}").into()),
    }
}

pub fn delete(app: &AppWindow, st: &mut State, id: i64) {
    let Some(c) = st.career.as_ref() else { return };
    let Ok((title, n)) = c.conn().query_row("SELECT title, (SELECT COUNT(*) FROM release_tracks WHERE release_id = ?1) FROM releases WHERE id = ?1", [id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    }) else {
        return;
    };
    let text = format!(
        "Supprimer la fiche « {title} » ?\n\nSes {n} morceau(x) restent dans la bibliothèque et les fichiers ne sont pas touchés. La pochette, les étapes, les crédits et la description de la fiche sont perdus."
    );
    if !confirm(st, "Supprimer la sortie", &text) {
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    if let Err(e) = releases::delete(c.conn(), id) {
        app.set_message(format!("Suppression impossible : {e}").into());
        return;
    }
    if st.rel.sheet.as_ref().is_some_and(|s| s.id == id) {
        st.rel.sheet = None;
    }
    app.set_message(format!("Sortie « {title} » supprimée.").into());
    gallery(app, st);
    crate::accueil::fill_home(app, st);
    crate::bibliotheque::fill(app, st);
}

// ---------------------------------------------------------------------- fiche

/// Ouvre la fiche d'une sortie (depuis la galerie, l'accueil, le planning, la bibliothèque).
pub fn open(app: &AppWindow, st: &mut State, id: i64) {
    let Some(sheet) = st.career.as_ref().and_then(|c| releases::load(c.conn(), id)) else { return };
    st.rel.sheet = Some(sheet);
    st.rel.mode = "sheet".into();
    let g = app.global::<Rel>();
    g.set_mode("sheet".into());
    g.set_msg("Chaque changement est enregistré aussitôt.".into());
    crate::raccourcis::tab(app, st, 2);
    push_sheet(app, st, true);
}

/// Les morceaux de la sortie ouverte, envoyés ensemble au mastering.
fn master(app: &AppWindow, st: &mut State) {
    let Some(id) = st.rel.sheet.as_ref().filter(|s| !s.tracks.is_empty()).map(|s| s.id) else {
        app.global::<Rel>().set_msg("Ajoute d'abord des morceaux à la sortie.".into());
        return;
    };
    crate::mastrage::add_release(app, st, id);
}

/// Le bouton d'un point du pilotage.
fn fix(app: &AppWindow, st: &mut State, what: &str) {
    let tracks = st.rel.sheet.as_ref().map(|s| s.tracks.clone()).unwrap_or_default();
    match what {
        "date" => {
            let today = releases::today().format("%Y-%m-%d").to_string();
            change(app, st, |s| s.date = today);
            let date = st.rel.sheet.as_ref().map(|s| releases::fr_date(&s.date)).unwrap_or_default();
            app.global::<Rel>().set_date_text(date.into());
        }
        "tracks" => picker_show(app, st),
        "cover" => pick_cover(app, st),
        "credits" => prefill_credits(app, st),
        "texts" => {
            if let Some(first) = tracks.first() {
                crate::publish::open(app, st, *first);
            }
        }
        "master" => master(app, st),
        "links" => {
            let id = st.rel.sheet.as_ref().map(|s| s.id);
            crate::fond::link(app, st, id);
        }
        "analyze" => {
            let lib = otoko_library::career::library::field;
            let todo: Vec<(i64, String)> = st.career.as_ref().map(|c| tracks.iter().filter(|id| lib(c.conn(), **id, "analyzed_at").is_empty()).map(|id| (*id, lib(c.conn(), *id, "path"))).collect()).unwrap_or_default();
            crate::analyse::start(app, st, todo, false);
        }
        "genre" => crate::outils::open(app, st, "batches"),
        "isrc" => app.global::<Rel>().set_msg("Note les ISRC dans « Pour le distributeur », plus bas.".into()),
        w if w.starts_with("step:") => {
            let key = w[5..].to_string();
            change(app, st, |s| {
                if !s.steps.contains(&key) {
                    s.steps.push(key.clone());
                }
            });
        }
        w if w.starts_with("track:") => {
            if let Ok(id) = w[6..].parse() {
                crate::bibliotheque::reveal(app, st, id);
            }
        }
        _ => {}
    }
}

/// (niveau 0 bon / 1 à voir / 2 problème, message) pour une pochette (comme `cover_check`).
pub fn cover_check(path: &str) -> (i32, String) {
    if path.is_empty() {
        return (1, "Aucune pochette.".into());
    }
    if !Path::new(path).exists() {
        return (2, "Fichier de pochette introuvable.".into());
    }
    let Ok((w, h)) = image::image_dimensions(path) else { return (2, "Image illisible.".into()) };
    let mut msgs = Vec::new();
    let mut level = 0;
    if w != h {
        msgs.push(format!("pas carrée ({w}×{h})"));
        level = 1;
    }
    if w.min(h) < 800 {
        msgs.push(format!("trop petite ({w}×{h}, SoundCloud demande au moins 800×800)"));
        level = 2;
    } else if w.min(h) < 3000 {
        msgs.push(format!("{w}×{h} : suffisant pour SoundCloud ; prévois 3000×3000 pour les autres plateformes"));
        level = level.max(1);
    }
    if msgs.is_empty() { (0, format!("{w}×{h}, carrée : parfait.")) } else { (level, format!("Pochette {}.", msgs.join(", "))) }
}

/// `cover_check`, plus exigeant quand la sortie passe par un distributeur : 3000×3000 carrée, demandée presque partout.
pub fn cover_check_for(path: &str, streaming: bool) -> (i32, String) {
    let (level, text) = cover_check(path);
    if !streaming || level == 2 || path.is_empty() {
        return (level, text);
    }
    match image::image_dimensions(path) {
        Ok((w, h)) if w != h || w < 3000 => (2, format!("Pochette {w}×{h} : les distributeurs demandent une image carrée d'au moins 3000×3000.")),
        _ => (level, text),
    }
}

fn load_image(path: &str, size: u32) -> Option<Image> {
    let img = image::open(path).ok()?.resize_to_fill(size, size, image::imageops::FilterType::Triangle).to_rgba8();
    Some(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(img.as_raw(), size, size)))
}

fn date_hint(iso: &str) -> String {
    if iso.is_empty() {
        return String::new();
    }
    let when = match otoko_library::career::board::days_until(iso) {
        Some(0) => "aujourd'hui".to_string(),
        Some(d) if d > 0 => format!("J-{d}"),
        Some(d) => format!("il y a {} j", -d),
        None => String::new(),
    };
    format!("{} — {when}", releases::long_date(iso))
}

/// Montre la fiche ; `full` remet aussi les textes modifiables (pas pendant qu'on tape).
pub fn push_sheet(app: &AppWindow, st: &State, full: bool) {
    let (Some(c), Some(s)) = (st.career.as_ref(), st.rel.sheet.as_ref()) else { return };
    let g = app.global::<Rel>();
    g.set_id(s.id as i32);
    if full {
        g.set_title(s.title.as_str().into());
        g.set_date_text(releases::fr_date(&s.date).into());
        g.set_sc_link(s.sc_link.as_str().into());
        g.set_credits(s.credits.as_str().into());
        g.set_description(s.description.as_str().into());
        g.set_notes(s.notes.as_str().into());
        g.set_upc(s.upc.as_str().into());
        g.set_label_name(s.label.as_str().into());
        g.set_language(s.language.as_str().into());
        // Pas pendant qu'on tape un ISRC : la case perdrait ce qui est en cours.
        g.set_isrcs(model(
            releases::track_lines(c.conn(), &s.tracks)
                .iter()
                .map(|t| IsrcUi { id: t.id as i32, title: t.title.as_str().into(), isrc: otoko_library::career::library::field(c.conn(), t.id, "isrc").into() })
                .collect(),
        ));
        let (level, info) = cover_check_for(&s.cover_path, s.streaming());
        g.set_cover_level(level);
        g.set_cover_info(info.into());
        let img = if s.cover_path.is_empty() { None } else { load_image(&s.cover_path, 320) };
        g.set_has_cover(img.is_some());
        g.set_cover(img.unwrap_or_default());
    }
    g.set_rtype(s.rtype.as_str().into());
    g.set_platforms(model(PLATFORMS.iter().map(|(k, label, hint)| PlatformUi { key: (*k).into(), label: (*label).into(), hint: (*hint).into(), on: s.on(k) }).collect()));
    g.set_streaming(s.streaming());
    g.set_soundcloud(s.on("soundcloud"));
    g.set_explicit(s.explicit);
    g.set_has_date(!s.date.is_empty());
    g.set_date_bad(false);
    g.set_date_hint(date_hint(&s.date).into());
    let lines = releases::track_lines(c.conn(), &s.tracks);
    g.set_tracks(model(
        lines
            .iter()
            .map(|t| RelTrackUi {
                id: t.id as i32,
                title: t.title.as_str().into(),
                sub: if t.private { format!("{} · prod privée (ne pas publier sur tes réseaux)", t.sub) } else { t.sub.clone() }.into(),
                warn: t.private || t.sub.contains("introuvable"),
            })
            .collect(),
    ));
    // Les étapes, avec leur date conseillée si la sortie est datée.
    let planned = releases::planning(c.conn(), releases::today()).into_iter().find(|p| p.id == s.id);
    g.set_steps(model(
        steps_for(&s.platforms)
            .iter()
            .map(|(k, _)| {
                let p = planned.as_ref().and_then(|p| p.steps.iter().find(|x| x.key == *k));
                RelStepUi {
                    key: (*k).into(),
                    label: releases::step_label(k).into(),
                    done: s.steps.iter().any(|x| x == k),
                    due: p.map(|p| format!("avant le {}", p.due.format("%d/%m"))).unwrap_or_default().into(),
                    late: p.is_some_and(|p| p.late),
                }
            })
            .collect(),
    ));
    let (level, info) = cover_check_for(&s.cover_path, s.streaming());
    let p = pilotage::pilot(c.conn(), s, releases::today(), (level, &info));
    g.set_pilot_count(p.countdown.into());
    g.set_pilot_tone(p.tone);
    g.set_pilot_date(p.date.into());
    g.set_pilot_next(p.next.into());
    g.set_pilot_done(p.done as i32);
    g.set_pilot_total(p.total as i32);
    g.set_points(model(p.points.into_iter().map(|x| RelPointUi { level: x.level, text: x.text.into(), action: x.action.into(), label: x.label.into() }).collect()));
}

fn save(app: &AppWindow, st: &State) -> bool {
    let (Some(c), Some(s)) = (st.career.as_ref(), st.rel.sheet.as_ref()) else { return false };
    match releases::save(c.conn(), s) {
        Ok(()) => true,
        Err(e) => {
            app.global::<Rel>().set_msg(format!("Enregistrement impossible : {e}").into());
            false
        }
    }
}

/// Modifie la fiche ouverte, l'enregistre et la remontre.
fn change(app: &AppWindow, st: &mut State, f: impl FnOnce(&mut Sheet)) {
    let Some(s) = st.rel.sheet.as_mut() else { return };
    f(s);
    if save(app, st) {
        app.global::<Rel>().set_msg("Enregistré.".into());
    }
    push_sheet(app, st, false);
}

fn edited(app: &AppWindow, st: &mut State, key: &str, value: &str) {
    let Some(s) = st.rel.sheet.as_mut() else { return };
    // Un lien collé d'un coup (pas tapé lettre à lettre) relie aussitôt les morceaux.
    let pasted = key == "sc_link" && otoko_library::career::liens::is_soundcloud(value) && value.len() > s.sc_link.len() + 8;
    match key {
        "title" => s.title = value.to_string(),
        "sc_link" => s.sc_link = value.to_string(),
        "credits" => s.credits = value.to_string(),
        "description" => s.description = value.to_string(),
        "notes" => s.notes = value.to_string(),
        "upc" => s.upc = value.to_string(),
        "label" => s.label = value.to_string(),
        "language" => s.language = value.to_string(),
        "date" => {
            let iso = releases::iso_date(value);
            let g = app.global::<Rel>();
            if iso.is_empty() {
                g.set_date_bad(true);
                g.set_date_hint("date à écrire JJ/MM/AAAA".into());
                return;
            }
            s.date = iso;
        }
        _ => return,
    }
    if save(app, st) {
        app.global::<Rel>().set_msg("Enregistré.".into());
    }
    // Le pilotage suit ce qu'on écrit (crédits remplis, lien collé…).
    push_sheet(app, st, false);
    if pasted {
        let id = st.rel.sheet.as_ref().map(|s| s.id);
        crate::fond::link(app, st, id);
    }
}

fn pick_cover(app: &AppWindow, st: &mut State) {
    let Some(f) = rfd::FileDialog::new().set_title("Choisir la pochette").add_filter("Images", &["jpg", "jpeg", "png"]).pick_file() else { return };
    let id = st.rel.sheet.as_ref().map(|s| s.id).unwrap_or(0);
    change(app, st, |s| s.cover_path = f.to_string_lossy().into_owned());
    // La pochette de la galerie et de l'accueil est relue.
    st.home.covers.remove(&id);
    st.home.covers_asked.remove(&id);
    push_sheet(app, st, true);
    push_gallery(app, st);
}

/// L'image ramenée en JPEG de 1000 px au plus (qualité 90), comme Studio Vault.
pub fn prepare_jpeg(path: &str, max_side: u32) -> Result<Vec<u8>, String> {
    let img = image::open(path).map_err(|e| format!("image illisible : {e}"))?;
    let img = if img.width().max(img.height()) > max_side { img.resize(max_side, max_side, image::imageops::FilterType::Lanczos3) } else { img };
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&rgb).map_err(|e| e.to_string())?;
    Ok(out)
}

fn embed_cover(app: &AppWindow, st: &mut State) {
    let (Some(c), Some(s)) = (st.career.as_ref(), st.rel.sheet.as_ref()) else { return };
    if s.cover_path.is_empty() || !Path::new(&s.cover_path).exists() {
        app.global::<Rel>().set_msg("Choisis d'abord une pochette.".into());
        return;
    }
    let tracks: Vec<(String, String)> = releases::track_lines(c.conn(), &s.tracks)
        .into_iter()
        .filter(|t| Path::new(&t.path).extension().is_some_and(|e| files::COVER_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str())))
        .map(|t| (t.title, t.path))
        .collect();
    if tracks.is_empty() {
        app.global::<Rel>().set_msg("Aucun morceau compatible dans cette sortie.".into());
        return;
    }
    // L'autotest n'écrit jamais dans les fichiers de l'utilisateur.
    if st.selftest {
        app.global::<Rel>().set_msg(format!("Autotest : la pochette irait dans {} fichier(s), rien n'est écrit.", tracks.len()).into());
        return;
    }
    let text = format!("Écrire la pochette dans {} fichier(s) ?\n\nElle remplace la pochette déjà intégrée, s'il y en a une. Le son n'est pas modifié.", tracks.len());
    if !confirm(st, "Intégrer la pochette", &text) {
        return;
    }
    let cover = s.cover_path.clone();
    app.global::<Rel>().set_msg(format!("Écriture de la pochette dans {} fichier(s)…", tracks.len()).into());
    st.spawn(move || {
        let jpeg = match prepare_jpeg(&cover, 1000) {
            Ok(j) => j,
            Err(e) => return Done::CoverEmbedded(0, vec![e]),
        };
        let mut failed = Vec::new();
        let mut done = 0;
        for (title, path) in tracks {
            match files::embed_cover(Path::new(&path), &jpeg) {
                Ok(()) => done += 1,
                Err(e) => failed.push(format!("{title} : {e}")),
            }
        }
        Done::CoverEmbedded(done, failed)
    });
}

pub fn embedded(app: &AppWindow, st: &mut State, done: usize, failed: Vec<String>) {
    let _ = st;
    let mut msg = format!("Pochette intégrée à {done} fichier(s).");
    if !failed.is_empty() {
        msg.push_str(&format!(" Échec : {}", failed.iter().take(4).cloned().collect::<Vec<_>>().join(" ; ")));
    }
    app.global::<Rel>().set_msg(msg.as_str().into());
    app.set_message(msg.into());
}

fn picker_show(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    st.rel.picker = c
        .conn()
        .prepare(
            "SELECT id, COALESCE(NULLIF(title, ''), filename), IFNULL(album, ''), IFNULL(NULLIF(nature, ''), 'original'), IFNULL(nature_meta, '{}')
             FROM tracks WHERE missing = 0 ORDER BY COALESCE(NULLIF(title, ''), filename) COLLATE NOCASE",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                let (nat, meta): (String, String) = (r.get(3)?, r.get(4)?);
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, otoko_library::career::nature::is_private(&nat, &otoko_library::career::nature::load_meta(&meta))))
            })?
            .collect()
        })
        .unwrap_or_default();
    picker_push(app, st, "");
    app.global::<Rel>().set_picker_open(true);
}

fn picker_push(app: &AppWindow, st: &State, q: &str) {
    let q = q.trim().to_lowercase();
    let present: Vec<i64> = st.rel.sheet.as_ref().map(|s| s.tracks.clone()).unwrap_or_default();
    let rows: Vec<RelTrackUi> = st
        .rel
        .picker
        .iter()
        .filter(|t| !present.contains(&t.0))
        .filter(|t| q.is_empty() || t.1.to_lowercase().contains(&q) || t.2.to_lowercase().contains(&q))
        .take(300)
        .map(|t| RelTrackUi {
            id: t.0 as i32,
            title: t.1.as_str().into(),
            sub: if t.3 { format!("{} · prod privée", t.2) } else { t.2.clone() }.into(),
            warn: t.3,
        })
        .collect();
    app.global::<Rel>().set_picker(model(rows));
}

fn picker_choose(app: &AppWindow, st: &mut State, id: i64) {
    let private = st.rel.picker.iter().any(|t| t.0 == id && t.3);
    if private && !confirm(st, "Prod privée", "Ce morceau est une prod marquée « ne pas publier sur mes réseaux ». L'ajouter quand même ?") {
        return;
    }
    change(app, st, |s| {
        if !s.tracks.contains(&id) {
            s.tracks.push(id);
        }
    });
    app.global::<Rel>().set_picker_open(false);
}

fn prefill_credits(app: &AppWindow, st: &mut State) {
    let (Some(c), Some(s)) = (st.career.as_ref(), st.rel.sheet.as_ref()) else { return };
    let artist = releases::artist_name(c.conn());
    let lines = releases::prefill_credits(c.conn(), &s.tracks, if artist.is_empty() { "moi" } else { &artist });
    let current = s.credits.trim().to_string();
    let text = if current.is_empty() { lines } else { format!("{lines}\n{current}") };
    change(app, st, |s| s.credits = text.clone());
    app.global::<Rel>().set_credits(text.into());
}

// ---------------------------------------------------------------------- contrôle

fn level_of(l: &str) -> i32 {
    match l {
        "ok" => 0,
        "attention" => 1,
        "problème" => 2,
        _ => 3,
    }
}

fn qc_run(app: &AppWindow, st: &mut State) {
    let Some(s) = st.rel.sheet.as_ref() else { return };
    if s.tracks.is_empty() {
        app.global::<Rel>().set_msg("Ajoute d'abord des morceaux à la sortie.".into());
        return;
    }
    let (ids, platforms) = (s.tracks.clone(), s.platforms.clone());
    qc_on(app, st, &ids, &platforms);
}

/// Le contrôle avant publication de ces morceaux (la sélection de la bibliothèque), pour les plateformes par défaut.
pub fn qc_for(app: &AppWindow, st: &mut State, ids: &[i64]) {
    let platforms = st.career.as_ref().map(|c| releases::default_platforms(c.conn())).unwrap_or_default();
    qc_on(app, st, ids, &platforms);
}

/// Le contrôle de ces morceaux pour ces plateformes.
fn qc_on(app: &AppWindow, st: &mut State, ids: &[i64], platforms: &[String]) {
    let Some(c) = st.career.as_ref() else { return };
    let lines = releases::track_lines(c.conn(), ids);
    st.rel.qc = lines.iter().map(|t| (t.id, t.title.clone(), releases::qc_on(c.conn(), t.id, platforms))).collect();
    let streaming = platforms.iter().any(|p| p == "streaming");
    let intro = if streaming || platforms.iter().any(|p| p == "youtube") {
        "Le streaming ramène chaque morceau vers -14 LUFS et convertit le fichier envoyé ; un distributeur refuse un fichier compressé. Format, niveau, crêtes, saturation, silences et infos de chaque morceau. Clique une ligne pour le détail."
    } else {
        "Ces plateformes ne touchent pas au volume et convertissent le fichier envoyé : format, niveau, crêtes, saturation, silences et infos de chaque morceau. Clique une ligne pour le détail."
    };
    let g = app.global::<Rel>();
    g.set_qc_title(format!("Contrôle avant publication — {}", platforms_label(platforms)).into());
    g.set_qc_intro(intro.into());
    let rows: Vec<QcRowUi> = st
        .rel
        .qc
        .iter()
        .map(|(_, title, res)| QcRowUi {
            title: title.as_str().into(),
            cells: model(
                releases::QC_NAMES
                    .iter()
                    .map(|n| {
                        let lvl = res.iter().find(|x| x.0 == *n).map(|x| x.1).unwrap_or("—");
                        let sym = match lvl {
                            "ok" => "✓",
                            "attention" => "!",
                            "problème" => "✗",
                            _ => "—",
                        };
                        QcCellUi { level: level_of(lvl), text: format!("{sym}  {lvl}").into() }
                    })
                    .collect(),
            ),
        })
        .collect();
    let g = app.global::<Rel>();
    g.set_qc(model(rows));
    g.set_qc_unanalyzed(st.rel.qc.iter().filter(|q| q.2.iter().any(|x| x.1 == "—")).count() as i32);
    g.set_qc_open(true);
    qc_detail(app, st, 0);
}

fn qc_detail(app: &AppWindow, st: &State, i: usize) {
    let Some((_, title, res)) = st.rel.qc.get(i) else { return };
    let mut text = vec![title.clone()];
    text.extend(res.iter().map(|(n, l, m)| format!("•  {n} — {l} : {m}")));
    app.global::<Rel>().set_qc_detail(text.join("\n").into());
}

fn export_sheet(app: &AppWindow, st: &mut State) {
    let (Some(c), Some(s)) = (st.career.as_ref(), st.rel.sheet.as_ref()) else { return };
    let name = if s.title.trim().is_empty() { "sortie".to_string() } else { s.title.trim().chars().map(|ch| if r#"\/:*?"<>|"#.contains(ch) { '_' } else { ch }).collect() };
    let Some(f) = rfd::FileDialog::new().set_title("Exporter la fiche").set_file_name(format!("{name}.txt")).add_filter("Texte", &["txt"]).save_file() else { return };
    let text = releases::export_text(c.conn(), s);
    let msg = match std::fs::write(&f, text) {
        Ok(()) => format!("Fiche exportée : {}", f.display()),
        Err(e) => format!("Export impossible : {e}"),
    };
    app.global::<Rel>().set_msg(msg.into());
}

// ---------------------------------------------------------------------- distributeur et agenda

/// Un nom de fichier sans les caractères que Windows refuse.
fn safe_name(s: &str) -> String {
    let t: String = s.trim().chars().map(|ch| if r#"\/:*?"<>|"#.contains(ch) || ch.is_control() { '_' } else { ch }).collect();
    if t.is_empty() { "sortie".into() } else { t }
}

/// Le dossier à envoyer au distributeur : choisi par l'utilisateur, rempli en fond.
fn delivery(app: &AppWindow, st: &mut State) {
    let Some(s) = st.rel.sheet.clone() else { return };
    if s.tracks.is_empty() {
        app.global::<Rel>().set_msg("Ajoute d'abord des morceaux à la sortie.".into());
        return;
    }
    if st.selftest {
        return;
    }
    let Some(parent) = rfd::FileDialog::new().set_title("Où préparer l'envoi au distributeur ?").pick_folder() else { return };
    let db = st.career_path.clone();
    app.global::<Rel>().set_msg("Préparation de l'envoi…".into());
    st.spawn(move || {
        let r = otoko_library::career::Career::open(&db).map_err(|e| e.to_string()).and_then(|c| releases::delivery(c.conn(), &s, &parent.join(format!("{} (envoi)", safe_name(&s.title)))));
        Done::Say(match r {
            Ok(d) if d.missing.is_empty() => format!("Envoi prêt : {} fichier(s), la pochette et les infos dans {}", d.copied, d.dir.display()),
            Ok(d) => format!("Envoi préparé dans {}, mais introuvable : {}", d.dir.display(), d.missing.join(", ")),
            Err(e) => format!("Envoi impossible : {e}"),
        })
    });
}

/// Le planning et les échéances en .ics, ouvert ensuite par l'agenda de Windows (ou importé à la main).
fn agenda(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let text = releases::agenda_ics(c.conn(), releases::today());
    if st.selftest {
        return;
    }
    let Some(f) = rfd::FileDialog::new().set_title("Ajouter à mon agenda").set_file_name("OTOKO - sorties et échéances.ics").add_filter("Agenda", &["ics"]).save_file() else { return };
    let msg = match std::fs::write(&f, text) {
        Ok(()) => {
            let _ = std::process::Command::new("explorer.exe").arg(&f).spawn();
            format!("Agenda écrit : {}. Ouvre-le avec ton agenda, ou importe-le dans Google Agenda (Paramètres › Importer).", f.display())
        }
        Err(e) => format!("Agenda impossible à écrire : {e}"),
    };
    app.global::<Rel>().set_msg(msg.clone().into());
    app.set_message(msg.into());
}

// ---------------------------------------------------------------------- planning

pub fn planning(app: &AppWindow, st: &mut State) {
    st.rel.mode = "planning".into();
    app.global::<Rel>().set_mode("planning".into());
    crate::raccourcis::tab(app, st, 2);
    // Le mois de la prochaine sortie, sinon celui d'aujourd'hui.
    if st.rel.month.is_none() {
        let next = st.career.as_ref().and_then(|c| releases::planning(c.conn(), releases::today()).into_iter().find(|p| p.days >= 0)).map(|p| p.date.format("%Y-%m").to_string());
        st.rel.month = next.and_then(|n| Some((n[..4].parse().ok()?, n[5..].parse().ok()?))).or_else(|| Some(this_month()));
    }
    push_planning(app, st);
}

pub fn push_planning(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let today = releases::today();
    let rels = releases::planning(c.conn(), today);
    let (y, m) = st.rel.month.unwrap_or_else(this_month);
    let days: Vec<DayUi> = releases::month_grid(y, m)
        .into_iter()
        .map(|(d, day, in_month)| {
            let release = rels.iter().find(|r| r.date == d);
            let step = rels.iter().filter(|r| r.days >= 0).find(|r| r.steps.iter().any(|s| !s.done && s.due == d));
            DayUi {
                day: day as i32,
                in_month,
                release: release.is_some(),
                step: step.is_some(),
                today: d == today,
                rel_id: release.or(step).map(|r| r.id as i32).unwrap_or(-1),
            }
        })
        .collect();
    let mut plan = Vec::new();
    let upcoming: Vec<&releases::Planned> = rels.iter().filter(|r| r.days >= 0).collect();
    let past: Vec<&releases::Planned> = rels.iter().filter(|r| r.days < 0).rev().take(5).collect();
    for (group, items, expand) in [("À VENIR", upcoming, true), ("DÉJÀ SORTIES (RÉCENTES)", past, false)] {
        if items.is_empty() {
            continue;
        }
        plan.push(PlanRelUi { title: group.into(), header: true, id: -1, ..Default::default() });
        for r in items {
            let when = match r.days {
                0 => "aujourd'hui".to_string(),
                d if d > 0 => format!("dans {d} j"),
                d => format!("il y a {} j", -d),
            };
            let steps: Vec<RelStepUi> = if expand {
                r.steps
                    .iter()
                    .map(|s| RelStepUi { key: s.key.into(), label: releases::step_label(s.key).into(), done: s.done, due: s.due.format("%d/%m/%Y").to_string().into(), late: s.late })
                    .collect()
            } else {
                Vec::new()
            };
            plan.push(PlanRelUi {
                id: r.id as i32,
                title: format!("{}  ·  {}", r.title, otoko_library::career::release_label(&r.kind)).into(),
                date: r.date.format("%d/%m/%Y").to_string().into(),
                state: format!("{when} — {}/{} étapes", r.steps.len() - r.remaining, r.steps.len()).into(),
                header: false,
                steps: model(steps),
            });
        }
    }
    if plan.is_empty() {
        plan.push(PlanRelUi { title: "Aucune sortie datée : fixe une date dans la fiche d'une sortie.".into(), header: true, id: -1, ..Default::default() });
    }
    let g = app.global::<Rel>();
    g.set_month_label(releases::month_label(y, m).into());
    g.set_days(model(days));
    g.set_plan(model(plan));
}

// ---------------------------------------------------------------------- discographie

/// Les pochettes de la discographie : celle de la fiche, sinon celle de ses morceaux (cache du
/// serveur mobile), ramenées en JPEG RVB de 440 px.
fn disc_covers(db: &Path, cache: &Path, discs: &[discography::Disc]) -> std::collections::HashMap<i64, discography::Cover> {
    let mut out = std::collections::HashMap::new();
    let Ok(con) = otoko_mobile::library::connect(db) else { return out };
    let media = otoko_mobile::media::Cache::new(cache.to_path_buf());
    for d in discs {
        let jpg = if !d.cover_path.is_empty() && Path::new(&d.cover_path).exists() {
            prepare_jpeg(&d.cover_path, 440).ok()
        } else {
            media.cover_jpeg(&otoko_mobile::library::release_cover_sources(&con, d.id), 440).map(|x| x.0)
        };
        // Toujours ré-encodée : un JPEG progressif ou en CMJN ne passerait pas dans le PDF.
        let Some(img) = jpg.and_then(|j| image::load_from_memory(&j).ok()) else { continue };
        let img = img.resize_to_fill(440, 440, image::imageops::FilterType::Triangle).to_rgb8();
        let mut bytes = Vec::new();
        if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 88).encode_image(&img).is_ok() {
            out.insert(d.id, (440, 440, bytes));
        }
    }
    out
}

/// Écrit la discographie (PDF ou page web selon l'extension).
pub fn write_discography(db: &Path, cache: &Path, out: &Path, only_released: bool) -> Result<PathBuf, String> {
    let c = otoko_library::career::Career::open(db).map_err(|e| e.to_string())?;
    let artist = releases::artist_name(c.conn());
    let discs = discography::collect(c.conn(), &artist, only_released);
    let covers = disc_covers(db, cache, &discs);
    let get = |d: &discography::Disc| covers.get(&d.id).cloned();
    let html = out.extension().is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
    let bytes = if html { discography::html(&artist, &discs, &get).into_bytes() } else { discography::pdf(&artist, &discs, &get) };
    std::fs::write(out, bytes).map_err(|e| format!("{} : {e}", out.display()))?;
    Ok(out.to_path_buf())
}

pub fn discography_dialog(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let artist = releases::artist_name(c.conn());
    let docs = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Documents")).unwrap_or_default();
    let Some(mut out) = rfd::FileDialog::new()
        .set_title("Exporter la discographie")
        .set_directory(&docs)
        .set_file_name(discography::file_name(&artist, "pdf"))
        .add_filter("PDF", &["pdf"])
        .add_filter("Page web autonome", &["html"])
        .save_file()
    else {
        return;
    };
    if out.extension().is_none() {
        out.set_extension("pdf");
    }
    let only = rfd::MessageDialog::new()
        .set_title("Exporter la discographie")
        .set_description("Inclure seulement les sorties déjà parues ?\n\n(« Non » ajoute aussi celles à venir et sans date.)")
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes;
    let db = st.career_path.clone();
    let cache = if st.career_path == crate::career::default_path() { crate::donnees::data_dir().join("mobile") } else { std::env::temp_dir().join("otoko-essai-pochettes") };
    app.set_message("Discographie en cours…".into());
    st.spawn(move || Done::Discography(write_discography(&db, &cache, &out, only)));
}

pub fn exported(app: &AppWindow, st: &mut State, r: Result<PathBuf, String>) {
    match r {
        Ok(p) => {
            app.set_message(format!("Discographie exportée : {}", p.display()).into());
            if !st.selftest {
                let _ = std::process::Command::new("explorer.exe").arg(&p).spawn();
            }
        }
        Err(e) => app.set_message(format!("Export impossible : {e}").into()),
    }
}
