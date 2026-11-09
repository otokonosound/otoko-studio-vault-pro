//! Ce que font les gestes de la fenêtre principale : changer de page, Réglages, Mobile. Chaque module branche les siens.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use slint::platform::{Key, WindowEvent};

use crate::state::State;
use crate::{AppWindow, Nav};

/// Les flèches font comme Tab : passer à la commande suivante ou précédente.
fn move_focus(app: &AppWindow, key: Key) {
    let text: slint::SharedString = key.into();
    app.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    app.window().dispatch_event(WindowEvent::KeyReleased { text });
}

/// Branche un rappel de l'interface : `app` et `st` sont disponibles dans le corps.
macro_rules! on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            // Un rappel peut arriver pendant qu'un autre tient l'état (rare) : il est alors ignoré.
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    {
        let weak = app.as_weak();
        app.global::<Nav>().on_next(move || move_focus(&weak.unwrap(), Key::Tab));
        let weak = app.as_weak();
        app.global::<Nav>().on_previous(move || move_focus(&weak.unwrap(), Key::Backtab));
    }
    // ---------- le rail : une page
    on!(app, state, on_goto, |app, st, space, tab| {
        crate::raccourcis::aller(&app, st, space, tab);
    });
    crate::bibliotheque::wire(app, state);
    crate::accueil::wire(app, state);
    crate::sorties::wire(app, state);
    crate::projets::wire(app, state);
    crate::publish::wire(app, state);
    crate::suno::wire(app, state);
    crate::mastrage::wire(app, state);
    crate::palette::wire(app, state);
    crate::preferences::wire(app, state);
    crate::partage::wire(app, state);
    // ---------- Mobile et partage
    on!(app, state, on_mobile_toggle, |app, st| {
        crate::mobile::toggle(&app, st);
    });
    on!(app, state, on_mobile_script, |app, st, which| {
        crate::mobile::script(&app, st, which.as_str());
    });
    on!(app, state, on_mobile_open, |app, st| {
        crate::mobile::open_page(&app, st);
    });
    on!(app, state, on_mobile_refresh, |app, st| {
        crate::mobile::refresh(&app, st);
    });
    on!(app, state, on_mobile_task_set, |app, st, on| {
        crate::mobile::task(&app, st, on);
    });
    on!(app, state, on_mobile_wifi_set, |app, st, on| {
        crate::mobile::wifi_set(&app, st, on);
    });
    on!(app, state, on_mobile_step, |app, st, action| {
        crate::mobile::step(&app, st, action.as_str());
    });
    // ---------- Réglages
    on!(app, state, on_open_guide, |app, st| {
        crate::apparence::open_help(&app, "demarrer", st.selftest);
    });
    // « ? » et F1 : le guide à la page de l'écran en cours.
    on!(app, state, on_open_help, |app, st| {
        let anchor = crate::apparence::help_anchor(app.get_space(), app.get_career_tab());
        crate::apparence::open_help(&app, anchor, st.selftest);
    });
    on!(app, state, on_set_theme, |app, st, mode| {
        let _ = st;
        crate::apparence::set_mode(&app, mode);
    });
    on!(app, state, on_set_scale, |app, st, choice| {
        let _ = st;
        crate::apparence::set_scale(&app, choice);
    });
    crate::apparence::wire_toast(app);
    on!(app, state, on_career_import, |app, st| {
        crate::career::import(&app, st);
    });
}
