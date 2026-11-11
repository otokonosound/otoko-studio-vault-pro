//! Mesures (mémoire, processeur), captures d'écran et autotest de l'interface.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Model, Timer, TimerMode};

use crate::state::State;
use crate::AppWindow;

pub fn memory_mb() -> (f64, f64) {
    #[repr(C)]
    #[derive(Default)]
    struct Counters { cb: u32, faults: u32, peak_ws: usize, ws: usize, a: usize, b: usize, c: usize, d: usize, private: usize, peak_private: usize }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(p: isize, c: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters { cb: size_of::<Counters>() as u32, ..Default::default() };
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (c.ws as f64 / 1048576.0, c.private as f64 / 1048576.0)
}

/// Temps processeur consommé par le processus (utilisateur + noyau).
pub fn cpu_time() -> Duration {
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(p: isize, c: *mut u64, e: *mut u64, k: *mut u64, u: *mut u64) -> i32;
    }
    let (mut c, mut e, mut k, mut u) = (0u64, 0u64, 0u64, 0u64);
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) };
    Duration::from_nanos((k + u) * 100)
}

fn write_bmp(img: &slint::SharedPixelBuffer<slint::Rgba8Pixel>, path: &str) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let row = (w * 3 + 3) & !3;
    let mut bmp = Vec::with_capacity(54 + row * h);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((54 + row * h) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0]);
    bmp.extend_from_slice(&(w as i32).to_le_bytes());
    bmp.extend_from_slice(&(h as i32).to_le_bytes());
    bmp.extend_from_slice(&[1, 0, 24, 0, 0, 0, 0, 0]);
    bmp.extend_from_slice(&((row * h) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0; 16]);
    let px = img.as_slice();
    for y in (0..h).rev() {
        for x in 0..w {
            let p = px[y * w + x];
            bmp.extend_from_slice(&[p.b, p.g, p.r]);
        }
        bmp.extend(std::iter::repeat_n(0u8, row - w * 3));
    }
    let _ = std::fs::write(path, bmp);
}

/// Met le logiciel dans l'état d'un écran de la vidéo de présentation.
fn vitrine(app: &AppWindow, state: &Rc<RefCell<State>>, quoi: &str, texte: &str, id: i64) {
    let lib = app.global::<crate::Lib>();
    let choisir = |n: i32| {
        lib.invoke_row_clicked(0, false, false);
        for i in 1..n {
            lib.invoke_row_clicked(i, true, false);
        }
    };
    match quoi {
        "cartes" => lib.invoke_cards_changed(true),
        "cherche" => {
            lib.set_query(texte.into());
            lib.invoke_search(texte.into());
        }
        "onglet" => lib.invoke_details_tab_changed(id as i32),
        "boucle" => {
            lib.invoke_player_seek(0.3);
            lib.invoke_player_loop("a".into());
            lib.invoke_player_seek(0.42);
            lib.invoke_player_loop("b".into());
            let file: Vec<i64> = texte.split(',').filter_map(|s| s.parse().ok()).collect();
            crate::lecteur::enqueue(app, &mut state.borrow_mut(), &file);
        }
        "commandes" => {
            let h = app.global::<crate::Home>();
            h.set_board_mode("clients".into());
            h.invoke_board_mode_changed("clients".into());
        }
        "mobile-bas" => app.set_mobile_scroll(-(id as f32)),
        "stats" => app.global::<crate::Home>().set_stats_tab(id as i32),
        "mobile-neuf" => crate::mobile::showcase(app, false),
        "mobile-pret" => {
            let mut st = state.borrow_mut();
            crate::mobile::showcase(app, true);
            crate::partage::base_landed(app, &mut st, Some(("https://ton-pc.exemple.ts.net".into(), true)));
        }
        // Les sauvegardes : le texte montre où elles sont, et ce dossier de travail n'a rien à faire à l'écran.
        "sauvegardes" => app.global::<crate::CPrefs>().set_backup_text(
            "Une sauvegarde de la base est faite chaque jour. Elle contient tes fiches de sortie, notes, marqueurs, prods, clients et textes (pas tes fichiers audio).".into(),
        ),
        "modele" => {
            let p = app.global::<crate::Pub>();
            p.invoke_select(id as i32);
            p.invoke_set_tab("mine".into());
            p.invoke_mine_new("modele".into());
            p.set_mine_name("Sortie Instagram".into());
            p.set_mine_body("nouveau son : {titre}\n{genre}, {bpm} bpm, en {tonalite}\n\n{lien}\n{hashtags}".into());
            p.invoke_mine_edited();
        }
        "compare" => {
            choisir(2);
            lib.invoke_key("compare".into());
        }
        "selection" => {
            choisir(1);
            lib.invoke_action(texte.into());
        }
        "outil" => {
            choisir(5);
            lib.invoke_action(texte.into());
        }
        "set" => {
            choisir(16);
            lib.invoke_action("set-builder".into());
        }
        "fiche" => {
            crate::projets::open(app, &mut state.borrow_mut(), id);
            let p = app.global::<crate::Proj>();
            p.invoke_set_nature("prod".into());
            p.invoke_choose("client".into(), 1);
            p.invoke_choose("order_status".into(), 3);
            p.invoke_toggle("deadline".into());
            p.invoke_version_add();
        }
        "clients" => crate::projets::clients_show(app, &mut state.borrow_mut(), None),
        "suno" => {
            let s = app.global::<crate::SunoUi>();
            if let Some(p) = s.get_prompts().row_data(0) {
                s.invoke_load(p.id);
            }
        }
        // Le volet de l'Atelier Suno (« templates », « versions », « works ») ; le bandeau des téléchargements qui
        // attendent dans le dossier surveillé de la base ouverte.
        "suno-volet" => app.global::<crate::SunoUi>().invoke_panel_open(texte.into()),
        "suno-attente" => {
            let mut st = state.borrow_mut();
            let found = st.career.as_ref().map(|c| otoko_library::career::suno::pending_downloads(c.conn(), std::time::SystemTime::now())).unwrap_or_default();
            crate::suno::downloads_found(app, &mut st, found);
        }
        "sortie" => crate::sorties::open(app, &mut state.borrow_mut(), id),
        "planning" => crate::sorties::planning(app, &mut state.borrow_mut()),
        "publier" => {
            let p = app.global::<crate::Pub>();
            p.invoke_select(id as i32);
            p.invoke_check();
            if !texte.is_empty() {
                p.invoke_set_tab(texte.into());
            }
        }
        // Le son donné se masterise pour de vrai, et le master reste à côté de lui : un dossier de travail, jamais ta musique.
        "mastering" => {
            let mast = app.global::<crate::Mast>();
            crate::mastrage::add(app, &mut state.borrow_mut(), vec![(std::path::PathBuf::from(texte), None)]);
            if state.borrow().mast.settings.get("use_reference").and_then(|v| v.as_bool()) == Some(true) {
                mast.invoke_param_toggle("use_reference".into());
            }
            mast.invoke_param_clear("out_dir".into());
            mast.invoke_set_preset("Essai rapide".into());
            mast.invoke_launch();
            mast.invoke_select(0);
        }
        "mastering-album" => crate::mastrage::add_release(app, &mut state.borrow_mut(), id),
        "mastering-avance" => app.global::<crate::Mast>().invoke_toggle_advanced(),
        "lien" => app.global::<crate::Part>().invoke_new_link(),
        _ => {}
    }
}

