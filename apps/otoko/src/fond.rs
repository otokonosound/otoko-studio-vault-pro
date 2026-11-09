//! Les travaux de fond : le dossier d'arrivée des exports, les statuts automatiques, les liens par morceau, le contrôle
//! des copies et les mini formes d'onde. Aucun ne bloque l'interface ; aucun n'écrit dans la vraie base pendant l'autotest.

use std::cell::RefCell;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use otoko_library::career::arrivee::{self, Landing};
use otoko_library::career::library as lib;
use otoko_library::career::statuts::{self, Change};
use otoko_library::career::{Career, copies, liens, releases};
use slint::{ComponentHandle, Timer, TimerMode};

use crate::state::{Done, State};
use crate::{AppWindow, Rel};

/// L'intervalle entre deux coups d'œil au dossier d'arrivée.
const ARRIVAL_EVERY: Duration = Duration::from_secs(45);

/// L'autotest ne touche pas à la vraie base, et ces travaux appartiennent au PC principal, pas à un PC qui s'y connecte.
fn protected(st: &State) -> bool {
    (st.selftest && st.career_path == crate::career::default_path()) || st.distant.is_some()
}

/// La minuterie du dossier d'arrivée : elle ne tourne que si un dossier est choisi.
pub fn install(app: &AppWindow, state: &Rc<RefCell<State>>) -> Timer {
    let timer = Timer::default();
    let (weak, s) = (app.as_weak(), state.clone());
    timer.start(TimerMode::Repeated, ARRIVAL_EVERY, move || {
        let Some(app) = weak.upgrade() else { return };
        if let Ok(mut st) = s.try_borrow_mut() {
            arrivals(&app, &mut st);
        }
    });
    if !state.borrow().career.as_ref().is_some_and(|c| c.setting(arrivee::DIR_KEY).is_some_and(|d| !d.is_empty())) {
        timer.stop();
    }
    timer
}

// ---------------------------------------------------------------------- le dossier d'arrivée

/// Regarde le dossier d'arrivée et range ce qui y est posé.
pub fn arrivals(_app: &AppWindow, st: &mut State) {
    if protected(st) || st.arrivals_busy {
        return;
    }
    let Some(dir) = st.career.as_ref().and_then(|c| c.setting(arrivee::DIR_KEY)).filter(|d| !d.is_empty()) else { return };
    st.arrivals_busy = true;
    let path = st.career_path.clone();
    st.spawn(move || {
        let Ok(c) = Career::open(&path) else { return Done::Arrivals(Vec::new()) };
        let found = arrivee::pending(c.conn(), Path::new(&dir), SystemTime::now());
        Done::Arrivals(found.iter().filter_map(|p| arrivee::land(c.conn(), p).ok()).collect())
    });
}

pub fn landed(app: &AppWindow, st: &mut State, list: Vec<Landing>) {
    st.arrivals_busy = false;
    if list.is_empty() {
        return;
    }
    st.career = crate::career::open(&st.career_path);
    let text: Vec<String> = list
        .iter()
        .map(|l| match l {
            Landing::Version { title, label, .. } => format!("« {title} » : version {label} rangée"),
            Landing::New { title, .. } => format!("« {title} » : nouveau morceau"),
            Landing::Unsure { title, matches, .. } => format!("« {title} » : {matches} morceaux portent ce nom, à ranger"),
        })
        .collect();
    let more = text.len().saturating_sub(2);
    let head = text.into_iter().take(2).collect::<Vec<_>>().join(" ; ");
    app.set_message(if more > 0 { format!("Exports : {head} et {more} autre(s).") } else { format!("Export : {head}.") }.into());
    // L'export est analysé aussitôt : tempo, tonalité, sonie.
    let todo: Vec<(i64, String)> = st.career.as_ref().map(|c| list.iter().map(|l| (l.id(), lib::field(c.conn(), l.id(), "path"))).collect()).unwrap_or_default();
    crate::analyse::start(app, st, todo, true);
    crate::career::fill(app, st);
}

