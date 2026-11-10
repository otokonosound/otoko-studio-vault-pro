//! Réglages › Carrière, comme les Préférences de Studio Vault : identité (`artist_alias`,
//! `artist_aliases`), lecture (`continuous_play`), apparence (`accent_color` — une pastille, une
//! couleur choisie ou « cover » —, `table_density`, `table_covers`), analyse (`analysis_write_tags`,
//! `auto_analyze`, `idle_analyze`), sauvegarde (`backup_extra_dir`, une par jour), raccourcis
//! (`shortcuts`). Mêmes clés que Studio Vault : ses choix sont repris à l'import.

use std::cell::RefCell;
use std::rc::Rc;

use otoko_library::career::{PLATFORMS, arrivee, backup, copies, library as lib, prefs, releases, statuts};
use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel};

use crate::state::State;
use crate::{AppWindow, CPrefs, KeyUi, Lib, PlatformUi, Theme};

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// Le dossier des sauvegardes quotidiennes de la Carrière.
pub fn backup_dir() -> std::path::PathBuf {
    crate::donnees::data_dir().join("sauvegardes")
}

fn setting(st: &State, key: &str) -> String {
    st.career.as_ref().and_then(|c| c.setting(key)).unwrap_or_default()
}

fn set(st: &State, key: &str, value: &str) {
    if let Some(c) = st.career.as_ref() {
        let _ = c.set_setting(key, value);
    }
}

fn rgb(c: (u8, u8, u8)) -> Color {
    Color::from_rgb_u8(c.0, c.1, c.2)
}

/// Couleur d'accent, densité et pochettes du tableau : appliqués à toute l'interface.
pub fn apply(app: &AppWindow, st: &State) {
    let acc = setting(st, "accent_color");
    let theme = app.global::<Theme>();
    match prefs::parse_hex(&acc) {
        Some(c) => {
            theme.set_accent(rgb(c));
            theme.set_custom_accent(true);
        }
        None if acc == "cover" => {}
        None => theme.set_custom_accent(false),
    }
    let g = app.global::<Lib>();
    let compact = setting(st, "table_density") == "compact";
    g.set_row_h(if compact { 30.0 } else { 40.0 });
    g.set_show_covers(setting(st, "table_covers") != "0");
    g.set_cards(st.career.as_ref().is_some_and(|c| lib::flag(c.conn(), "library_cards", false)));
}

/// « Prendre la couleur de la pochette du morceau en cours » : la couleur marquante de sa pochette.
pub fn cover_accent(app: &AppWindow, st: &State, cover: Option<&slint::Image>) {
    if setting(st, "accent_color") != "cover" {
        return;
    }
    let c = cover
        .and_then(|img| img.to_rgba8())
        .and_then(|buf| prefs::dominant_color(buf.as_bytes(), buf.width() as usize, buf.height() as usize))
        .or_else(|| prefs::parse_hex(prefs::ACCENTS[0].1));
    if let Some(c) = c {
        let theme = app.global::<Theme>();
        theme.set_accent(rgb(c));
        theme.set_custom_accent(true);
    }
}

/// Remplit la partie sortie d'écoute des Réglages.
fn fill_audio(app: &AppWindow, st: &State) {
    app.set_audio_text(
        match st.ecoute.description() {
            Some(d) => format!("En lecture sur : {d}."),
            None => "Le son sort par la sortie de Windows. La carte son ne s'ouvre que pendant l'écoute : le reste du temps, elle est libre pour tes autres logiciels.".to_string(),
        }
        .into(),
    );
}