/// Installe les modes de mesure demandés ; les minuteries rendues doivent rester en vie.
pub fn install(app: &AppWindow, state: &Rc<RefCell<State>>, args: &[String]) -> Vec<Timer> {
    let value = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let mut timers = Vec::new();
    // Avant d'ouvrir la page : elle ne doit pas interroger ce PC.
    if value("--vitrine").is_some_and(|q| q.contains("mobile-")) {
        crate::mobile::hold_for_showcase();
    }

    // `--showcase-career [onglet]` : la Carrière avec Studio Vault importé, sur une base temporaire.
    if let Some(i) = args.iter().position(|a| a == "--showcase-career") {
        let tab: i32 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(1);
        let tmp = std::env::temp_dir().join("otoko-vitrine-carriere.db");
        let _ = std::fs::remove_file(&tmp);
        {
            let mut st = state.borrow_mut();
            st.career_path = tmp.clone();
            st.career = crate::career::open(&tmp);
        }
        crate::career::fill(app, &mut state.borrow_mut());
        app.invoke_goto(4, tab);
        app.invoke_career_import();
        // Publier : « Concrete Jungle » contrôlé ; Atelier Suno : le premier prompt ouvert ;
        // Sorties : `--showcase-sheet` ouvre la fiche de la sortie à venir, `--showcase-planning` le planning.
        let weak = app.as_weak();
        let t = Timer::default();
        let sheet = args.iter().any(|a| a == "--showcase-sheet");
        let planning = args.iter().any(|a| a == "--showcase-planning");
        // `--showcase-project <nature>` : la fiche projet du premier morceau ; `--showcase-setbuilder`.
        let project = args.iter().position(|a| a == "--showcase-project").and_then(|i| args.get(i + 1)).cloned();
        let setbuilder = args.iter().any(|a| a == "--showcase-setbuilder");
        let advanced = args.iter().any(|a| a == "--showcase-advanced");
        let dialog = args.iter().position(|a| a == "--showcase-dialog").and_then(|i| args.get(i + 1)).cloned();
        let palette_q = args.iter().position(|a| a == "--showcase-palette").and_then(|i| args.get(i + 1)).cloned();
        let prefs_view = args.iter().any(|a| a == "--showcase-prefs");
        let share_view = args.iter().any(|a| a == "--showcase-share");
        let st2 = state.clone();
        t.start(TimerMode::SingleShot, Duration::from_millis(2500), move || {
            let app = weak.unwrap();
            if let Some(n) = &project {
                let lib = app.global::<crate::Lib>();
                lib.invoke_row_clicked(0, false, false);
                lib.invoke_action("project".into());
                app.global::<crate::Proj>().invoke_set_nature(n.as_str().into());
                if n == "prod" {
                    let p = app.global::<crate::Proj>();
                    p.invoke_choose("order_status".into(), 3);
                    p.invoke_toggle("deadline".into());
                    p.invoke_version_add();
                }
            }
            if setbuilder {
                let lib = app.global::<crate::Lib>();
                lib.invoke_row_clicked(0, false, false);
                for i in 1..16 {
                    lib.invoke_row_clicked(i, true, false);
                }
                lib.invoke_action("set-builder".into());
            }
            if tab == 2 && (sheet || planning) {
                let mut st = st2.borrow_mut();
                if planning {
                    crate::sorties::planning(&app, &mut st);
                } else {
                    let next = st.career.as_ref().and_then(|c| otoko_library::career::releases::planning(c.conn(), otoko_library::career::releases::today()).into_iter().find(|p| p.days >= 0)).map(|p| p.id);
                    if let Some(id) = next {
                        crate::sorties::open(&app, &mut st, id);
                    }
                }
            }
            if tab == 3 {
                let id = st2.borrow().career.as_ref().and_then(|c| c.songs("Concrete Jungle", "", 20).ok()).and_then(|s| s.into_iter().find(|s| std::path::Path::new(&s.path).exists()).map(|s| s.id));
                if let Some(id) = id {
                    let p = app.global::<crate::Pub>();
                    p.invoke_select(id as i32);
                    p.invoke_check();
                }
            }
            if tab == 4 {
                let s = app.global::<crate::SunoUi>();
                if let Some(p) = s.get_prompts().row_data(0) {
                    s.invoke_load(p.id);
                }
            }
            // Mastering : un son de synthèse masterisé pour de vrai (dans un dossier temporaire),
            // deux morceaux de la bibliothèque prêts ; `--showcase-advanced` déplie les réglages.
            if tab == crate::mastrage::TAB {
                app.invoke_goto(4, tab);
                let dir = std::env::temp_dir().join("otoko-vitrine-mastering");
                let _ = std::fs::remove_dir_all(&dir);
                let _ = std::fs::create_dir_all(&dir);
                let wav = dir.join("Nuit d'essai.wav");
                synth_wav(&wav, 12);
                let mast = app.global::<crate::Mast>();
                crate::mastrage::add(&app, &mut st2.borrow_mut(), vec![(wav, None)]);
                if st2.borrow().mast.settings.get("use_reference").and_then(|v| v.as_bool()) == Some(true) {
                    mast.invoke_param_toggle("use_reference".into());
                }
                // Le master reste à côté du son d'essai (jamais dans le dossier choisi dans Studio Vault).
                mast.invoke_param_clear("out_dir".into());
                mast.invoke_set_preset("Essai rapide".into());
                mast.invoke_launch();
                let more: Vec<(std::path::PathBuf, Option<i64>)> = st2
                    .borrow()
                    .career
                    .as_ref()
                    .and_then(|c| c.songs("", "", 60).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| std::path::Path::new(&s.path).exists())
                    .take(2)
                    .map(|s| (std::path::PathBuf::from(s.path), Some(s.id)))
                    .collect();
                crate::mastrage::add(&app, &mut st2.borrow_mut(), more);
                mast.invoke_select(0);
                if advanced {
                    mast.invoke_toggle_advanced();
                }
            }
            // `--showcase-dialog <outil>` : un outil de la bibliothèque sur les 5 premiers morceaux
            // (« edit-tags » : le panneau de détails en lot) ; `--showcase-palette <texte>` ; `--showcase-prefs`.
            if let Some(d) = &dialog {
                let lib = app.global::<crate::Lib>();
                lib.invoke_row_clicked(0, false, false);
                for i in 1..5 {
                    lib.invoke_row_clicked(i, true, false);
                }
                lib.invoke_action(d.as_str().into());
            }
            if let Some(q) = &palette_q {
                app.invoke_open_palette();
                let pal = app.global::<crate::Pal>();
                pal.set_query(q.as_str().into());
                pal.invoke_search(q.as_str().into());
            }
            if share_view {
                app.invoke_goto(7, 0);
                app.global::<crate::Part>().invoke_new_link();
            }
            if prefs_view {
                app.invoke_goto(6, 0);
                crate::preferences::show(&app, "prefs");
            }
        });
        timers.push(t);
        // `--showcase-player` : un morceau ouvert dans le lecteur, une boucle, un repère, les détails.
        if args.iter().any(|a| a == "--showcase-player") {
            let weak = app.as_weak();
            let t = Timer::default();
            t.start(TimerMode::SingleShot, Duration::from_millis(3500), move || {
                let app = weak.unwrap();
                let lib = app.global::<crate::Lib>();
                lib.invoke_row_clicked(2, false, false);
                lib.invoke_key("i".into());
                lib.invoke_play(2);
                lib.invoke_player_seek(0.3);
                lib.invoke_player_loop("a".into());
                lib.invoke_player_seek(0.42);
                lib.invoke_player_loop("b".into());
                lib.invoke_player_mark(0.62);
            });
            timers.push(t);
        }
        // `--details-tab N` : un onglet du panneau de détails ; `--player-big` : le lecteur ouvert ; `--pub-tab t` : un
        // onglet de Publier ; `--showcase-album` : la sortie à venir au mastering ; `--showcase-waves` : les mini ondes.
        let details_tab = value("--details-tab").and_then(|s| s.parse::<i32>().ok());
        let pub_tab = value("--pub-tab");
        let big = args.iter().any(|a| a == "--player-big");
        let album = args.iter().any(|a| a == "--showcase-album");
        let waves = args.iter().any(|a| a == "--showcase-waves");
        // `--showcase-link` (avec `--idle`, pas de capture) : relie les titres pour de vrai, sur la base temporaire.
        let link = args.iter().any(|a| a == "--showcase-link");
        if details_tab.is_some() || pub_tab.is_some() || big || album || waves || link {
            let weak = app.as_weak();
            let st2 = state.clone();
            let t = Timer::default();
            t.start(TimerMode::SingleShot, Duration::from_millis(3800), move || {
                let app = weak.unwrap();
                let lib = app.global::<crate::Lib>();
                if let Some(k) = details_tab {
                    lib.invoke_row_clicked(2, false, false);
                    if !lib.get_details_open() {
                        lib.invoke_key("i".into());
                    }
                    lib.invoke_details_tab_changed(k);
                }
                if big {
                    lib.set_player_big(true);
                }
                if let Some(t) = &pub_tab {
                    let p = app.global::<crate::Pub>();
                    if t == "mine" {
                        p.invoke_set_tab("mine".into());
                        p.invoke_mine_new("modele".into());
                        p.set_mine_name("Sortie Instagram".into());
                        p.set_mine_body("nouveau son : {titre}\n{genre}, {bpm} bpm, en {tonalite}\n\n{lien}\n{hashtags}".into());
                        p.invoke_mine_edited();
                    } else {
                        p.invoke_set_tab(t.as_str().into());
                    }
                }
                if album {
                    let mut st = st2.borrow_mut();
                    let next = st.career.as_ref().and_then(|c| otoko_library::career::releases::planning(c.conn(), otoko_library::career::releases::today()).into_iter().find(|p| p.days >= 0)).map(|p| p.id);
                    if let Some(id) = next {
                        crate::mastrage::add_release(&app, &mut st, id);
                    }
                }
                if waves {
                    crate::fond::waves(&mut st2.borrow_mut());
                }
                if link {
                    crate::fond::link(&app, &mut st2.borrow_mut(), None);
                }
            });
            timers.push(t);
        }
    }
    // `--theme clair`, `--space N` et `--page N` (une page de ta carrière) : captures d'écran.
    if value("--theme").as_deref() == Some("clair") {
        app.set_theme_mode(2);
        app.global::<crate::Theme>().set_dark(false);
    }
    if let Some(s) = value("--space").and_then(|s| s.parse::<i32>().ok()) {
        app.invoke_goto(s, 0);
    }
    if let Some(n) = value("--page").and_then(|s| s.parse::<i32>().ok()) {
        app.invoke_goto(4, n);
    }
    // `--prefs partie` : une partie des Réglages ; `--scroll N` : défilée de N pixels (captures).
    if let Some(p) = value("--prefs") {
        app.set_prefs_part(p.as_str().into());
    }
    if let Some(y) = value("--scroll").and_then(|s| s.parse::<f32>().ok()) {
        app.set_prefs_scroll(-y);
    }
    // `--remote-test adresse [--remote-code code]` : le bouton Tester des Réglages, pour voir sa réponse.
    if let Some(addr) = value("--remote-test") {
        let code = value("--remote-code").unwrap_or_default();
        let weak = app.as_weak();
        let st2 = state.clone();
        let t = Timer::default();
        t.start(TimerMode::SingleShot, Duration::from_millis(1500), move || {
            let app = weak.unwrap();
            app.global::<crate::CPrefs>().set_remote_address(addr.as_str().into());
            app.global::<crate::CPrefs>().set_remote_code(code.as_str().into());
            crate::distant::try_address(&app, &mut st2.borrow_mut(), false);
        });
        timers.push(t);
    }
    // `--joue ID` : ce morceau dans le lecteur (`--joue-grand` : lecteur ouvert) ; `--palette-texte t` : la palette ouverte,
    // ce texte cherché. Pour les captures de la vidéo de présentation.
    let joue = value("--joue").and_then(|s| s.parse::<i64>().ok());
    let grand = args.iter().any(|a| a == "--joue-grand");
    let onglet = value("--joue-onglet").and_then(|s| s.parse::<i32>().ok());
    let cherche = value("--palette-texte");
    if joue.is_some() || cherche.is_some() {
        let weak = app.as_weak();
        let st2 = state.clone();
        let t = Timer::default();
        t.start(TimerMode::SingleShot, Duration::from_millis(1500), move || {
            let app = weak.unwrap();
            let mut st = st2.borrow_mut();
            if let Some(id) = joue {
                crate::bibliotheque::reveal(&app, &mut st, id);
                crate::lecteur::play_id(&app, &mut st, id, 32.0);
                if grand {
                    app.global::<crate::Lib>().set_player_big(true);
                }
                if let Some(k) = onglet {
                    app.global::<crate::Lib>().invoke_details_tab_changed(k);
                }
            }
            if let Some(q) = &cherche {
                crate::palette::open(&app, &mut st);
                app.global::<crate::Pal>().set_query(q.as_str().into());
                crate::palette::search(&app, &mut st, q);
            }
        });
        timers.push(t);
    }
    // Essai de bout en bout de la restauration, sans la fenêtre de confirmation. Il faut aussi la variable
    // d'environnement OTOKO_ESSAI_BASE : personne ne peut le déclencher par erreur.
    if let (Some(quoi), Some(_)) = (value("--essai-base"), std::env::var_os("OTOKO_ESSAI_BASE")) {
        let weak = app.as_weak();
        let st2 = state.clone();
        let t = Timer::default();
        t.start(TimerMode::SingleShot, Duration::from_millis(2000), move || {
            let app = weak.unwrap();
            let mut st = st2.borrow_mut();
            if quoi == "vider" {
                crate::preferences::reset_now(&app, &mut st);
            } else {
                crate::preferences::restore_now(&app, &mut st, std::path::Path::new(&quoi));
            }
        });
        timers.push(t);
    }
    // `--vitrine quoi` : un écran de la vidéo de présentation, sur la base ouverte. Les `--showcase-*` importent
    // Studio Vault, celui-ci ne montre que ce que la base de démonstration contient.
    if let Some(quoi) = value("--vitrine") {
        let texte = value("--vitrine-texte").unwrap_or_default();
        let id = value("--vitrine-id").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
        let weak = app.as_weak();
        let st2 = state.clone();
        let t = Timer::default();
        // Plusieurs états à la suite : « mobile-pret+lien ».
        t.start(TimerMode::SingleShot, Duration::from_millis(2500), move || quoi.split('+').for_each(|q| vitrine(&weak.unwrap(), &st2, q, &texte, id)));
        timers.push(t);
    }
    // `--tabs N` : N fois la touche Tab, pour voir l'anneau de la commande atteinte au clavier.
    if let Some(n) = value("--tabs").and_then(|s| s.parse::<usize>().ok()) {
        let weak = app.as_weak();
        let t = Timer::default();
        t.start(TimerMode::SingleShot, Duration::from_millis(4200), move || {
            let app = weak.unwrap();
            for _ in 0..n {
                let text: slint::SharedString = slint::platform::Key::Tab.into();
                app.window().dispatch_event(slint::platform::WindowEvent::KeyPressed { text: text.clone() });
                app.window().dispatch_event(slint::platform::WindowEvent::KeyReleased { text });
            }
        });
        timers.push(t);
    }

    // `--taille LxH` : la fenêtre à cette taille logique (captures à plusieurs tailles d'écran).
    if let Some((w, h)) = value("--taille").and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?)))) {
        app.window().set_size(slint::LogicalSize::new(w, h));
    }
    // `--hors-ecran` : la fenêtre de capture ne passe pas devant ce que tu fais.
    if args.iter().any(|a| a == "--hors-ecran") {
        app.window().set_position(slint::PhysicalPosition::new(-6000, -6000));
    }

    if let Some(n) = value("--idle").and_then(|s| s.parse::<u64>().ok()) {
        // Hors de l'écran : la souris de l'utilisateur ne déclenche pas de redessin pendant la mesure.
        app.window().set_position(slint::PhysicalPosition::new(-4000, -4000));
        let start = Rc::new(RefCell::new((Duration::ZERO, Instant::now())));
        let probe = Timer::default();
        {
            let start = start.clone();
            probe.start(TimerMode::SingleShot, Duration::from_secs(4), move || *start.borrow_mut() = (cpu_time(), Instant::now()));
        }
        let done = Timer::default();
        let state = state.clone();
        done.start(TimerMode::SingleShot, Duration::from_secs(n + 4), move || {
            let (c0, w0) = *start.borrow();
            let cpu = (cpu_time() - c0).as_secs_f64() / w0.elapsed().as_secs_f64() * 100.0;
            let (ws, private) = memory_mb();
            println!("Repos : processeur {cpu:.2} % d'un cœur, mémoire {ws:.1} Mo (privée {private:.1} Mo) · carte son {}", if state.borrow().ecoute.ouverte() { "ouverte" } else { "fermée" });
            let _ = slint::quit_event_loop();
        });
        timers.push(probe);
        timers.push(done);
    }

    if let Some(path) = value("--snapshot") {
        let weak = app.as_weak();
        let delay = value("--snapshot-delay").and_then(|s| s.parse().ok()).unwrap_or(1500);
        let t = Timer::default();
        t.start(TimerMode::SingleShot, Duration::from_millis(delay), move || {
            match weak.unwrap().window().take_snapshot() {
                Ok(img) => {
                    write_bmp(&img, &path);
                    println!("capture : {path}");
                }
                Err(e) => println!("capture impossible : {e}"),
            }
            let _ = slint::quit_event_loop();
        });
        timers.push(t);
    }

    if args.iter().any(|a| a == "--selftest") {
        timers.push(selftest(app, state));
    }
    timers
}

