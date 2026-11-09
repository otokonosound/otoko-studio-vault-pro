//! Mobile et partage › les liens d'écoute. Studio Vault ne les gérait que depuis le téléphone : ici ils se créent et se
//! suivent aussi depuis le PC. Le serveur mobile, s'il tourne, fait le travail (il prépare les versions d'écoute et oublie
//! ce qu'il a en mémoire) ; sinon la base `partages.db` est écrite directement.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;

use otoko_library::career::library as lib;
use otoko_mobile::shares::{DOWNLOAD_QUALITIES, KINDS, Share, ShareStore};
use serde_json::{Value as Json, json};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::state::{Done, State};
use crate::{AppWindow, Part, ShareItemUi, ShareUi};

/// Les validités proposées, en jours (0 : sans limite).
const EXPIRIES: [i64; 4] = [7, 30, 90, 0];
/// Le formulaire montre au plus autant de morceaux : la recherche précise le reste.
const MAX_ITEMS: usize = 60;

#[derive(Default)]
pub struct View {
    /// Le début des adresses publiques, donné par le serveur quand il tourne.
    pub base: Option<String>,
    pub funnel: bool,
    pub list: Vec<Share>,
    /// Dans le formulaire : ce qui est choisi (le type et le reste sont dans le global `Part`).
    pub picked: BTreeSet<i64>,
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// La base des liens. Pendant l'autotest, celle de l'utilisateur n'est ni créée ni touchée.
fn store(st: &State) -> Option<ShareStore> {
    if st.selftest && st.career_path == crate::career::default_path() && std::env::var_os("OTOKO_CAPTURES").is_none() {
        return None;
    }
    ShareStore::open(&crate::career::shares_path(&st.career_path)).ok()
}

fn port(st: &State) -> u16 {
    st.mobile.as_ref().map(|s| s.port()).unwrap_or(otoko_mobile::PORT)
}

/// Relit les liens, et demande au serveur où ils sont publiés.
pub fn fill(app: &AppWindow, st: &mut State) {
    let mut list = store(st).map(|s| s.all()).unwrap_or_default();
    list.sort_by(|a, b| a.revoked.cmp(&b.revoked).then(b.created_at.cmp(&a.created_at)).then(b.id.cmp(&a.id)));
    st.partage.list = list;
    push(app, st);
    if st.selftest {
        return;
    }
    let port = port(st);
    st.spawn(move || {
        let s = otoko_mobile::ask(port, "/api/shares/status");
        Done::PartageBase(s.map(|s| (s["public_base"].as_str().unwrap_or("").to_string(), s["funnel"] == true)))
    });
}

pub fn base_landed(app: &AppWindow, st: &mut State, base: Option<(String, bool)>) {
    (st.partage.base, st.partage.funnel) = match base {
        Some((b, f)) if !b.is_empty() => (Some(b), f),
        _ => (None, false),
    };
    push(app, st);
}

fn plural(n: i64, word: &str) -> String {
    format!("{n} {word}{}", if n > 1 { "s" } else { "" })
}

/// Le titre d'un lien : le sien, sinon ce qu'il contient.
fn title(st: &State, s: &Share) -> String {
    if !s.label.is_empty() {
        return s.label.clone();
    }
    let first = s.items.first().copied();
    let name = |id: i64| {
        let c = st.career.as_ref()?;
        if s.kind == "releases" { c.releases().ok()?.into_iter().find(|r| r.id == id).map(|r| r.title) } else { c.song(id).ok().flatten().map(|t| t.title) }
    };
    match (s.kind.as_str(), first.and_then(name)) {
        ("discography", _) => "Ma discographie".into(),
        (_, Some(t)) if s.items.len() > 1 => format!("{t} et {} autre{}", s.items.len() - 1, if s.items.len() > 2 { "s" } else { "" }),
        (_, Some(t)) => t,
        _ => "Lien sans titre".into(),
    }
}

fn row(st: &State, s: &Share) -> ShareUi {
    let state = ShareStore::state(s);
    let what = match s.kind.as_str() {
        "discography" => "Discographie".to_string(),
        "releases" => plural(s.items.len() as i64, "sortie"),
        _ => plural(s.items.len() as i64, "morceau").replace("morceaus", "morceaux"),
    };
    let when = match (&s.expires_at, state) {
        (_, "revoked") => "révoqué".to_string(),
        (Some(e), "expired") => format!("expiré le {}", crate::career::fr_date(e)),
        (Some(e), _) => format!("jusqu'au {}", crate::career::fr_date(e)),
        (None, _) => "sans limite".to_string(),
    };
    let mut activity = vec![plural(s.views, "visite"), plural(s.plays, "écoute")];
    if s.can_download {
        activity.push(plural(s.downloads, "téléchargement"));
    }
    if let Some(l) = &s.last_access {
        activity.push(format!("dernière visite le {}", crate::career::fr_date(l)));
    }
    ShareUi {
        id: s.id as i32,
        title: title(st, s).into(),
        what: what.into(),
        state: match state {
            "revoked" => "Révoqué",
            "expired" => "Expiré",
            _ => "Actif",
        }
        .into(),
        tone: match state {
            "revoked" => 2,
            "expired" => 1,
            _ => 0,
        },
        activity: activity.join(" · ").into(),
        when: when.into(),
        url: st.partage.base.as_ref().map(|b| format!("{b}/s/{}", s.token)).unwrap_or_default().into(),
        locked: s.pw_hash.is_some(),
        revoked: s.revoked,
    }
}

fn push(app: &AppWindow, st: &State) {
    let g = app.global::<Part>();
    g.set_shares(model(st.partage.list.iter().map(|s| row(st, s)).collect()));
    g.set_base_text(
        match (&st.partage.base, st.partage.funnel) {
            (Some(b), true) => format!("Publiés sur Internet : {b}/s/…"),
            (Some(b), false) => format!("Pas encore publiés sur Internet : « Partage public… » les met sur {b}."),
            (None, _) => "Le serveur mobile ne répond pas : démarre-le pour obtenir l'adresse des liens.".to_string(),
        }
        .into(),
    );
}

/// Ouvre le formulaire, éventuellement avec des morceaux ou des sorties déjà choisis.
pub fn new_link(app: &AppWindow, st: &mut State, kind: i32, picked: &[i64]) {
    let g = app.global::<Part>();
    st.partage.picked = picked.iter().copied().collect();
    g.set_kind(kind);
    g.set_query("".into());
    g.set_label("".into());
    g.set_note("".into());
    g.set_password("".into());
    g.set_expiry(0);
    g.set_download(false);
    g.set_quality(0);
    g.set_error("".into());
    g.set_busy(false);
    g.set_creating(true);
    push_items(app, st);
}

/// Depuis la bibliothèque ou une sortie : la page, puis le formulaire déjà rempli.
pub fn share(app: &AppWindow, st: &mut State, kind: i32, ids: &[i64]) {
    crate::raccourcis::aller(app, st, 7, 0);
    new_link(app, st, kind, ids);
}

fn push_items(app: &AppWindow, st: &State) {
    let g = app.global::<Part>();
    let q = g.get_query();
    let q = q.trim();
    let on = |id: i64| st.partage.picked.contains(&id);
    let Some(c) = st.career.as_ref() else { return };
    let mut items: Vec<ShareItemUi> = match g.get_kind() {
        0 => lib::quick_find(c.conn(), q, MAX_ITEMS).into_iter().map(|(id, t, a)| ShareItemUi { id: id as i32, title: t.into(), sub: a.into(), on: on(id) }).collect(),
        1 => {
            let fold = crate::palette::fold(q);
            c.releases()
                .unwrap_or_default()
                .into_iter()
                .filter(|r| fold.is_empty() || crate::palette::fold(&r.title).contains(&fold))
                .take(MAX_ITEMS)
                .map(|r| ShareItemUi { id: r.id as i32, sub: r.date.as_str().into(), title: r.title.into(), on: on(r.id) })
                .collect()
        }
        _ => Vec::new(),
    };
    // Ce qui est choisi reste en haut de la liste.
    items.sort_by_key(|i| !i.on);
    g.set_items(model(items));
    g.set_picked(st.partage.picked.len() as i32);
}

/// Parle au serveur s'il tourne, à la base sinon.
fn apply(port: u16, path: PathBuf, method: &str, url: String, body: Json, direct: impl FnOnce(&ShareStore, &Json) -> Result<(), String>) -> Result<(), String> {
    match otoko_mobile::send(port, method, &url, &body) {
        Some(r) => r.map(|_| ()),
        None => direct(&ShareStore::open(&path).map_err(|e| e.to_string())?, &body),
    }
}

fn create(app: &AppWindow, st: &mut State) {
    let g = app.global::<Part>();
    let kind = KINDS[[1, 0, 2][g.get_kind().clamp(0, 2) as usize]];
    let body = json!({
        "kind": kind,
        "items": st.partage.picked.iter().collect::<Vec<_>>(),
        "label": g.get_label().as_str(),
        "message": g.get_note().as_str(),
        "expires_days": EXPIRIES[g.get_expiry().clamp(0, 3) as usize],
        "can_download": g.get_download(),
        "download_quality": DOWNLOAD_QUALITIES[g.get_quality().clamp(0, 2) as usize],
        "password": g.get_password().as_str(),
    });
    if st.selftest && st.career_path == crate::career::default_path() {
        return;
    }
    g.set_busy(true);
    g.set_error("".into());
    let (port, path) = (port(st), crate::career::shares_path(&st.career_path));
    // Pendant un autotest, le serveur de l'utilisateur n'est pas sollicité.
    let port = if st.selftest { 0 } else { port };
    st.spawn(move || {
        Done::Partage(apply(port, path, "POST", "/api/shares".into(), body, |s, b| s.create(b).map(|_| ()).map_err(|e| e.0)).map(|_| "Lien créé.".to_string()))
    });
}

/// Le retour du serveur ou de la base.
pub fn done(app: &AppWindow, st: &mut State, result: Result<String, String>) {
    let g = app.global::<Part>();
    g.set_busy(false);
    match result {
        Ok(text) => {
            g.set_creating(false);
            app.set_message(text.into());
            fill(app, st);
        }
        Err(e) if g.get_creating() => g.set_error(e.into()),
        Err(e) => app.set_message(format!("Lien non modifié : {e}").into()),
    }
}

fn set_revoked(st: &mut State, id: i64) {
    let Some(s) = st.partage.list.iter().find(|s| s.id == id) else { return };
    let revoked = !s.revoked;
    let (port, path) = (if st.selftest { 0 } else { port(st) }, crate::career::shares_path(&st.career_path));
    st.spawn(move || {
        let body = json!({ "revoked": revoked });
        let r = apply(port, path, "POST", format!("/api/shares/{id}"), body, |s, b| s.update(id, b).map(|_| ()).map_err(|e| e.0));
        Done::Partage(r.map(|_| if revoked { "Lien révoqué : il ne marche plus.".to_string() } else { "Lien réactivé.".to_string() }))
    });
}

fn remove(st: &mut State, id: i64) {
    let (port, path) = (if st.selftest { 0 } else { port(st) }, crate::career::shares_path(&st.career_path));
    st.spawn(move || {
        let r = apply(port, path, "DELETE", format!("/api/shares/{id}"), json!({}), |s, _| {
            s.delete(id);
            Ok(())
        });
        Done::Partage(r.map(|_| "Lien supprimé.".to_string()))
    });
}

/// L'adresse d'un lien, ou le message qui dit pourquoi il n'y en a pas.
fn url_of(app: &AppWindow, st: &State, id: i64) -> Option<String> {
    let s = st.partage.list.iter().find(|s| s.id == id)?;
    let Some(b) = st.partage.base.as_ref() else {
        app.set_message("Démarre le serveur mobile : c'est lui qui donne l'adresse des liens.".into());
        return None;
    };
    Some(format!("{b}/s/{}", s.token))
}

macro_rules! part_on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.global::<Part>().$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    part_on!(app, state, on_new_link, |app, st| { new_link(&app, st, 0, &[]); });
    part_on!(app, state, on_set_kind, |app, st, kind| {
        let g = app.global::<Part>();
        if g.get_kind() != kind {
            g.set_kind(kind);
            g.set_query("".into());
            st.partage.picked.clear();
            push_items(&app, st);
        }
    });
    part_on!(app, state, on_search, |app, st, _text| { push_items(&app, st); });
    part_on!(app, state, on_toggle, |app, st, id| {
        let id = id as i64;
        if !st.partage.picked.remove(&id) {
            st.partage.picked.insert(id);
        }
        push_items(&app, st);
    });
    part_on!(app, state, on_create, |app, st| { create(&app, st); });
    part_on!(app, state, on_copy, |app, st, id| {
        if let Some(url) = url_of(&app, st, id as i64) {
            if !st.selftest {
                crate::publish::copy(&url);
            }
            app.set_message("Lien copié.".into());
        }
    });
    part_on!(app, state, on_open, |app, st, id| {
        if let Some(url) = url_of(&app, st, id as i64) {
            if !st.selftest {
                let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
            }
        }
    });
    part_on!(app, state, on_revoke, |_app, st, id| { set_revoked(st, id as i64); });
    part_on!(app, state, on_remove, |_app, st, id| { remove(st, id as i64); });
}
