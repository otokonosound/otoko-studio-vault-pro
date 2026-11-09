//! Otoko Studio Vault Pro : la gestion de carrière d'un producteur de musique (bibliothèque, production, mastering,
//! sorties, statistiques, mobile et liens de partage), reprise de Studio Vault.
//!
//! Options : --demo (pas de sauvegarde quotidienne, pas de serveur mobile), --silent (lecture sans un son), et pour les
//! mesures --idle N, --snapshot f.bmp, --selftest.

// Une application graphique : pas de console qui s'ouvre derrière la fenêtre. Lancé depuis un
// terminal avec des options, le logiciel s'y rattache pour écrire ses résultats.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accueil;
mod analyse;
mod apparence;
mod audio;
mod bibliotheque;
mod callbacks;
mod career;
mod distant;
mod fond;
mod images;
mod lecteur;
mod library;
mod loudness;
mod majs;
mod mastrage;
mod measure;
mod mobile;
mod outils;
mod palette;
mod partage;
mod preferences;
mod prefs;
mod projets;
mod publish;
mod raccourcis;
mod samples;
mod settings;
mod sorties;
mod state;
mod suno;
mod zone;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use slint::{Timer, TimerMode};

use state::State;

slint::include_modules!();

/// Avec des options, la sortie texte va au terminal qui a lancé OTOKO (s'il y en a un et que la
/// sortie n'est pas déjà redirigée vers un fichier ou un tube).
fn attach_console() {
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn GetStdHandle(which: u32) -> isize;
        fn SetStdHandle(which: u32, h: isize) -> i32;
        fn CreateFileW(name: *const u16, access: u32, share: u32, sec: *const u8, disp: u32, flags: u32, tpl: isize) -> isize;
    }
    const STD_OUTPUT: u32 = -11i32 as u32;
    const STD_ERROR: u32 = -12i32 as u32;
    unsafe {
        let redirected = GetStdHandle(STD_OUTPUT) > 0;
        if redirected || AttachConsole(u32::MAX) == 0 {
            return;
        }
        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        let h = CreateFileW(name.as_ptr(), 0x4000_0000 | 0x8000_0000, 3, std::ptr::null(), 3, 0, 0);
        if h > 0 {
            SetStdHandle(STD_OUTPUT, h);
            SetStdHandle(STD_ERROR, h);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        attach_console();
    } else if zone::already_running() {
        // Déjà ouvert (peut-être près de l'horloge) : sa fenêtre se montre, pas de seconde copie.
        return;
    }
    let flag = |f: &str| args.iter().any(|a| a == f);

    // --vault-schema : les tables de la base Studio Vault (lue sur une copie, jamais modifiée).
    if flag("--vault-schema") {
        match otoko_library::vault::default_dir().ok_or("Studio Vault introuvable".to_string()).and_then(|d| otoko_library::vault::snapshot(&d)) {
            Ok((conn, tmp)) => {
                for (name, cols, count) in otoko_library::vault::schema(&conn).unwrap_or_default() {
                    println!("{name} ({count}) : {}", cols.join(", "));
                }
                for q in [
                    "SELECT status, COUNT(*) FROM tracks GROUP BY status",
                    "SELECT role, COUNT(*) FROM tracks GROUP BY role",
                    "SELECT nature, COUNT(*) FROM tracks GROUP BY nature",
                    "SELECT rtype, COUNT(*) FROM releases GROUP BY rtype",
                    "SELECT archived, COUNT(*) FROM tracks GROUP BY archived",
                    "SELECT substr(steps, 1, 120), 1 FROM releases LIMIT 2",
                    "SELECT title || ' · ' || IFNULL(bpm, '') || ' · ' || IFNULL(music_key, '') || ' · ' || IFNULL(lufs, ''), 1 FROM tracks WHERE path LIKE '%Concrete%' OR path LIKE '%Year 2551%' OR path LIKE '%The Call%' OR path LIKE '%Time Lag%' OR path LIKE '%Five%' OR path LIKE '%Grid%' OR path LIKE '%Jugement%'",
                ] {
                    if let Ok(mut st) = conn.prepare(q) {
                        let rows: Vec<String> = st
                            .query_map([], |r| Ok(format!("{}={}", r.get::<_, Option<String>>(0).ok().flatten().unwrap_or_else(|| "∅".into()), r.get::<_, i64>(1).unwrap_or(0))))
                            .map(|it| it.flatten().collect())
                            .unwrap_or_default();
                        println!("{q}\n   {}", rows.join(" | "));
                    }
                }
                drop(conn);
                let _ = std::fs::remove_dir_all(tmp);
            }
            Err(e) => eprintln!("{e}"),
        }
        return;
    }
    // --bpm fichier… : tempo estimé de chaque son (mise au point de la détection).
    if let Some(i) = args.iter().position(|a| a == "--bpm") {
        for f in &args[i + 1..] {
            match samples::load(f, 48000.0, None) {
                Ok(d) => println!("{:>8} BPM  {f}", samples::estimate_bpm(&d, 48000.0).map(|b| format!("{b:.2}")).unwrap_or("—".into())),
                Err(e) => println!("illisible : {f} ({e})"),
            }
        }
        return;
    }

    // Rendu logiciel par défaut : très peu de mémoire au repos, et rien à attendre de la carte graphique (bureau à
    // distance compris). `SLINT_BACKEND` reste prioritaire.
    if std::env::var_os("SLINT_BACKEND").is_none() {
        let _ = slint::BackendSelector::new().backend_name("winit".into()).renderer_name("software".into()).select();
    }
    let mesure = ["--selftest", "--idle", "--snapshot", "--demo"].iter().any(|f| flag(f));
    // Taille de l'interface choisie dans les Réglages (pas pendant les mesures et captures).
    if !mesure {
        apparence::prepare_scale();
    }
    let app = AppWindow::new().expect("fenêtre");
    app.set_version(env!("CARGO_PKG_VERSION").into());

    let (jobs_tx, jobs_rx) = mpsc::channel();
    let poll = Rc::new(Timer::default());
    // Relié au PC principal : on travaille sur une copie de sa base.
    let lien = if flag("--selftest") || flag("--demo") { None } else { distant::address().map(|url| distant::boot(&url)) };
    let base = if lien.is_some() { distant::base_path() } else { career::default_path() };
    let state = Rc::new(RefCell::new(State {
        jobs: jobs_tx,
        pending: 0,
        poll: poll.clone(),
        ecoute: audio::Ecoute::new(flag("--silent")),
        career: career::open(&base),
        career_path: base,
        pages_lues: [false; 8],
        career_pick: None,
        suno: Default::default(),
        // Autotest et captures : rien n'est écrit chez l'utilisateur (sauvegardes, réglages, téléchargements).
        selftest: flag("--selftest") || flag("--snapshot"),
        mobile: None,
        lib: Default::default(),
        player: Default::default(),
        home: Default::default(),
        rel: Default::default(),
        proj: Default::default(),
        publ: Default::default(),
        mast: Default::default(),
        partage: Default::default(),
        palette: Default::default(),
        prefs_extra_pending: false,
        arrivals_busy: false,
        copies_busy: false,
        waves_busy: false,
        arrival_timer: None,
        update_ready: None,
        update_source: None,
        distant: lien,
    }));
    // Préférences (couleur d'accent, tableau) ; la sauvegarde du jour de la base (celle du PC principal se fait sur le PC principal).
    {
        let mut st = state.borrow_mut();
        preferences::apply(&app, &st);
        if !mesure && st.distant.is_none() {
            preferences::daily_backup(&mut st);
        }
    }
    // Les connaissances sur Suno : une copie posée dans les données remplace celles fournies.
    otoko_library::career::suno::init_caps(Some(&library::data_dir().join("suno_capabilities.json")));
    // Le serveur de l'app mobile, si on a demandé que le logiciel le fasse tourner (jamais sur une copie).
    if !mesure && state.borrow().distant.is_none() {
        mobile::autostart(&mut state.borrow_mut());
    }
    // Apparence : thème (comme Windows par défaut, suivi tant que le logiciel tourne), taille.
    apparence::apply_theme(&app);
    let _theme_watch = apparence::watch(&app);
    app.set_ui_scale(apparence::scale_choice());

    // Les travaux d'arrière-plan sont relevés tant qu'il y en a : au repos, rien ne tourne.
    {
        let weak = app.as_weak();
        let state = state.clone();
        poll.start(TimerMode::Repeated, Duration::from_millis(60), move || {
            if let Ok(mut st) = state.try_borrow_mut() {
                state::collect(&weak.unwrap(), &mut st, &jobs_rx);
            }
        });
        poll.stop();
    }

    callbacks::wire(&app, &state);
    lecteur::wire(&app, &state);
    // On ouvre sur l'accueil : la seule page lue au lancement.
    career::fill(&app, &mut state.borrow_mut());
    // Le dossier d'arrivée, et le contrôle des copies s'il est dû.
    let arrival = fond::install(&app, &state);
    state.borrow_mut().arrival_timer = Some(arrival);
    if !mesure {
        let mut st = state.borrow_mut();
        fond::arrivals(&app, &mut st);
        fond::copies_if_due(&app, &mut st);
        majs::check(&mut st);
    }
    distant::show(&app, &state.borrow());
    // Le lien avec le PC principal : une minuterie, seulement si ce PC y est relié.
    let lien_mini = Timer::default();
    if state.borrow().distant.is_some() {
        let s = state.clone();
        lien_mini.start(TimerMode::Repeated, Duration::from_secs(distant::TICK_SECS), move || {
            if let Ok(mut st) = s.try_borrow_mut() {
                distant::tick(&mut st);
            }
        });
    }

    // Le logiciel peut rester ouvert des jours près de l'horloge : la sauvegarde du jour se vérifie chaque heure.
    let sauvegarde = Timer::default();
    if !mesure && state.borrow().distant.is_none() {
        let s = state.clone();
        sauvegarde.start(TimerMode::Repeated, Duration::from_secs(3600), move || {
            if let Ok(mut st) = s.try_borrow_mut() {
                preferences::daily_backup(&mut st);
            }
        });
    }

    // ---------- mesures et autotest
    let _measure = measure::install(&app, &state, &args);

    zone::wire(&app, &state);
    if !mesure {
        zone::start(&app);
    }
    app.show().expect("fenêtre");
    slint::run_event_loop_until_quit().expect("boucle d'interface");
    let _ = app.hide();
    zone::stop();
    // Arrêt propre : un mastering en cours s'arrête (le moteur ne continue pas seul en arrière-plan), la carte se ferme.
    if let Ok(mut st) = state.try_borrow_mut() {
        mastrage::shutdown(&mut st);
        st.ecoute.fermer();
    }
}
