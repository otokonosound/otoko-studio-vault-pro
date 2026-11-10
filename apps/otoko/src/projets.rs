//! Carrière › fiche projet (remix, edit, mashup, set, prod), carnet de clients et constructeur de
//! set (comme `project_dialog.py`, `clients_dialog.py` et `set_builder_dialog.py` de Studio Vault).
//! La fiche se modifie sur une copie de travail, enregistrée par « Enregistrer » (comme Studio Vault) ;
//! le carnet de clients et le titre « (Artiste Remix) » s'écrivent aussitôt.

use std::cell::RefCell;
use std::rc::Rc;

use otoko_library::career::nature::{self, AGREEMENTS, AUTHORIZATION, DELIVERABLES, ORDER_STATUSES};
use otoko_library::career::projects::{self, Client, Project, SetEntry};
use otoko_library::career::set_builder::{self as sb, SetTrack};
use otoko_library::career::{NATURES, camelot, releases};
use serde_json::{Value as Json, json};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::state::State;
use crate::{AppWindow, ClientUi, DelivUi, EntryUi, PickUi, Proj, SbRowUi, SrcUi, VersionRowUi};

macro_rules! proj_on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.global::<Proj>().$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

#[derive(Default)]
pub struct View {
    /// La fiche ouverte (copie de travail).
    pub project: Option<Project>,
    /// « source » (mashup) ou « entry » (set) : à quoi sert le morceau choisi.
    pub picker_for: String,
    pub pick: Vec<(i64, String, String, Option<f64>, String)>,
    pub clients: Vec<Client>,
    pub client_sel: Option<usize>,
    pub sb_pool: Vec<SetTrack>,
    pub sb_order: Vec<SetTrack>,
    pub sb_starts: Vec<Option<i64>>,
    pub sb_sets: Vec<(i64, String)>,
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

fn strings(v: impl IntoIterator<Item = impl Into<SharedString>>) -> ModelRc<SharedString> {
    model(v.into_iter().map(Into::into).collect())
}

fn confirm(st: &State, title: &str, text: &str) -> bool {
    st.selftest || rfd::MessageDialog::new().set_title(title).set_description(text).set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes
}

fn artist(st: &State) -> String {
    st.career.as_ref().map(|c| releases::artist_name(c.conn())).unwrap_or_default()
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    proj_on!(app, state, on_set_nature, |app, st, n| {
        if let Some(p) = st.proj.project.as_mut() {
            p.nature = n.to_string();
        }
        push(&app, st, true);
    });
    proj_on!(app, state, on_field, |app, st, key, value| { field(&app, st, key.as_str(), value.as_str()); });
    proj_on!(app, state, on_toggle, |app, st, key| { toggle(&app, st, key.as_str()); });
    proj_on!(app, state, on_choose, |app, st, key, i| { choose(&app, st, key.as_str(), i.max(0) as usize); });
    proj_on!(app, state, on_source_edited, |app, st, r, c, t| { source_edited(&app, st, r as usize, c, t.as_str()); });
    proj_on!(app, state, on_source_add, |app, st| {
        meta_mut(st, |m| sources_mut(m).push(json!({})));
        push(&app, st, false);
    });
    proj_on!(app, state, on_source_remove, |app, st, r| {
        meta_mut(st, |m| {
            let s = sources_mut(m);
            if (r as usize) < s.len() {
                s.remove(r as usize);
            }
        });
        push(&app, st, false);
    });
    proj_on!(app, state, on_entry_edited, |_app, st, r, c, t| { entry_edited(st, r as usize, c, t.as_str()); });
    proj_on!(app, state, on_entry_add, |app, st| {
        if let Some(p) = st.proj.project.as_mut() {
            let time = next_time(&p.entries);
            p.entries.push(SetEntry { time, ..Default::default() });
        }
        push(&app, st, false);
    });
    proj_on!(app, state, on_entry_remove, |app, st, r| {
        if let Some(p) = st.proj.project.as_mut() {
            if (r as usize) < p.entries.len() {
                p.entries.remove(r as usize);
            }
        }
        push(&app, st, false);
    });
    proj_on!(app, state, on_import_markers, |app, st| { import_markers(&app, st); });
    proj_on!(app, state, on_copy_tracklist, |app, st| {
        let Some(p) = st.proj.project.as_ref() else { return };
        let list: Vec<nature::Entry> = sorted(&p.entries).iter().map(|e| e.entry()).collect();
        let text = nature::tracklist_text(&list, p.duration);
        if !st.selftest {
            crate::publish::copy(&text);
        }
        app.global::<Proj>().set_msg("Liste copiée : colle-la dans la description SoundCloud.".into());
    });
    proj_on!(app, state, on_export_cue, |app, st| { export_cue(&app, st); });
    proj_on!(app, state, on_check_transitions, |app, st| {
        let report = st.proj.project.as_ref().map(|p| projects::transitions_report(&p.entries)).unwrap_or_default();
        app.global::<Proj>().set_set_report(report.into());
    });
    proj_on!(app, state, on_deliverable, |app, st, key| {
        meta_mut(st, |m| {
            if !m["deliverables"].is_object() {
                m["deliverables"] = json!({});
            }
            let on = m["deliverables"][key.as_str()].as_bool().unwrap_or(false);
            m["deliverables"][key.as_str()] = json!(!on);
        });
        push(&app, st, false);
    });
    proj_on!(app, state, on_version_add, |app, st| {
        let today = releases::fr_date(&releases::today().format("%Y-%m-%d").to_string());
        meta_mut(st, |m| versions_mut(m).push(json!({"date": today, "label": "", "note": ""})));
        push(&app, st, false);
    });
    proj_on!(app, state, on_version_edited, |_app, st, r, c, t| {
        let key = ["date", "label", "note"][c.clamp(0, 2) as usize];
        meta_mut(st, |m| {
            if let Some(v) = versions_mut(m).get_mut(r as usize) {
                v[key] = json!(t.as_str());
            }
        });
    });
    proj_on!(app, state, on_version_remove, |app, st, r| {
        meta_mut(st, |m| {
            let v = versions_mut(m);
            if (r as usize) < v.len() {
                v.remove(r as usize);
            }
        });
        push(&app, st, false);
    });
    proj_on!(app, state, on_sent_add, |app, st| {
        let Some(id) = st.proj.project.as_ref().map(|p| p.id) else { return };
        let sent = crate::partage::sent_versions(st, id);
        let mut added = 0;
        meta_mut(st, |m| {
            let v = versions_mut(m);
            for s in sent {
                // Déjà reporté (même date, même lien) : la note est mise à jour, la ligne n'est pas doublée.
                match v.iter_mut().find(|x| text(&x["date"]) == s.date && text(&x["label"]) == s.label) {
                    Some(x) => x["note"] = json!(s.note),
                    None => {
                        v.push(json!({ "date": s.date, "label": s.label, "note": s.note }));
                        added += 1;
                    }
                }
            }
        });
        push(&app, st, false);
        app.global::<Proj>().set_msg(if added == 0 { "Versions à jour (pense à enregistrer).".into() } else { format!("{added} ligne{} ajoutée{} (pense à enregistrer).", if added > 1 { "s" } else { "" }, if added > 1 { "s" } else { "" }).into() });
    });
    proj_on!(app, state, on_pick_dir, |app, st| {
        let Some(p) = st.proj.project.as_ref() else { return };
        let start = p.meta["stems_dir"].as_str().filter(|s| !s.is_empty()).map(std::path::PathBuf::from).or_else(|| std::path::Path::new(&p.path).parent().map(|d| d.to_path_buf()));
        let mut dlg = rfd::FileDialog::new().set_title("Dossier des stems reçus");
        if let Some(s) = start {
            dlg = dlg.set_directory(s);
        }
        if let Some(d) = dlg.pick_folder() {
            let d = d.to_string_lossy().into_owned();
            meta_mut(st, |m| m["stems_dir"] = json!(d));
            app.global::<Proj>().set_stems_dir(d.into());
        }
    });
    proj_on!(app, state, on_open_dir, |_app, st| {
        let dir = st.proj.project.as_ref().and_then(|p| p.meta["stems_dir"].as_str().map(str::to_string)).unwrap_or_default();
        if !dir.is_empty() && std::path::Path::new(&dir).exists() && !st.selftest {
            let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
        }
    });
    proj_on!(app, state, on_remix_title, |app, st| { remix_title(&app, st); });
    proj_on!(app, state, on_copy_credits, |app, st| {
        let Some(p) = st.proj.project.as_ref() else { return };
        let a = artist(st);
        let text = nature::credits_for(&p.nature, &p.meta, if a.is_empty() { "moi" } else { &a });
        if !st.selftest {
            crate::publish::copy(&text);
        }
        app.global::<Proj>().set_msg(format!("Copié : {text}").into());
    });
    proj_on!(app, state, on_save, |app, st| { save(&app, st); });
    proj_on!(app, state, on_cancel, |app, st| {
        st.proj.project = None;
        app.global::<Proj>().set_open(false);
    });
    proj_on!(app, state, on_picker_show, |app, st, what| {
        st.proj.picker_for = what.to_string();
        st.proj.pick = st.career.as_ref().map(|c| projects::pickable(c.conn())).unwrap_or_default();
        picker_push(&app, st, "");
        app.global::<Proj>().set_picker_open(true);
    });
    proj_on!(app, state, on_picker_search, |app, st, q| { picker_push(&app, st, q.as_str()); });
    proj_on!(app, state, on_picker_choose, |app, st, id| { picker_choose(&app, st, id as i64); });
    proj_on!(app, state, on_picker_close, |app, st| {
        let _ = st;
        app.global::<Proj>().set_picker_open(false);
    });
    // Carnet de clients.
    proj_on!(app, state, on_clients_show, |app, st| { clients_show(&app, st, None); });
    proj_on!(app, state, on_client_select, |app, st, i| { client_select(&app, st, i.max(0) as usize); });
    proj_on!(app, state, on_client_new, |app, st| {
        let id = st.career.as_ref().and_then(|c| projects::new_client(c.conn()).ok());
        clients_show(&app, st, id);
    });
    proj_on!(app, state, on_client_save, |app, st| {
        let g = app.global::<Proj>();
        let Some(c) = st.proj.client_sel.and_then(|i| st.proj.clients.get(i)).cloned() else { return };
        let edited = Client { id: c.id, name: g.get_c_name().to_string(), contact: g.get_c_contact().to_string(), notes: g.get_c_notes().to_string() };
        if let Some(car) = st.career.as_ref() {
            let _ = projects::save_client(car.conn(), &edited);
        }
        clients_show(&app, st, Some(c.id));
    });
    proj_on!(app, state, on_client_delete, |app, st| {
        let Some(c) = st.proj.client_sel.and_then(|i| st.proj.clients.get(i)).cloned() else { return };
        if !confirm(st, "Supprimer", &format!("Supprimer « {} » du carnet ?\nLes morceaux liés gardent leurs informations.", c.name)) {
            return;
        }
        if let Some(car) = st.career.as_ref() {
            let _ = projects::delete_client(car.conn(), c.id);
        }
        clients_show(&app, st, None);
    });
    proj_on!(app, state, on_clients_close, |app, st| {
        app.global::<Proj>().set_clients_open(false);
        // La fiche ouverte retrouve la liste à jour.
        if st.proj.project.is_some() {
            push(&app, st, false);
        }
    });
    // Constructeur de set.
    proj_on!(app, state, on_sb_set_mode, |app, st, m| {
        app.global::<Proj>().set_sb_mode(m.clone());
        sb_build(&app, st);
    });
    proj_on!(app, state, on_sb_build, |app, st| { sb_build(&app, st); });
    proj_on!(app, state, on_sb_refresh, |app, st| { sb_push(&app, st); });
    proj_on!(app, state, on_sb_move, |app, st, step| {
        let g = app.global::<Proj>();
        let r = g.get_sb_sel();
        let j = r + step;
        if r < 0 || j < 0 || j as usize >= st.proj.sb_order.len() {
            return;
        }
        st.proj.sb_order.swap(r as usize, j as usize);
        g.set_sb_sel(j);
        sb_push(&app, st);
    });
    proj_on!(app, state, on_sb_remove, |app, st| {
        let r = app.global::<Proj>().get_sb_sel();
        if r >= 0 && (r as usize) < st.proj.sb_order.len() {
            st.proj.sb_order.remove(r as usize);
            sb_push(&app, st);
        }
    });
    proj_on!(app, state, on_sb_copy, |app, st| {
        let overlap = overlap(&app);
        let text = nature::tracklist_text(&sb_entries(&st.proj.sb_order, overlap).iter().map(|e| e.entry()).collect::<Vec<_>>(), Some(sb::total_duration(&st.proj.sb_order, overlap)));
        if !st.selftest {
            crate::publish::copy(&text);
        }
        app.global::<Proj>().set_sb_summary("Liste copiée : colle-la dans la description SoundCloud.".into());
    });
    proj_on!(app, state, on_sb_export, |app, st| {
        if st.proj.sb_order.is_empty() {
            return;
        }
        let docs = std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join("Documents")).unwrap_or_default();
        let Some(mut f) = rfd::FileDialog::new().set_title("Exporter la playlist").set_directory(docs).set_file_name("set.m3u8").add_filter("Playlist", &["m3u8", "m3u"]).save_file() else { return };
        if f.extension().is_none() {
            f.set_extension("m3u8");
        }
        let msg = match std::fs::write(&f, sb::to_m3u(&st.proj.sb_order)) {
            Ok(()) => format!("Playlist enregistrée : {}", f.display()),
            Err(e) => format!("Export impossible : {e}"),
        };
        app.global::<Proj>().set_sb_summary(msg.into());
    });
    proj_on!(app, state, on_sb_save, |app, st| {
        st.proj.sb_sets = st
            .career
            .as_ref()
            .and_then(|c| {
                c.conn()
                    .prepare("SELECT id, IFNULL(title, ''), IFNULL(filename, '') FROM tracks WHERE missing = 0 AND nature = 'set' ORDER BY title")
                    .and_then(|mut s| s.query_map([], |r| Ok((r.get::<_, i64>(0)?, projects::clean_title(&r.get::<_, String>(1)?, &r.get::<_, String>(2)?))))?.collect())
                    .ok()
            })
            .unwrap_or_default();
        let g = app.global::<Proj>();
        if st.proj.sb_sets.is_empty() {
            g.set_sb_summary("Aucun morceau de nature « Set » : ajoute l'enregistrement de ton mix, puis choisis « Set » dans sa fiche projet.".into());
            return;
        }
        g.set_sb_sets(strings(st.proj.sb_sets.iter().map(|s| s.1.clone())));
        g.set_sb_saving(true);
    });
    proj_on!(app, state, on_sb_save_to, |app, st, i| { sb_save_to(&app, st, i); });
    proj_on!(app, state, on_sb_close, |app, st| {
        let _ = st;
        app.global::<Proj>().set_sb_open(false);
    });
}