/// Choisit le dossier d'arrivée (`None` : le retire).
pub fn set_arrival(app: &AppWindow, st: &mut State, dir: Option<PathBuf>) {
    let Some(c) = st.career.as_ref() else { return };
    let value = dir.as_ref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let _ = c.set_setting(arrivee::DIR_KEY, &value);
    if let Some(t) = st.arrival_timer.as_ref() {
        if value.is_empty() {
            t.stop();
        } else {
            t.restart();
        }
    }
    if !value.is_empty() {
        arrivals(app, st);
    }
}

// ---------------------------------------------------------------------- les statuts automatiques

const SEEN_KEY: &str = "auto_status_seen";

/// Fait avancer les morceaux que les données prouvent. La première fois, la liste est montrée et rien n'est appliqué
/// sans accord : le tableau peut beaucoup bouger d'un coup.
pub fn auto_status(app: &AppWindow, st: &mut State) {
    if protected(st) {
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    if !lib::flag(c.conn(), statuts::FLAG, true) {
        return;
    }
    if c.setting(SEEN_KEY).is_none() {
        let list = statuts::plan(c.conn());
        if list.is_empty() {
            let _ = c.set_setting(SEEN_KEY, "1");
        } else if !st.selftest && matches!(st.lib.dlg, crate::outils::Dlg::None) {
            crate::outils::open(app, st, "statuses");
        }
        return;
    }
    let moved = statuts::apply(c.conn());
    tell(app, st, &moved);
}

/// La liste de la première fois a été validée.
pub fn status_seen(st: &State) {
    if let Some(c) = st.career.as_ref() {
        let _ = c.set_setting(SEEN_KEY, "1");
    }
}

fn tell(app: &AppWindow, st: &mut State, moved: &[Change]) {
    match moved {
        [] => return,
        [one] => app.set_message(format!("Statut : {}.", one.text()).into()),
        many => app.set_message(format!("{} morceaux ont changé de colonne dans Production.", many.len()).into()),
    }
    crate::career::fill(app, st);
}

// ---------------------------------------------------------------------- les liens par morceau

/// `curl` est fourni avec Windows : pas de bibliothèque réseau à embarquer. L'en-tête de l'application et le code de liaison
/// ne partent que vers le PC principal (`code`) : un autre site n'a pas à les connaître.
fn curl(code: Option<&str>, args: &[&str]) -> std::process::Command {
    let mut c = std::process::Command::new("curl.exe");
    c.arg("-sL").arg("--fail");
    if let Some(code) = code {
        c.args(["-H", "X-Studio-Vault: 1", "-H"]).arg(format!("{}: {code}", otoko_mobile::server::LINK_HEADER));
    }
    c.args(args).creation_flags(0x0800_0000);
    c
}

/// Lit une page.
fn fetch(url: &str) -> Result<String, String> {
    fetch_from(None, url)
}

fn fetch_from(code: Option<&str>, url: &str) -> Result<String, String> {
    let out = curl(code, &["--max-time", "25", "-A", "Mozilla/5.0", "-w", "\n%{http_code}", url]).output().map_err(|e| format!("curl : {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        // « --fail » : pas de corps, mais le code HTTP reste lisible à la fin.
        return Err(match text.trim().rsplit('\n').next().unwrap_or("") {
            "401" => REFUSED_CODE.to_string(),
            _ => format!("{url} ne répond pas"),
        });
    }
    Ok(text.rsplit_once('\n').map(|(body, _)| body.to_string()).unwrap_or(text))
}

/// Ce que dit le PC principal quand le code de liaison manque ou ne va pas.
pub const REFUSED_CODE: &str = "Le PC principal refuse le code de liaison : recopie celui qu'il affiche (Réglages › Général › Travailler depuis un autre PC).";

pub fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&fetch(url)?).map_err(|e| e.to_string())
}

/// Une lecture chez le PC principal, avec son code de liaison.
pub fn main_json(url: &str, code: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&fetch_from(Some(code), url)?).map_err(|e| e.to_string())
}

