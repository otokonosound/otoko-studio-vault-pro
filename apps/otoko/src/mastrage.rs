//! Carrière › Mastering (comme l'écran Mastering de Studio Vault) : des morceaux masterisés par le
//! moteur PhaseLimiter (`career::mastering`), un à la fois dans un fil à part, avec les réglages
//! affichés au moment du lancement ; les mesures de l'original et du master ; puis, propre à OTOKO,
//! le master rangé comme version de son morceau dans la bibliothèque (et « À masteriser » →
//! « Terminé »). Rien ne démarre seul, l'original n'est jamais modifié.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use otoko_library::career::library as lib;
use otoko_library::career::mastering::{self as m, Settings};
use serde_json::{Value as Json, json};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::state::{Done, State};
use crate::{AppWindow, MAlbumUi, MJobUi, MMeasureUi, MParamUi, MTileUi, Mast};

/// L'onglet Mastering de la Carrière.
pub const TAB: i32 = 7;

/// Les réglages montrés d'emblée ; les autres sont dans « Réglages avancés ».
const SIMPLE_KEYS: [&str; 5] = ["loudness", "intensity", "use_reference", "reference_audio", "format"];
const TILES: [(&str, &str); 4] = [("loudness", "Volume"), ("true_peak", "Crête réelle"), ("loudness_range", "Dynamique (LRA)"), ("sound_quality2", "Ressemblance pro")];
const AUDIO: [&str; 9] = ["wav", "flac", "mp3", "aiff", "aif", "ogg", "m4a", "aac", "opus"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ready,
    Waiting,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl Status {
    fn key(self) -> &'static str {
        match self {
            Status::Ready => "ready",
            Status::Waiting => "waiting",
            Status::Running => "running",
            Status::Done => "done",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
        }
    }
    fn chip(self) -> &'static str {
        match self {
            Status::Ready => "Prêt",
            Status::Waiting => "En file",
            Status::Running => "En cours",
            Status::Done => "Terminé",
            Status::Failed => "Échec",
            Status::Cancelled => "Annulé",
        }
    }
}

/// Un morceau de la liste.
pub struct Job {
    pub id: i32,
    pub src: PathBuf,
    /// Le morceau de la bibliothèque d'où il vient.
    pub song: Option<i64>,
    pub duration: Option<f64>,
    pub status: Status,
    /// Les réglages figés au lancement.
    pub settings: Settings,
    pub progress: f64,
    pub step: String,
    pub error: String,
    /// Le fichier que ce lancement écrit (effacé s'il est arrêté ou échoue).
    pub planned: PathBuf,
    pub out: Option<m::Outcome>,
    /// Le master rangé dans la bibliothèque.
    pub master_id: Option<i64>,
    pub filed: String,
    pub started: Option<Instant>,
    pub ended: Option<Instant>,
    pub cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn name(&self) -> String {
        self.src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
    fn elapsed(&self) -> f64 {
        match (self.started, self.ended) {
            (Some(a), Some(b)) => b.duration_since(a).as_secs_f64(),
            (Some(a), None) => a.elapsed().as_secs_f64(),
            _ => 0.0,
        }
    }
}

/// Une sortie masterisée d'un bloc : ses morceaux, dans l'ordre de la sortie.
pub struct Album {
    pub title: String,
    pub songs: Vec<i64>,
}

/// L'écart de sonie (en LU) à partir duquel un titre se remarque dans la sortie.
const ALBUM_GAP: f64 = 1.0;

/// Un titre de la sortie, tel que le mastering d'album le compare.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumRow {
    pub title: String,
    pub before: Option<f64>,
    pub after: Option<f64>,
    pub peak: Option<f64>,
    pub state: String,
    pub done: bool,
}

/// La sonie centrale de la sortie : les écarts se comptent depuis elle.
pub fn album_center(rows: &[AlbumRow]) -> Option<f64> {
    let mut v: Vec<f64> = rows.iter().filter_map(|r| r.after).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

fn lu(v: f64) -> String {
    format!("{v:+.1}").replace('.', ",")
}

fn lufs(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}").replace('.', ",")).unwrap_or_else(|| "—".into())
}

/// Le constat sur la sortie : l'avancement, puis les écarts entre titres.
pub fn album_text(rows: &[AlbumRow], mixed: bool) -> (String, bool) {
    let done = rows.iter().filter(|r| r.done).count();
    let mut out = if done == 0 {
        format!("{} titre{}, mêmes réglages pour tous. Lance le mastering pour comparer leur sonie.", rows.len(), if rows.len() > 1 { "s" } else { "" })
    } else if done < rows.len() {
        format!("{done} titre{} sur {} masterisé{}.", if done > 1 { "s" } else { "" }, rows.len(), if done > 1 { "s" } else { "" })
    } else {
        "Tous les titres sont masterisés.".to_string()
    };
    let mut warn = mixed;
    if let Some(c) = album_center(rows) {
        let far: Vec<String> = rows.iter().filter_map(|r| r.after.filter(|a| (a - c).abs() > ALBUM_GAP).map(|a| format!("« {} » ({} LU)", r.title, lu(a - c)))).collect();
        out.push_str(&format!(" Sonie de la sortie : {} LUFS.", lufs(Some(c))));
        if far.is_empty() {
            if done > 1 {
                out.push_str(" Les titres se tiennent à moins de 1 LU.");
            }
        } else {
            warn = true;
            out.push_str(&format!(" S'écarte{} de plus de 1 LU : {}.", if far.len() > 1 { "nt" } else { "" }, far.join(", ")));
        }
    }
    if mixed {
        out.push_str(" Attention : les titres n'ont pas tous été masterisés avec les mêmes réglages.");
    }
    (out, warn)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Le rapport de la sortie : une page autonome, à garder avec les masters.
pub fn album_report(title: &str, date: &str, settings: &str, verdict: &str, rows: &[AlbumRow]) -> String {
    let center = album_center(rows);
    let lines: String = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let gap = match (r.after, center) {
                (Some(a), Some(c)) => format!("{} LU", lu(a - c)),
                _ => "—".into(),
            };
            let far = matches!((r.after, center), (Some(a), Some(c)) if (a - c).abs() > ALBUM_GAP);
            format!(
                "<tr{}><td>{:02}</td><td>{}</td><td>{}</td><td>{}</td><td>{gap}</td><td>{}</td></tr>",
                if far { " class=\"far\"" } else { "" },
                i + 1,
                esc(&r.title),
                lufs(r.before),
                lufs(r.after),
                r.peak.map(|p| format!("{p:.1} dB").replace('.', ",")).unwrap_or_else(|| "—".into()),
            )
        })
        .collect();
    format!(
        "<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><title>Rapport de mastering — {t}</title><style>\
         body{{font:15px/1.5 'Segoe UI',system-ui,sans-serif;color:#1c1c20;max-width:820px;margin:40px auto;padding:0 20px}}\
         h1{{font-size:26px;margin:0}}p.sub{{color:#6b6b73;margin:4px 0 24px}}\
         table{{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}}\
         th{{text-align:left;font-size:12px;color:#6b6b73;border-bottom:1px solid #d9d9df;padding:6px 10px 6px 0}}\
         td{{border-bottom:1px solid #ececf0;padding:7px 10px 7px 0}}tr.far td{{color:#c2410c;font-weight:600}}\
         p.verdict{{margin:20px 0}}p.small{{color:#6b6b73;font-size:13px}}\
         </style></head><body><h1>{t}</h1><p class=\"sub\">Rapport de mastering · {d}</p>\
         <table><tr><th>N°</th><th>Titre</th><th>Avant (LUFS)</th><th>Après (LUFS)</th><th>Écart</th><th>Crête</th></tr>{lines}</table>\
         <p class=\"verdict\">{v}</p><p class=\"small\">Réglages : {s}</p></body></html>",
        t = esc(title),
        d = esc(date),
        v = esc(verdict),
        s = esc(settings),
    )
}