// ---------------------------------------------------------------------- fiche projet

/// Ouvre la fiche projet d'un morceau.
pub fn open(app: &AppWindow, st: &mut State, id: i64) {
    let Some(p) = st.career.as_ref().and_then(|c| projects::load(c.conn(), id)) else { return };
    st.proj.clients = st.career.as_ref().map(|c| projects::clients(c.conn())).unwrap_or_default();
    st.proj.project = Some(p);
    let g = app.global::<Proj>();
    g.set_msg("".into());
    g.set_set_report("".into());
    g.set_natures(strings(NATURES.iter().map(|n| n.0)));
    g.set_nature_labels(strings(NATURES.iter().map(|n| n.1)));
    g.set_auth_labels(strings(AUTHORIZATION.iter().map(|a| a.1)));
    g.set_order_keys(strings(ORDER_STATUSES.iter().map(|o| o.0)));
    g.set_order_labels(strings(ORDER_STATUSES.iter().map(|o| o.1)));
    g.set_agreement_labels(strings(AGREEMENTS.iter().map(|a| a.1)));
    push(app, st, true);
    g.set_open(true);
}

fn meta_mut(st: &mut State, f: impl FnOnce(&mut Json)) {
    if let Some(p) = st.proj.project.as_mut() {
        if !p.meta.is_object() {
            p.meta = json!({});
        }
        f(&mut p.meta);
    }
}

