//! Aller d'une page à l'autre, et les actions (celles de Studio Vault : `career::prefs::ACTIONS`), lancées par leur
//! raccourci clavier (modifiable dans les Réglages, réglage `shortcuts` repris de Studio Vault) ou par la palette de
//! commandes (Ctrl+K).

use otoko_library::career::prefs;
use slint::ComponentHandle;

use crate::state::State;
use crate::{AppWindow, Lib};

/// La combinaison d'une touche telle que Slint la donne, écrite comme Studio Vault (« Ctrl+Shift+A »).
pub fn combo(text: &str, ctrl: bool, shift: bool, alt: bool) -> String {
    let key = match text {
        " " => "Space".to_string(),
        "\u{1b}" => "Escape".into(),
        "\u{7f}" => "Del".into(),
        "\n" | "\r" => "Return".into(),
        "\t" => "Tab".into(),
        "\u{8}" => "Backspace".into(),
        "\u{f700}" => "Up".into(),
        "\u{f701}" => "Down".into(),
        "\u{f702}" => "Left".into(),
        "\u{f703}" => "Right".into(),
        "\u{f729}" => "Home".into(),
        "\u{f72b}" => "End".into(),
        t if t.chars().count() == 1 => {
            let c = t.chars().next().unwrap_or(' ');
            match c as u32 {
                // F1 à F12.
                0xf704..=0xf70f => format!("F{}", c as u32 - 0xf704 + 1),
                // Clavier AZERTY : avec Ctrl, la rangée du haut donne ses chiffres (Ctrl+1).
                _ => match c {
                    '&' if ctrl => "1".into(),
                    'é' if ctrl => "2".into(),
                    '"' if ctrl => "3".into(),
                    '\'' if ctrl => "4".into(),
                    '(' if ctrl => "5".into(),
                    '-' if ctrl => "6".into(),
                    'è' if ctrl => "7".into(),
                    '_' if ctrl => "8".into(),
                    'ç' if ctrl => "9".into(),
                    'à' if ctrl => "0".into(),
                    // Un caractère de contrôle (Ctrl+lettre sur certains systèmes) : la lettre.
                    c if (c as u32) < 27 && ctrl => ((b'A' + c as u8 - 1) as char).to_string(),
                    c => c.to_uppercase().to_string(),
                },
            }
        }
        _ => return String::new(),
    };
    let mut s = String::new();
    if ctrl {
        s.push_str("Ctrl+");
    }
    if alt {
        s.push_str("Alt+");
    }
    // Maj ne compte pas pour une ponctuation déjà tapée avec Maj (« ? », « . » sur AZERTY).
    if shift && (key.len() > 1 || key.chars().all(|c| c.is_ascii_alphanumeric())) {
        s.push_str("Shift+");
    }
    s.push_str(&key);
    prefs::normalize(&s)
}

/// Une touche tapée : rend vrai si elle lance une action. Hors du tableau de la bibliothèque, seules les combinaisons
/// (Ctrl, Alt) et les touches F comptent : les lettres seules restent aux champs de texte.
pub fn key(app: &AppWindow, st: &mut State, text: &str, ctrl: bool, shift: bool, alt: bool, table: bool) -> bool {
    let seq = combo(text, ctrl, shift, alt);
    if seq.is_empty() {
        return false;
    }
    // Échap ferme d'abord ce qui est ouvert par-dessus la page.
    if seq == "Escape" && close_top(app, st) {
        return true;
    }
    let fkey = seq.starts_with('F') && seq.len() <= 3;
    if !table && !ctrl && !alt && !fkey {
        return false;
    }
    let Some(c) = st.career.as_ref() else { return false };
    let Some(id) = prefs::lookup(&prefs::shortcuts(c.conn()), &seq) else { return false };
    run(app, st, id)
}