/// Ce que le mastering en cours renvoie à l'interface.
pub enum Event {
    Step(String, f64),
    Finished(Result<m::Outcome, String>),
}

#[derive(Default)]
pub struct View {
    pub jobs: Vec<Job>,
    next_id: i32,
    pub sel: Option<i32>,
    /// La sortie masterisée d'un bloc, s'il y en a une.
    pub album: Option<Album>,
    pub settings: Settings,
    loaded: bool,
    /// La Carrière d'où viennent les réglages (une autre base ouverte : ils sont relus).
    loaded_from: PathBuf,
    pub advanced: bool,
    pub all_measures: bool,
    /// Les références déjà analysées (une fois par session, comme Studio Vault).
    ref_cache: Arc<Mutex<Vec<(String, PathBuf)>>>,
    simple: Rc<VecModel<MParamUi>>,
    more: Rc<VecModel<MParamUi>>,
    /// L'horloge qui fait avancer le temps écoulé du morceau en cours.
    tick: Option<Timer>,
    /// Vrai tant que le fil de mastering tourne (l'arrêt d'OTOKO l'attend).
    active: Arc<AtomicBool>,
}

macro_rules! mast_on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.global::<Mast>().$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

fn say(app: &AppWindow, msg: &str) {
    app.global::<Mast>().set_msg(msg.into());
    app.set_message(msg.into());
}

fn warn(st: &State, title: &str, text: &str) {
    if !st.selftest {
        rfd::MessageDialog::new().set_title(title).set_description(text).set_level(rfd::MessageLevel::Warning).show();
    }
}

/// « 42 s », « 3 min 05 s », « 1 h 02 min », comme Studio Vault.
pub fn duration(sec: f64) -> String {
    let s = sec.round().max(0.0) as i64;
    if s < 60 {
        return format!("{s} s");
    }
    let (mi, se) = (s / 60, s % 60);
    if mi < 60 { format!("{mi} min {se:02} s") } else { format!("{} h {:02} min", mi / 60, mi % 60) }
}