/// Remplit les Réglages : la sortie d'écoute, puis la partie Carrière.
pub fn fill(app: &AppWindow, st: &mut State) {
    fill_audio(app, st);
    let g = app.global::<CPrefs>();
    let Some(c) = st.career.as_ref() else {
        g.set_ready(false);
        return;
    };
    g.set_ready(true);
    g.set_alias(setting(st, "artist_alias").into());
    g.set_aliases(model(prefs::aliases(c.conn()).into_iter().map(SharedString::from).collect()));
    g.set_continuous(lib::flag(c.conn(), "continuous_play", true));
    g.set_pick_advance(lib::flag(c.conn(), "pick_advance", true));
    g.set_accents(model(prefs::ACCENTS.iter().filter_map(|a| prefs::parse_hex(a.1)).map(rgb).collect()));
    g.set_accent_names(model(prefs::ACCENTS.iter().map(|a| SharedString::from(a.0)).collect()));
    let acc = setting(st, "accent_color");
    g.set_cover_accent(acc == "cover");
    let idx = prefs::ACCENTS.iter().position(|a| a.1.eq_ignore_ascii_case(&acc));
    g.set_accent(match (idx, acc.as_str()) {
        (Some(i), _) => i as i32,
        (None, "") | (None, "cover") => -1,
        _ => 8,
    });
    g.set_custom(if idx.is_none() && acc.starts_with('#') { acc.as_str().into() } else { "".into() });
    g.set_compact(setting(st, "table_density") == "compact");
    g.set_covers(setting(st, "table_covers") != "0");
    g.set_write_tags(lib::flag(c.conn(), "analysis_write_tags", true));
    g.set_auto_analyze(lib::flag(c.conn(), "auto_analyze", false));
    g.set_idle_analyze(lib::flag(c.conn(), "idle_analyze", true));
    let extra = setting(st, backup::EXTRA_SETTING);
    g.set_extra_on(!extra.is_empty() || st.prefs_extra_pending);
    g.set_extra_dir(extra.as_str().into());
    g.set_backup_text(
        format!(
            "Une sauvegarde de la base est faite chaque jour dans :\n{}\nElle contient tes fiches de sortie, notes, marqueurs, prods, clients et textes (pas les fichiers audio). Pour ne rien perdre si l'ordinateur lâche, envoie aussi une copie dans un dossier synchronisé.",
            backup_dir().display()
        )
        .into(),
    );
    // Les plus récentes d'abord.
    let list: Vec<SharedString> = backup::list(&backup_dir())
        .iter()
        .take(BACKUPS_SHOWN)
        .map(|p| {
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) as f64 / 1_048_576.0;
            SharedString::from(format!("{}  ({size:.1} Mo)", backup_label(p)))
        })
        .collect();
    g.set_backups(model(list));
    g.set_tray_on(crate::zone::enabled());
    g.set_arrival_dir(setting(st, arrivee::DIR_KEY).into());
    g.set_remote_on(crate::distant::address().is_some());
    if g.get_remote_address().is_empty() {
        g.set_remote_address(crate::distant::address().unwrap_or_default().into());
    }
    if g.get_remote_code().is_empty() {
        g.set_remote_code(crate::distant::code().into());
    }
    // Le code que l'autre PC doit recopier : seulement si ce PC n'en suit pas un autre (et jamais pendant l'autotest).
    let main = crate::distant::address().is_none() && !st.selftest;
    g.set_link_code(if main { otoko_mobile::link_code(&crate::donnees::data_dir()) } else { String::new() }.into());
    crate::majs::push(app, st);
    if let Some(c) = st.career.as_ref() {
        g.set_auto_status(lib::flag(c.conn(), statuts::FLAG, true));
        let chosen = releases::default_platforms(c.conn());
        let on = |k: &str| chosen.iter().any(|x| x == k);
        g.set_platforms(model(PLATFORMS.iter().map(|(k, label, hint)| PlatformUi { key: (*k).into(), label: (*label).into(), hint: (*hint).into(), on: on(k) }).collect()));
        let roots = copies::roots(c.conn());
        g.set_copies_roots(model(roots.iter().map(|r| SharedString::from(r.as_str())).collect()));
        g.set_copies_busy(st.copies_busy);
        let last = copies::last(c.conn());
        g.set_copies_single(last.as_ref().map(|l| l.0 as i32).unwrap_or(0));
        g.set_copies_text(
            match last {
                _ if st.copies_busy => "Contrôle en cours, en arrière-plan.".to_string(),
                _ if roots.is_empty() => otoko_library::career::home::suggested_drive_dir().and_then(|d| d.parent().map(|p| format!("Proposition : {}", p.display()))).unwrap_or_default(),
                None => "Pas encore contrôlé.".to_string(),
                Some((0, when)) => format!("Tous tes morceaux ont une copie (contrôle du {}).", crate::career::fr_date(&when)),
                Some((n, when)) => format!("{n} morceau{} sans copie (contrôle du {}).", if n > 1 { "x" } else { "" }, crate::career::fr_date(&when)),
            }
            .into(),
        );
    }
    push_keys(app, st);
}

fn push_keys(app: &AppWindow, st: &State) {
    let Some(c) = st.career.as_ref() else { return };
    let map = prefs::shortcuts(c.conn());
    let rows: Vec<KeyUi> = prefs::ACTIONS
        .iter()
        .map(|a| KeyUi { label: a.label.into(), key: map.iter().find(|k| k.0 == a.id).map(|k| k.1.clone()).unwrap_or_default().into(), tip: a.tip.into() })
        .collect();
    app.global::<CPrefs>().set_keys(model(rows));
}