/// Atelier Suno, de bout en bout, sur la base d'essai ouverte (avec ou sans Studio Vault importé).
fn essai_suno(app: &AppWindow, state: &Rc<RefCell<State>>, check: &mut dyn FnMut(bool, &str) -> u32) {
    // Atelier Suno v6 : les prompts de Studio Vault, un prompt composé, vérifié, corrigé, gardé, copié ;
    // puis l'import d'un fichier Suno.
    app.invoke_goto(4, 4);
    let s = app.global::<crate::SunoUi>();
    let prompts = s.get_prompts().row_count();
    s.invoke_new();
    s.invoke_chip("genre".into(), "uk garage".into());
    s.invoke_edited("mood".into(), "late night drive, sad, 120 BPM".into());
    check(s.get_issues().iter().any(|i| i.fixable), "le vérificateur signale le tempo dans le style");
    s.invoke_set_tag_tab("structure".into());
    let tag = s.get_tags().row_data(0).map(|t| t.to_string()).unwrap_or_default();
    s.invoke_tag(tag.as_str().into());
    s.invoke_fix_all();
    let style = s.get_style().to_string();
    check(
        style.starts_with("uk garage") && !style.contains("BPM") && !s.get_any_fixable() && s.get_lyrics().contains(tag.as_str()),
        &format!("composer, ajouter un tag, tout corriger : « {style} »"),
    );
    s.invoke_preset(0);
    s.invoke_save();
    s.invoke_copy("all".into());
    let kept = state.borrow().suno.prompt.id;
    let stored = state.borrow().career.as_ref().and_then(|c| otoko_library::career::suno::get(c.conn(), kept));
    check(
        s.get_prompts().row_count() == prompts + 1 && stored.is_some_and(|p| p.weirdness == 30 && p.genre.starts_with("uk garage")),
        &format!("garder le prompt, préréglage compris ({prompts} importés de Studio Vault)"),
    );
    // Import : un fichier « téléchargé de Suno », copié et renommé dans un dossier d'essai.
    let tmp = std::env::temp_dir().join(format!("otoko-autotest-suno-{}", std::process::id()));
    let _ = std::fs::create_dir_all(tmp.join("in"));
    let src = tmp.join("in").join("3f9c2a1e-suno.wav");
    {
        let spec = hound::WavSpec { channels: 2, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        if let Ok(mut w) = hound::WavWriter::create(&src, spec) {
            for i in 0..44_100 {
                let v = ((i as f32 * 0.06).sin() * 8000.0) as i16;
                let _ = w.write_sample(v);
                let _ = w.write_sample(v);
            }
            let _ = w.finalize();
        }
    }
    if let Some(c) = state.borrow().career.as_ref() {
        let _ = c.set_setting("suno_import_folder", &tmp.join("out").to_string_lossy());
    }
    {
        let mut st = state.borrow_mut();
        crate::suno::import_open(app, &mut st, vec![src.clone()]);
    }
    s.set_imp_title("Nuit d'essai".into());
    s.set_imp_version("v2".into());
    s.set_imp_tags("garage".into());
    s.set_imp_prompt("uk garage, late night drive".into());
    s.invoke_import_edited();
    s.invoke_import_toggle("copy".into());
    s.invoke_import_run();
    let dst = tmp.join("out").join("Nuit d'essai - v2.wav");
    let row = state.borrow().career.as_ref().and_then(|c| {
        c.conn()
            .query_row(
                "SELECT t.role || '|' || IFNULL(t.version_label, '') || '|' || IFNULL(t.suno_prompt, '') || '|' || (SELECT COUNT(*) FROM track_tags tt JOIN tags g ON g.id = tt.tag_id WHERE tt.track_id = t.id AND g.name IN ('suno', 'garage'))
                 FROM tracks t WHERE t.path = ?1",
                [dst.to_string_lossy()],
                |r| r.get::<_, String>(0),
            )
            .ok()
    });
    check(src.exists() && dst.exists() && row.as_deref() == Some("suno_generation|v2|uk garage, late night drive|2"), &format!("import Suno : copié, renommé, indexé ({})", row.unwrap_or_default()));
    // Deux téléchargements arrivent dans le dossier surveillé : proposés, puis rangés d'un clic, reliés
    // au dernier prompt copié, en v1 et v2.
    s.invoke_import_toggle("copy".into());
    let dl = tmp.join("telechargements");
    let _ = std::fs::create_dir_all(&dl);
    {
        let mut st = state.borrow_mut();
        crate::suno::set_watch(app, &mut st, Some(dl.clone()));
    }
    for name in ["Nuit claire.wav", "Nuit claire (1).wav"] {
        synth_wav(&dl.join(name), 1);
    }
    {
        let mut st = state.borrow_mut();
        let later = std::time::SystemTime::now() + Duration::from_secs(60);
        let found = st.career.as_ref().map(|c| otoko_library::career::suno::pending_downloads(c.conn(), later)).unwrap_or_default();
        crate::suno::downloads_found(app, &mut st, found);
    }
    let waiting = s.get_waiting();
    s.invoke_waiting_store();
    let (v1, v2) = (tmp.join("out").join("Nuit claire - v1.wav"), tmp.join("out").join("Nuit claire - v2.wav"));
    let linked = state.borrow().career.as_ref().and_then(|c| {
        c.conn().query_row("SELECT COUNT(*) FROM tracks WHERE prompt_id = ?1 AND title = 'Nuit claire' AND prompt_version_no IS NOT NULL AND IFNULL(suno_prompt, '') <> ''", [kept], |r| r.get::<_, i64>(0)).ok()
    });
    check(
        waiting == 2 && s.get_waiting() == 0 && v1.exists() && v2.exists() && !dl.join("Nuit claire.wav").exists() && linked == Some(2),
        &format!("téléchargements Suno : {waiting} proposés, rangés d'un clic en v1 et v2, reliés au dernier prompt copié ({linked:?})"),
    );
    // Ce qui marche : un morceau gardé, l'autre rejeté.
    if let Some(c) = state.borrow().career.as_ref() {
        let _ = c.conn().execute("UPDATE tracks SET pick = 1 WHERE path = ?1", [v1.to_string_lossy()]);
        let _ = c.conn().execute("UPDATE tracks SET pick = -1 WHERE path = ?1", [v2.to_string_lossy()]);
    }
    s.invoke_panel_open("works".into());
    let works: Vec<String> = s.get_panel_rows().iter().filter(|r| !r.head).map(|r| format!("{} {}", r.title, r.right)).collect();
    check(works.iter().any(|w| w.ends_with("50 %")) && works.iter().any(|w| w.starts_with("uk garage")), &format!("ce qui marche : {} ligne(s), le prompt et ses mots à 50 %", works.len()));
    s.invoke_panel_close();
    // Les versions : le prompt change, l'ancienne version se reprend.
    s.invoke_load(kept as i32);
    s.invoke_edited("mood".into(), "foggy harbour at dawn".into());
    s.invoke_save();
    s.invoke_panel_open("versions".into());
    let versions = s.get_panel_rows().row_count();
    s.invoke_panel_pick(versions as i32 - 1);
    check(versions >= 2 && s.get_style() == style.as_str() && s.get_panel() == "", &format!("versions : {versions} gardées, la première reprise (« {} »)", s.get_style()));
    // Un modèle de départ fourni passe le vérificateur ; le prompt ouvert se garde comme modèle.
    s.invoke_panel_open("templates".into());
    s.invoke_panel_extra();
    let mine = s.get_panel_rows().iter().filter(|r| r.removable).count();
    let first = s.get_panel_rows().iter().position(|r| !r.head && !r.removable).unwrap_or(0);
    s.invoke_panel_pick(first as i32);
    check(
        mine == 1 && state.borrow().suno.prompt.id == 0 && !s.get_style().is_empty() && s.get_issues().row_count() == 0,
        &format!("modèles de départ : le mien gardé, un modèle fourni ouvert sans piège (« {} »)", s.get_style()),
    );
    // Depuis un morceau : une variation de son prompt, pas encore enregistrée.
    let tid = state.borrow().career.as_ref().and_then(|c| c.conn().query_row("SELECT id FROM tracks WHERE path = ?1", [v1.to_string_lossy()], |r| r.get::<_, i64>(0)).ok()).unwrap_or(0);
    {
        let mut st = state.borrow_mut();
        crate::suno::from_track(app, &mut st, tid, true);
    }
    check(s.get_name().ends_with("(variation)") && state.borrow().suno.prompt.parent_id == Some(kept), &format!("variation depuis un morceau : « {} »", s.get_name()));
    {
        let mut st = state.borrow_mut();
        crate::suno::set_watch(app, &mut st, None);
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Pilote l'interface comme un utilisateur et vérifie chaque effet sur la base et le lecteur.
fn selftest(app: &AppWindow, state: &Rc<RefCell<State>>) -> Timer {
    let weak = app.as_weak();
    let state = state.clone();
    let mut step = 0u32;
    let mut failures = 0u32;
    let mut vault_files: Vec<(std::ffi::OsString, u64, std::time::SystemTime)> = Vec::new();
    let mut reimport = false;
    // Le réimport peut prendre du temps si le PC est occupé : on l'attend, sans dépasser une minute.
    let mut reimport_wait = 0u32;
    // Mastering : le dossier du moteur repris de Studio Vault, la Carrière d'essai, l'étape.
    let mut mast_engine = String::new();
    let mut mast_tmp: Option<std::path::PathBuf> = None;
    let mut mast_planned = std::path::PathBuf::new();
    let mut check = move |ok: bool, what: &str| {
        println!("  {} {what}", if ok { "ok  " } else { "ÉCHEC" });
        if !ok {
            failures += 1;
        }
        failures
    };
    let t = Timer::default();
    t.start(TimerMode::Repeated, Duration::from_millis(700), move || {
        let app = weak.unwrap();
        step += 1;
        match step {
            1 => step = 249,
            250 => {
                // Une base temporaire (la vraie reste à l'utilisateur), vide au départ.
                let tmp = std::env::temp_dir().join(format!("otoko-autotest-carriere-{}.db", std::process::id()));
                let _ = std::fs::remove_file(&tmp);
                {
                    let mut st = state.borrow_mut();
                    st.career_path = tmp.clone();
                    st.career = crate::career::open(&tmp);
                    crate::career::fill(&app, &mut st);
                }
                app.invoke_goto(4, 0);
                check(app.global::<crate::Home>().get_empty(), "une base neuve est vide");
                if app.get_career_vault() {
                    let before: Vec<_> = std::fs::read_dir(otoko_library::vault::default_dir().unwrap()).into_iter().flatten().flatten().filter_map(|e| Some((e.file_name(), e.metadata().ok()?.len(), e.metadata().ok()?.modified().ok()?))).collect();
                    vault_files = before;
                    app.invoke_career_import();
                } else {
                    check(true, "pas de Studio Vault sur ce PC : import sauté");
                    // L'Atelier Suno n'a besoin d'aucune donnée : il est essayé quand même.
                    essai_suno(&app, &state, &mut check);
                    app.invoke_goto(4, 0);
                    step = 262;
                }
            }
            251..=262 => {
                let n = state.borrow().career.as_ref().and_then(|c| c.overview(&crate::career::today()).ok()).map(|o| o.songs as i32).unwrap_or(0);
                if n > 0 {
                    check(app.get_career_text().contains("importé"), &format!("Studio Vault importé ({})", app.get_career_text()));
                    let after: Vec<_> = std::fs::read_dir(otoko_library::vault::default_dir().unwrap()).into_iter().flatten().flatten().filter_map(|e| Some((e.file_name(), e.metadata().ok()?.len(), e.metadata().ok()?.modified().ok()?))).collect();
                    check(after == vault_files, "la base Studio Vault n'a pas bougé (fichiers, tailles, dates)");
                    // Bibliothèque : navigation, pastilles, recherche, sélection, archives, vues, fiche.
                    app.invoke_goto(4, 1);
                    let lib = app.global::<crate::Lib>();
                    let all = lib.get_rows().row_count();
                    let nav: Vec<String> = lib.get_nav().iter().filter(|n| !n.header).map(|n| n.label.to_string()).collect();
                    check(all > 400 && nav.iter().any(|n| n == "Tous les morceaux") && nav.iter().any(|n| n == "Archives"), &format!("bibliothèque : {all} morceaux, {} entrées de navigation", nav.len()));
                    lib.invoke_set_chip("status".into(), "released".into());
                    let released = state.borrow().lib.rows.clone();
                    check(!released.is_empty() && released.iter().all(|r| r.status == "released"), &format!("pastille « Statut : Publié » ({})", released.len()));
                    lib.invoke_reset();
                    lib.invoke_search("camelot:8A bpm:120-130".into());
                    let found = state.borrow().lib.rows.clone();
                    check(!found.is_empty() && found.iter().all(|r| r.key == "Am" && r.bpm.is_some_and(|b| (120.0..=130.0).contains(&b))), &format!("recherche « camelot:8A bpm:120-130 » ({})", found.len()));
                    lib.invoke_sort("bpm".into());
                    lib.invoke_sort("bpm".into());
                    let bpms: Vec<f64> = state.borrow().lib.rows.iter().filter_map(|r| r.bpm).collect();
                    check(bpms.windows(2).all(|w| w[0] >= w[1]), "trier par tempo, décroissant");
                    lib.invoke_save_view("Autotest 8A".into());
                    {
                        let vue: Vec<String> = lib.get_nav().iter().filter(|n| n.label.contains("Autotest")).map(|n| format!("{} = {}", n.label, n.count)).collect();
                        check(lib.get_nav().iter().any(|n| n.label == "Autotest 8A" && n.count as usize == found.len()), &format!("enregistrer la vue (« Mes vues » : {vue:?} pour {} morceaux ; {})", found.len(), app.get_message()));
                    }
                    lib.invoke_reset();
                    lib.invoke_search("Concrete".into());
                    check(state.borrow().lib.rows.iter().any(|r| r.title.contains("Concrete")), "chercher « Concrete »");
                    lib.invoke_row_clicked(0, false, false);
                    let id = state.borrow().lib.rows[0].id;
                    let before = state.borrow().lib.rows[0].pick;
                    lib.invoke_key(if before == 1 { "x".into() } else { "p".into() });
                    check(state.borrow().lib.rows[0].pick == if before == 1 { -1 } else { 1 }, &format!("garder ou rejeter un morceau (P / X) : {before} → {} ; {}", state.borrow().lib.rows[0].pick, app.get_message()));
                    lib.invoke_key("i".into());
                    lib.invoke_field_edited("genre".into(), "UK Garage autotest".into());
                    let genre = state.borrow().career.as_ref().map(|c| otoko_library::career::library::field(c.conn(), id, "genre")).unwrap_or_default();
                    check(lib.get_details_open() && genre == "UK Garage autotest", "modifier le genre dans le panneau de détails");
                    lib.invoke_action("archive".into());
                    let gone = !state.borrow().lib.rows.iter().any(|r| r.id == id);
                    check(gone && lib.get_nav().iter().any(|n| n.label == "Archives" && n.count > 0), "archiver : le morceau passe dans « Archives »");
                    // Outils : des aperçus (l'autotest ne touche jamais aux fichiers).
                    lib.invoke_reset();
                    lib.invoke_row_clicked(0, false, false);
                    lib.invoke_action("rename".into());
                    let preview = lib.get_dialog_items().row_data(0).map(|i| i.sub.to_string()).unwrap_or_default();
                    check(lib.get_dialog() == "rename" && (preview.starts_with('→') || preview == "inchangé"), &format!("renommer : aperçu « {preview} »"));
                    lib.invoke_dialog_button("close".into());
                    lib.invoke_action("complete".into());
                    check(lib.get_dialog() == "complete", &format!("compléter genres et artistes : {} propositions", lib.get_dialog_items().row_count()));
                    lib.invoke_dialog_button("close".into());
                    lib.invoke_action("dupes".into());
                    check(lib.get_dialog() == "dupes", &format!("doublons : {}", lib.get_dialog_title()));
                    lib.invoke_dialog_button("close".into());
                    let (main, title, other) = {
                        let st = state.borrow();
                        (st.lib.rows[0].id, st.lib.rows[0].title.clone(), st.lib.rows[1].id)
                    };
                    lib.invoke_row_clicked(1, false, false);
                    lib.invoke_action("attach".into());
                    lib.invoke_dialog_input_edited(title.as_str().into());
                    let k = lib.get_dialog_items().iter().position(|i| i.id as i64 == main);
                    if let Some(k) = k {
                        lib.invoke_dialog_toggle(k as i32);
                        lib.invoke_dialog_button("apply".into());
                    }
                    let parent = state.borrow().career.as_ref().map(|c| otoko_library::career::library::field(c.conn(), other, "parent_id")).unwrap_or_default();
                    check(k.is_some() && parent == main.to_string(), &format!("rattacher une version à « {title} »"));
                    lib.invoke_action("detach".into());
                    let parent = state.borrow().career.as_ref().map(|c| otoko_library::career::library::field(c.conn(), other, "parent_id")).unwrap_or_default();
                    check(parent.is_empty(), "la détacher");
                    // Lecteur : un vrai morceau joué (en silence), décodé au fil de l'eau ; la carte n'est ouverte qu'à ce moment.
                    lib.invoke_reset();
                    let playable: Vec<(i64, Option<f64>)> = state.borrow().lib.rows.iter().filter(|r| !r.missing && std::path::Path::new(&r.path).exists()).take(2).map(|r| (r.id, r.lufs)).collect();
                    if playable.len() == 2 {
                        let i0 = state.borrow().lib.rows.iter().position(|r| r.id == playable[0].0).unwrap_or(0);
                        check(!state.borrow().ecoute.ouverte(), "au repos, la carte son est fermée");
                        lib.invoke_play(i0 as i32);
                        check(state.borrow().ecoute.ouverte(), &format!("lecture : la carte son s'ouvre ({})", app.get_message()));
                        std::thread::sleep(Duration::from_millis(700));
                        crate::lecteur::tick(&app, &mut state.borrow_mut());
                        let pos = state.borrow().player.pos;
                        check(lib.get_player_open() && pos > 0.2, &format!("lecteur : « {} » joue ({:.1} s)", lib.get_player_title(), pos));
                        lib.invoke_player_seek(0.5);
                        std::thread::sleep(Duration::from_millis(400));
                        crate::lecteur::tick(&app, &mut state.borrow_mut());
                        let (pos, dur) = (state.borrow().player.pos, state.borrow().player.duration);
                        check((pos - dur / 2.0).abs() < 3.0, &format!("aller au milieu ({pos:.1} s sur {dur:.0})"));
                        lib.invoke_player_toggle();
                        check(lib.get_player_paused(), "pause");
                        lib.invoke_player_toggle();
                        lib.invoke_player_mark(0.25);
                        let marks = state.borrow().career.as_ref().map(|c| otoko_library::career::library::markers(c.conn(), playable[0].0).len()).unwrap_or(0);
                        check(lib.get_player_markers().row_count() > 0 && marks > 0, "repère posé sur la forme d'onde, gardé avec le morceau");
                        lib.invoke_player_unmark(lib.get_player_markers().row_count() as i32 - 1);
                        lib.invoke_player_loop("a".into());
                        lib.invoke_player_loop("clear".into());
                        // Comparateur : les deux morceaux à sonie égale.
                        state.borrow_mut().lib.selected = playable.iter().map(|p| p.0).collect();
                        lib.invoke_key("compare".into());
                        let gains = state.borrow().player.compare.as_ref().map(|c| (c.a.1, c.b.1));
                        lib.invoke_player_switch();
                        let on_b = state.borrow().player.compare.as_ref().is_some_and(|c| c.on_b) && state.borrow().player.id == Some(playable[1].0);
                        check(on_b && gains.is_some_and(|(a, b)| a <= 1.0 && b <= 1.0 && (a == 1.0 || b == 1.0)), &format!("comparer A/B à sonie égale (gains {gains:?})"));
                        // File, puis morceau suivant.
                        state.borrow_mut().lib.selected = [playable[0].0].into_iter().collect();
                        lib.invoke_key("q".into());
                        lib.invoke_player_next();
                        check(state.borrow().player.id == Some(playable[0].0) && state.borrow().player.queue.is_empty(), "la file de lecture passe en premier");
                        // Compatibles et roue de Camelot.
                        lib.invoke_action("compatible".into());
                        let n = lib.get_nav().iter().find(|n| n.label.starts_with("Compatibles avec")).map(|n| n.count);
                        check(n.is_some(), &format!("morceaux compatibles : {}", n.unwrap_or(0)));
                        lib.invoke_action("wheel".into());
                        check(lib.get_wheel_open() && lib.get_wheel().row_count() == 24 && lib.get_wheel().iter().any(|w| w.current), "roue de Camelot (24 cases, la tonalité du morceau marquée)");
                        lib.invoke_action("wheel-close".into());
                        lib.invoke_player_stop();
                        check(!lib.get_player_open() && state.borrow().player.id.is_none() && !state.borrow().ecoute.ouverte(), "fermer le lecteur rend la carte son");
                    }
                    // Analyse audio d'un vrai morceau (lu seulement), écrite dans la Carrière d'essai.
                    let target = state.borrow().lib.rows.iter().find(|r| !r.missing && std::path::Path::new(&r.path).exists() && r.duration.is_some_and(|d| d < 400.0)).map(|r| (r.id, r.path.clone()));
                    if let Some((id, path)) = target {
                        let t = Instant::now();
                        match crate::analyse::analyze(&path) {
                            Ok(a) => {
                                let saved = state.borrow().career.as_ref().map(|c| otoko_library::career::library::save_analysis(c.conn(), id, &a).is_ok()).unwrap_or(false);
                                let mood = state.borrow().career.as_ref().map(|c| otoko_library::career::library::field(c.conn(), id, "mood_auto")).unwrap_or_default();
                                check(
                                    saved && a.bpm.is_some() && a.key.is_some() && a.lufs.is_some() && !mood.is_empty(),
                                    &format!(
                                        "analyse en {:.1} s : {} BPM, {} ({}), {} LUFS, énergie {}, ambiance « {mood} »",
                                        t.elapsed().as_secs_f64(),
                                        a.bpm.unwrap_or(0.0),
                                        a.key.clone().unwrap_or_default(),
                                        a.mode.clone().unwrap_or_default(),
                                        a.lufs.unwrap_or(0.0),
                                        a.energy.unwrap_or(0.0)
                                    ),
                                );
                            }
                            Err(e) => {
                                check(false, &format!("analyse impossible : {e}"));
                            }
                        }
                    }
                    lib.invoke_reset();
                    {
                            let n = state.borrow().career.as_ref().and_then(|c| c.releases().ok()).map(|r| r.len()).unwrap_or(0);
                            check(n > 0, &format!("{n} sorties avec leurs morceaux"));
                        }
                    app.invoke_career_import();
                    step = 262;
                    reimport = true;
                } else if step == 262 {
                    check(false, &format!("l'import n'a pas abouti : {}", app.get_career_text()));
                }
            }
            263..=270 => {
                if reimport && step == 266 && reimport_wait < 85 {
                    let answered = app.get_career_text().contains("déjà présents");
                    // Après un import, les dossiers sont relus en fond : on attend la fin, sinon les compteurs
                    // bougeraient pendant les contrôles qui suivent.
                    let mut st = state.borrow_mut();
                    if answered && st.lib.last_scan.is_none() {
                        crate::bibliotheque::scan(&app, &mut st);
                    }
                    if !answered || st.lib.scanning {
                        reimport_wait += 1;
                        step -= 1;
                        return;
                    }
                }
                if reimport {
                    if app.get_career_text().contains("déjà présents") {
                        check(app.get_career_text().contains("0 morceaux"), &format!("réimporter n'ajoute rien ({})", app.get_career_text()));
                        reimport = false;
                        // Publier : un morceau dont le fichier existe (pas forcément « Concrete Jungle »,
                        // que la bibliothèque a archivé plus haut).
                        let song = {
                            let st = state.borrow();
                            st.career.as_ref().and_then(|c| {
                                let pool = c.songs("Concrete Jungle", "", 20).ok().unwrap_or_default().into_iter().chain(c.songs("", "", 500).ok().unwrap_or_default());
                                pool.into_iter().find(|s| s.nature == "original" && std::path::Path::new(&s.path).exists())
                            })
                        };
                        match song {
                            Some(s) => {
                                let p = app.global::<crate::Pub>();
                                {
                                    let mut st = state.borrow_mut();
                                    crate::publish::open(&app, &mut st, s.id);
                                }
                                let sc = p.get_text().to_string();
                                let titre = otoko_library::career::textes::nettoyer_titre(&s.title, &s.filename);
                                check(
                                    app.get_career_tab() == 3 && sc.contains(&format!("« {titre} »")) && !p.get_sc_tags().is_empty() && !p.get_sc_genre().is_empty(),
                                    &format!("publier « {titre} » : description SoundCloud ({} caractères), genre « {} », tags", sc.chars().count(), p.get_sc_genre()),
                                );
                                p.invoke_set_platform("instagram".into());
                                let ig = p.get_text().to_string();
                                p.invoke_variant();
                                check(ig.contains("#") && ig.contains("— ") && p.get_text() != ig, "Instagram : hashtags, signature, autre variante");
                                p.invoke_set_platform("story".into());
                                check(p.get_text().chars().count() <= 160, &format!("story : {}", p.get_text()));
                                let t = Instant::now();
                                let r = crate::publish::analyze(&s.path, &s);
                                let lufs = r.as_ref().ok().and_then(|c| c.iter().find(|x| x.text.contains("LUFS")).map(|x| x.text.clone())).unwrap_or_default();
                                check(r.as_ref().is_ok_and(|c| c.iter().any(|x| x.text.contains("dBTP"))) && !lufs.is_empty(), &format!("contrôle avant publication en {} ms : {lufs}", t.elapsed().as_millis()));
                                // Historique : enregistré, puis comme publié ; « Envoyer… » (sans navigateur pendant l'autotest) le garde aussi.
                                p.invoke_set_platform("soundcloud".into());
                                p.invoke_save(false);
                                p.invoke_save(true);
                                p.invoke_send();
                                p.invoke_set_tab("history".into());
                                p.invoke_hist_scope(true);
                                let rows = p.get_history().row_count();
                                let posted = p.get_history().iter().filter(|h| h.posted).count();
                                p.invoke_hist_select(0);
                                check(rows >= 3 && posted >= 1 && !p.get_hist_text().is_empty(), &format!("historique du morceau : {rows} textes, {posted} publié(s)"));
                                // Univers de marque : la ville entre dans les tags SoundCloud.
                                p.invoke_brand_edited("city".into(), "Villetest".into());
                                p.invoke_set_tab("write".into());
                                p.invoke_brand_save();
                                check(p.get_sc_tags().contains("Villetest"), &format!("univers de marque → tags : {}", p.get_sc_tags()));
                                p.set_link("https://soundcloud.com/otoko/autotest".into());
                                p.invoke_published();
                                check(p.get_meta().contains("publié le"), &format!("marquer publié ({})", p.get_meta()));
                            }
                            None => {
                                check(true, "aucun fichier de morceau sur ce PC : publication sautée");
                            }
                        }
                        essai_suno(&app, &state, &mut check);
                        // Statistiques.
                        app.invoke_goto(4, 5);
                        check(!app.get_stats_facts().is_empty(), &format!("statistiques : {}", app.get_stats_facts()));
                        pages_speed(&app, &state, &mut check);
                        career_home_checks(&app, &state, &mut check);
                        tools_checks(&app, &state, &mut check);
                        step = 267;
                    } else if step == 270 {
                        check(false, &format!("le réimport n'a pas répondu ({} travaux en cours) : {}", state.borrow().pending, app.get_career_text()));
                    }
                }
                if step == 268 {
                    partage_start(&app, &mut check);
                }
                if step == 269 {
                    partage_done(&app, &mut check);
                }
                if step == 270 {
                    let tmp = state.borrow().career_path.clone();
                    mast_engine = state.borrow().career.as_ref().and_then(|c| c.setting(otoko_library::career::mastering::ENGINE_KEY)).unwrap_or_default();
                    state.borrow_mut().career = None;
                    // Le serveur de l'app mobile sur cette Carrière (ports libres, cache temporaire).
                    let shares = crate::career::shares_path(&tmp);
                    let mut cfg = otoko_mobile::Config::new(&std::env::temp_dir(), "autotest");
                    cfg.db = tmp.clone();
                    cfg.shares_db = shares.clone();
                    cfg.cache = std::env::temp_dir().join(format!("otoko-autotest-mobile-{}", std::process::id()));
                    cfg.port = 0;
                    cfg.public_port = Some(0);
                    cfg.warm = false;
                    cfg.convert_ahead = false;
                    cfg.apk = None;
                    let cache = cfg.cache.clone();
                    match otoko_mobile::start(cfg) {
                        Ok(server) => {
                            let p = server.port();
                            let ping = otoko_mobile::ask(p, "/api/ping").unwrap_or_default();
                            let list = otoko_mobile::ask(p, "/api/tracks?limit=5").unwrap_or_default();
                            let n = ping["tracks"].as_i64().unwrap_or(0);
                            let expected = if app.get_career_vault() { n > 0 && list.as_array().is_some_and(|l| l.len() == 5) } else { ping["name"] == "Otoko Vault" };
                            check(expected, &format!("serveur mobile : {n} morceaux, {} sorties, page {}", ping["releases"], ping["lan"]));
                            let listed = otoko_mobile::ask(p, "/api/shares").map(|s| s["shares"].as_array().map_or(0, |a| a.len()));
                            check(listed.is_some(), &format!("partages gérés depuis cet ordinateur ({} repris de Studio Vault)", listed.unwrap_or(0)));
                            if let (Some(first), Some(pp)) = (list.get(0).and_then(|t| t["id"].as_i64()), server.public_port()) {
                                let made = otoko_mobile::shares::ShareStore::open(&shares).ok().and_then(|s| s.create(&serde_json::json!({ "kind": "tracks", "items": [first], "label": "Autotest" })).ok());
                                let token = made.as_ref().and_then(|m| m["token"].as_str().map(str::to_string)).unwrap_or_default();
                                let public = otoko_mobile::ask(pp, &format!("/s/{token}/api")).unwrap_or_default();
                                check(public["tracks"][0]["id"] == first && public["label"] == "Autotest", "lien de partage : la page publique montre le morceau, et rien d'autre");
                                check(otoko_mobile::ask(pp, "/api/tracks").is_none(), "le serveur public ne donne pas la bibliothèque");
                            }
                            server.stop();
                        }
                        Err(e) => {
                            check(false, &format!("serveur mobile : {e}"));
                        }
                    }
                    let _ = std::fs::remove_dir_all(&cache);
                    for f in [shares.clone(), shares.with_extension("db-wal"), shares.with_extension("db-shm")] {
                        let _ = std::fs::remove_file(f);
                    }
                    let _ = std::fs::remove_file(&tmp);
                    let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
                    let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
                }
            }
            271 => step = 301,
            302 => {
                // Réglages : les sorties d'écoute, et la carte qui reste libre tant que rien ne joue.
                app.invoke_goto(6, 0);
                check(!state.borrow().ecoute.ouverte(), "Réglages : carte son libre");
            }
            303 => {
                // Thème clair puis sombre (sans toucher aux réglages enregistrés).
                app.global::<crate::Theme>().set_dark(false);
                check(!app.global::<crate::Theme>().get_dark(), "thème clair");
                app.global::<crate::Theme>().set_dark(true);
            }
            304 | 305 => {}
            306 => {
                // Mastering PhaseLimiter, sur une Carrière d'essai et un son de synthèse (rien chez l'utilisateur).
                mast_tmp = mastering_start(&app, &state, &mast_engine, &mut check);
                if mast_tmp.is_none() {
                    step = 345;
                }
            }
            307..=345 => {
                let Some(tmp) = mast_tmp.clone() else { return };
                let (status, err) = state.borrow().mast.jobs.first().map(|j| (j.status, j.error.clone())).unwrap_or((crate::mastrage::Status::Failed, "liste vide".into()));
                use crate::mastrage::Status;
                let finished = match status {
                    Status::Done if mast_planned.as_os_str().is_empty() => {
                        mast_planned = mastering_done(&app, &state, &mut check);
                        false
                    }
                    Status::Cancelled if !mast_planned.as_os_str().is_empty() => {
                        let gone = !mast_planned.exists() && !app.global::<crate::Mast>().get_busy();
                        check(gone, &format!("annuler : arrêté, rien laissé à côté de l'original ({})", app.global::<crate::Mast>().get_jobs().row_data(0).map(|j| j.sub.to_string()).unwrap_or_default()));
                        true
                    }
                    Status::Failed => {
                        check(false, &format!("mastering : {err}"));
                        true
                    }
                    _ if step == 345 => {
                        check(false, &format!("le mastering n'a pas abouti à temps ({})", app.global::<crate::Mast>().get_launch_info()));
                        true
                    }
                    _ => false,
                };
                if finished {
                    mastering_cleanup(&state, &tmp);
                    mast_tmp = None;
                    step = 345;
                }
            }
            346 => interface_checks(&app, &state, &mut check),
            _ => {
                let n = check(true, "fin");
                println!("Autotest : {}", if n == 0 { "tout est bon".to_string() } else { format!("{n} échec(s)") });
                let _ = slint::quit_event_loop();
            }
        }
    });
    t
}

fn tools_checks(app: &AppWindow, state: &Rc<RefCell<State>>, check: &mut impl FnMut(bool, &str) -> u32) {
    use otoko_library::career::library::field;
    let lib = app.global::<crate::Lib>();
    let pal = app.global::<crate::Pal>();
    let field_of = |id: i64, key: &str| state.borrow().career.as_ref().map(|c| field(c.conn(), id, key)).unwrap_or_default();
    // Palette : une page, une action, un morceau.
    app.invoke_open_palette();
    pal.invoke_search("mast".into());
    let labels: Vec<String> = pal.get_items().iter().map(|i| i.label.to_string()).collect();
    let page = labels.iter().position(|l| l == "Aller à : Mastering");
    check(pal.get_open() && page.is_some() && labels.iter().any(|l| l == "Ajouter au mastering"), &format!("palette « mast » : {}", labels.join(" · ")));
    if let Some(i) = page {
        pal.invoke_run(i as i32);
    }
    check(!pal.get_open() && app.get_career_tab() == crate::mastrage::TAB, "palette : aller à Mastering");
    app.invoke_open_palette();
    pal.invoke_search("controle".into());
    let first = pal.get_items().row_data(0).map(|i| format!("{} {}", i.label, i.hint)).unwrap_or_default();
    pal.invoke_search("Concrete".into());
    let track = pal.get_items().iter().any(|i| i.kind == "track");
    pal.invoke_close();
    check(first == "Contrôle avant publication Ctrl+T" && track, &format!("palette sans accents (« {first} ») et morceaux à écouter"));
    // Raccourcis : ceux de Studio Vault, puis un raccourci changé.
    app.invoke_goto(4, 1);
    lib.invoke_reset();
    lib.invoke_row_clicked(0, false, false);
    let id = state.borrow().lib.rows[0].id;
    let took = lib.invoke_shortcut("x".into(), false, false, false);
    let pick = state.borrow().lib.rows.iter().find(|r| r.id == id).map(|r| r.pick);
    check(took && pick == Some(-1), "X : rejeter (raccourci de Studio Vault)");
    lib.invoke_shortcut("n".into(), false, false, false);
    check(lib.invoke_shortcut("k".into(), true, false, false) && pal.get_open(), "Ctrl+K ouvre la palette");
    pal.invoke_close();
    let cp = app.global::<crate::CPrefs>();
    {
        let mut st = state.borrow_mut();
        crate::preferences::fill(app, &mut st);
    }
    let master = otoko_library::career::prefs::ACTIONS.iter().position(|a| a.id == "master").unwrap_or(0) as i32;
    cp.invoke_capture(master);
    cp.invoke_key_pressed("k".into(), true, false, false);
    let refused = cp.get_capturing() == master && cp.get_key_msg().contains("Palette");
    cp.invoke_key_pressed("m".into(), true, false, true);
    let jobs = state.borrow().mast.jobs.len();
    let took = lib.invoke_shortcut("m".into(), true, false, true);
    let added = state.borrow().mast.jobs.len() == jobs + 1;
    check(refused && took && added && app.get_career_tab() == crate::mastrage::TAB, &format!("raccourci changé : Ctrl+Alt+M ajoute au mastering ({})", cp.get_key_msg()));
    {
        let mut st = state.borrow_mut();
        st.mast.jobs.retain(|j| j.status != crate::mastrage::Status::Ready);
    }
    cp.invoke_keys_reset();
    // Préférences : couleur d'accent, tableau compact.
    cp.invoke_pick_accent(1);
    let theme = app.global::<crate::Theme>();
    let blue = theme.get_custom_accent() && theme.get_accent() == slint::Color::from_rgb_u8(0x3d, 0x8b, 0xfd);
    cp.invoke_pick_accent(-1);
    cp.invoke_set_density(true);
    let compact = lib.get_row_h() == 30.0;
    cp.invoke_set_density(false);
    check(blue && !theme.get_custom_accent() && compact && lib.get_row_h() == 40.0, "préférences : couleur d'accent (Bleu), tableau compact, puis d'origine");
    // Genre et ambiance en lot.
    app.invoke_goto(4, 1);
    lib.invoke_reset();
    lib.invoke_row_clicked(1, false, false);
    lib.invoke_row_clicked(2, true, false);
    let ids: Vec<i64> = crate::bibliotheque::selection(&state.borrow());
    lib.invoke_action("genre".into());
    let mood = lib.get_dialog_items().iter().position(|i| i.text == "sombre");
    if let Some(k) = mood {
        lib.invoke_dialog_toggle(k as i32);
    }
    lib.set_dialog_input("UK Garage".into());
    let opened = lib.get_dialog() == "genre";
    lib.invoke_dialog_button("apply".into());
    let tags = state.borrow().career.as_ref().map(|c| otoko_library::career::library::tags_of(c.conn(), ids[0])).unwrap_or_default();
    check(
        opened && ids.len() == 2 && ids.iter().all(|i| field_of(*i, "genre") == "UK Garage") && tags.iter().any(|t| t == "sombre"),
        &format!("genre et ambiance sur 2 morceaux (tags : {})", tags.join(", ")),
    );
    // Tags en lot dans le panneau de détails.
    lib.invoke_row_clicked(3, true, false);
    lib.invoke_action("edit-tags".into());
    let batch = lib.get_details_batch();
    lib.invoke_field_edited("year".into(), "1999".into());
    let ids3 = crate::bibliotheque::selection(&state.borrow());
    check(batch == 3 && ids3.iter().all(|i| field_of(*i, "year") == "1999"), "éditer les tags en lot (3 morceaux, année 1999)");
    lib.invoke_close_details();
    // File de lecture.
    state.borrow_mut().player.queue.clear();
    lib.invoke_key("q".into());
    lib.invoke_action("queue".into());
    let n = lib.get_dialog_items().row_count();
    lib.invoke_dialog_toggle(0);
    lib.invoke_dialog_button("down".into());
    let moved = state.borrow().player.queue.get(1).copied() == ids3.first().copied();
    lib.invoke_dialog_button("clear".into());
    check(n == 3 && moved && state.borrow().player.queue.is_empty(), "file de lecture : voir, descendre, vider");
    lib.invoke_dialog_button("close".into());
    // Dossiers : surveillés, en double, changer d'emplacement (dialogues, rien d'appliqué).
    lib.invoke_action("watch".into());
    let watch = lib.get_dialog_items().row_count();
    lib.invoke_dialog_button("close".into());
    lib.invoke_action("relocate".into());
    let relocate = lib.get_dialog() == "relocate" && lib.get_dialog_items().row_count() > 1 && !lib.get_dialog_input().is_empty();
    lib.invoke_dialog_button("close".into());
    lib.invoke_action("dupe-folders".into());
    let dupes = lib.get_dialog() == "dupe-folders";
    lib.invoke_dialog_button("close".into());
    check(watch >= 1 && relocate && dupes, &format!("dossiers surveillés ({watch}), changer d'emplacement, dossiers en double"));
    // CSV de la vue et contrôle d'une sélection (Ctrl+T).
    let csv = crate::bibliotheque::csv_text(&state.borrow().lib.rows);
    let lines = csv.lines().count();
    check(csv.starts_with("\u{feff}Titre;") && lines == state.borrow().lib.rows.len() + 1, &format!("export CSV de la vue ({} lignes)", lines - 1));
    lib.invoke_shortcut("t".into(), true, false, false);
    let rel = app.global::<crate::Rel>();
    check(app.get_career_tab() == 2 && rel.get_qc_open() && rel.get_qc().row_count() == 3, "Ctrl+T : contrôle avant publication de la sélection");
    rel.invoke_qc_close();
}

/// Un son de synthèse (une basse et un charleston), en WAV 16 bits stéréo.
fn synth_wav(path: &std::path::Path, secs: u32) {
    let spec = hound::WavSpec { channels: 2, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let Ok(mut w) = hound::WavWriter::create(path, spec) else { return };
    for i in 0..44_100 * secs {
        let t = i as f32 / 44_100.0;
        let v = (t * 55.0 * std::f32::consts::TAU).sin() * 0.3 + if i % 11_025 < 600 { ((i * 7919) % 200) as f32 / 400.0 - 0.25 } else { 0.0 };
        let s = (v.clamp(-1.0, 1.0) * 20_000.0) as i16;
        let _ = w.write_sample(s);
        let _ = w.write_sample(s);
    }
    let _ = w.finalize();
}

/// Carrière › Mastering, première partie : la Carrière d'essai (un morceau « À masteriser »), le
/// morceau ajouté depuis la bibliothèque, les réglages, puis le lancement. Rend le dossier d'essai.
fn mastering_start(app: &AppWindow, state: &Rc<RefCell<State>>, engine_dir: &str, check: &mut impl FnMut(bool, &str) -> u32) -> Option<std::path::PathBuf> {
    use otoko_library::career::mastering as m;
    let tmp = std::env::temp_dir().join(format!("otoko-autotest-mastering-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::create_dir_all(&tmp);
    let wav = tmp.join("Essai.wav");
    synth_wav(&wav, 12);
    let db = tmp.join("carriere.db");
    let song = {
        let mut st = state.borrow_mut();
        st.career_path = db.clone();
        st.career = crate::career::open(&db);
        st.career.as_ref().and_then(|c| {
            if !engine_dir.is_empty() {
                let _ = c.set_setting(m::ENGINE_KEY, engine_dir);
            }
            let (_, id) = otoko_library::career::scan::upsert(c.conn(), &wav, &[]).ok()?;
            c.set_status(id, "to_master").ok()?;
            Some(id)
        })
    };
    let Some(song) = song else {
        check(false, "Carrière d'essai du mastering");
        return Some(tmp);
    };
    crate::bibliotheque::fill(app, &mut state.borrow_mut());
    let lib = app.global::<crate::Lib>();
    let mast = app.global::<crate::Mast>();
    app.invoke_goto(4, 1);
    lib.invoke_row_clicked(0, false, false);
    lib.invoke_action("master".into());
    let first = mast.get_jobs().row_data(0);
    check(
        app.get_career_tab() == crate::mastrage::TAB && mast.get_jobs().row_count() == 1 && first.as_ref().is_some_and(|j| j.state == "ready" && j.name == "Essai.wav") && state.borrow().mast.jobs[0].song == Some(song),
        &format!("bibliothèque › Ajouter au mastering : {} ({})", mast.get_launch_title(), first.map(|j| j.sub.to_string()).unwrap_or_default()),
    );
    // Depuis le tableau Production : le même morceau n'entre pas deux fois dans la liste.
    app.invoke_goto(4, 6);
    app.global::<crate::Home>().invoke_board_menu(song as i32, "master".into());
    check(app.get_career_tab() == crate::mastrage::TAB && mast.get_jobs().row_count() == 1, "Production › Ajouter au mastering (pas deux fois le même morceau)");
    let setting = |k: &str| state.borrow().mast.settings.get(k).map(m::text).unwrap_or_default();
    let more = mast.get_more();
    let params = mast.get_simple().row_count() + more.iter().filter(|r| r.kind != "section").count();
    check(
        mast.get_simple().row_count() == 5 && params == m::PARAMS.len() && mast.get_presets().row_count() == m::PRESETS.len(),
        &format!("réglages : {params} réglages expliqués, {} profils de vitesse ; {}", m::PRESETS.len(), mast.get_engine_text()),
    );
    mast.invoke_set_preset("Essai rapide".into());
    check(mast.get_preset() == "Essai rapide" && setting("oversample") == "1", &format!("profil « Essai rapide » ({})", mast.get_estimate()));
    mast.invoke_toggle_cool();
    let half = (std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8) / 2).max(1);
    check(mast.get_cool() && setting("workers") == half.to_string(), &format!("Moins chauffer : {half} cœurs"));
    mast.invoke_toggle_cool();
    let format = m::param("format").map(|p| p.choices).unwrap_or(&[]);
    let at = |v: &str| format.iter().position(|c| c.0 == v).unwrap_or(0) as i32;
    mast.invoke_param_choice("format".into(), at("mp3"));
    let mp3_rates = m::SAMPLE_RATES.iter().find(|r| r.0 == "mp3").map(|r| r.1).unwrap_or(&[]);
    let bits_off = mast.get_more().iter().find(|r| r.key == "bit_depth").is_some_and(|r| r.disabled);
    check(
        setting("format") == "mp3" && mp3_rates.contains(&setting("sample_rate").as_str()) && bits_off && mast.get_summary().contains("MP3"),
        &format!("format MP3 : la résolution ne s'applique plus ({})", mast.get_summary()),
    );
    mast.invoke_param_choice("format".into(), at("wav"));
    mast.invoke_param_text("loudness".into(), "-12".into());
    let twelve = setting("loudness") == "-12" || setting("loudness") == "-12.0";
    check(twelve && mast.get_summary().starts_with("-12 LUFS"), "volume visé à -12 LUFS");
    mast.invoke_param_text("loudness".into(), "-99".into());
    check(setting("loudness").starts_with("-12") && mast.get_msg().contains("entre"), &format!("valeur hors bornes refusée ({})", mast.get_msg()));
    mast.invoke_param_text("loudness".into(), "-9".into());
    check(mast.get_dirty(), "réglages modifiés signalés");
    mast.invoke_save_defaults();
    let saved = state.borrow().career.as_ref().and_then(|c| c.setting(m::DEFAULTS_KEY)).unwrap_or_default();
    check(!mast.get_dirty() && saved.contains("\"oversample\":\"1\""), "enregistrer comme mes réglages par défaut");
    if !mast.get_engine_ok() {
        check(true, "moteur PhaseLimiter introuvable : mastering réel sauté");
        mastering_cleanup(state, &tmp);
        return None;
    }
    mast.invoke_launch();
    check(mast.get_busy() && state.borrow().mast.jobs[0].status == crate::mastrage::Status::Running, &format!("lancer : {}", mast.get_launch_title()));
    Some(tmp)
}

/// Carrière › Mastering, seconde partie, une fois le morceau traité : le master, ses mesures, son
/// rangement dans la bibliothèque, l'écoute et la comparaison ; puis refaire et annuler aussitôt.
fn mastering_done(app: &AppWindow, state: &Rc<RefCell<State>>, check: &mut impl FnMut(bool, &str) -> u32) -> std::path::PathBuf {
    use otoko_library::career::library::field;
    let mast = app.global::<crate::Mast>();
    let lib = app.global::<crate::Lib>();
    let (id, song, master, dst, secs) = {
        let st = state.borrow();
        let j = &st.mast.jobs[0];
        (j.id, j.song.unwrap_or(0), j.master_id.unwrap_or(0), j.out.as_ref().map(|o| o.dst.clone()).unwrap_or_default(), j.started.zip(j.ended).map(|(a, b)| b.duration_since(a).as_secs_f64()).unwrap_or(0.0))
    };
    let field_of = |id: i64, k: &str| state.borrow().career.as_ref().map(|c| field(c.conn(), id, k)).unwrap_or_default();
    let tiles: Vec<String> = mast.get_tiles().iter().map(|t| format!("{} {}", t.label, t.value)).collect();
    let loud = mast.get_tiles().row_data(0).map(|t| t.value.to_string()).unwrap_or_default();
    let near = loud.split(' ').next().and_then(|v| v.replace(',', ".").parse::<f64>().ok()).is_some_and(|l| (l + 9.0).abs() < 1.5);
    check(
        dst.ends_with("Essai (master).wav") && dst.exists() && mast.get_show_tiles() && near && mast.get_result_info().starts_with("Enregistré"),
        &format!("un vrai master en {secs:.1} s : {}", tiles.join(" · ")),
    );
    check(
        master > 0 && field_of(master, "parent_id") == song.to_string() && field_of(master, "role") == "version" && field_of(song, "status") == "done" && mast.get_attach_text().contains("Rangé"),
        &format!("rangé avec son morceau (version master) ; « À masteriser » → « Terminé »"),
    );
    mast.invoke_toggle_all_measures();
    let all = mast.get_measures().row_count();
    check(mast.get_all_measures() && all == otoko_library::career::mastering::MEASURES.len() && mast.get_measures().iter().any(|r| r.after != "—"), &format!("toutes les mesures, original et master ({all})"));
    mast.invoke_show_log();
    check(mast.get_log_open() && mast.get_log_text().contains("phase_limiter.exe"), "détails techniques : la commande envoyée au moteur");
    mast.invoke_close_log();
    mast.invoke_play("out".into());
    check(lib.get_player_open() && lib.get_player_title().contains("master"), &format!("écouter le master ({})", lib.get_player_title()));
    mast.invoke_compare();
    check(lib.get_player_compare() == "A", "comparer l'original et le master à volume égal");
    lib.invoke_player_stop();
    // Refaire, puis annuler aussitôt.
    mast.invoke_job_action(id, "requeue".into());
    check(mast.get_can_launch() && mast.get_jobs().row_data(0).is_some_and(|j| j.state == "ready"), "refaire : le morceau redevient prêt");
    mast.invoke_launch();
    let planned = state.borrow().mast.jobs[0].planned.clone();
    mast.invoke_job_action(id, "cancel".into());
    planned
}

/// L'interface : toast, aide de l'écran, thème selon Windows, accueil. Rien n'est écrit dans les réglages.
fn interface_checks(app: &AppWindow, state: &Rc<RefCell<State>>, check: &mut impl FnMut(bool, &str) -> u32) {
    use crate::apparence;
    app.set_message("Autotest : un message en toast.".into());
    app.invoke_toasted();
    check(app.get_toast_on(), "toast : le message s'affiche, le minuteur l'effacera");
    app.set_toast_on(false);
    check(apparence::help_anchor(4, 7) == "mastering" && apparence::help_anchor(7, 0) == "mobile" && apparence::guide_path().exists(), "aide de l'écran : ancre selon l'espace, guide trouvé");
    app.invoke_goto(6, 0);
    app.invoke_open_help();
    check(app.get_message().starts_with("Guide ouvert"), "bouton « ? » : le guide s'ouvre (simulé)");
    check(apparence::dark_for(1) && !apparence::dark_for(2) && apparence::windows_light().is_some(), &format!("thème : sombre, clair, et Windows lisible (clair = {:?})", apparence::windows_light()));
    app.invoke_goto(4, 0);
    let st = state.borrow();
    let week = st.career.as_ref().map(|c| otoko_library::career::home::week(c.conn()));
    let home = app.global::<crate::Home>();
    let ok = match &week {
        Some(w) => home.get_week().row_count() == 4 && home.get_week().row_data(3).is_some_and(|s| s == w.unfinished.to_string()),
        None => true,
    };
    check(ok, &format!("accueil : la semaine en chiffres ({})", week.map(|w| format!("{} ajouté(s), {} à finir", w.added, w.unfinished)).unwrap_or_else(|| "sans Carrière à cet instant".into())));
}

fn mastering_cleanup(state: &Rc<RefCell<State>>, tmp: &std::path::Path) {
    {
        let mut st = state.borrow_mut();
        for j in &st.mast.jobs {
            j.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        st.mast.jobs.clear();
        st.mast.sel = None;
        st.career = None;
    }
    let _ = std::fs::remove_dir_all(tmp);
}

/// Ce que le QCM du 6 novembre 2026 a ajouté, sur la Carrière d'essai.
fn qcm_checks(app: &AppWindow, state: &Rc<RefCell<State>>, check: &mut impl FnMut(bool, &str) -> u32) {
    use otoko_library::career::library::field;
    let (home, rel, lib, publ, mast) = (app.global::<crate::Home>(), app.global::<crate::Rel>(), app.global::<crate::Lib>(), app.global::<crate::Pub>(), app.global::<crate::Mast>());
    // Accueil : au plus trois actions, le reste ensuite.
    app.invoke_goto(4, 0);
    let n = home.get_today().row_count();
    check((1..=3).contains(&n), &format!("accueil : {n} action(s) aujourd'hui, {} ensuite", home.get_next_up().row_count()));
    // Pilotage d'une sortie.
    let first = state.borrow().career.as_ref().and_then(|c| c.conn().query_row("SELECT r.id FROM releases r WHERE EXISTS (SELECT 1 FROM release_tracks t WHERE t.release_id = r.id) ORDER BY r.release_date DESC LIMIT 1", [], |r| r.get::<_, i64>(0)).ok());
    let Some(first) = first else {
        check(false, "pilotage : aucune sortie dans la base d'essai");
        return;
    };
    rel.invoke_open(first as i32);
    check(!rel.get_pilot_count().is_empty() && rel.get_pilot_total() == 7, &format!("pilotage : {} · {}/{} · {} point(s)", rel.get_pilot_count(), rel.get_pilot_done(), rel.get_pilot_total(), rel.get_points().row_count()));
    // Détails en onglets.
    app.invoke_goto(4, 1);
    lib.invoke_row_clicked(0, false, false);
    if !lib.get_details_open() {
        lib.invoke_key("i".into());
    }
    let counts: Vec<usize> = (0..5)
        .map(|t| {
            lib.invoke_details_tab_changed(t);
            lib.get_fields().row_count()
        })
        .collect();
    check(counts == [9, 3, 3, 3, 0], &format!("détails : cinq onglets ({counts:?} champs)"));
    // Correction rapide : tempo doublé, gardé, rendu à l'analyse.
    let id = state.borrow().lib.selected.iter().next().copied().unwrap_or(0);
    let bpm = |s: &Rc<RefCell<State>>| s.borrow().career.as_ref().map(|c| field(c.conn(), id, "bpm")).unwrap_or_default().parse::<f64>().unwrap_or(0.0);
    let before = bpm(state);
    lib.invoke_details_tab_changed(1);
    lib.invoke_bpm_scale(2.0);
    check(before > 0.0 && (bpm(state) - before * 2.0).abs() < 0.2 && lib.get_details_locked(), &format!("tempo × 2 : {before:.1} → {:.1}, gardé contre l'analyse", bpm(state)));
    lib.invoke_bpm_scale(0.5);
    lib.invoke_unlock();
    check(!lib.get_details_locked(), "tempo rendu à l'analyse");
    // L'historique du morceau, et Ctrl+Z qui défait la dernière modification.
    lib.invoke_details_tab_changed(4);
    let hist: Vec<String> = lib.get_history().iter().map(|h| h.text.to_string()).collect();
    check(hist.iter().any(|h| h.starts_with("Tempo : ")), &format!("historique : {hist:?}"));
    check(lib.get_undo_label().starts_with("Rendu à l'analyse"), &format!("Ctrl+Z propose : {}", lib.get_undo_label()));
    lib.invoke_action("undo".into());
    check(lib.get_details_locked(), "Ctrl+Z : le tempo est de nouveau gardé contre l'analyse");
    lib.invoke_details_tab_changed(0);
    lib.invoke_close_details();
    // Mes textes.
    app.invoke_goto(4, 3);
    publ.invoke_set_tab("mine".into());
    publ.invoke_mine_new("modele".into());
    publ.set_mine_body("son : {titre}\n{inconnu}\nfin".into());
    publ.invoke_mine_edited();
    let preview = publ.get_mine_preview().to_string();
    check(preview.starts_with("son : ") && preview.ends_with("fin") && !preview.contains('{'), &format!("mes textes : le modèle s'écrit ({})", preview.replace('\n', " / ")));
    publ.invoke_mine_delete();
    check(publ.get_mines().row_count() == 0, "mes textes : modèle retiré");
    publ.invoke_set_tab("write".into());
    // Les outils de lots.
    for what in ["batches", "statuses"] {
        lib.invoke_action(what.into());
        check(lib.get_dialog() == what, &format!("outil « {what} » : {} ligne(s)", lib.get_dialog_items().row_count()));
        lib.invoke_dialog_button("close".into());
    }
    // Mastering d'une sortie.
    {
        let mut st = state.borrow_mut();
        crate::mastrage::add_release(app, &mut st, first);
    }
    check(mast.get_album_on() && mast.get_album().row_count() > 0, &format!("mastering d'album : {} titre(s) ({})", mast.get_album().row_count(), mast.get_album_text()));
    mast.invoke_clear();
    check(!mast.get_album_on(), "mastering d'album : liste vidée");
    // Le clavier : Ctrl+1 à 8 sur AZERTY, Échap referme le lecteur ouvert.
    let keys = [("&", 0, 4), ("é", 1, 4), ("\"", 6, 4), ("'", 7, 4), ("(", 2, 4), ("-", 3, 4), ("è", 5, 4), ("_", 0, 7)];
    let reached: Vec<bool> = keys
        .iter()
        .map(|(k, tab, space)| {
            app.invoke_career_key((*k).into(), true, false, false);
            app.get_space() == *space && (*space != 4 || app.get_career_tab() == *tab)
        })
        .collect();
    check(reached.iter().all(|r| *r), &format!("Ctrl+1 à 8 : {reached:?}"));
    app.invoke_goto(4, 0);
    lib.set_player_big(true);
    app.invoke_career_key("\u{1b}".into(), false, false, false);
    check(!lib.get_player_big(), "Échap referme le lecteur ouvert");
    app.global::<crate::Nav>().invoke_next();
    app.global::<crate::Nav>().invoke_previous();
    check(true, "flèches : passer d'une commande à l'autre");
}

/// Carrière › Accueil, Production, Statistiques, sur la Carrière d'essai (Studio Vault importé).
fn career_home_checks(app: &AppWindow, state: &Rc<RefCell<State>>, check: &mut impl FnMut(bool, &str) -> u32) {
    use otoko_library::career::library::field;
    qcm_checks(app, state, check);
    let home = app.global::<crate::Home>();
    let lib = app.global::<crate::Lib>();
    let field_of = |id: i64, key: &str| state.borrow().career.as_ref().map(|c| field(c.conn(), id, key)).unwrap_or_default();
    // Accueil.
    app.invoke_goto(4, 0);
    let stats: Vec<String> = home.get_stats().iter().map(|s| s.to_string()).collect();
    let total = state.borrow().career.as_ref().map(|c| c.conn().query_row("SELECT COUNT(*) FROM tracks WHERE missing = 0", [], |r| r.get::<_, i64>(0)).unwrap_or(0)).unwrap_or(0);
    check(
        stats.first() == Some(&total.to_string()) && home.get_months().row_count() == 12 && home.get_releases().row_count() > 0 && home.get_hello().starts_with("Bon"),
        &format!("accueil : {} morceaux, {} à terminer, {} sorties au bandeau, {} chose(s) à faire", stats[0], stats[1], home.get_releases().row_count(), home.get_todo().row_count()),
    );
    home.invoke_stat(1);
    let open = lib.get_nav().iter().find(|n| n.label == "À terminer").map(|n| n.count.to_string());
    check(app.get_career_tab() == 1 && open.as_ref() == stats.get(1), &format!("« à terminer » ouvre la bibliothèque sur ces morceaux ({})", open.unwrap_or_default()));
    home.invoke_act("sync".into());
    check(lib.get_dialog() == "sync", &format!("rattacher les albums : {}", lib.get_dialog_title()));
    lib.invoke_dialog_button("close".into());
    home.invoke_act("nature".into());
    check(lib.get_dialog() == "nature", &format!("classer remix et mashups : {}", lib.get_dialog_title()));
    lib.invoke_dialog_button("close".into());
    // Production : glisser une idée dans « Terminés », puis la remettre.
    app.invoke_goto(4, 6);
    let board = home.get_board();
    let keys: Vec<String> = board.iter().map(|c| c.key.to_string()).collect();
    let card = board.row_data(0).and_then(|c| c.cards.row_data(0));
    check(keys == ["idea", "wip", "mix", "done", "released"], &format!("production : {} colonnes ({})", keys.len(), board.iter().map(|c| format!("{} {}", c.title, c.count)).collect::<Vec<_>>().join(", ")));
    if let Some(card) = card {
        let id = card.id as i64;
        home.invoke_board_drop(card.id, 3);
        let moved = field_of(id, "status");
        let in_done = home.get_board().row_data(3).is_some_and(|c| c.cards.iter().any(|k| k.id == card.id));
        home.invoke_board_drop(card.id, 0);
        check(moved == "done" && in_done && field_of(id, "status") == "idea", &format!("glisser « {} » dans « Terminés » puis le remettre", card.title));
        home.invoke_board_menu(card.id, "publish".into());
        let on = field_of(id, "to_publish");
        home.invoke_board_menu(card.id, "unpublish".into());
        check(on == "1" && field_of(id, "to_publish") == "0", "marquer « À publier » depuis la carte, puis retirer");
        home.invoke_board_menu(card.id, "reveal".into());
        check(app.get_career_tab() == 1 && lib.get_details_id() == card.id && lib.get_details_open(), "« voir dans la bibliothèque » ouvre sa fiche");
    }
    home.set_board_mode("clients".into());
    home.invoke_board_mode_changed("clients".into());
    check(home.get_board().row_count() == 6, &format!("commandes clients : {} étapes", home.get_board().row_count()));
    home.invoke_board_mode_changed("mine".into());
    // Statistiques en graphiques.
    app.invoke_goto(4, 5);
    let summary: Vec<String> = home.get_summary().iter().map(|s| s.to_string()).collect();
    let rings = home.get_creation().row_data(1).map(|c| c.slices.iter().filter(|s| s.path.starts_with("M ")).count()).unwrap_or(0);
    check(
        summary.first() == Some(&total.to_string()) && home.get_creation().row_count() == 3 && home.get_music().row_count() == 4 && home.get_progress().row_count() == 5 && rings > 0,
        &format!("statistiques : {} · {} · {} gardés · {} analysés · {} BPM ; anneau des types de sortie en {rings} parts", summary[0], summary[1], summary[2], summary[3], summary[4]),
    );
    release_checks(app, state, check);
}

/// Carrière › Sorties : galerie, fiche enregistrée à chaque changement, planning, contrôle,
/// nouvelle sortie, ajout depuis la bibliothèque, suppression, discographie.
fn release_checks(app: &AppWindow, state: &Rc<RefCell<State>>, check: &mut impl FnMut(bool, &str) -> u32) {
    let rel = app.global::<crate::Rel>();
    let lib = app.global::<crate::Lib>();
    let db = |sql: &str| state.borrow().career.as_ref().and_then(|c| c.conn().query_row(sql, [], |r| r.get::<_, String>(0)).ok()).unwrap_or_default();
    rel.invoke_back();
    let n = db("SELECT CAST(COUNT(*) AS TEXT) FROM releases");
    check(app.get_career_tab() == 2 && rel.get_tiles().row_count().to_string() == n, &format!("sorties : galerie de {n} pochettes"));
    // La fiche de la sortie qui a le plus de morceaux.
    let id: i64 = db("SELECT CAST(release_id AS TEXT) FROM release_tracks GROUP BY release_id ORDER BY COUNT(*) DESC LIMIT 1").parse().unwrap_or(0);
    rel.invoke_open(id as i32);
    let tracks = rel.get_tracks().row_count();
    check(rel.get_mode() == "sheet" && rel.get_id() as i64 == id && tracks > 1 && rel.get_steps().row_count() == 7, &format!("fiche « {} » : {tracks} morceaux, 7 étapes", rel.get_title()));
    rel.set_title("Autotest sortie".into());
    rel.invoke_edited("title".into(), "Autotest sortie".into());
    rel.invoke_edited("date".into(), "08/07/2030".into());
    let saved = db(&format!("SELECT title || ' ' || release_date FROM releases WHERE id = {id}"));
    check(saved == "Autotest sortie 2030-07-08" && rel.get_date_hint().contains("lundi 8 juillet 2030"), &format!("titre et date enregistrés aussitôt ({saved} · {})", rel.get_date_hint()));
    let first = db(&format!("SELECT CAST(track_id AS TEXT) FROM release_tracks WHERE release_id = {id} ORDER BY position LIMIT 1"));
    rel.invoke_move_track(0, 1);
    let second = db(&format!("SELECT CAST(track_id AS TEXT) FROM release_tracks WHERE release_id = {id} ORDER BY position LIMIT 1 OFFSET 1"));
    rel.invoke_toggle_step("infos".into());
    let steps = db(&format!("SELECT steps FROM releases WHERE id = {id}"));
    check(second == first && steps.contains("\"infos\":"), "déplacer un morceau, cocher une étape");
    rel.invoke_prefill_credits();
    check(!db(&format!("SELECT credits FROM releases WHERE id = {id}")).is_empty(), &format!("crédits pré-remplis : {}", rel.get_credits().lines().next().unwrap_or("")));
    rel.invoke_qc_run();
    let rows = rel.get_qc().row_count();
    check(rel.get_qc_open() && rows == tracks && rel.get_qc().row_data(0).is_some_and(|r| r.cells.row_count() == 6) && rel.get_qc_detail().contains("Format"), &format!("contrôle avant publication : {rows} morceaux × 6 contrôles"));
    rel.invoke_qc_close();
    // Le planning : la sortie de 2030 y est, ses étapes à rebours.
    rel.invoke_planning();
    let planned = rel.get_plan().iter().find(|p| p.id as i64 == id).map(|p| p.steps.row_count()).unwrap_or(0);
    check(rel.get_mode() == "planning" && rel.get_days().row_count() == 42 && planned == 7, &format!("planning : {} · la sortie et ses 7 étapes", rel.get_month_label()));
    // Nouvelle sortie, un morceau ajouté, puis supprimée.
    rel.invoke_new_confirm("Sortie d'essai".into());
    let new_id = rel.get_id() as i64;
    rel.invoke_picker_show();
    let pick = rel.get_picker().row_data(0).map(|t| t.id);
    if let Some(t) = pick {
        rel.invoke_picker_choose(t);
    }
    let in_db = db(&format!("SELECT CAST(COUNT(*) AS TEXT) FROM release_tracks WHERE release_id = {new_id}"));
    check(new_id != id && rel.get_title() == "Sortie d'essai" && in_db == "1", "nouvelle sortie, un morceau ajouté");
    rel.invoke_delete(new_id as i32);
    check(db(&format!("SELECT CAST(COUNT(*) AS TEXT) FROM releases WHERE id = {new_id}")) == "0" && rel.get_mode() == "gallery", "supprimer la sortie (les morceaux restent)");
    // Depuis la bibliothèque : deux morceaux dans une nouvelle sortie.
    app.invoke_goto(4, 1);
    lib.invoke_reset();
    lib.invoke_row_clicked(0, false, false);
    lib.invoke_row_clicked(1, true, false);
    lib.invoke_action("add-to-release".into());
    let opened = lib.get_dialog() == "add-to-release";
    lib.set_dialog_input("Depuis la bibliothèque".into());
    lib.invoke_dialog_button("apply".into());
    let made = db("SELECT CAST(COUNT(*) AS TEXT) FROM release_tracks rt JOIN releases r ON r.id = rt.release_id WHERE r.title = 'Depuis la bibliothèque'");
    check(opened && made == "2" && app.get_career_tab() == 2 && rel.get_mode() == "sheet", "bibliothèque › ajouter 2 morceaux à une nouvelle sortie (la fiche s'ouvre)");
    // Discographie (PDF et page web, pochettes comprises), dans le dossier temporaire.
    let path = state.borrow().career_path.clone();
    let cache = std::env::temp_dir().join("otoko-essai-pochettes");
    let t = Instant::now();
    let pdf = std::env::temp_dir().join(format!("otoko-autotest-discographie-{}.pdf", std::process::id()));
    let html = pdf.with_extension("html");
    let r1 = crate::sorties::write_discography(&path, &cache, &pdf, false);
    let r2 = crate::sorties::write_discography(&path, &cache, &html, true);
    let bytes = std::fs::read(&pdf).unwrap_or_default();
    let images = String::from_utf8_lossy(&bytes).matches("/Subtype /Image").count();
    let page = std::fs::read_to_string(&html).unwrap_or_default();
    check(
        r1.is_ok() && r2.is_ok() && bytes.starts_with(b"%PDF") && images > 0 && page.contains("data:image/jpeg;base64,"),
        &format!("discographie en {:.1} s : PDF de {} Ko ({images} pochettes), page web de {} Ko", t.elapsed().as_secs_f64(), bytes.len() / 1024, page.len() / 1024),
    );
    if std::env::var_os("OTOKO_KEEP_DISCOGRAPHY").is_none() {
        let _ = std::fs::remove_file(&pdf);
        let _ = std::fs::remove_file(&html);
    }
    project_checks(app, state, check);
}

/// Fiche projet (remix, prod et client, set minuté, mashup) et constructeur de set.
fn project_checks(app: &AppWindow, state: &Rc<RefCell<State>>, check: &mut impl FnMut(bool, &str) -> u32) {
    use otoko_library::career::projects;
    let proj = app.global::<crate::Proj>();
    let lib = app.global::<crate::Lib>();
    let load = |id: i64| state.borrow().career.as_ref().and_then(|c| projects::load(c.conn(), id));
    app.invoke_goto(4, 1);
    lib.invoke_reset();
    let ids: Vec<i64> = state.borrow().lib.rows.iter().filter(|r| r.nature.is_empty() || r.nature == "original").take(4).map(|r| r.id).collect();
    if ids.len() < 4 {
        check(false, "fiche projet : pas assez de morceaux originaux");
        return;
    }
    let open = |id: i64| {
        let i = state.borrow().lib.rows.iter().position(|r| r.id == id).unwrap_or(0);
        lib.invoke_row_clicked(i as i32, false, false);
        lib.invoke_action("project".into());
    };
    // Remix officiel.
    open(ids[0]);
    let opened = proj.get_open() && proj.get_id() as i64 == ids[0];
    proj.invoke_set_nature("remix".into());
    proj.set_orig_artist("Daft Punk".into());
    proj.invoke_field("orig_artist".into(), "Daft Punk".into());
    proj.invoke_field("orig_title".into(), "One More Time".into());
    proj.invoke_toggle("official".into());
    proj.invoke_choose("authorization".into(), 2);
    let rights = proj.get_rights().to_string();
    proj.invoke_copy_credits();
    let credits = proj.get_msg().to_string();
    proj.invoke_save();
    let p = load(ids[0]).unwrap_or_default();
    check(
        opened && !proj.get_open() && p.nature == "remix" && p.meta["orig_title"] == "One More Time" && rights.starts_with("Remix officiel") && credits.contains("titre original : Daft Punk – One More Time"),
        &format!("fiche projet › remix officiel ({credits})"),
    );
    // Prod pour un client, livrée puis payée.
    open(ids[1]);
    proj.invoke_set_nature("prod".into());
    proj.invoke_clients_show();
    proj.invoke_client_new();
    proj.set_c_name("Client autotest".into());
    proj.set_c_contact("autotest@exemple.fr".into());
    proj.invoke_client_save();
    let in_book = proj.get_clients().iter().any(|c| c.name == "Client autotest");
    proj.invoke_clients_close();
    let k = proj.get_client_names().iter().position(|n| n == "Client autotest").unwrap_or(0);
    proj.invoke_choose("client".into(), k as i32);
    proj.invoke_choose("order_status".into(), 4);
    proj.invoke_field("fee".into(), "250".into());
    proj.invoke_toggle("deadline".into());
    proj.invoke_deliverable("master".into());
    proj.invoke_version_add();
    proj.invoke_toggle("paid".into());
    proj.invoke_save();
    let p = load(ids[1]).unwrap_or_default();
    check(
        in_book && p.nature == "prod" && p.meta["client_name"] == "Client autotest" && p.meta["order_status"] == "paid" && p.meta["fee"].as_f64() == Some(250.0) && p.meta["deliverables"]["master"] == true && p.meta["versions"].as_array().is_some_and(|v| v.len() == 1),
        "fiche projet › prod pour un client du carnet, livrables, version, payée",
    );
    // Set minuté.
    open(ids[2]);
    proj.invoke_set_nature("set".into());
    proj.invoke_entry_add();
    proj.invoke_entry_add();
    proj.invoke_entry_edited(0, 0, "0:00".into());
    proj.invoke_entry_edited(0, 2, "Intro".into());
    proj.invoke_entry_edited(1, 0, "3:30".into());
    proj.invoke_entry_edited(1, 2, "Suite".into());
    proj.invoke_picker_show("entry".into());
    let pick = proj.get_picker().row_data(0).map(|p| p.id);
    if let Some(id) = pick {
        proj.invoke_picker_choose(id);
    }
    proj.invoke_check_transitions();
    let report = proj.get_set_report().to_string();
    proj.invoke_save();
    let p = load(ids[2]).unwrap_or_default();
    check(p.nature == "set" && p.entries.len() == 3 && p.entries[1].time == 210.0 && p.entries[2].track.is_some() && report.contains("Intro → Suite"), &format!("fiche projet › set : 3 titres minutés ({})", report.lines().next().unwrap_or("")));
    // Mashup de deux titres de la bibliothèque.
    open(ids[3]);
    proj.invoke_set_nature("mashup".into());
    for _ in 0..2 {
        proj.invoke_picker_show("source".into());
        let n = proj.get_picker().row_count();
        let id = proj.get_picker().row_data(n / 2 + proj.get_sources().row_count()).map(|p| p.id);
        if let Some(id) = id {
            proj.invoke_picker_choose(id);
        }
    }
    let check_text = proj.get_mashup_check().to_string();
    proj.invoke_save();
    let p = load(ids[3]).unwrap_or_default();
    check(p.nature == "mashup" && p.meta["sources"].as_array().is_some_and(|s| s.len() == 2), &format!("fiche projet › mashup de 2 sources ({})", check_text.lines().nth(1).unwrap_or("—")));
    // Constructeur de set sur 10 morceaux (pas ceux des fiches ci-dessus : un set n'entre pas dans
    // un set), enregistré dans le set.
    lib.invoke_reset();
    lib.invoke_row_clicked(10, false, false);
    for i in 11..20 {
        lib.invoke_row_clicked(i, true, false);
    }
    lib.invoke_action("set-builder".into());
    let rows = proj.get_sb_rows().row_count();
    proj.invoke_sb_set_mode("plateau".into());
    proj.set_sb_sel(0);
    proj.invoke_sb_move(1);
    let moved = proj.get_sb_sel() == 1;
    proj.invoke_sb_save();
    let sets: Vec<String> = proj.get_sb_sets().iter().map(|s| s.to_string()).collect();
    let k = sets.iter().position(|s| *s == load(ids[2]).map(|p| p.title).unwrap_or_default()).unwrap_or(0);
    proj.invoke_sb_save_to(k as i32);
    let saved = load(ids[2]).map(|p| p.entries.len()).unwrap_or(0);
    proj.invoke_sb_close();
    check(
        rows == 10 && moved && saved == 10 && proj.get_sb_summary().contains("Liste enregistrée"),
        &format!("constructeur de set : 10 morceaux ordonnés, retouchés, enregistrés dans le set ({saved} titres)"),
    );
}

/// Le navigateur du studio : chaque effet d'OTOKO s'ouvre dans le panneau, un kit, une base, une banque SFZ et un preset
/// personnel se posent, et tout s'annule. Les sons et la Carrière sont temporaires : rien chez l'utilisateur.

/// Mobile et partage : un lien créé depuis le PC, dans la base de partages de l'essai (le résultat revient au tour suivant).
fn partage_start(app: &AppWindow, check: &mut impl FnMut(bool, &str) -> u32) {
    app.invoke_goto(7, 0);
    let g = app.global::<crate::Part>();
    g.invoke_new_link();
    check(g.get_creating() && app.get_space() == 7, "Mobile et partage : le formulaire « Nouveau lien » s'ouvre");
    g.invoke_set_kind(2);
    g.set_label("Autotest".into());
    g.invoke_create();
}

fn partage_done(app: &AppWindow, check: &mut impl FnMut(bool, &str) -> u32) {
    let g = app.global::<crate::Part>();
    let made = g.get_shares().iter().find(|s| s.title == "Autotest");
    check(!g.get_creating() && made.as_ref().is_some_and(|s| s.tone == 0), &format!("lien créé depuis le PC ({} lien(s), « {} »)", g.get_shares().row_count(), g.get_error()));
    if let Some(s) = made {
        g.invoke_revoke(s.id);
    }
}

/// Ultra performant : ouvrir une page, base relue, tient dans un dixième de seconde.
fn pages_speed(app: &AppWindow, state: &Rc<RefCell<State>>, check: &mut impl FnMut(bool, &str) -> u32) {
    let mut worst = (Duration::ZERO, "");
    let mut text = Vec::new();
    for (tab, name) in [(1, "bibliothèque"), (6, "production"), (7, "mastering"), (2, "sorties"), (3, "publier"), (4, "suno"), (5, "statistiques"), (0, "accueil")] {
        state.borrow_mut().pages_lues = [false; 8];
        let t = Instant::now();
        app.invoke_goto(4, tab);
        let d = t.elapsed();
        text.push(format!("{name} {} ms", d.as_millis()));
        if d > worst.0 {
            worst = (d, name);
        }
    }
    check(worst.0 < Duration::from_millis(100), &format!("pages relues : {}", text.join(", ")));
}
