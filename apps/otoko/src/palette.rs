//! La palette de commandes (Ctrl+K, ou « Tout trouver » en haut de la fenêtre), comme celle de Studio Vault : aller à une
//! page, lancer une action (avec son raccourci), écouter un morceau.

use std::cell::RefCell;
use std::rc::Rc;

use otoko_library::career::{library as lib, prefs};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::state::State;
use crate::{AppWindow, Pal, PalItemUi};

/// Une entrée : sorte (page, action, geste, morceau), ce qu'elle lance, libellé, raccourci.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub kind: &'static str,
    pub key: String,
    pub label: String,
    pub hint: String,
}

#[derive(Default)]
pub struct View {
    pub shown: Vec<Entry>,
}

/// Les pages : (espace, onglet, libellé).
const PAGES: [(i32, i32, &str); 10] = [
    (4, 0, "Accueil"),
    (4, 1, "Bibliothèque"),
    (4, 6, "Production"),
    (4, 4, "Atelier Suno"),
    (4, 7, "Mastering"),
    (4, 2, "Sorties"),
    (4, 3, "Publier"),
    (4, 5, "Statistiques"),
    (7, 0, "Mobile et partage"),
    (6, 0, "Réglages"),
];
/// Les gestes de la fenêtre : (nom, libellé, raccourci).
const GESTES: [(&str, &str, &str); 2] = [("open-guide", "Ouvrir le guide", "F1"), ("theme", "Thème clair / sombre", "")];

/// Minuscules sans accents, pour chercher « controle » et trouver « Contrôle ».
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ÿ' => 'y',
            'œ' => 'o',
            c => c,
        })
        .collect()
}

fn entries(st: &State) -> Vec<Entry> {
    let e = |kind: &'static str, key: String, label: String, hint: &str| Entry { kind, key, label, hint: hint.to_string() };
    let mut out = Vec::new();
    for (space, tab, l) in PAGES {
        out.push(e("page", format!("{space}:{tab}"), format!("Aller à : {l}"), ""));
    }
    let keys = st.career.as_ref().map(|c| prefs::shortcuts(c.conn())).unwrap_or_default();
    for a in prefs::ACTIONS.iter().filter(|a| a.id != "palette") {
        let hint = keys.iter().find(|k| k.0 == a.id).map(|k| k.1.clone()).unwrap_or_default();
        out.push(e("action", a.id.to_string(), a.label.trim_end_matches('…').to_string(), &hint));
    }
    for (geste, l, h) in GESTES {
        out.push(e("geste", geste.to_string(), l.to_string(), h));
    }
    out
}

/// Ouvre la palette.
pub fn open(app: &AppWindow, st: &mut State) {
    let g = app.global::<Pal>();
    g.set_query("".into());
    g.set_open(true);
    search(app, st, "");
}

pub fn search(app: &AppWindow, st: &mut State, text: &str) {
    let q = fold(text.trim());
    let mut shown: Vec<Entry> = entries(st).into_iter().filter(|e| q.is_empty() || fold(&e.label).contains(&q)).collect();
    if q.chars().count() >= 2 {
        if let Some(c) = st.career.as_ref() {
            for (id, title, album) in lib::quick_find(c.conn(), text.trim(), 12) {
                let label = if album.is_empty() { format!("Écouter : {title}") } else { format!("Écouter : {title}  —  {album}") };
                shown.push(Entry { kind: "track", key: id.to_string(), label, hint: String::new() });
            }
        }
    }
    let g = app.global::<Pal>();
    g.set_items(ModelRc::new(VecModel::from(shown.iter().map(|e| PalItemUi { label: e.label.as_str().into(), hint: e.hint.as_str().into(), kind: e.kind.into() }).collect::<Vec<_>>())));
    g.set_sel(0);
    st.palette.shown = shown;
}

/// Lance l'entrée `i`. Rend le geste à déclencher une fois l'état relâché.
fn run(app: &AppWindow, st: &mut State, i: usize) -> Option<String> {
    let e = st.palette.shown.get(i)?.clone();
    app.global::<Pal>().set_open(false);
    match e.kind {
        "page" => {
            let (space, tab) = e.key.split_once(':')?;
            crate::raccourcis::aller(app, st, space.parse().ok()?, tab.parse().ok()?);
        }
        "action" => {
            crate::raccourcis::run(app, st, &e.key);
        }
        "track" => {
            let id: i64 = e.key.parse().ok()?;
            crate::bibliotheque::reveal(app, st, id);
            crate::lecteur::play_id(app, st, id, 0.0);
        }
        "geste" => return Some(e.key),
        _ => {}
    }
    None
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    {
        let (weak, state) = (app.as_weak(), state.clone());
        app.global::<Pal>().on_search(move |t| {
            let app = weak.unwrap();
            if let Ok(mut st) = state.try_borrow_mut() {
                search(&app, &mut st, t.as_str());
            }
        });
    }
    {
        let (weak, state) = (app.as_weak(), state.clone());
        app.global::<Pal>().on_run(move |i| {
            let app = weak.unwrap();
            let gesture = {
                let Ok(mut st) = state.try_borrow_mut() else { return };
                run(&app, &mut st, i.max(0) as usize)
            };
            // Les gestes passent par les rappels de la fenêtre (l'état est relâché).
            match gesture.as_deref() {
                Some("open-guide") => app.invoke_open_guide(),
                Some("theme") => app.invoke_set_theme(if app.global::<crate::Theme>().get_dark() { 2 } else { 1 }),
                _ => {}
            }
        });
    }
    {
        let weak = app.as_weak();
        app.global::<Pal>().on_close(move || {
            weak.unwrap().global::<Pal>().set_open(false);
        });
    }
    {
        let (weak, state) = (app.as_weak(), state.clone());
        app.on_open_palette(move || {
            let app = weak.unwrap();
            if let Ok(mut st) = state.try_borrow_mut() {
                open(&app, &mut st);
            }
        });
    }
    {
        let (weak, state) = (app.as_weak(), state.clone());
        app.on_career_key(move |text, ctrl, shift, alt| {
            let app = weak.unwrap();
            let Ok(mut st) = state.try_borrow_mut() else { return false };
            crate::raccourcis::key(&app, &mut st, text.as_str(), ctrl, shift, alt, false)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_ignores_accents_and_case() {
        assert_eq!(fold("Contrôle avant publication"), "controle avant publication");
        assert!(fold("Écouter : Nuit").contains(&fold("ecouter")));
    }
}