/// Un nombre comme le `:g` de Python : « -9 », « 0.5 ».
fn g(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

fn num(s: &Settings, k: &str) -> f64 {
    match s.get(k) {
        Some(Json::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Json::String(t)) => t.replace(',', ".").parse().unwrap_or(0.0),
        Some(Json::Bool(b)) => *b as i64 as f64,
        _ => 0.0,
    }
}

fn flag(s: &Settings, k: &str) -> bool {
    match s.get(k) {
        Some(Json::Bool(b)) => *b,
        Some(Json::String(t)) => t == "true" || t == "True",
        Some(Json::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

fn text(s: &Settings, k: &str) -> String {
    s.get(k).map(m::text).unwrap_or_default()
}

fn cores() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8)
}

// ---------------------------------------------------------------------- réglages

/// « Mes réglages par défaut » enregistrés (sinon les derniers de Studio Vault).
fn saved(st: &State) -> Settings {
    let raw = st.career.as_ref().map(|c| c.setting(m::DEFAULTS_KEY).filter(|v| !v.trim().is_empty()).or_else(|| c.setting(m::SETTINGS_KEY)).unwrap_or_default()).unwrap_or_default();
    m::load_settings(&raw)
}

fn ensure_loaded(st: &mut State) {
    if !st.mast.loaded || st.mast.loaded_from != st.career_path {
        st.mast.settings = saved(st);
        st.mast.loaded = true;
        st.mast.loaded_from = st.career_path.clone();
    }
}

/// Des réglages repris d'un autre PC (dossier de destination, morceau de référence) peuvent ne pas
/// exister ici : on le dit dès l'ouverture, sans rien changer à la place de l'utilisateur.
fn notice(st: &State) -> String {
    let errors = m::validate(&st.mast.settings);
    if errors.is_empty() { String::new() } else { format!("Réglages à revoir avant de lancer : {}", errors.join(" ")) }
}

/// Le moteur : le dossier choisi (repris de Studio Vault à l'import), puis les emplacements habituels.
pub fn engine(st: &State) -> Option<m::Engine> {
    let chosen = st.career.as_ref().and_then(|c| c.setting(m::ENGINE_KEY)).unwrap_or_default();
    let mut extra: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        extra.push(dir.join("Phaselimiter"));
    }
    extra.push(crate::library::data_dir().join("Phaselimiter"));
    extra.push(otoko_mobile::complements::engine_root());
    if let Some(p) = std::env::var_os("OTOKO_PHASELIMITER") {
        extra.push(PathBuf::from(p));
    }
    m::find_engine(&chosen, &extra)
}

fn param_row(p: &m::Param, s: &Settings) -> MParamUi {
    let v = s.get(p.key).cloned().unwrap_or_else(|| p.default.json());
    let value = match p.kind {
        "bool" => if flag(s, p.key) { "true" } else { "false" }.to_string(),
        "float" if p.slider => format!("{:.2}", num(s, p.key)).replace('.', ","),
        "float" => format!("{:.prec$}", num(s, p.key), prec = if p.step.unwrap_or(1.0) < 0.1 { 2 } else { 1 }).replace('.', ","),
        "int" => format!("{}", num(s, p.key) as i64),
        _ => m::text(&v),
    };
    let cur = m::text(&v);
    let ci = p.choices.iter().position(|c| c.0 == cur);
    MParamUi {
        key: p.key.into(),
        label: p.label.into(),
        kind: p.kind.into(),
        value: value.into(),
        num: num(s, p.key) as f32,
        slider: p.slider,
        choices: model(p.choices.iter().map(|c| SharedString::from(c.1)).collect()),
        choice_index: ci.map(|i| i as i32).unwrap_or(0),
        choice_label: ci.map(|i| p.choices[i].1).unwrap_or("").into(),
        choice_help: ci.map(|i| p.choices[i].2).unwrap_or("").into(),
        help: p.help.into(),
        unit: p.unit.into(),
        placeholder: if p.key == "out_dir" { "À côté de l'original" } else { "Aucun fichier choisi" }.into(),
        disabled: m::is_disabled(p, s),
        note: p.disabled_note.into(),
    }
}

fn section_row(title: &str) -> MParamUi {
    MParamUi { key: SharedString::new(), label: title.to_uppercase().into(), kind: "section".into(), choices: model(Vec::new()), ..Default::default() }
}

fn rows(s: &Settings) -> (Vec<MParamUi>, Vec<MParamUi>) {
    let simple = SIMPLE_KEYS.iter().filter_map(|k| m::param(k)).map(|p| param_row(p, s)).collect();
    let mut more = Vec::new();
    for (sec, title) in m::SECTIONS {
        let ps: Vec<&m::Param> = m::PARAMS.iter().filter(|p| p.section == *sec && !SIMPLE_KEYS.contains(&p.key)).collect();
        if ps.is_empty() {
            continue;
        }
        more.push(section_row(title));
        more.extend(ps.into_iter().map(|p| param_row(p, s)));
    }
    (simple, more)
}

/// Remet une liste de réglages à jour sans tout recréer : une ligne dont la valeur a changé est
/// recréée (ses champs repartent de la nouvelle valeur), les autres sont seulement mises à jour.
/// `keep` : le réglage en cours de saisie, jamais recréé (le curseur reste où il est).
fn sync(list: &VecModel<MParamUi>, new: Vec<MParamUi>, keep: Option<&str>) {
    if list.row_count() != new.len() {
        list.set_vec(new);
        return;
    }
    for (i, mut row) in new.into_iter().enumerate() {
        let Some(old) = list.row_data(i) else { continue };
        // La liste des choix ne change jamais : on garde la même (une liste neuve ferait tout redessiner).
        row.choices = old.choices.clone();
        if old == row {
            continue;
        }
        let same_value = old.value == row.value && old.choice_index == row.choice_index;
        if same_value || keep == Some(row.key.as_str()) {
            list.set_row_data(i, row);
        } else {
            list.remove(i);
            list.insert(i, row);
        }
    }
}

fn push_params(app: &AppWindow, st: &State, keep: Option<&str>) {
    let (simple, more) = rows(&st.mast.settings);
    sync(&st.mast.simple, simple, keep);
    sync(&st.mast.more, more, keep);
    push_header(app, st);
}

/// Les réglages en une ligne.
fn summary(s: &Settings) -> String {
    let unit = if text(s, "reference_mode") == "loudness" { "LUFS" } else { "dB" };
    let reference = text(s, "reference_audio");
    let r = if flag(s, "mastering") && flag(s, "use_reference") && !reference.is_empty() {
        format!("référence « {} »", Path::new(&reference).file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default())
    } else if flag(s, "mastering") {
        "sans référence".into()
    } else {
        "limiteur seul".into()
    };
    let out_dir = text(s, "out_dir");
    let out = if out_dir.is_empty() { "à côté de l'original".to_string() } else { Path::new(&out_dir).file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or(out_dir.clone()) };
    let format = text(s, "format");
    format!(
        "{} {unit} · plafond {} dBFS · intensité {} · {r} · {} {} kHz{} · dossier : {out}",
        g(num(s, "loudness")),
        g(num(s, "ceiling")),
        g(num(s, "intensity")),
        format.to_uppercase(),
        g(num(s, "sample_rate") / 1000.0),
        if format == "wav" { format!(" {} bits", text(s, "bit_depth")) } else { String::new() }
    )
    .replace('.', ",")
}

/// Le résumé, le profil de vitesse, l'estimation, « modifiés », et la barre de lancement.
fn push_header(app: &AppWindow, st: &State) {
    let s = &st.mast.settings;
    let g_ = app.global::<Mast>();
    g_.set_summary(summary(s).into());
    let preset = m::preset_of(s);
    g_.set_presets(model(m::PRESETS.iter().map(|p| SharedString::from(p.0)).collect()));
    g_.set_preset(preset.unwrap_or("").into());
    g_.set_preset_hint(
        match preset.and_then(|p| m::PRESETS.iter().find(|x| x.0 == p)) {
            Some(p) => p.1.to_string(),
            None => format!("Réglage personnalisé : suréchantillonnage {}×, {} essais.", text(s, "oversample"), spaced(num(s, "evaluations") as i64)),
        }
        .into(),
    );
    g_.set_estimate(format!("≈ {} pour 3 min de musique", duration(m::estimate_seconds(s, 190.0))).into());
    let w = num(s, "workers") as usize;
    g_.set_cool(w > 0 && w < cores());
    g_.set_dirty(st.career.is_some() && !m::same_settings(s, &saved(st)));
    g_.set_advanced(st.mast.advanced);
    push_launch(app, st);
}

/// 40000 → « 40 000 ».
fn spaced(n: i64) -> String {
    let t = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in t.chars().enumerate() {
        if i > 0 && (t.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn push_engine(app: &AppWindow, st: &State) {
    let g_ = app.global::<Mast>();
    g_.set_engine_hint(m::ENGINE_HINT.into());
    match engine(st) {
        Some(e) => {
            g_.set_engine_ok(true);
            g_.set_engine_text(format!("✓ Moteur trouvé : {}", e.root.display()).into());
        }
        None => {
            g_.set_engine_ok(false);
            g_.set_engine_text("Le moteur n'est pas encore installé.".into());
        }
    }
}

/// Installe le moteur de mastering, en fond.
fn install_engine(app: &AppWindow, st: &mut State) {
    if st.selftest {
        return;
    }
    app.global::<Mast>().set_engine_busy(true);
    let tx = st.jobs.clone();
    st.spawn(move || {
        let say = |text: String| {
            let _ = tx.send(Done::Say(text));
        };
        Done::EngineDone(otoko_mobile::complements::install_engine(&say).map(|_| ()))
    });
}

/// Le moteur est installé : le mastering est prêt.
pub fn engine_done(app: &AppWindow, st: &mut State, r: Result<(), String>) {
    app.global::<Mast>().set_engine_busy(false);
    push_engine(app, st);
    app.set_message(match r {
        Ok(()) => "Le moteur de mastering est prêt.".into(),
        Err(e) => e.into(),
    });
}

// ---------------------------------------------------------------------- la liste et le résultat

fn estimate(st: &State, j: &Job) -> Option<f64> {
    j.duration.map(|d| m::estimate_seconds(&st.mast.settings, d))
}

fn push_jobs(app: &AppWindow, st: &State) {
    let rows: Vec<MJobUi> = st
        .mast
        .jobs
        .iter()
        .map(|j| {
            let dur = j.duration.map(duration).unwrap_or_else(|| "durée inconnue".into());
            let sub = match j.status {
                Status::Running => format!("{}… {:.0} % · {} écoulées", if j.step.is_empty() { "Préparation" } else { &j.step }, j.progress * 100.0, duration(j.elapsed())),
                Status::Done => format!("Fait en {} → {}", duration(j.elapsed()), j.out.as_ref().and_then(|o| o.dst.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                Status::Failed => j.error.lines().next().filter(|l| !l.is_empty()).unwrap_or("Erreur inconnue").to_string(),
                Status::Waiting => format!("{dur} · passe après le morceau en cours"),
                Status::Cancelled => format!("{dur} · arrêté"),
                Status::Ready => format!("{dur}{}", estimate(st, j).map(|e| format!(" · ≈ {} de calcul", duration(e))).unwrap_or_default()),
            };
            MJobUi { id: j.id, name: j.name().into(), sub: sub.into(), state: j.status.key().into(), chip: j.status.chip().into(), progress: j.progress as f32 }
        })
        .collect();
    let g_ = app.global::<Mast>();
    g_.set_jobs(model(rows));
    g_.set_sel(st.mast.sel.unwrap_or(-1));
}

fn running(st: &State) -> Option<&Job> {
    st.mast.jobs.iter().find(|j| j.status == Status::Running)
}

fn push_launch(app: &AppWindow, st: &State) {
    let g_ = app.global::<Mast>();
    let ready: Vec<&Job> = st.mast.jobs.iter().filter(|j| j.status == Status::Ready).collect();
    let waiting = st.mast.jobs.iter().filter(|j| j.status == Status::Waiting).count();
    g_.set_can_launch(!ready.is_empty());
    g_.set_launch_text(if ready.len() > 1 { format!("Lancer le mastering ({})", ready.len()) } else { "Lancer le mastering".into() }.into());
    g_.set_busy(running(st).is_some());
    g_.set_can_clear(st.mast.jobs.iter().any(|j| j.status != Status::Running));
    let (title, info) = if let Some(cur) = running(st) {
        let rest = if waiting > 0 { format!(" · {waiting} morceau(x) en file") } else { String::new() };
        (
            format!("Mastering en cours : {}", cur.name()),
            format!(
                "{}… {:.0} %{rest}. Tu peux ajouter d'autres morceaux : ils seront prêts pour le prochain lancement.",
                if cur.step.is_empty() { "Préparation" } else { &cur.step },
                cur.progress * 100.0
            ),
        )
    } else if !ready.is_empty() {
        let known: Vec<f64> = ready.iter().filter_map(|j| estimate(st, j)).collect();
        let est = if known.is_empty() { "durée de calcul inconnue".to_string() } else { format!("≈ {} de calcul", duration(known.iter().sum())) };
        (format!("{} morceau(x) prêt(s)", ready.len()), format!("{est} avec les réglages affichés. Vérifie-les à droite, puis lance."))
    } else {
        let done = st.mast.jobs.iter().filter(|j| j.status == Status::Done).count();
        ("Rien à lancer".into(), if done > 0 { format!("{done} master(s) terminé(s). Ajoute des morceaux pour continuer.") } else { "Ajoute des morceaux pour commencer.".into() })
    };
    g_.set_launch_title(title.into());
    g_.set_launch_info(info.into());
}

fn measure_of(list: &[(&'static str, Option<f64>)], key: &str) -> Option<f64> {
    list.iter().find(|x| x.0 == key).and_then(|x| x.1)
}

fn push_result(app: &AppWindow, st: &State) {
    let g_ = app.global::<Mast>();
    let Some(j) = st.mast.sel.and_then(|id| st.mast.jobs.iter().find(|j| j.id == id)) else {
        g_.set_has_result(false);
        return;
    };
    let done = j.status == Status::Done;
    g_.set_has_result(true);
    g_.set_result_title(j.name().into());
    g_.set_result_failed(j.status == Status::Failed);
    let info = match j.status {
        Status::Failed => format!("Échec : {}", j.error),
        Status::Done => {
            let o = j.out.as_ref();
            let mut t = format!("Enregistré : {}", o.map(|o| o.dst.display().to_string()).unwrap_or_default());
            if let Some(n) = o.map(|o| o.note.as_str()).filter(|n| !n.is_empty()) {
                t.push('\n');
                t.push_str(n);
            }
            t
        }
        Status::Running => "Mastering en cours… Les mesures s'afficheront à la fin.".into(),
        _ => "Pas encore masterisé. Tu peux déjà écouter l'original.".into(),
    };
    g_.set_result_info(info.into());
    let (before, after) = j.out.as_ref().map(|o| (o.before.clone(), o.after.clone())).unwrap_or_default();
    g_.set_show_tiles(done || !before.is_empty());
    g_.set_tiles(model(
        TILES
            .iter()
            .map(|(k, label)| {
                let b = measure_of(&before, k);
                MTileUi {
                    label: (*label).into(),
                    value: m::format_measure(k, measure_of(&after, k)).into(),
                    before: if b.is_some() { format!("original : {}", m::format_measure(k, b)) } else { String::new() }.into(),
                    help: m::MEASURES.iter().find(|x| x.0 == *k).map(|x| x.4).unwrap_or("").into(),
                }
            })
            .collect(),
    ));
    g_.set_measures(model(
        m::MEASURES
            .iter()
            .map(|(k, label, _, _, help)| MMeasureUi {
                label: (*label).into(),
                before: m::format_measure(k, measure_of(&before, k)).into(),
                after: m::format_measure(k, measure_of(&after, k)).into(),
                help: (*help).into(),
            })
            .collect(),
    ));
    g_.set_all_measures(st.mast.all_measures);
    g_.set_can_play_out(done);
    g_.set_can_log(j.out.as_ref().is_some_and(|o| !o.command.is_empty()) || !j.error.is_empty());
    g_.set_attach_text(j.filed.as_str().into());
    g_.set_can_compare(done && j.master_id.is_some() && j.song.is_some());
}

fn push_all(app: &AppWindow, st: &State) {
    push_jobs(app, st);
    push_launch(app, st);
    push_result(app, st);
    push_album(app, st);
}

/// Le dernier passage de ce morceau dans la liste.
fn job_of(st: &State, song: i64) -> Option<&Job> {
    st.mast.jobs.iter().rev().find(|j| j.song == Some(song))
}

/// Les titres de la sortie : mesures du master s'il existe, sonie de l'analyse sinon.
fn album_rows(st: &State) -> Vec<AlbumRow> {
    let Some(a) = st.mast.album.as_ref() else { return Vec::new() };
    a.songs
        .iter()
        .map(|id| {
            let row = st.career.as_ref().and_then(|c| lib::row_by_id(c.conn(), *id).ok().flatten());
            let job = job_of(st, *id);
            let out = job.and_then(|j| j.out.as_ref());
            AlbumRow {
                title: row.as_ref().map(|r| r.title.clone()).unwrap_or_default(),
                before: out.and_then(|o| measure_of(&o.before, "loudness")).or(row.as_ref().and_then(|r| r.lufs)),
                after: out.and_then(|o| measure_of(&o.after, "loudness")),
                peak: out.and_then(|o| measure_of(&o.after, "true_peak")),
                state: job.map(|j| j.status.chip()).unwrap_or("Introuvable").to_string(),
                done: job.is_some_and(|j| j.status == Status::Done),
            }
        })
        .collect()
}

/// Les titres finis n'ont pas tous reçu les mêmes réglages.
fn album_mixed(st: &State) -> bool {
    let Some(a) = st.mast.album.as_ref() else { return false };
    let done: Vec<&Settings> = a.songs.iter().filter_map(|id| job_of(st, *id)).filter(|j| j.status == Status::Done).map(|j| &j.settings).collect();
    done.windows(2).any(|w| !m::same_settings(w[0], w[1]))
}

fn push_album(app: &AppWindow, st: &State) {
    let g_ = app.global::<Mast>();
    let Some(a) = st.mast.album.as_ref() else {
        g_.set_album_on(false);
        return;
    };
    let rows = album_rows(st);
    let center = album_center(&rows);
    let (verdict, warn) = album_text(&rows, album_mixed(st));
    g_.set_album_on(true);
    g_.set_album_title(a.title.as_str().into());
    g_.set_album_text(verdict.into());
    g_.set_album_warn(warn);
    g_.set_album_can_report(rows.iter().any(|r| r.done));
    g_.set_album(model(
        rows.iter()
            .map(|r| {
                let gap = r.after.zip(center).map(|(a, c)| a - c);
                MAlbumUi {
                    title: r.title.as_str().into(),
                    before: lufs(r.before).into(),
                    after: lufs(r.after).into(),
                    gap: gap.map(|x| format!("{} LU", lu(x))).unwrap_or_else(|| "—".into()).into(),
                    state: r.state.as_str().into(),
                    far: gap.is_some_and(|x| x.abs() > ALBUM_GAP),
                }
            })
            .collect(),
    ));
}

/// Écrit le rapport à côté des masters et l'ouvre.
fn write_report(app: &AppWindow, st: &mut State) {
    let Some(title) = st.mast.album.as_ref().map(|a| a.title.clone()) else { return };
    let rows = album_rows(st);
    let (verdict, _) = album_text(&rows, album_mixed(st));
    let first = st.mast.album.as_ref().and_then(|a| a.songs.iter().filter_map(|id| job_of(st, *id)).find(|j| j.status == Status::Done));
    let Some(job) = first else { return };
    let settings = summary(&job.settings);
    let dir = if st.selftest { std::env::temp_dir() } else { job.out.as_ref().and_then(|o| o.dst.parent().map(Path::to_path_buf)).unwrap_or_else(std::env::temp_dir) };
    let safe: String = title.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let path = dir.join(format!("Rapport de mastering - {safe}.html"));
    let html = album_report(&title, &crate::career::fr_date(&crate::career::today()), &settings, &verdict, &rows);
    match std::fs::write(&path, html) {
        Ok(()) => {
            say(app, &format!("Rapport écrit : {}", path.display()));
            if !st.selftest {
                let _ = std::process::Command::new("explorer.exe").arg(&path).spawn();
            }
        }
        Err(e) => say(app, &format!("Rapport impossible : {e}")),
    }
}

/// L'onglet Mastering, quand on l'ouvre.
pub fn fill(app: &AppWindow, st: &mut State) {
    ensure_loaded(st);
    let (simple, more) = rows(&st.mast.settings);
    st.mast.simple.set_vec(simple);
    st.mast.more.set_vec(more);
    push_header(app, st);
    push_engine(app, st);
    push_all(app, st);
    if !st.mast.jobs.iter().any(|j| j.status == Status::Running) {
        app.global::<Mast>().set_msg(notice(st).into());
    }
}

// ---------------------------------------------------------------------- les morceaux

/// Ajoute des fichiers à la liste, sans lancer le mastering. Rend le nombre ajouté.
pub fn add(app: &AppWindow, st: &mut State, files: Vec<(PathBuf, Option<i64>)>) -> usize {
    ensure_loaded(st);
    let mut n = 0;
    let mut last = None;
    for (path, song) in files {
        let busy = |j: &Job| j.src == path && matches!(j.status, Status::Ready | Status::Waiting | Status::Running);
        if !path.is_file() || st.mast.jobs.iter().any(busy) {
            continue;
        }
        let song = song.or_else(|| st.career.as_ref().and_then(|c| m::track_of(c.conn(), &path)));
        let duration = crate::samples::probe(&path.to_string_lossy()).map(|p| p.0);
        let id = st.mast.next_id;
        st.mast.next_id += 1;
        st.mast.jobs.push(Job {
            id,
            src: path,
            song,
            duration,
            status: Status::Ready,
            settings: Settings::new(),
            progress: 0.0,
            step: String::new(),
            error: String::new(),
            planned: PathBuf::new(),
            out: None,
            master_id: None,
            filed: String::new(),
            started: None,
            ended: None,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        n += 1;
        last = Some(id);
    }
    if last.is_some() {
        st.mast.sel = last;
        say(app, &format!("{n} morceau(x) ajouté(s). Vérifie les réglages, puis « Lancer le mastering »."));
    }
    push_all(app, st);
    n
}

/// Des morceaux de la bibliothèque : ajoutés à la liste, l'onglet Mastering ouvert.
pub fn add_songs(app: &AppWindow, st: &mut State, ids: &[i64]) {
    let files: Vec<(PathBuf, Option<i64>)> = st
        .career
        .as_ref()
        .map(|c| ids.iter().filter_map(|id| lib::row_by_id(c.conn(), *id).ok().flatten().map(|r| (PathBuf::from(r.path), Some(*id)))).collect())
        .unwrap_or_default();
    let missing = files.iter().filter(|f| !f.0.is_file()).count();
    crate::raccourcis::tab(app, st, TAB);
    let n = add(app, st, files);
    if missing > 0 {
        say(app, &format!("{n} morceau(x) ajouté(s) ; {missing} introuvable(s) sur le disque."));
    }
}

/// Une sortie entière, dans son ordre : mêmes réglages pour tous, écarts de sonie comparés, un rapport à la fin. Un
/// titre déjà rangé comme master repart de son morceau d'origine.
pub fn add_release(app: &AppWindow, st: &mut State, release: i64) {
    let Some(c) = st.career.as_ref() else { return };
    let Some(sheet) = otoko_library::career::releases::load(c.conn(), release) else { return };
    let source = |id: i64| c.conn().query_row("SELECT parent_id FROM tracks WHERE id = ?1 AND version_label = 'master'", [id], |r| r.get::<_, Option<i64>>(0)).ok().flatten().unwrap_or(id);
    let songs: Vec<i64> = sheet.tracks.iter().map(|id| source(*id)).collect();
    st.mast.album = Some(Album { title: sheet.title, songs: songs.clone() });
    add_songs(app, st, &songs);
    push_album(app, st);
}

fn add_dialog(app: &AppWindow, st: &mut State) {
    if st.selftest {
        return;
    }
    let Some(files) = rfd::FileDialog::new().set_title("Morceaux à masteriser").add_filter("Audio", &AUDIO).pick_files() else { return };
    add(app, st, files.into_iter().map(|f| (f, None)).collect());
}

fn from_library(app: &AppWindow, st: &mut State) {
    let ids = crate::bibliotheque::selection(st);
    if ids.is_empty() {
        let t = "Sélectionne un ou plusieurs morceaux dans la Bibliothèque, puis reviens ici (ou fais clic droit › « Ajouter au mastering »).";
        if st.selftest {
            say(app, t);
        } else {
            rfd::MessageDialog::new().set_title("Sélection vide").set_description(t).show();
        }
        return;
    }
    add_songs(app, st, &ids);
}

fn job_mut(st: &mut State, id: i32) -> Option<&mut Job> {
    st.mast.jobs.iter_mut().find(|j| j.id == id)
}

fn job_action(app: &AppWindow, st: &mut State, id: i32, what: &str) {
    let Some(j) = job_mut(st, id) else { return };
    match (what, j.status) {
        ("cancel", Status::Running) => j.cancel.store(true, Ordering::Relaxed),
        ("cancel", Status::Waiting) => j.status = Status::Ready,
        ("requeue", Status::Done | Status::Failed | Status::Cancelled) => {
            j.status = Status::Ready;
            j.progress = 0.0;
            j.step.clear();
        }
        ("remove", s) if s != Status::Running => {
            st.mast.jobs.retain(|j| j.id != id);
            if st.mast.sel == Some(id) {
                st.mast.sel = st.mast.jobs.last().map(|j| j.id);
            }
        }
        _ => return,
    }
    push_all(app, st);
}

/// Lance tous les morceaux prêts, avec les réglages affichés à cet instant.
pub fn launch(app: &AppWindow, st: &mut State) {
    ensure_loaded(st);
    if st.distant.is_some() {
        say(app, "Le mastering se lance sur le PC principal, où sont les fichiers et le moteur.");
        return;
    }
    if !st.mast.jobs.iter().any(|j| j.status == Status::Ready) {
        return;
    }
    let errors = m::validate(&st.mast.settings);
    if !errors.is_empty() {
        let t = errors.iter().map(|e| format!("• {e}")).collect::<Vec<_>>().join("\n");
        warn(st, "Réglages à corriger", &t);
        say(app, &format!("Réglages à corriger : {}", errors.join(" ")));
        return;
    }
    if engine(st).is_none() {
        warn(st, "Moteur introuvable", m::ENGINE_HINT);
        say(app, "Moteur introuvable : Réglages avancés › Moteur.");
        return;
    }
    let s = st.mast.settings.clone();
    for j in st.mast.jobs.iter_mut().filter(|j| j.status == Status::Ready) {
        j.settings = s.clone();
        j.status = Status::Waiting;
        j.error.clear();
        j.progress = 0.0;
        j.step.clear();
        j.out = None;
        j.master_id = None;
        j.filed.clear();
        j.started = None;
        j.ended = None;
        j.cancel = Arc::new(AtomicBool::new(false));
    }
    // Les derniers réglages utilisés (comme l'ancienne version de Studio Vault).
    if let Some(c) = st.career.as_ref() {
        let _ = c.set_setting(m::SETTINGS_KEY, &m::to_json(&s));
    }
    start_next(app, st);
}

/// Le morceau suivant de la file, s'il n'y en a pas déjà un en cours.
fn start_next(app: &AppWindow, st: &mut State) {
    if running(st).is_some() {
        return;
    }
    let Some(i) = st.mast.jobs.iter().position(|j| j.status == Status::Waiting) else {
        if let Some(t) = st.mast.tick.as_ref() {
            t.stop();
        }
        push_all(app, st);
        return;
    };
    let Some(e) = engine(st) else {
        for j in st.mast.jobs.iter_mut().filter(|j| j.status == Status::Waiting) {
            j.status = Status::Ready;
        }
        say(app, "Moteur introuvable : Réglages avancés › Moteur.");
        push_all(app, st);
        return;
    };
    let j = &mut st.mast.jobs[i];
    j.status = Status::Running;
    j.started = Some(Instant::now());
    j.planned = m::output_path(&j.src, &j.settings);
    let (id, src, s, cancel) = (j.id, j.src.clone(), j.settings.clone(), j.cancel.clone());
    let (cache, active, tx) = (st.mast.ref_cache.clone(), st.mast.active.clone(), st.jobs.clone());
    let tmp = std::env::temp_dir().join("otoko-mastering");
    active.store(true, Ordering::Relaxed);
    st.spawn(move || {
        let mut rep = Reporter { tx, id, label: String::new(), last: Instant::now() - Duration::from_secs(1) };
        let mut refs = cache.lock().map(|g| g.clone()).unwrap_or_default();
        let r = m::master(&e, &src, &s, &tmp, &mut refs, &cancel, &mut rep);
        if let Ok(mut g) = cache.lock() {
            *g = refs;
        }
        active.store(false, Ordering::Relaxed);
        Done::Mastering(id, Event::Finished(r))
    });
    if let Some(t) = st.mast.tick.as_ref() {
        t.restart();
    }
    push_all(app, st);
}

/// L'avancement, renvoyé à l'interface au plus quatre fois par seconde (et à chaque étape).
struct Reporter {
    tx: mpsc::Sender<Done>,
    id: i32,
    label: String,
    last: Instant,
}

impl m::Progress for Reporter {
    fn step(&mut self, label: &str, progress: f64) {
        if label != self.label || self.last.elapsed() >= Duration::from_millis(250) {
            self.label = label.to_string();
            self.last = Instant::now();
            let _ = self.tx.send(Done::Mastering(self.id, Event::Step(label.to_string(), progress)));
        }
    }
    fn log(&mut self, _line: &str) {}
}

/// Ce que renvoie le fil de mastering.
pub fn event(app: &AppWindow, st: &mut State, id: i32, ev: Event) {
    match ev {
        Event::Step(label, p) => {
            st.pending += 1;
            if let Some(j) = job_mut(st, id) {
                if !label.is_empty() {
                    j.step = label;
                }
                j.progress = p;
            }
            push_jobs(app, st);
            push_launch(app, st);
        }
        Event::Finished(r) => {
            let Some(j) = job_mut(st, id) else { return start_next(app, st) };
            j.ended = Some(Instant::now());
            let cancelled = j.cancel.load(Ordering::Relaxed);
            match r {
                Ok(out) => {
                    j.status = Status::Done;
                    j.progress = 1.0;
                    j.out = Some(out);
                }
                Err(e) => {
                    j.status = if cancelled || e == "annulé" { Status::Cancelled } else { Status::Failed };
                    j.error = if j.status == Status::Failed { e } else { String::new() };
                    // Un master à moitié écrit ne reste pas à côté de l'original.
                    if j.planned.is_file() {
                        let _ = std::fs::remove_file(&j.planned);
                    }
                }
            }
            let (status, name) = (j.status, j.name());
            match status {
                Status::Done => {
                    file_away(app, st, id);
                    let cur_done = st.mast.sel.and_then(|s| st.mast.jobs.iter().find(|j| j.id == s)).is_some_and(|j| j.status == Status::Done && j.id != id);
                    if !cur_done {
                        st.mast.sel = Some(id);
                    }
                    let dst = st.mast.jobs.iter().find(|j| j.id == id).and_then(|j| j.out.as_ref()).and_then(|o| o.dst.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let filed = st.mast.jobs.iter().find(|j| j.id == id).map(|j| j.filed.clone()).unwrap_or_default();
                    say(app, &format!("Master prêt : {dst}.{}", if filed.is_empty() { String::new() } else { format!(" {filed}") }));
                }
                Status::Failed => say(app, &format!("Mastering impossible : {name}")),
                _ => say(app, &format!("Mastering arrêté : {name}")),
            }
            start_next(app, st);
            push_all(app, st);
        }
    }
}

/// Range le master avec son morceau dans la bibliothèque (s'il vient de la bibliothèque).
fn file_away(app: &AppWindow, st: &mut State, id: i32) {
    let Some(j) = st.mast.jobs.iter().find(|j| j.id == id) else { return };
    let (Some(c), Some(out)) = (st.career.as_ref(), j.out.as_ref()) else { return };
    let Some(parent) = j.song.or_else(|| m::track_of(c.conn(), &j.src)) else { return };
    let title = lib::row_by_id(c.conn(), parent).ok().flatten().map(|r| r.title).unwrap_or_default();
    match m::register_master(c.conn(), parent, &out.dst, &out.before, &out.after) {
        Ok((mid, finished)) => {
            let filed = format!(
                "Rangé dans la bibliothèque avec « {title} » (version master){}.",
                if finished { " ; le morceau passe de « À masteriser » à « Terminé »" } else { "" }
            );
            if let Some(j) = job_mut(st, id) {
                j.song = Some(parent);
                j.master_id = Some(mid);
                j.filed = filed;
            }
            crate::career::fill(app, st);
        }
        Err(e) => {
            if let Some(j) = job_mut(st, id) {
                j.filed = format!("Le master n'a pas pu être rangé dans la bibliothèque ({e}).");
            }
        }
    }
}

/// Arrête le morceau en cours ; ceux qui attendaient redeviennent « prêts ».
fn stop_all(app: &AppWindow, st: &mut State) {
    for j in st.mast.jobs.iter_mut() {
        match j.status {
            Status::Waiting => j.status = Status::Ready,
            Status::Running => j.cancel.store(true, Ordering::Relaxed),
            _ => {}
        }
    }
    push_all(app, st);
}

/// À la fermeture d'OTOKO : le moteur s'arrête (il ne continue pas seul en arrière-plan).
pub fn shutdown(st: &mut State) {
    for j in &st.mast.jobs {
        j.cancel.store(true, Ordering::Relaxed);
    }
    let t = Instant::now();
    while st.mast.active.load(Ordering::Relaxed) && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn current(st: &State) -> Option<&Job> {
    st.mast.sel.and_then(|id| st.mast.jobs.iter().find(|j| j.id == id))
}

fn play(app: &AppWindow, st: &mut State, which: &str) {
    let Some(j) = current(st) else { return };
    let (id, path, label) = if which == "out" {
        let Some(o) = j.out.as_ref() else { return };
        (j.master_id, o.dst.clone(), "Master")
    } else {
        (j.song, j.src.clone(), "Original")
    };
    let name = j.name();
    st.player.compare = None;
    match id {
        Some(id) => crate::lecteur::play_id(app, st, id, 0.0),
        None => crate::lecteur::play_path(app, st, &path, &format!("{label} · {name}")),
    }
}

// ---------------------------------------------------------------------- les réglages, modifiés

fn set(st: &mut State, key: &str, v: Json) {
    st.mast.settings.insert(key.to_string(), v);
    // Format changé : la fréquence la plus proche que ce format accepte.
    if key == "format" {
        let allowed = m::SAMPLE_RATES.iter().find(|r| r.0 == text(&st.mast.settings, "format")).map(|r| r.1).unwrap_or(&[]);
        let sr = num(&st.mast.settings, "sample_rate") as i64;
        if !allowed.is_empty() && !allowed.contains(&sr.to_string().as_str()) {
            if let Some(best) = allowed.iter().min_by_key(|r| (r.parse::<i64>().unwrap_or(0) - sr).abs()) {
                st.mast.settings.insert("sample_rate".into(), json!(best));
            }
        }
    }
}

fn param_text(app: &AppWindow, st: &mut State, key: &str, t: &str) {
    let Some(p) = m::param(key) else { return };
    let Ok(v) = t.trim().replace(',', ".").replace(' ', "").parse::<f64>() else { return };
    let (lo, hi) = (p.min.unwrap_or(f64::MIN), p.max.unwrap_or(f64::MAX));
    if v < lo || v > hi {
        let unit = if p.unit.is_empty() { String::new() } else { format!(" {}", p.unit) };
        app.global::<Mast>().set_msg(format!("{} : entre {} et {}{unit}.", p.label, g(lo).replace('.', ","), g(hi).replace('.', ",")).into());
        return;
    }
    set(st, key, if p.kind == "int" { json!(v.round() as i64) } else { json!(v) });
    app.global::<Mast>().set_msg("".into());
    push_params(app, st, Some(key));
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    // L'horloge du temps écoulé : tourne tant qu'un morceau est en cours.
    let timer = Timer::default();
    {
        let weak = app.as_weak();
        let s = state.clone();
        timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(app) = weak.upgrade() else { return };
            let Ok(st) = s.try_borrow() else { return };
            if running(&st).is_some() {
                push_jobs(&app, &st);
                push_launch(&app, &st);
            } else if let Some(t) = st.mast.tick.as_ref() {
                t.stop();
            }
        });
        timer.stop();
    }
    {
        let mut st = state.borrow_mut();
        st.mast.tick = Some(timer);
        let g_ = app.global::<Mast>();
        g_.set_simple(ModelRc::from(st.mast.simple.clone()));
        g_.set_more(ModelRc::from(st.mast.more.clone()));
    }
    mast_on!(app, state, on_add_files, |app, st| { add_dialog(&app, st); });
    mast_on!(app, state, on_add_library, |app, st| { from_library(&app, st); });
    mast_on!(app, state, on_clear, |app, st| {
        st.mast.jobs.retain(|j| j.status == Status::Running);
        st.mast.sel = st.mast.jobs.first().map(|j| j.id);
        st.mast.album = None;
        push_all(&app, st);
    });
    mast_on!(app, state, on_album_close, |app, st| {
        st.mast.album = None;
        push_album(&app, st);
    });
    mast_on!(app, state, on_album_report, |app, st| { write_report(&app, st); });
    mast_on!(app, state, on_launch, |app, st| { launch(&app, st); });
    mast_on!(app, state, on_stop, |app, st| { stop_all(&app, st); });
    mast_on!(app, state, on_select, |app, st, id| {
        st.mast.sel = Some(id);
        push_jobs(&app, st);
        push_result(&app, st);
    });
    mast_on!(app, state, on_job_action, |app, st, id, what| { job_action(&app, st, id, what.as_str()); });
    mast_on!(app, state, on_play, |app, st, which| { play(&app, st, which.as_str()); });
    mast_on!(app, state, on_compare, |app, st| {
        let Some((a, b)) = current(st).and_then(|j| Some((j.song?, j.master_id?))) else { return };
        crate::lecteur::compare(&app, st, &[a, b]);
        say(&app, "Original (A) et master (B) à volume égal : « A | B » pour passer de l'un à l'autre au même endroit.");
    });
    mast_on!(app, state, on_reveal, |app, st| {
        let Some(dst) = current(st).and_then(|j| j.out.as_ref()).map(|o| o.dst.clone()) else { return };
        if !st.selftest {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", dst.display())).spawn();
        }
        let _ = app;
    });
    mast_on!(app, state, on_show_library, |app, st| {
        if let Some(id) = current(st).and_then(|j| j.master_id) {
            crate::bibliotheque::reveal(&app, st, id);
        }
    });
    mast_on!(app, state, on_toggle_all_measures, |app, st| {
        st.mast.all_measures = !st.mast.all_measures;
        push_result(&app, st);
    });
    mast_on!(app, state, on_show_log, |app, st| {
        let Some(j) = current(st) else { return };
        let mut t = j.out.as_ref().map(|o| o.command.clone()).unwrap_or_default();
        if !j.error.is_empty() {
            t.push_str(&format!("\n\n{}", j.error));
        }
        if let Some(o) = j.out.as_ref() {
            let from = o.log.len().saturating_sub(200);
            t.push_str(&format!("\n\n{}", o.log[from..].join("\n")));
        }
        let g_ = app.global::<Mast>();
        g_.set_log_title(format!("Détails techniques — {}", j.name()).into());
        g_.set_log_text(t.trim().into());
        g_.set_log_open(true);
    });
    mast_on!(app, state, on_close_log, |app, st| {
        let _ = st;
        app.global::<Mast>().set_log_open(false);
    });
    mast_on!(app, state, on_set_preset, |app, st, label| {
        ensure_loaded(st);
        m::apply_preset(&mut st.mast.settings, label.as_str());
        push_params(&app, st, None);
    });
    mast_on!(app, state, on_toggle_cool, |app, st| {
        ensure_loaded(st);
        let w = num(&st.mast.settings, "workers") as usize;
        let on = w > 0 && w < cores();
        set(st, "workers", json!(if on { 0 } else { (cores() / 2).max(1) }));
        push_params(&app, st, None);
    });
    mast_on!(app, state, on_toggle_advanced, |app, st| {
        st.mast.advanced = !st.mast.advanced;
        app.global::<Mast>().set_advanced(st.mast.advanced);
    });
    mast_on!(app, state, on_param_text, |app, st, key, t| { param_text(&app, st, key.as_str(), t.as_str()); });
    mast_on!(app, state, on_param_num, |app, st, key, v| {
        // Le curseur va de 0 à 1 par pas de 0,05.
        set(st, key.as_str(), json!(((v as f64) * 20.0).round() / 20.0));
        push_params(&app, st, Some(key.as_str()));
    });
    mast_on!(app, state, on_param_choice, |app, st, key, i| {
        let Some(c) = m::param(key.as_str()).and_then(|p| p.choices.get(i.max(0) as usize)) else { return };
        set(st, key.as_str(), json!(c.0));
        push_params(&app, st, Some(key.as_str()));
    });
    mast_on!(app, state, on_param_toggle, |app, st, key| {
        let v = !flag(&st.mast.settings, key.as_str());
        set(st, key.as_str(), json!(v));
        push_params(&app, st, None);
    });
    mast_on!(app, state, on_param_path, |app, st, key| {
        if st.selftest {
            return;
        }
        let picked = if key == "out_dir" {
            rfd::FileDialog::new().set_title("Dossier de destination des masters").pick_folder()
        } else {
            rfd::FileDialog::new().set_title("Morceau de référence").add_filter("Audio", &AUDIO).pick_file()
        };
        if let Some(p) = picked {
            set(st, key.as_str(), json!(p.to_string_lossy()));
            push_params(&app, st, None);
        }
    });
    mast_on!(app, state, on_param_clear, |app, st, key| {
        set(st, key.as_str(), json!(""));
        push_params(&app, st, None);
    });
    mast_on!(app, state, on_install_engine, |app, st| { install_engine(&app, st); });
    mast_on!(app, state, on_choose_engine, |app, st| {
        if st.selftest {
            return;
        }
        let Some(d) = rfd::FileDialog::new().set_title("Dossier du moteur PhaseLimiter").pick_folder() else { return };
        let mut e = m::Engine { root: d.clone() };
        if !e.ok() {
            if let Some(parent) = d.parent().map(|p| m::Engine { root: p.to_path_buf() }).filter(|p| p.ok()) {
                e = parent;
            }
        }
        if !e.ok() {
            warn(st, "Moteur", "Ce dossier ne contient pas le moteur PhaseLimiter (ffmpeg.exe et phaselimiter\\bin\\phase_limiter.exe).");
            return;
        }
        if let Some(c) = st.career.as_ref() {
            let _ = c.set_setting(m::ENGINE_KEY, &e.root.to_string_lossy());
        }
        push_engine(&app, st);
    });
    mast_on!(app, state, on_save_defaults, |app, st| {
        let errors = m::validate(&st.mast.settings);
        if !errors.is_empty() {
            warn(st, "Réglages à corriger", &errors.iter().map(|e| format!("• {e}")).collect::<Vec<_>>().join("\n"));
            return;
        }
        if let Some(c) = st.career.as_ref() {
            let _ = c.set_setting(m::DEFAULTS_KEY, &m::to_json(&st.mast.settings));
        }
        push_header(&app, st);
        say(&app, "Réglages de mastering enregistrés comme réglages par défaut.");
    });
    mast_on!(app, state, on_restore_defaults, |app, st| {
        st.mast.settings = saved(st);
        push_params(&app, st, None);
    });
    mast_on!(app, state, on_factory, |app, st| {
        st.mast.settings = m::defaults();
        push_params(&app, st, None);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_numbers_read_like_studio_vault() {
        assert_eq!(duration(42.4), "42 s");
        assert_eq!(duration(185.0), "3 min 05 s");
        assert_eq!(duration(3720.0), "1 h 02 min");
        assert_eq!(g(-9.0), "-9");
        assert_eq!(g(0.55), "0.55");
        assert_eq!(spaced(40000), "40 000");
        assert_eq!(spaced(1234567), "1 234 567");
    }

    fn row(title: &str, after: Option<f64>) -> AlbumRow {
        AlbumRow { title: title.into(), before: Some(-14.0), after, peak: Some(-1.0), state: "Terminé".into(), done: after.is_some() }
    }

    #[test]
    fn an_album_shows_who_stands_out() {
        assert_eq!(album_text(&[row("A", None), row("B", None)], false).0, "2 titres, mêmes réglages pour tous. Lance le mastering pour comparer leur sonie.");
        let close = [row("A", Some(-9.0)), row("B", Some(-9.4)), row("C", Some(-8.8))];
        let (text, warn) = album_text(&close, false);
        assert_eq!(text, "Tous les titres sont masterisés. Sonie de la sortie : -9,0 LUFS. Les titres se tiennent à moins de 1 LU.");
        assert!(!warn);
        let far = [row("A", Some(-9.0)), row("B", Some(-7.2)), row("C", Some(-9.1)), row("D", None)];
        let (text, warn) = album_text(&far, false);
        assert!(warn);
        assert_eq!(text, "3 titres sur 4 masterisés. Sonie de la sortie : -9,0 LUFS. S'écarte de plus de 1 LU : « B » (+1,8 LU).");
        let (text, warn) = album_text(&close, true);
        assert!(warn && text.ends_with("les mêmes réglages."), "{text}");
    }

    #[test]
    fn the_report_is_a_page_of_its_own() {
        let html = album_report("I'm <Alive>", "6 novembre 2026", "-9 LUFS", "Tout va.", &[row("A", Some(-9.0)), row("B", Some(-7.0)), row("C", Some(-9.1))]);
        assert!(html.contains("I'm &lt;Alive&gt;") && html.contains("<td>B</td>") && html.contains("class=\"far\""), "{html}");
    }

    #[test]
    fn every_setting_has_a_row() {
        let s = m::defaults();
        let (simple, more) = rows(&s);
        let params = simple.len() + more.iter().filter(|r| r.kind != "section").count();
        assert_eq!(params, m::PARAMS.len());
        assert_eq!(simple[0].value, "-9,0");
        let fmt = simple.iter().find(|r| r.key == "format").unwrap();
        assert!(fmt.choice_label.len() > 2 && fmt.choice_help.len() > 10);
    }
}
