//! Les pages du menu : chacun garde celles qui lui servent (Production, Atelier Suno, Mastering, Sorties et Publier,
//! Statistiques, Mobile et partage). L'Accueil et la Bibliothèque restent toujours. Rien n'est effacé : une page retirée
//! du menu garde ses données et revient d'un clic dans Réglages › Général. La première fois, l'Accueil pose la question.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::state::State;
use crate::{AppWindow, CPrefs, Home, PlatformUi};

/// (clé, nom, à quoi elle sert).
pub const PAGES: [(&str, &str, &str); 6] = [
    ("production", "Production", "le tableau de tes morceaux, de l'idée à la sortie"),
    ("suno", "Atelier Suno", "tes prompts et tes générations Suno"),
    ("mastering", "Mastering", "masteriser tes morceaux ici, avec PhaseLimiter"),
    ("sorties", "Sorties et Publier", "planifier tes sorties et préparer leurs textes"),
    ("stats", "Statistiques", "tes chiffres de création et d'écoute"),
    ("mobile", "Mobile et partage", "écouter sur ton téléphone, envoyer des liens privés"),
];

const HIDDEN: &str = "pages_hidden";
const CHOSEN: &str = "pages_chosen";

fn setting(st: &State, key: &str) -> String {
    st.career.as_ref().and_then(|c| c.setting(key)).unwrap_or_default()
}

fn hidden(st: &State) -> Vec<String> {
    setting(st, HIDDEN).split(',').map(str::trim).filter(|k| PAGES.iter().any(|p| p.0 == *k)).map(String::from).collect()
}

pub fn shown(st: &State, key: &str) -> bool {
    !hidden(st).iter().any(|k| k == key)
}

/// Le menu, la question de l'Accueil et la liste des Réglages, tels que choisis.
pub fn push(app: &AppWindow, st: &State) {
    let on = |k: &str| shown(st, k);
    app.set_page_production(on("production"));
    app.set_page_suno(on("suno"));
    app.set_page_mastering(on("mastering"));
    app.set_page_sorties(on("sorties"));
    app.set_page_stats(on("stats"));
    app.set_page_mobile(on("mobile"));
    let list: Vec<PlatformUi> = PAGES.iter().map(|(k, label, hint)| PlatformUi { key: (*k).into(), label: (*label).into(), hint: (*hint).into(), on: on(k) }).collect();
    app.global::<Home>().set_pages(ModelRc::new(VecModel::from(list.clone())));
    app.global::<CPrefs>().set_pages(ModelRc::new(VecModel::from(list)));
    // Les captures et l'autotest montrent l'appli telle quelle.
    app.global::<Home>().set_pages_ask(st.career.is_some() && !st.selftest && setting(st, CHOSEN).is_empty());
}

/// Montre ou retire une page du menu.
pub fn toggle(app: &AppWindow, st: &mut State, key: &str) {
    let Some(c) = st.career.as_ref() else { return };
    let mut list = hidden(st);
    if list.iter().any(|k| k == key) {
        list.retain(|k| k != key);
    } else if PAGES.iter().any(|p| p.0 == key) {
        list.push(key.to_string());
    }
    let _ = c.set_setting(HIDDEN, &list.join(","));
    push(app, st);
}

/// La question de l'Accueil a sa réponse.
pub fn done(app: &AppWindow, st: &mut State) {
    if let Some(c) = st.career.as_ref() {
        let _ = c.set_setting(CHOSEN, "1");
    }
    let n = PAGES.len() - hidden(st).len();
    app.set_message(format!("{n} page{} dans le menu. Tu peux en remettre ou en retirer dans Réglages › Général.", if n > 1 { "s" } else { "" }).into());
    push(app, st);
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    let g = app.global::<Home>();
    let (weak, s) = (app.as_weak(), state.clone());
    g.on_page_toggle(move |key| {
        let app = weak.unwrap();
        toggle(&app, &mut s.borrow_mut(), key.as_str());
    });
    let (weak, s) = (app.as_weak(), state.clone());
    g.on_pages_done(move || {
        let app = weak.unwrap();
        done(&app, &mut s.borrow_mut());
    });
}