/// Télécharge un fichier.
pub fn fetch_file(url: &str, out: &Path) -> Result<(), String> {
    file_from(None, url, out)
}

/// Télécharge un fichier du PC principal, avec son code de liaison.
pub fn main_file(url: &str, out: &Path, code: &str) -> Result<(), String> {
    file_from(Some(code), url, out)
}

fn file_from(code: Option<&str>, url: &str, out: &Path) -> Result<(), String> {
    let path = out.to_string_lossy().into_owned();
    let done = curl(code, &["--max-time", "900", "-o", &path, url]).status().map_err(|e| format!("curl : {e}"))?;
    if done.success() { Ok(()) } else { Err(format!("{url} ne répond pas")) }
}

/// Envoie du JSON au PC principal et rend la réponse.
pub fn post_json(url: &str, body: &serde_json::Value, code: &str) -> Result<serde_json::Value, String> {
    use std::io::Write;
    let mut child = curl(Some(code), &["--max-time", "60", "-H", "Content-Type: application/json", "--data-binary", "@-", "-w", "\n%{http_code}", url])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("curl : {e}"))?;
    child.stdin.take().ok_or("curl")?.write_all(body.to_string().as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, status) = text.rsplit_once('\n').unwrap_or(("", text.as_str()));
    if !out.status.success() {
        return Err(if status.trim() == "401" { REFUSED_CODE.into() } else { "Le PC principal a refusé.".into() });
    }
    serde_json::from_str(body).map_err(|e| e.to_string())
}

/// Le flux complet d'un compte : toutes ses pages.
fn feed_of(user: &str) -> Result<Vec<(String, String)>, String> {
    let mut all = Vec::new();
    let mut next = Some(liens::feed_url(user));
    for _ in 0..30 {
        let Some(url) = next.take() else { break };
        let (list, more) = liens::parse_feed(&fetch(&url)?);
        if list.is_empty() {
            break;
        }
        all.extend(list);
        next = more;
    }
    Ok(all)
}