/// Montre une partie des Réglages (les préférences, les raccourcis, « À propos »).
pub fn show(app: &AppWindow, part: &str) {
    app.set_prefs_part(
        match part {
            "shortcuts" => "keys",
            "about" => "about",
            _ => "general",
        }
        .into(),
    );
    app.set_prefs_scroll(0.0);
}

const BACKUPS_SHOWN: usize = 8;

/// « carriere-20261106-112721.db » → « 6 novembre 2026 à 11 h 27 ».
fn backup_label(p: &std::path::Path) -> String {
    let name = p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let d = name.trim_start_matches("carriere-");
    match (d.get(0..4), d.get(4..6), d.get(6..8), d.get(9..11), d.get(11..13)) {
        (Some(y), Some(m), Some(j), Some(h), Some(mi)) => format!("{} à {h} h {mi}", crate::career::fr_date(&format!("{y}-{m}-{j}"))),
        _ => name,
    }
}

/// Referme la base et la remplace (avec une sauvegarde juste avant), puis relance le logiciel. Si ça échoue, rien n'a
/// changé : la base est rouverte telle quelle.
fn swap_base(app: &AppWindow, st: &mut State, change: impl FnOnce(&std::path::Path) -> Result<(), String>) {
    let Some(c) = st.career.as_ref() else { return };
    if let Err(e) = backup::now(c.conn(), &backup_dir(), None) {
        app.set_message(format!("Sauvegarde préalable impossible, rien n'a changé : {e}").into());
        return;
    }
    if let Some(s) = st.mobile.take() {
        s.stop();
    }
    st.career = None;
    let db = st.career_path.clone();
    match change(&db) {
        Ok(()) => {
            let _ = std::env::current_exe().and_then(|me| std::process::Command::new(me).arg("--relance").spawn());
            let _ = slint::quit_event_loop();
        }
        Err(e) => {
            st.career = crate::career::open(&db);
            crate::mobile::autostart(st);
            app.set_message(e.into());
        }
    }
}

/// Remplace la base par une sauvegarde, après avoir montré ce qu'elle contient.
fn restore_from(app: &AppWindow, st: &mut State, src: std::path::PathBuf) {
    if st.selftest {
        return;
    }
    if st.distant.is_some() {
        app.set_message("Ta base est sur le PC principal : restaure-la depuis celui-ci.".into());
        return;
    }
    let (tracks, releases) = match backup::inspect(&src) {
        Ok(n) => n,
        Err(e) => {
            app.set_message(e.into());
            return;
        }
    };
    let ok = rfd::MessageDialog::new()
        .set_title("Restaurer cette sauvegarde ?")
        .set_description(format!(
            "Sauvegarde : {}\nElle contient {tracks} morceaux et {releases} sorties.\n\nTa base actuelle sera remplacée. Elle est sauvegardée juste avant : tu pourras revenir en arrière. Tes fichiers audio ne sont pas touchés.\n\nLe logiciel redémarre ensuite.",
            backup_label(&src)
        ))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();
    if ok != rfd::MessageDialogResult::Yes {
        return;
    }
    restore_now(app, st, &src);
}

/// La restauration elle-même, une fois confirmée.
pub fn restore_now(app: &AppWindow, st: &mut State, src: &std::path::Path) {
    // La sauvegarde préalable peut faire disparaître la plus ancienne : on travaille sur une copie.
    let keep = crate::donnees::data_dir().join("restauration-en-cours.db");
    if let Err(e) = std::fs::copy(src, &keep) {
        app.set_message(format!("Restauration impossible : {e}").into());
        return;
    }
    swap_base(app, st, |db| backup::restore(db, &keep));
    let _ = std::fs::remove_file(&keep);
}

/// Repartir de zéro : la base est mise de côté dans les sauvegardes, les liens de partage cessent de marcher.
fn reset_base(app: &AppWindow, st: &mut State) {
    if st.selftest {
        return;
    }
    if st.distant.is_some() {
        app.set_message("Ta base est sur le PC principal : réinitialise-la depuis celui-ci.".into());
        return;
    }
    let ok = rfd::MessageDialog::new()
        .set_title("Réinitialiser ta base ?")
        .set_description("Ta bibliothèque, tes sorties et tes réglages seront vidés dans le logiciel, et tes liens de partage cesseront de marcher.\n\nTes fichiers audio ne sont pas touchés. Une sauvegarde est faite juste avant : Réglages › Analyse et sauvegarde permet de revenir en arrière.\n\nLe logiciel redémarre ensuite.")
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();
    if ok != rfd::MessageDialogResult::Yes {
        return;
    }
    reset_now(app, st);
}

