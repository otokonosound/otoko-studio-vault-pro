//! Glisser-déposer avec Windows, dans les deux sens.
//!
//! - Vers l'appli : des fichiers ou des dossiers déposés depuis l'Explorateur. Sur une sortie ouverte, une image devient sa
//!   pochette ; au Mastering, les morceaux rejoignent la liste ; ailleurs, ils entrent dans la bibliothèque (un dossier
//!   devient un dossier surveillé). Slint ne transmet pas ces dépôts : on les lit dans les événements de winit.
//! - Depuis l'appli : un morceau (ou la sélection) tiré hors de la bibliothèque part comme un fichier, vers ton logiciel
//!   de musique, l'Explorateur ou une page d'envoi. Slint ne sait pas le faire : c'est Windows (OLE) qui s'en charge, par la
//!   bibliothèque `drag`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use otoko_library::career::library as lib;
use otoko_library::career::scan::AUDIO_EXTENSIONS;
use slint::ComponentHandle;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};

use crate::state::State;
use crate::AppWindow;

const IMAGES: [&str; 3] = ["jpg", "jpeg", "png"];

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn is_audio(p: &Path) -> bool {
    AUDIO_EXTENSIONS.contains(&ext(p).as_str())
}

fn is_image(p: &Path) -> bool {
    IMAGES.contains(&ext(p).as_str())
}

/// Où tomberait un dépôt maintenant.
enum Target {
    Cover,
    Mastering,
    Library,
}

fn target(app: &AppWindow, st: &State, sample: &Path) -> Target {
    let tab = if app.get_space() == 4 { app.get_career_tab() } else { -1 };
    if tab == 2 && st.rel.sheet.is_some() && is_image(sample) {
        Target::Cover
    } else if tab == 7 && (is_audio(sample) || sample.is_dir()) {
        Target::Mastering
    } else {
        Target::Library
    }
}

fn hover_text(app: &AppWindow, st: &State, sample: &Path) -> String {
    match target(app, st, sample) {
        Target::Cover => "Dépose l'image : elle devient la pochette de la sortie".into(),
        Target::Mastering => "Dépose : les morceaux rejoignent la liste du mastering".into(),
        Target::Library if sample.is_dir() => "Dépose le dossier : il rejoint la bibliothèque (dossier surveillé)".into(),
        Target::Library if is_image(sample) => "Une image se dépose sur une sortie ouverte, pour en faire la pochette".into(),
        Target::Library => "Dépose : les morceaux entrent dans la bibliothèque".into(),
    }
}