/// Relie les titres de ces sorties à leur lien (toutes celles qui en manquent si la liste est vide).
pub fn link(app: &AppWindow, st: &mut State, only: Option<i64>) {
    if st.selftest {
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    let list: Vec<(i64, String, String)> = liens::to_link(c.conn()).into_iter().filter(|r| only.is_none_or(|id| r.0 == id)).collect();
    if list.is_empty() {
        app.global::<Rel>().set_msg("Tous les titres ont déjà leur lien.".into());
        return;
    }
    app.global::<Rel>().set_msg(format!("Lecture de SoundCloud pour {} sortie(s)…", list.len()).into());
    let path = st.career_path.clone();
    st.spawn(move || {
        let Ok(c) = Career::open(&path) else { return Done::Links(Err("base indisponible".into())) };
        let mut feeds: std::collections::HashMap<String, Vec<(String, String)>> = Default::default();
        let (mut linked, mut unknown, mut errors) = (0, Vec::new(), Vec::new());
        for (id, title, url) in list {
            let single = !url.contains("/sets/") && releases::load(c.conn(), id).is_some_and(|s| s.tracks.len() == 1);
            let feed: &[(String, String)] = if single {
                &[]
            } else {
                match fetch(&url).ok().and_then(|p| liens::user_id(&p)) {
                    Some(user) => {
                        if !feeds.contains_key(&user) {
                            match feed_of(&user) {
                                Ok(f) => {
                                    feeds.insert(user.clone(), f);
                                }
                                Err(e) => {
                                    errors.push(e);
                                    continue;
                                }
                            }
                        }
                        feeds.get(&user).map(Vec::as_slice).unwrap_or(&[])
                    }
                    None => {
                        errors.push(format!("« {title} » : page illisible"));
                        continue;
                    }
                }
            };
            match liens::link_release(c.conn(), id, feed) {
                Ok(r) => {
                    linked += r.linked;
                    unknown.extend(r.unknown);
                }
                Err(e) => errors.push(e.to_string()),
            }
        }
        Done::Links(Ok((linked, unknown, errors)))
    });
}

pub fn linked(app: &AppWindow, st: &mut State, r: Result<(usize, Vec<String>, Vec<String>), String>) {
    st.career = crate::career::open(&st.career_path);
    let text = match r {
        Ok((n, unknown, errors)) => {
            let mut t = format!("{n} morceau{} relié{} à SoundCloud.", if n > 1 { "x" } else { "" }, if n > 1 { "s" } else { "" });
            if !unknown.is_empty() {
                let some = unknown.iter().take(3).map(|u| format!("« {u} »")).collect::<Vec<_>>().join(", ");
                t.push_str(&format!(" {} titre(s) introuvable(s) sur le compte : {some}{}.", unknown.len(), if unknown.len() > 3 { "…" } else { "" }));
            }
            if let Some(e) = errors.first() {
                t.push_str(&format!(" {e}."));
            }
            t
        }
        Err(e) => format!("Liens impossibles : {e}."),
    };
    app.global::<Rel>().set_msg(text.as_str().into());
    app.set_message(text.into());
    auto_status(app, st);
    crate::career::fill(app, st);
}

// ---------------------------------------------------------------------- le contrôle des copies

/// Compare la bibliothèque aux emplacements de copie.
pub fn check_copies(app: &AppWindow, st: &mut State) {
    if protected(st) || st.copies_busy {
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    let roots = copies::roots(c.conn());
    if roots.is_empty() {
        app.set_message("Choisis d'abord où sont tes copies (Réglages › Analyse et sauvegarde).".into());
        return;
    }
    st.copies_busy = true;
    let path = st.career_path.clone();
    st.spawn(move || {
        let found = copies::index(&roots, &AtomicBool::new(false));
        let r = Career::open(&path).map_err(|e| e.to_string()).and_then(|c| copies::record(c.conn(), &found).map_err(|e| e.to_string()));
        Done::Copies(r)
    });
    crate::preferences::fill(app, st);
}

pub fn copies_done(app: &AppWindow, st: &mut State, r: Result<copies::Report, String>) {
    st.copies_busy = false;
    st.career = crate::career::open(&st.career_path);
    app.set_message(
        match r {
            Ok(r) if r.single == 0 => format!("Copies : les {} morceaux en ont au moins une.", r.checked),
            Ok(r) => format!("Copies : {} morceau{} sur {} n'existe{} qu'à un seul endroit.", r.single, if r.single > 1 { "x" } else { "" }, r.checked, if r.single > 1 { "nt" } else { "" }),
            Err(e) => format!("Contrôle impossible : {e}"),
        }
        .into(),
    );
    crate::preferences::fill(app, st);
    st.pages_lues[0] = false;
    crate::career::page(app, st, app.get_career_tab());
}

/// Au démarrage : le contrôle de la semaine, s'il est dû.
pub fn copies_if_due(app: &AppWindow, st: &mut State) {
    if st.career.as_ref().is_some_and(|c| copies::due(c.conn())) {
        check_copies(app, st);
    }
}

// ---------------------------------------------------------------------- les mini formes d'onde

/// Le nombre de points d'une mini forme d'onde.
pub const WAVE_POINTS: usize = 60;
const WAVE_W: u32 = 120;
const WAVE_H: u32 = 24;

/// Calcule en fond, un morceau à la fois, les ondes qui manquent. Elles se voient au fil de l'eau dans le tableau.
pub fn waves(st: &mut State) {
    if protected(st) || st.waves_busy {
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    let todo = lib::without_wave(c.conn());
    if todo.is_empty() {
        return;
    }
    st.waves_busy = true;
    let db = st.career_path.clone();
    let cache = if st.career_path == crate::career::default_path() { crate::library::data_dir().join("mobile") } else { std::env::temp_dir().join("otoko-essai-pochettes") };
    let tx = st.jobs.clone();
    st.spawn(move || {
        let Ok(c) = Career::open(&db) else { return Done::Waves(true) };
        let media = otoko_mobile::media::Cache::new(cache);
        for (k, (id, path)) in todo.iter().enumerate() {
            let fine: Vec<f32> = media.waveform(Path::new(path), 960).and_then(|v| v["peaks"].as_array().cloned()).map(|a| a.iter().filter_map(|p| p.as_f64()).map(|p| p as f32).collect()).unwrap_or_default();
            // Un fichier illisible n'est pas relu à chaque lancement : sa ligne reste vide.
            let _ = lib::save_wave(c.conn(), *id, &envelope(&fine, WAVE_POINTS));
            if k % 8 == 7 {
                let _ = tx.send(Done::Waves(false));
            }
            // Ce travail n'est jamais pressé : FL Studio tourne peut-être à côté.
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        Done::Waves(true)
    });
}

/// La silhouette d'un morceau : un master est au plafond presque partout, ses crêtes ne disent rien. La moyenne de chaque
/// tranche, étalée entre le passage le plus calme et le plus fort, fait ressortir intro, breaks et drops.
pub fn envelope(peaks: &[f32], points: usize) -> Vec<u8> {
    if peaks.is_empty() {
        return Vec::new();
    }
    let n = peaks.len();
    let mean: Vec<f32> = (0..points)
        .map(|i| {
            let (a, b) = (i * n / points, ((i + 1) * n / points).max(i * n / points + 1).min(n));
            peaks[a.min(n - 1)..b].iter().sum::<f32>() / (b - a.min(n - 1)).max(1) as f32
        })
        .collect();
    let (lo, hi) = mean.iter().fold((f32::MAX, 0.0f32), |(l, h), v| (l.min(*v), h.max(*v)));
    let span = (hi - lo).max(0.02);
    mean.iter().map(|v| ((0.12 + 0.88 * (v - lo) / span).clamp(0.0, 1.0) * 255.0) as u8).collect()
}

pub fn waves_landed(app: &AppWindow, st: &mut State, last: bool) {
    if last {
        st.waves_busy = false;
    } else {
        st.pending += 1;
    }
    st.lib.waves.clear();
    crate::bibliotheque::push_rows(app, st);
}

/// La mini forme d'onde en image : des barres blanches sur fond transparent, teintées par le thème.
pub fn wave_image(peaks: &[u8]) -> slint::Image {
    let (w, h) = (WAVE_W as usize, WAVE_H as usize);
    let mut px = vec![0u8; w * h * 4];
    if !peaks.is_empty() {
        let bar = w / peaks.len().max(1);
        for (i, p) in peaks.iter().enumerate() {
            let tall = (*p as f32 / 255.0 * h as f32).max(1.0) as usize;
            let top = (h - tall.min(h)) / 2;
            for y in top..top + tall.min(h) {
                for x in i * bar..(i * bar + bar.saturating_sub(1).max(1)).min(w) {
                    px[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
    }
    slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&px, WAVE_W, WAVE_H))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_wave_draws_its_peaks() {
        let img = super::wave_image(&[0, 255, 64]);
        let buf = img.to_rgba8().unwrap();
        let px = buf.as_bytes();
        let at = |x: usize, y: usize| px[(y * 120 + x) * 4 + 3];
        // Trois points de 40 px : la pleine va du haut en bas, la nulle ne garde qu'un trait au milieu.
        assert_eq!(at(40, 0), 255);
        assert_eq!(at(40, 23), 255);
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(0, 11), 255);
    }

    #[test]
    fn the_envelope_shows_the_shape_of_a_master() {
        // Une intro calme, puis un master au plafond : la silhouette doit les séparer.
        let peaks: Vec<f32> = (0..960).map(|i| if i < 240 { 0.55 } else { 0.97 }).collect();
        let e = super::envelope(&peaks, 60);
        assert_eq!(e.len(), 60);
        assert!(e[5] < 60 && e[40] > 240, "{} {}", e[5], e[40]);
        assert!(super::envelope(&[], 60).is_empty());
    }
}