/// La réinitialisation elle-même, une fois confirmée.
pub fn reset_now(app: &AppWindow, st: &mut State) {
    // Les liens visaient les morceaux de cette base : avec une base neuve, ils mèneraient à d'autres morceaux.
    let shares = crate::career::shares_path(&st.career_path);
    if shares.is_file() {
        let copy = backup_dir().join(format!("partages-{}.db", backup::stamp()));
        let _ = std::fs::create_dir_all(backup_dir());
        let wiped = otoko_mobile::shares::ShareStore::open(&shares).map_err(|e| e.to_string()).and_then(|s| s.wipe(&copy));
        if let Err(e) = wiped {
            app.set_message(format!("Réinitialisation impossible, rien n'a changé : {e}").into());
            return;
        }
    }
    swap_base(app, st, backup::clear);
}

/// Sauvegarde la base maintenant (et la copie dans le second dossier).
pub fn backup_now(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let extra = setting(st, backup::EXTRA_SETTING);
    let extra = (!extra.is_empty()).then(|| std::path::PathBuf::from(extra));
    match backup::now(c.conn(), &backup_dir(), extra.as_deref()) {
        Ok(p) => app.set_message(format!("Base sauvegardée : {}{}", p.display(), extra.map(|e| format!(" ; copie dans {}", e.display())).unwrap_or_default()).into()),
        Err(e) => app.set_message(format!("Sauvegarde impossible : {e}").into()),
    }
    fill(app, st);
}

/// La sauvegarde du jour, au démarrage (une par jour, 15 gardées ; 10 dans le second dossier).
pub fn daily_backup(st: &mut State) {
    if st.selftest || st.career_path != crate::career::default_path() {
        return;
    }
    let db = st.career_path.clone();
    let extra = setting(st, backup::EXTRA_SETTING);
    std::thread::spawn(move || {
        let Ok(c) = otoko_library::career::Career::open(&db) else { return };
        let extra = (!extra.is_empty()).then(|| std::path::PathBuf::from(extra));
        let _ = backup::daily(c.conn(), &backup_dir(), extra.as_deref());
    });
}