/// Écoute les fichiers tirés au-dessus de la fenêtre.
pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    let weak = app.as_weak();
    let state = state.clone();
    app.window().on_winit_window_event(move |_, event| {
        let Some(app) = weak.upgrade() else { return EventResult::Propagate };
        match event {
            WindowEvent::HoveredFile(p) => {
                if app.get_drop_text().is_empty() {
                    let text = hover_text(&app, &state.borrow(), p);
                    app.set_drop_text(text.into());
                }
            }
            WindowEvent::HoveredFileCancelled => app.set_drop_text("".into()),
            WindowEvent::DroppedFile(p) => {
                // Plusieurs fichiers arrivent l'un après l'autre : on les traite ensemble, juste après.
                let first = {
                    let mut st = state.borrow_mut();
                    st.dropped.push(p.clone());
                    st.dropped.len() == 1
                };
                if first {
                    let (weak, state) = (app.as_weak(), state.clone());
                    slint::Timer::single_shot(Duration::from_millis(120), move || {
                        let Some(app) = weak.upgrade() else { return };
                        let mut st = state.borrow_mut();
                        let paths = std::mem::take(&mut st.dropped);
                        dropped(&app, &mut st, paths);
                    });
                }
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

/// Les fichiers audio d'un dossier (et de ses sous-dossiers).
fn audio_in(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            audio_in(&p, out);
        } else if is_audio(&p) {
            out.push(p);
        }
    }
}

pub fn dropped(app: &AppWindow, st: &mut State, paths: Vec<PathBuf>) {
    app.set_drop_text("".into());
    let Some(first) = paths.first().cloned() else { return };
    if st.career.is_none() {
        return;
    }
    match target(app, st, &first) {
        Target::Cover => crate::sorties::set_cover(app, st, &first),
        Target::Mastering => {
            let mut files = Vec::new();
            for p in &paths {
                if p.is_dir() {
                    audio_in(p, &mut files);
                } else if is_audio(p) {
                    files.push(p.clone());
                }
            }
            files.sort();
            let n = crate::mastrage::add(app, st, files.into_iter().map(|f| (f, None)).collect());
            app.set_message(format!("{n} morceau{} ajouté{} au mastering.", if n > 1 { "x" } else { "" }, if n > 1 { "s" } else { "" }).into());
        }
        Target::Library => to_library(app, st, &paths),
    }
}

/// Les dossiers deviennent des dossiers surveillés, les fichiers audio entrent un par un.
fn to_library(app: &AppWindow, st: &mut State, paths: &[PathBuf]) {
    let Some(c) = st.career.as_ref() else { return };
    let (dirs, files): (Vec<&PathBuf>, Vec<&PathBuf>) = paths.iter().partition(|p| p.is_dir());
    let files: Vec<&PathBuf> = files.into_iter().filter(|p| is_audio(p)).collect();
    if dirs.is_empty() && files.is_empty() {
        app.set_message("Rien à ajouter : dépose des fichiers audio ou un dossier (une image se dépose sur une sortie ouverte).".into());
        return;
    }
    let mut folders = 0;
    for d in &dirs {
        if otoko_library::career::scan::add_folder(c.conn(), &d.to_string_lossy()).unwrap_or(false) {
            folders += 1;
        }
    }
    let ignored = otoko_library::career::scan::ignored_folders(c.conn());
    let mut added = Vec::new();
    for f in &files {
        if let Ok((otoko_library::career::scan::Action::Added, id)) = otoko_library::career::scan::upsert(c.conn(), f, &ignored) {
            added.push((id, f.to_string_lossy().into_owned()));
        }
    }
    let mut bits = Vec::new();
    if folders > 0 {
        bits.push(format!("{folders} dossier{} surveillé{} en plus", if folders > 1 { "s" } else { "" }, if folders > 1 { "s" } else { "" }));
    }
    if !files.is_empty() {
        let known = files.len() - added.len();
        bits.push(format!("{} morceau{} ajouté{}", added.len(), if added.len() > 1 { "x" } else { "" }, if added.len() > 1 { "s" } else { "" }));
        if known > 0 {
            bits.push(format!("{known} déjà là"));
        }
    }
    app.set_message(format!("{}.", bits.join(", ")).into());
    let analyze = !added.is_empty() && lib::flag(c.conn(), "auto_analyze", false) && !st.selftest;
    crate::raccourcis::tab(app, st, 1);
    if folders > 0 {
        st.lib.last_scan = None;
        crate::bibliotheque::scan(app, st);
    }
    crate::bibliotheque::fill(app, st);
    if analyze {
        crate::analyse::start(app, st, added, true);
    }
}

/// Un glisser est en cours (le geste envoie plusieurs « moved » avant que Windows ne prenne la main).
static DRAGGING: AtomicBool = AtomicBool::new(false);

/// Tire la ligne (ou la sélection, si la ligne en fait partie) hors de l'appli, comme des fichiers.
pub fn drag_out(app: &AppWindow, st: &mut State, row: usize) {
    let Some(r) = st.lib.rows.get(row) else { return };
    let ids = if st.lib.selected.contains(&r.id) { crate::bibliotheque::selection(st) } else { vec![r.id] };
    let paths: Vec<PathBuf> = st.lib.rows.iter().filter(|r| ids.contains(&r.id)).map(|r| PathBuf::from(&r.path)).filter(|p| p.is_file()).collect();
    if paths.is_empty() {
        app.set_message("Fichier introuvable : il n'y a rien à glisser.".into());
        return;
    }
    if st.selftest || DRAGGING.swap(true, Ordering::SeqCst) {
        return;
    }
    // Hors de ce rappel : Windows tient la souris jusqu'au dépôt, et l'état ne doit pas rester emprunté pendant ce temps.
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(1), move || {
        let Some(app) = weak.upgrade() else {
            DRAGGING.store(false, Ordering::SeqCst);
            return;
        };
        let n = paths.len();
        let r = app.window().with_winit_window(|w| drag::start_drag(w, drag::DragItem::Files(paths), drag::Image::Raw(Vec::new()), |_, _| {}, drag::Options::default()));
        DRAGGING.store(false, Ordering::SeqCst);
        // Le bouton a été relâché chez Windows : Slint ne l'a pas vu. On le lui dit, hors de tout bouton.
        let w = app.window();
        w.dispatch_event(slint::platform::WindowEvent::PointerReleased { position: slint::LogicalPosition::new(-100.0, -100.0), button: slint::platform::PointerEventButton::Left });
        w.dispatch_event(slint::platform::WindowEvent::PointerExited);
        if let Some(Err(e)) = r {
            app.set_message(format!("Glisser impossible ({n} fichier{}) : {e}", if n > 1 { "s" } else { "" }).into());
        }
    });
}