/// Ferme la fenêtre posée par-dessus la page, s'il y en a une.
fn close_top(app: &AppWindow, st: &mut State) -> bool {
    use crate::{Mast, Part, Proj, Rel};
    if app.global::<Part>().get_creating() {
        app.global::<Part>().set_creating(false);
        return true;
    }
    if !matches!(st.lib.dlg, crate::outils::Dlg::None) {
        crate::outils::button(app, st, "close");
        return true;
    }
    let r = app.global::<Rel>();
    if r.get_qc_open() {
        r.set_qc_open(false);
        return true;
    }
    if r.get_picker_open() {
        r.set_picker_open(false);
        return true;
    }
    if r.get_naming() {
        r.set_naming(false);
        return true;
    }
    if app.global::<Mast>().get_log_open() {
        app.global::<Mast>().set_log_open(false);
        return true;
    }
    let p = app.global::<Proj>();
    if p.get_clients_open() {
        p.set_clients_open(false);
        return true;
    }
    if p.get_sb_open() {
        p.set_sb_open(false);
        return true;
    }
    if p.get_open() {
        p.set_open(false);
        return true;
    }
    let g = app.global::<Lib>();
    if g.get_wheel_open() {
        g.set_wheel_open(false);
        return true;
    }
    if g.get_player_big() {
        g.set_player_big(false);
        return true;
    }
    false
}

fn later(app: &AppWindow, f: impl FnOnce(&Lib) + 'static) {
    let weak = app.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(80), move || {
        if let Some(app) = weak.upgrade() {
            f(&app.global::<Lib>());
        }
    });
}

/// Va à une page de l'application : 4 les pages de ta carrière (`onglet`), 6 les Réglages, 7 Mobile et partage.
pub fn aller(app: &AppWindow, st: &mut State, espace: i32, onglet: i32) {
    match espace {
        6 => {
            app.set_space(6);
            crate::preferences::fill(app, st);
        }
        7 => {
            app.set_space(7);
            crate::mobile::refresh(app, st);
            crate::partage::fill(app, st);
        }
        _ => tab(app, st, onglet),
    }
}

/// Va à une page de ta carrière, relue si la base a changé depuis la dernière visite.
pub fn tab(app: &AppWindow, st: &mut State, tab: i32) {
    app.set_space(4);
    app.set_career_tab(tab);
    crate::career::page(app, st, tab);
}

/// Va dans les Réglages, à une partie (préférences, raccourcis, à propos).
pub fn preferences(app: &AppWindow, st: &mut State, part: &str) {
    aller(app, st, 6, 0);
    crate::preferences::show(app, part);
}