macro_rules! cp_on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.global::<CPrefs>().$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    cp_on!(app, state, on_alias_edited, |app, st, t| {
        set(st, "artist_alias", t.trim());
        let _ = app;
    });
    cp_on!(app, state, on_alias_add, |app, st, t| {
        let Some(c) = st.career.as_ref() else { return };
        let mut list = prefs::aliases(c.conn());
        if !t.trim().is_empty() && !list.iter().any(|a| a.eq_ignore_ascii_case(t.trim())) {
            list.push(t.trim().to_string());
            let _ = prefs::set_aliases(c.conn(), &list);
        }
        fill(&app, st);
    });
    cp_on!(app, state, on_alias_remove, |app, st, i| {
        let Some(c) = st.career.as_ref() else { return };
        let mut list = prefs::aliases(c.conn());
        if (i as usize) < list.len() {
            list.remove(i as usize);
            let _ = prefs::set_aliases(c.conn(), &list);
        }
        fill(&app, st);
    });
    cp_on!(app, state, on_toggle, |app, st, key| {
        match key.as_str() {
            "cover_accent" => {
                let on = setting(st, "accent_color") != "cover";
                set(st, "accent_color", if on { "cover" } else { "" });
                if on {
                    let cover = st.player.id.and_then(|id| st.lib.covers.get(&id).cloned().flatten());
                    cover_accent(&app, st, cover.as_ref());
                }
            }
            "table_covers" => set(st, "table_covers", if setting(st, "table_covers") == "0" { "1" } else { "0" }),
            "close_to_tray" => {
                if !st.selftest {
                    crate::zone::set_enabled(!crate::zone::enabled());
                }
            }
            "backup_extra" => {
                if setting(st, backup::EXTRA_SETTING).is_empty() {
                    st.prefs_extra_pending = true;
                    // Le dossier proposé : celui de Google Drive s'il y en a un.
                    if let Some(d) = otoko_library::career::home::suggested_drive_dir() {
                        set(st, backup::EXTRA_SETTING, &d.to_string_lossy());
                    }
                } else {
                    st.prefs_extra_pending = false;
                    set(st, backup::EXTRA_SETTING, "");
                }
            }
            k if k.starts_with("page:") => crate::pages::toggle(&app, st, &k["page:".len()..]),
            k if k.starts_with("platform:") => {
                let Some(c) = st.career.as_ref() else { return };
                let key = &k["platform:".len()..];
                let mut list = releases::default_platforms(c.conn());
                if list.iter().any(|x| x == key) {
                    // Au moins une : sans plateforme, une sortie n'a plus d'étapes.
                    if list.len() > 1 {
                        list.retain(|x| x != key);
                    }
                } else {
                    list.push(key.to_string());
                }
                let _ = releases::set_default_platforms(c.conn(), &list);
            }
            k => {
                let Some(c) = st.career.as_ref() else { return };
                let default = !matches!(k, "auto_analyze");
                lib::set_flag(c.conn(), k, !lib::flag(c.conn(), k, default));
            }
        }
        apply(&app, st);
        fill(&app, st);
        crate::bibliotheque::push_rows(&app, st);
    });
    cp_on!(app, state, on_set_density, |app, st, compact| {
        set(st, "table_density", if compact { "compact" } else { "aere" });
        apply(&app, st);
        fill(&app, st);
    });
    cp_on!(app, state, on_pick_accent, |app, st, i| {
        let v = if i < 0 { String::new() } else { prefs::ACCENTS.get(i as usize).map(|a| a.1.to_string()).unwrap_or_default() };
        set(st, "accent_color", &v);
        apply(&app, st);
        fill(&app, st);
    });
    cp_on!(app, state, on_custom_accent, |app, st, t| {
        let t = t.trim();
        let hex = if t.starts_with('#') { t.to_string() } else { format!("#{t}") };
        if prefs::parse_hex(&hex).is_some() {
            set(st, "accent_color", &hex.to_lowercase());
            apply(&app, st);
            fill(&app, st);
        }
    });
    cp_on!(app, state, on_choose_extra, |app, st| {
        if st.selftest {
            return;
        }
        let Some(d) = rfd::FileDialog::new().set_title("Dossier de sauvegarde").pick_folder() else { return };
        set(st, backup::EXTRA_SETTING, &d.to_string_lossy());
        backup_now(&app, st);
    });
    cp_on!(app, state, on_arrival_choose, |app, st| {
        if st.selftest {
            return;
        }
        let Some(d) = rfd::FileDialog::new().set_title("Dossier d'arrivée des exports").pick_folder() else { return };
        crate::fond::set_arrival(&app, st, Some(d));
        fill(&app, st);
    });
    cp_on!(app, state, on_arrival_clear, |app, st| {
        crate::fond::set_arrival(&app, st, None);
        fill(&app, st);
    });
    cp_on!(app, state, on_update_choose, |app, st| {
        if st.selftest {
            return;
        }
        let Some(d) = rfd::FileDialog::new().set_title("Dossier des installateurs").pick_folder() else { return };
        crate::majs::set_folder(Some(&d));
        crate::majs::check(st);
        crate::majs::push(&app, st);
    });
    cp_on!(app, state, on_update_clear, |app, st| {
        crate::majs::set_folder(None);
        crate::majs::check(st);
        crate::majs::push(&app, st);
    });
    cp_on!(app, state, on_update_check, |app, st| {
        crate::majs::check(st);
        app.set_message("Recherche d'une nouvelle version…".into());
    });
    cp_on!(app, state, on_update_install, |app, st| { crate::majs::install(&app, st); });
    cp_on!(app, state, on_remote_test, |app, st| { crate::distant::try_address(&app, st, false); });
    cp_on!(app, state, on_remote_connect, |app, st| { crate::distant::try_address(&app, st, true); });
    cp_on!(app, state, on_link_copy, |app, st| {
        let _ = st;
        let code = app.global::<CPrefs>().get_link_code();
        if crate::publish::copy(&code) {
            app.set_message("Code de liaison copié : colle-le sur l'autre PC, à côté de l'adresse de ce PC.".into());
        }
    });
    cp_on!(app, state, on_link_renew, |app, st| {
        if st.selftest {
            return;
        }
        let code = otoko_mobile::link_renew(&crate::donnees::data_dir());
        app.global::<CPrefs>().set_link_code(code.into());
        app.set_message("Nouveau code de liaison : l'autre PC devra recopier celui-ci.".into());
    });
    cp_on!(app, state, on_remote_leave, |app, st| {
        let _ = st;
        crate::distant::set_address(None);
        let g = app.global::<CPrefs>();
        g.set_remote_on(false);
        g.set_remote_info("Ce PC ne suit plus le PC principal : relance le logiciel pour revenir à sa propre base.".into());
    });
    cp_on!(app, state, on_copies_add, |app, st| {
        if st.selftest {
            return;
        }
        let start = otoko_library::career::home::suggested_drive_dir().and_then(|d| d.parent().map(std::path::Path::to_path_buf)).unwrap_or_default();
        let Some(d) = rfd::FileDialog::new().set_title("Où sont tes copies ?").set_directory(start).pick_folder() else { return };
        let Some(c) = st.career.as_ref() else { return };
        let mut list = copies::roots(c.conn());
        let d = d.to_string_lossy().into_owned();
        if !list.contains(&d) {
            list.push(d);
            let _ = copies::set_roots(c.conn(), &list);
        }
        crate::fond::check_copies(&app, st);
        fill(&app, st);
    });
    cp_on!(app, state, on_copies_remove, |app, st, i| {
        let Some(c) = st.career.as_ref() else { return };
        let mut list = copies::roots(c.conn());
        if (i as usize) < list.len() {
            list.remove(i as usize);
            let _ = copies::set_roots(c.conn(), &list);
        }
        fill(&app, st);
    });
    cp_on!(app, state, on_copies_check, |app, st| { crate::fond::check_copies(&app, st); });
    cp_on!(app, state, on_copies_show, |app, st| { crate::bibliotheque::open_view(&app, st, "Une seule copie", copies::SINGLE_SQL); });
    cp_on!(app, state, on_backup_now, |app, st| { backup_now(&app, st); });
    cp_on!(app, state, on_backup_restore, |app, st, i| {
        if let Some(src) = backup::list(&backup_dir()).into_iter().take(BACKUPS_SHOWN).nth(i.max(0) as usize) {
            restore_from(&app, st, src);
        }
    });
    cp_on!(app, state, on_backup_restore_file, |app, st| {
        if st.selftest {
            return;
        }
        let Some(f) = rfd::FileDialog::new().set_title("Choisis une sauvegarde").add_filter("Sauvegarde", &["db"]).set_directory(backup_dir()).pick_file() else { return };
        restore_from(&app, st, f);
    });
    cp_on!(app, state, on_base_reset, |app, st| { reset_base(&app, st); });
    cp_on!(app, state, on_open_backups, |app, st| {
        let _ = std::fs::create_dir_all(backup_dir());
        if !st.selftest {
            let _ = std::process::Command::new("explorer.exe").arg(backup_dir()).spawn();
        }
        let _ = app;
    });
    cp_on!(app, state, on_capture, |app, st, i| {
        let g = app.global::<CPrefs>();
        g.set_capturing(i);
        g.set_key_msg("".into());
        let _ = st;
    });
    cp_on!(app, state, on_key_pressed, |app, st, text, ctrl, shift, alt| {
        let g = app.global::<CPrefs>();
        let Some(a) = prefs::ACTIONS.get(g.get_capturing().max(0) as usize) else { return };
        let seq = crate::raccourcis::combo(text.as_str(), ctrl, shift, alt);
        if seq.is_empty() {
            return;
        }
        let Some(c) = st.career.as_ref() else { return };
        match prefs::set_shortcut(c.conn(), a.id, &seq) {
            Ok(()) => {
                g.set_capturing(-1);
                g.set_key_msg(format!("« {} » : {seq}.", a.label.trim_end_matches('…')).into());
            }
            Err(other) => g.set_key_msg(format!("« {seq} » est déjà utilisé : {other}. Choisis une autre combinaison (Échap pour annuler).").into()),
        }
        push_keys(&app, st);
    });
    cp_on!(app, state, on_key_clear, |app, st| {
        let g = app.global::<CPrefs>();
        if let (Some(a), Some(c)) = (prefs::ACTIONS.get(g.get_capturing().max(0) as usize), st.career.as_ref()) {
            let _ = prefs::set_shortcut(c.conn(), a.id, "");
        }
        g.set_capturing(-1);
        push_keys(&app, st);
    });
    cp_on!(app, state, on_keys_reset, |app, st| {
        if let Some(c) = st.career.as_ref() {
            let _ = prefs::reset_shortcuts(c.conn());
        }
        app.global::<CPrefs>().set_key_msg("Raccourcis d'origine remis.".into());
        push_keys(&app, st);
    });
}