fn sources_mut(m: &mut Json) -> &mut Vec<Json> {
    if !m["sources"].is_array() {
        m["sources"] = json!([]);
    }
    m["sources"].as_array_mut().expect("tableau")
}

fn versions_mut(m: &mut Json) -> &mut Vec<Json> {
    if !m["versions"].is_array() {
        m["versions"] = json!([]);
    }
    m["versions"].as_array_mut().expect("tableau")
}

fn text(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn num_text(v: &Json) -> String {
    match v.as_f64().or_else(|| v.as_str().and_then(|s| s.replace(',', ".").parse().ok())) {
        Some(f) if f.fract() == 0.0 => format!("{f:.0}"),
        Some(f) => format!("{f}"),
        None => String::new(),
    }
}

fn sorted(entries: &[SetEntry]) -> Vec<SetEntry> {
    let mut v = entries.to_vec();
    v.sort_by(|a, b| a.time.total_cmp(&b.time));
    v
}

fn next_time(entries: &[SetEntry]) -> f64 {
    entries.iter().map(|e| e.time).reduce(f64::max).map(|m| m + 180.0).unwrap_or(0.0)
}

fn deadline_hint(iso: &str) -> String {
    if iso.is_empty() {
        return String::new();
    }
    let alert = otoko_library::career::board::deadline_alert(iso).map(|a| a.0).unwrap_or_default();
    format!("{} — {alert}", releases::long_date(iso))
}

fn rights_text(p: &Project) -> String {
    nature::rights_notes(&p.nature, &p.meta).into_iter().map(|(lvl, msg)| if lvl == "attention" { format!("Attention — {msg}") } else { msg }).collect::<Vec<_>>().join("\n")
}

fn src_ui(s: &Json) -> SrcUi {
    let key = text(&s["key"]);
    SrcUi {
        artist: text(&s["artist"]).into(),
        title: text(&s["title"]).into(),
        bpm: s["bpm"].as_f64().map(|b| format!("{}", b.round() as i64)).unwrap_or_default().into(),
        camelot: camelot::camelot(&key).into(),
        key: key.into(),
        part: text(&s["part"]).into(),
        linked: s["track_id"].as_i64().is_some(),
    }
}

/// Montre la fiche ; `full` remet aussi les textes modifiables.
fn push(app: &AppWindow, st: &State, full: bool) {
    let Some(p) = st.proj.project.as_ref() else { return };
    let g = app.global::<Proj>();
    let m = &p.meta;
    g.set_id(p.id as i32);
    g.set_title(p.title.as_str().into());
    g.set_nature(p.nature.as_str().into());
    g.set_rights(rights_text(p).into());
    if full {
        g.set_orig_artist(text(&m["orig_artist"]).into());
        g.set_orig_title(text(&m["orig_title"]).into());
        g.set_commissioned_by(text(&m["commissioned_by"]).into());
        g.set_stems_dir(text(&m["stems_dir"]).into());
        g.set_deadline_text(releases::fr_date(m["deadline"].as_str().unwrap_or("")).into());
        g.set_fee_text(num_text(&m["fee"]).into());
        g.set_split_text(num_text(&m["split"]).into());
    }
    g.set_official(m["official"].as_bool().unwrap_or(false));
    g.set_auth_index(AUTHORIZATION.iter().position(|a| m["authorization"] == a.0).unwrap_or(0) as i32);
    let dl = m["deadline"].as_str().unwrap_or("");
    g.set_deadline_on(!dl.is_empty());
    g.set_deadline_hint(deadline_hint(dl).into());
    let sources: Vec<Json> = m["sources"].as_array().cloned().unwrap_or_default();
    g.set_sources(model(sources.iter().map(src_ui).collect()));
    g.set_mashup_check(projects::mashup_check(&sources).into());
    g.set_entries(model(
        p.entries
            .iter()
            .map(|e| EntryUi {
                time: nature::fmt_time(e.time, e.time >= 3600.0).into(),
                artist: e.artist.as_str().into(),
                title: e.title.as_str().into(),
                bpm: e.bpm.map(|b| format!("{}", b.round() as i64)).unwrap_or_default().into(),
                camelot: camelot::camelot(&e.key).into(),
                linked: e.track.is_some(),
            })
            .collect(),
    ));
    let mut names: Vec<SharedString> = vec!["— Aucun —".into()];
    names.extend(st.proj.clients.iter().map(|c| SharedString::from(c.name.as_str())));
    g.set_client_names(model(names));
    g.set_client_index(m["client_id"].as_i64().and_then(|id| st.proj.clients.iter().position(|c| c.id == id)).map(|i| i as i32 + 1).unwrap_or(0));
    g.set_order_status(m["order_status"].as_str().filter(|s| !s.is_empty()).unwrap_or("brief").into());
    g.set_paid(m["paid"].as_bool().unwrap_or(false));
    g.set_agreement_index(AGREEMENTS.iter().position(|a| m["agreement"].as_str().unwrap_or("") == a.0).unwrap_or(0) as i32);
    g.set_private(m["private"].as_bool().unwrap_or(false));
    g.set_deliverables(model(DELIVERABLES.iter().map(|(k, l)| DelivUi { key: (*k).into(), label: (*l).into(), done: m["deliverables"][*k].as_bool().unwrap_or(false) }).collect()));
    g.set_versions(model(
        m["versions"].as_array().into_iter().flatten().map(|v| VersionRowUi { date: text(&v["date"]).into(), label: text(&v["label"]).into(), note: text(&v["note"]).into() }).collect(),
    ));
    let sent = if p.nature == "prod" { crate::partage::sent_versions(st, p.id) } else { Vec::new() };
    g.set_sent(model(sent.into_iter().map(|s| VersionRowUi { date: s.date.into(), label: s.label.into(), note: s.note.into() }).collect()));
}

fn field(app: &AppWindow, st: &mut State, key: &str, value: &str) {
    match key {
        "orig_artist" | "orig_title" | "commissioned_by" | "stems_dir" => meta_mut(st, |m| m[key] = json!(value.trim())),
        "deadline" => {
            let iso = releases::iso_date(value);
            if iso.is_empty() {
                app.global::<Proj>().set_deadline_hint("date à écrire JJ/MM/AAAA".into());
                return;
            }
            meta_mut(st, |m| m["deadline"] = json!(iso));
        }
        "fee" => {
            let v: Option<f64> = value.trim().replace(',', ".").replace('€', "").trim().parse().ok();
            meta_mut(st, |m| m["fee"] = v.map(|f| json!(f)).unwrap_or(json!(0.0)));
        }
        "split" => {
            let v: i64 = value.trim().trim_end_matches('%').trim().parse::<f64>().map(|f| f.round().clamp(0.0, 100.0) as i64).unwrap_or(0);
            meta_mut(st, |m| m["split"] = json!(v));
        }
        _ => return,
    }
    // Les champs qui changent les repères sur les droits ou l'échéance.
    if let Some(p) = st.proj.project.as_ref() {
        let g = app.global::<Proj>();
        g.set_rights(rights_text(p).into());
        g.set_deadline_hint(deadline_hint(p.meta["deadline"].as_str().unwrap_or("")).into());
    }
}

fn toggle(app: &AppWindow, st: &mut State, key: &str) {
    match key {
        "official" | "paid" | "private" => meta_mut(st, |m| {
            let on = m[key].as_bool().unwrap_or(false);
            m[key] = json!(!on);
        }),
        "deadline" => {
            let today = releases::today().format("%Y-%m-%d").to_string();
            meta_mut(st, |m| {
                let on = !m["deadline"].as_str().unwrap_or("").is_empty();
                m["deadline"] = json!(if on { String::new() } else { today });
            });
            let dl = st.proj.project.as_ref().and_then(|p| p.meta["deadline"].as_str().map(str::to_string)).unwrap_or_default();
            app.global::<Proj>().set_deadline_text(releases::fr_date(&dl).into());
        }
        _ => return,
    }
    push(app, st, false);
}

fn choose(app: &AppWindow, st: &mut State, key: &str, i: usize) {
    match key {
        "authorization" => meta_mut(st, |m| m["authorization"] = json!(AUTHORIZATION.get(i).map(|a| a.0).unwrap_or("none"))),
        "agreement" => meta_mut(st, |m| m["agreement"] = json!(AGREEMENTS.get(i).map(|a| a.0).unwrap_or(""))),
        "order_status" => meta_mut(st, |m| m["order_status"] = json!(ORDER_STATUSES.get(i).map(|o| o.0).unwrap_or("brief"))),
        "client" => {
            let c = if i == 0 { None } else { st.proj.clients.get(i - 1).cloned() };
            meta_mut(st, |m| {
                m["client_id"] = c.as_ref().map(|c| json!(c.id)).unwrap_or(Json::Null);
                m["client_name"] = json!(c.map(|c| c.name).unwrap_or_default());
            });
        }
        _ => return,
    }
    push(app, st, false);
}

fn source_edited(app: &AppWindow, st: &mut State, r: usize, c: i32, t: &str) {
    let key = match c {
        0 => "artist",
        1 => "title",
        2 => "bpm",
        3 => "key",
        5 => "part",
        _ => return,
    };
    meta_mut(st, |m| {
        if let Some(s) = sources_mut(m).get_mut(r) {
            if key == "bpm" {
                s["bpm"] = t.trim().replace(',', ".").parse::<f64>().map(|b| json!(b)).unwrap_or(Json::Null);
            } else {
                s[key] = json!(t.trim());
            }
        }
    });
    // La clé Camelot de la ligne et le contrôle suivent, sans recréer le tableau (on tape dedans).
    let Some(p) = st.proj.project.as_ref() else { return };
    let sources: Vec<Json> = p.meta["sources"].as_array().cloned().unwrap_or_default();
    let g = app.global::<Proj>();
    if let (Some(s), Some(m)) = (sources.get(r), g.get_sources().as_any().downcast_ref::<VecModel<SrcUi>>()) {
        if let Some(mut row) = m.row_data(r) {
            row.camelot = camelot::camelot(&text(&s["key"])).into();
            m.set_row_data(r, row);
        }
    }
    g.set_mashup_check(projects::mashup_check(&sources).into());
}

fn entry_edited(st: &mut State, r: usize, c: i32, t: &str) {
    let Some(e) = st.proj.project.as_mut().and_then(|p| p.entries.get_mut(r)) else { return };
    match c {
        0 => e.time = projects::parse_time(t),
        1 => e.artist = t.trim().to_string(),
        2 => e.title = t.trim().to_string(),
        _ => {}
    }
}

fn import_markers(app: &AppWindow, st: &mut State) {
    let Some(p) = st.proj.project.as_ref() else { return };
    let Some(c) = st.career.as_ref() else { return };
    let any: i64 = c.conn().query_row("SELECT COUNT(*) FROM markers WHERE track_id = ?1", [p.id], |r| r.get(0)).unwrap_or(0);
    if any == 0 {
        app.global::<Proj>().set_msg("Aucun repère sur ce set : lance la lecture, puis clic droit sur la forme d'onde à chaque nouveau titre.".into());
        return;
    }
    let new = projects::markers_as_entries(c.conn(), p.id, &p.entries);
    let n = new.len();
    if let Some(p) = st.proj.project.as_mut() {
        p.entries.extend(new);
        p.entries = sorted(&p.entries);
    }
    push(app, st, false);
    app.global::<Proj>().set_msg(format!("{n} repère(s) importé(s). Complète les artistes et titres.").into());
}

fn picker_push(app: &AppWindow, st: &State, q: &str) {
    let q = q.trim().to_lowercase();
    let rows: Vec<PickUi> = st
        .proj
        .pick
        .iter()
        .filter(|t| q.is_empty() || t.1.to_lowercase().contains(&q) || t.2.to_lowercase().contains(&q))
        .take(300)
        .map(|t| {
            let mut sub = vec![t.2.clone()];
            if let Some(b) = t.3 {
                sub.push(format!("{} BPM", b.round() as i64));
            }
            if !t.4.is_empty() {
                sub.push(t.4.clone());
            }
            PickUi { id: t.0 as i32, title: t.1.as_str().into(), sub: sub.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ").into() }
        })
        .collect();
    app.global::<Proj>().set_picker(model(rows));
}

fn picker_choose(app: &AppWindow, st: &mut State, id: i64) {
    let Some(t) = st.proj.pick.iter().find(|t| t.0 == id).cloned() else { return };
    if st.proj.picker_for == "source" {
        meta_mut(st, |m| sources_mut(m).push(json!({"artist": t.2, "title": t.1, "bpm": t.3, "key": t.4, "part": "", "track_id": t.0})));
    } else if let Some(p) = st.proj.project.as_mut() {
        let time = next_time(&p.entries);
        p.entries.push(SetEntry { time, artist: t.2, title: t.1, track: Some(t.0), bpm: t.3, key: t.4 });
    }
    app.global::<Proj>().set_picker_open(false);
    push(app, st, false);
}

fn export_cue(app: &AppWindow, st: &mut State) {
    let Some(p) = st.proj.project.as_ref() else { return };
    let default = std::path::Path::new(&p.path).with_extension("cue");
    let mut dlg = rfd::FileDialog::new().set_title("Exporter en .cue").add_filter("Fichier CUE", &["cue"]);
    if let (Some(dir), Some(name)) = (default.parent(), default.file_name()) {
        dlg = dlg.set_directory(dir).set_file_name(name.to_string_lossy());
    }
    let Some(f) = dlg.save_file() else { return };
    let list: Vec<nature::Entry> = sorted(&p.entries).iter().map(|e| e.entry()).collect();
    let audio = std::path::Path::new(&p.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let cue = nature::cue_sheet(&list, &audio, &artist(st), &p.title);
    let msg = match std::fs::write(&f, cue) {
        Ok(()) => format!("Fichier .cue enregistré : {}", f.display()),
        Err(e) => format!("Écriture impossible : {e}"),
    };
    app.global::<Proj>().set_msg(msg.into());
}

fn remix_title(app: &AppWindow, st: &mut State) {
    let Some(p) = st.proj.project.as_ref() else { return };
    let base = p.meta["orig_title"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| p.raw_title.trim().to_string());
    if base.is_empty() {
        return;
    }
    let new = projects::remix_title(&base, &artist(st), &p.nature);
    let id = p.id;
    if let Some(c) = st.career.as_ref() {
        let _ = otoko_library::career::journal::record(c.conn(), "Titre du remix", "", || projects::set_title(c.conn(), id, &new));
    }
    if let Some(p) = st.proj.project.as_mut() {
        p.raw_title = new.clone();
        p.title = new.clone();
    }
    let g = app.global::<Proj>();
    g.set_title(new.as_str().into());
    g.set_msg(format!("Titre : « {new} » (dans la bibliothèque ; « Écrire les tags » le met dans le fichier).").into());
}

fn save(app: &AppWindow, st: &mut State) {
    let Some(p) = st.proj.project.take() else { return };
    let r = st.career.as_ref().map(|c| otoko_library::career::journal::record(c.conn(), "Fiche projet", "", || projects::save(c.conn(), &p)));
    app.global::<Proj>().set_open(false);
    match r {
        Some(Ok(())) => app.set_message(format!("Fiche projet de « {} » enregistrée.", p.title).into()),
        Some(Err(e)) => app.set_message(format!("Enregistrement impossible : {e}").into()),
        None => {}
    }
    crate::career::fill(app, st);
}

// ---------------------------------------------------------------------- clients

pub fn clients_show(app: &AppWindow, st: &mut State, select: Option<i64>) {
    st.proj.clients = st.career.as_ref().map(|c| projects::clients(c.conn())).unwrap_or_default();
    let g = app.global::<Proj>();
    g.set_clients(model(st.proj.clients.iter().map(|c| ClientUi { id: c.id as i32, name: c.name.as_str().into() }).collect()));
    let i = select.and_then(|id| st.proj.clients.iter().position(|c| c.id == id)).or(if st.proj.clients.is_empty() { None } else { Some(0) });
    st.proj.client_sel = None;
    g.set_client_sel(-1);
    if let Some(i) = i {
        client_select(app, st, i);
    }
    g.set_clients_open(true);
}

fn client_select(app: &AppWindow, st: &mut State, i: usize) {
    let Some(c) = st.proj.clients.get(i).cloned() else { return };
    st.proj.client_sel = Some(i);
    let n = st.career.as_ref().map(|car| projects::client_prods(car.conn(), c.id)).unwrap_or(0);
    let g = app.global::<Proj>();
    g.set_client_sel(i as i32);
    g.set_c_name(c.name.into());
    g.set_c_contact(c.contact.into());
    g.set_c_notes(c.notes.into());
    g.set_c_stats(format!("{n} prod(s) pour ce client dans la bibliothèque.").into());
}

// ---------------------------------------------------------------------- constructeur de set

/// Ouvre le constructeur sur la sélection (deux morceaux ou plus), sinon sur la vue affichée.
pub fn set_builder(app: &AppWindow, st: &mut State) {
    let selected: Vec<i64> = st.lib.rows.iter().filter(|r| st.lib.selected.contains(&r.id)).map(|r| r.id).collect();
    let (ids, source) = if selected.len() >= 2 {
        (selected.clone(), format!("{} sélectionné(s)", selected.len()))
    } else {
        let label = st.lib.nav.get(st.lib.active).map(|n| n.label.clone()).unwrap_or_default();
        (st.lib.rows.iter().map(|r| r.id).collect(), format!("vue « {label} »"))
    };
    let Some(c) = st.career.as_ref() else { return };
    let pool = sb::tracks(c.conn(), &ids);
    if pool.len() < 2 {
        app.set_message("Construire un set : sélectionne au moins deux morceaux, ou affiche un dossier ou une vue.".into());
        return;
    }
    if pool.len() > 300 && !confirm(st, "Construire un set", &format!("{} morceaux : c'est beaucoup pour un set. Continuer quand même ?", pool.len())) {
        return;
    }
    let g = app.global::<Proj>();
    g.set_sb_intro(
        format!(
            "{} morceau(x) — {source}. L'ordre proposé enchaîne des tempos proches (à 6 % près, demi et double tempo compris) et des tonalités compatibles sur la roue de Camelot. Retouche avec les flèches.",
            pool.len()
        )
        .into(),
    );
    let missing = pool.iter().filter(|t| t.bpm.is_none() || t.key.is_empty()).count();
    g.set_sb_warn(if missing > 0 { format!("{missing} morceau(x) sans tempo ou tonalité : lance l'analyse pour un meilleur ordre.") } else { String::new() }.into());
    let mut starts: Vec<(String, Option<i64>)> = pool.iter().map(|t| (t.title.clone(), Some(t.id))).collect();
    starts.sort_by_key(|s| s.0.to_lowercase());
    starts.insert(0, ("Au choix de l'appli".into(), None));
    g.set_sb_starts(strings(starts.iter().map(|s| s.0.clone())));
    st.proj.sb_starts = starts.into_iter().map(|s| s.1).collect();
    g.set_sb_start(0);
    g.set_sb_mode("montee".into());
    g.set_sb_target("0".into());
    g.set_sb_overlap("30".into());
    g.set_sb_sel(-1);
    st.proj.sb_pool = pool;
    sb_build(app, st);
    g.set_sb_open(true);
}

fn overlap(app: &AppWindow) -> f64 {
    app.global::<Proj>().get_sb_overlap().trim().parse::<f64>().unwrap_or(30.0).clamp(0.0, 120.0)
}

fn sb_build(app: &AppWindow, st: &mut State) {
    let g = app.global::<Proj>();
    let start = st.proj.sb_starts.get(g.get_sb_start().max(0) as usize).copied().flatten();
    let target = g.get_sb_target().trim().parse::<f64>().unwrap_or(0.0).clamp(0.0, 600.0);
    st.proj.sb_order = sb::build(&st.proj.sb_pool, g.get_sb_mode().as_str(), start, target);
    sb_push(app, st);
}

fn sb_entries(order: &[SetTrack], overlap: f64) -> Vec<SetEntry> {
    sb::timeline(order, overlap).into_iter().zip(order).map(|(s, t)| SetEntry { time: s, artist: t.artist.clone(), title: t.title.clone(), track: Some(t.id), ..Default::default() }).collect()
}

fn sb_push(app: &AppWindow, st: &State) {
    let ov = overlap(app);
    let order = &st.proj.sb_order;
    let starts = sb::timeline(order, ov);
    let trs = sb::transitions(order);
    let total = sb::total_duration(order, ov);
    let long = total >= 3600.0;
    let rows: Vec<SbRowUi> = order
        .iter()
        .enumerate()
        .map(|(r, t)| SbRowUi {
            id: t.id as i32,
            start: nature::fmt_time(starts[r], long).into(),
            title: t.title.as_str().into(),
            artist: t.artist.as_str().into(),
            bpm: t.bpm.map(|b| format!("{b:.0}")).unwrap_or_default().into(),
            camelot: camelot::camelot(&t.key).into(),
            energy: t.energy.map(|e| format!("{:.0} %", e * 100.0)).unwrap_or_default().into(),
            duration: t.duration.map(|d| nature::fmt_time(d, false)).unwrap_or_default().into(),
            transition: trs.get(r).map(|x| sb::describe(x.0, x.1)).unwrap_or_else(|| "fin du set".into()).into(),
            ok: trs.get(r).is_some_and(|x| x.2),
            last: r + 1 == order.len(),
        })
        .collect();
    let g = app.global::<Proj>();
    g.set_sb_rows(model(rows));
    let good = trs.iter().filter(|x| x.2).count();
    g.set_sb_summary(
        if trs.is_empty() { String::new() } else { format!("{} morceau(x) · environ {} · {good}/{} enchaînement(s) sans souci", order.len(), nature::fmt_time(total, long), trs.len()) }.into(),
    );
}

fn sb_save_to(app: &AppWindow, st: &mut State, i: i32) {
    let g = app.global::<Proj>();
    g.set_sb_saving(false);
    let Some((sid, label)) = (i >= 0).then(|| st.proj.sb_sets.get(i as usize).cloned()).flatten() else { return };
    let Some(c) = st.career.as_ref() else { return };
    let n: i64 = c.conn().query_row("SELECT COUNT(*) FROM set_entries WHERE set_id = ?1", [sid], |r| r.get(0)).unwrap_or(0);
    if n > 0 && !confirm(st, "Enregistrer dans un set", &format!("« {label} » a déjà une liste de {n} titre(s). La remplacer ?")) {
        return;
    }
    let Some(mut p) = st.career.as_ref().and_then(|c| projects::load(c.conn(), sid)) else { return };
    p.nature = "set".into();
    p.entries = sb_entries(&st.proj.sb_order, overlap(app));
    let r = st.career.as_ref().map(|c| otoko_library::career::journal::record(c.conn(), "Liste du set", "", || projects::save(c.conn(), &p)));
    g.set_sb_summary(match r {
        Some(Ok(())) => format!("Liste enregistrée dans « {label} ». Ajuste les minutages dans sa fiche projet.").into(),
        Some(Err(e)) => format!("Enregistrement impossible : {e}").into(),
        None => "".into(),
    });
}