/// Lance une action ; rend faux si elle n'existe pas.
pub fn run(app: &AppWindow, st: &mut State, id: &str) -> bool {
    let lib = |app: &AppWindow, st: &mut State, what: &str| crate::bibliotheque::action(app, st, what);
    match id {
        "add_folder" => crate::bibliotheque::add_folder(app, st),
        "rescan" => crate::bibliotheque::scan(app, st),
        "watch_folders" => crate::outils::open(app, st, "watch"),
        "relocate" => crate::outils::open(app, st, "relocate"),
        "import_suno" => {
            tab(app, st, 4);
            crate::suno::import_show(app, st);
        }
        "export_csv" => lib(app, st, "export-csv"),
        "backup" => crate::preferences::backup_now(app, st),
        "preferences" => preferences(app, st, "prefs"),
        "shortcuts" => preferences(app, st, "shortcuts"),
        "about" => preferences(app, st, "about"),
        "undo" => crate::outils::undo(app, st),
        "edit_tags" => {
            tab(app, st, 1);
            crate::bibliotheque::edit_tags(app, st);
        }
        "genre" => crate::outils::open(app, st, "genre"),
        "rename" | "rename_one" => crate::outils::open(app, st, "rename"),
        "reorganize" => crate::outils::open(app, st, "reorganize"),
        "project" => lib(app, st, "project"),
        "detect_nature" => crate::outils::open(app, st, "nature"),
        "clients" => crate::projets::clients_show(app, st, None),
        "complete_library" => crate::outils::open(app, st, "complete"),
        "dupe_folders" => crate::outils::open(app, st, "dupe-folders"),
        "dupes" => crate::outils::open(app, st, "dupes"),
        "set_builder" => crate::projets::set_builder(app, st),
        "discography" => {
            tab(app, st, 2);
            crate::sorties::discography_dialog(app, st);
        }
        "planning" => {
            tab(app, st, 2);
            crate::sorties::planning(app, st);
        }
        "open_soundcloud" => {
            let url = crate::bibliotheque::selection(st).first().and_then(|id| st.career.as_ref().map(|c| otoko_library::career::library::field(c.conn(), *id, "sc_url"))).unwrap_or_default();
            let url = url.trim();
            if url.is_empty() {
                app.set_message("Ce morceau n'a pas de lien SoundCloud (panneau de détails › Lien SoundCloud).".into());
            } else if !(url.starts_with("https://") || url.starts_with("http://")) {
                // L'explorateur lancerait un programme ou un fichier partagé aussi bien qu'une page.
                app.set_message("Ce lien n'est pas une adresse web : il n'a pas été ouvert.".into());
            } else if !st.selftest {
                let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
            }
        }
        "archive_tracks" => lib(app, st, "archive"),
        "remove" => crate::outils::forget(app, st),
        "home" => tab(app, st, 0),
        "library" => tab(app, st, 1),
        "production" => tab(app, st, 6),
        "mastering" => tab(app, st, crate::mastrage::TAB),
        "releases" => tab(app, st, 2),
        "publishing" => {
            match crate::bibliotheque::selection(st).first().copied() {
                Some(first) => crate::publish::open(app, st, first),
                None => tab(app, st, 3),
            }
        }
        "prompt_builder" | "prompt_library" => tab(app, st, 4),
        "stats" => tab(app, st, 5),
        "details" => lib(app, st, "details"),
        // Une fois la bibliothèque à l'écran (elle se crée au prochain dessin) : la demande à l'écran.
        "focus_search" => {
            tab(app, st, 1);
            later(app, |g| g.set_focus_search(g.get_focus_search() + 1));
        }
        "save_view" => {
            tab(app, st, 1);
            later(app, |g| g.set_ask_view(g.get_ask_view() + 1));
        }
        "help_search" => {
            tab(app, st, 1);
            later(app, |g| g.set_ask_help(g.get_ask_help() + 1));
        }
        "play" => crate::bibliotheque::key(app, st, " "),
        "stop" => crate::lecteur::stop(app, st),
        "prev" => crate::lecteur::previous(app, st),
        "next" => crate::lecteur::next(app, st),
        "queue_add" => crate::bibliotheque::key(app, st, "q"),
        "queue_show" => crate::outils::open(app, st, "queue"),
        // Échap : la boucle A-B s'efface ; sans boucle, le panneau de détails se ferme.
        "loop_clear" => {
            if st.player.loop_a.is_some() || st.player.loop_b.is_some() {
                crate::lecteur::set_loop(app, st, "clear");
            } else {
                crate::bibliotheque::key(app, st, "\u{1b}");
            }
        }
        "pick_keep" => lib(app, st, "keep"),
        "pick_reject" => lib(app, st, "reject"),
        "pick_neutral" => lib(app, st, "neutral"),
        "analyze" => lib(app, st, "analyze"),
        "analyze_missing" => lib(app, st, "analyze-missing"),
        "master" => lib(app, st, "master"),
        "qc" => {
            let ids = crate::bibliotheque::selection(st);
            if ids.is_empty() {
                app.set_message("Sélectionne d'abord les morceaux à contrôler.".into());
            } else {
                tab(app, st, 2);
                crate::sorties::qc_for(app, st, &ids);
            }
        }
        "compare" => crate::bibliotheque::key(app, st, "compare"),
        "compatible" => lib(app, st, "compatible"),
        "camelot" => lib(app, st, "wheel"),
        "open_studio" => lib(app, st, "open-project"),
        "palette" => crate::palette::open(app, st),
        "mobile" => aller(app, st, 7, 0),
        "batches" => crate::outils::open(app, st, "batches"),
        "statuses" => crate::outils::open(app, st, "statuses"),
        "link_tracks" => {
            tab(app, st, 2);
            crate::fond::link(app, st, None);
        }
        "check_copies" => crate::fond::check_copies(app, st),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_like_studio_vault() {
        assert_eq!(combo("k", true, false, false), "Ctrl+K");
        assert_eq!(combo("A", true, true, false), "Ctrl+Shift+A");
        assert_eq!(combo("&", true, false, false), "Ctrl+1");
        assert_eq!(combo("\u{f705}", false, false, false), "F2");
        assert_eq!(combo("\u{f708}", false, false, false), "F5");
        assert_eq!(combo("\u{7f}", false, false, false), "Del");
        assert_eq!(combo("\u{1b}", false, false, false), "Escape");
        assert_eq!(combo(" ", false, false, false), "Space");
        assert_eq!(combo("p", false, false, false), "P");
        assert_eq!(combo(",", true, false, false), "Ctrl+,");
        assert_eq!(combo("\u{f702}", true, false, false), "Ctrl+Left");
        assert_eq!(combo("\u{b}", true, false, false), "Ctrl+K");
    }
}
