//! Mastering de fichiers avec le moteur PhaseLimiter (logiciel libre, licence MIT), comme
//! `services/mastering.py` de Studio Vault : OTOKO ne contient pas le moteur, il pilote les
//! programmes livrés avec lui (`phase_limiter.exe`, `audio_analyzer.exe`, `ffmpeg.exe`), avec les
//! mêmes réglages, la même ligne de commande, le même contournement des longueurs qui font planter
//! le limiteur, et les mêmes mesures avant / après.

use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value as Json, json};

pub use super::mastering_params::{LIMITER_S, MEASURES, PARAMS, PRESETS, SAMPLE_RATES, SECTIONS};

pub const DEFAULTS_KEY: &str = "mastering_defaults";
pub const SETTINGS_KEY: &str = "mastering_settings";
pub const ENGINE_KEY: &str = "mastering_engine_dir";

/// Une valeur par défaut.
#[derive(Clone, Copy, Debug)]
pub enum Def {
    F(f64),
    I(i64),
    B(bool),
    S(&'static str),
}

impl Def {
    pub fn json(&self) -> Json {
        match self {
            Def::F(f) => json!(f),
            Def::I(i) => json!(i),
            Def::B(b) => json!(b),
            Def::S(s) => json!(s),
        }
    }
}

/// Un réglage : clé, section, libellé, type (« float », « int », « bool », « choice », « path »),
/// valeur par défaut, bornes, explication, choix (valeur, libellé, explication), et la condition
/// qui le rend inactif : (réglage, égal ?, valeur JSON — ou texte quand « différent de »).
pub struct Param {
    pub key: &'static str,
    pub section: &'static str,
    pub label: &'static str,
    pub kind: &'static str,
    pub default: Def,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub unit: &'static str,
    pub slider: bool,
    pub help: &'static str,
    pub choices: &'static [(&'static str, &'static str, &'static str)],
    pub disabled_when: Option<(&'static str, bool, &'static str)>,
    pub disabled_note: &'static str,
}

pub type Settings = Map<String, Json>;

pub fn param(key: &str) -> Option<&'static Param> {
    PARAMS.iter().find(|p| p.key == key)
}

pub fn defaults() -> Settings {
    PARAMS.iter().map(|p| (p.key.to_string(), p.default.json())).collect()
}

/// Le texte d'une valeur, comme `str()` de Python (pour comparer « 1 » et 1).
pub fn text(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Bool(b) => if *b { "True" } else { "False" }.into(),
        Json::Number(n) => n.as_i64().map(|i| i.to_string()).unwrap_or_else(|| n.to_string()),
        _ => String::new(),
    }
}

fn num(s: &Settings, k: &str) -> f64 {
    match &s[k] {
        Json::Number(n) => n.as_f64().unwrap_or(0.0),
        Json::String(t) => t.replace(',', ".").parse().unwrap_or(0.0),
        Json::Bool(b) => *b as i64 as f64,
        _ => 0.0,
    }
}

fn flag(s: &Settings, k: &str) -> bool {
    match &s[k] {
        Json::Bool(b) => *b,
        Json::String(t) => t == "true" || t == "True",
        Json::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

fn str_of(s: &Settings, k: &str) -> String {
    text(&s[k])
}

/// « Mes réglages par défaut » enregistrés (sinon les derniers utilisés), sur les valeurs du moteur.
pub fn load_settings(saved: &str) -> Settings {
    let mut s = defaults();
    if let Ok(Json::Object(m)) = serde_json::from_str::<Json>(saved) {
        for (k, v) in m {
            if s.contains_key(&k) {
                s.insert(k, v);
            }
        }
    }
    s
}

pub fn to_json(s: &Settings) -> String {
    let only: Settings = defaults().keys().filter_map(|k| s.get(k).map(|v| (k.clone(), v.clone()))).collect();
    Json::Object(only).to_string()
}

pub fn same_settings(a: &Settings, b: &Settings) -> bool {
    defaults().keys().all(|k| text(&a[k.as_str()]) == text(&b[k.as_str()]))
}

/// Le profil de vitesse qui correspond aux réglages (aucun : personnalisé).
pub fn preset_of(s: &Settings) -> Option<&'static str> {
    PRESETS.iter().find(|(_, _, values)| values.iter().all(|(k, v)| text(&s[*k]) == text(&v.json()))).map(|p| p.0)
}

pub fn apply_preset(s: &mut Settings, label: &str) {
    if let Some((_, _, values)) = PRESETS.iter().find(|p| p.0 == label) {
        for (k, v) in values.iter() {
            s.insert(k.to_string(), v.json());
        }
    }
}

pub fn is_disabled(p: &Param, s: &Settings) -> bool {
    let Some((key, eq, val)) = p.disabled_when else { return false };
    let cur = &s[key];
    if eq {
        serde_json::from_str::<Json>(val).map(|v| *cur == v).unwrap_or(false)
    } else {
        text(cur) != val
    }
}

/// Les erreurs bloquantes (vide : tout va bien).
pub fn validate(s: &Settings) -> Vec<String> {
    let mut err = Vec::new();
    let (lc, hc) = (num(s, "low_cut"), num(s, "high_cut"));
    if !(lc == 0.0 && hc == 0.0) && lc >= hc {
        err.push("Filtres : le coupe-bas doit être plus bas que le coupe-haut (ou les deux à 0).".into());
    }
    let sr = num(s, "sample_rate") as i64;
    let format = str_of(s, "format");
    let ok_rates = SAMPLE_RATES.iter().find(|r| r.0 == format).map(|r| r.1).unwrap_or(&[]);
    if !ok_rates.contains(&sr.to_string().as_str()) {
        err.push(format!("Le format {} n'accepte pas {} kHz.", format.to_uppercase(), (sr as f64 / 1000.0).to_string().replace('.', ",")));
    }
    if str_of(s, "ceiling_mode") == "lowpass_true_peak" && num(s, "lowpass_cut") > (sr.min(44100) as f64) / 2.0 {
        err.push("La fréquence du true peak filtré ne doit pas dépasser la moitié de la fréquence d'échantillonnage.".into());
    }
    if flag(s, "mastering") && flag(s, "use_reference") {
        let r = str_of(s, "reference_audio");
        if r.trim().is_empty() {
            err.push("Choisis un morceau de référence, ou décoche « Utiliser un morceau de référence ».".into());
        } else if !Path::new(r.trim()).is_file() {
            err.push("Le morceau de référence est introuvable.".into());
        }
    }
    let out = str_of(s, "out_dir");
    if !out.is_empty() && !Path::new(&out).is_dir() {
        err.push("Le dossier de destination n'existe pas.".into());
    }
    err
}

/// Durée approximative d'un mastering, d'après des mesures faites sur le PC de l'utilisateur.
pub fn estimate_seconds(s: &Settings, duration: f64) -> f64 {
    let mut t = LIMITER_S.iter().find(|l| l.0 == str_of(s, "oversample")).map(|l| l.1).unwrap_or(40.0);
    if flag(s, "mastering") {
        t += 0.002 * num(s, "evaluations");
    }
    t += 6.0;
    let workers = num(s, "workers") as usize;
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);
    if workers > 0 && workers < cores {
        t *= (cores as f64 / workers as f64).powf(0.6).min(3.0);
    }
    t * duration / 190.0
}

// ---------------------------------------------------------------------- le moteur

#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    pub root: PathBuf,
}

impl Engine {
    pub fn bin(&self) -> PathBuf {
        self.root.join("phaselimiter").join("bin")
    }
    pub fn phase_limiter(&self) -> PathBuf {
        self.bin().join("phase_limiter.exe")
    }
    pub fn analyzer(&self) -> PathBuf {
        self.bin().join("audio_analyzer.exe")
    }
    pub fn ffmpeg(&self) -> PathBuf {
        self.root.join("ffmpeg.exe")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("phaselimiter").join("resource").join("sound_quality2_cache")
    }
    pub fn ok(&self) -> bool {
        [self.phase_limiter(), self.analyzer(), self.ffmpeg(), self.cache()].iter().all(|p| p.exists())
    }
}

/// Le moteur : le dossier enregistré, puis les emplacements habituels.
pub fn find_engine(saved: &str, extra: &[PathBuf]) -> Option<Engine> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if !saved.trim().is_empty() {
        cands.push(PathBuf::from(saved.trim()));
    }
    cands.extend(extra.iter().cloned());
    if let Some(home) = std::env::var_os("USERPROFILE") {
        cands.push(PathBuf::from(home).join("Downloads").join("Logiciels et installeurs").join("Phaselimiter"));
    }
    for c in cands {
        let e = Engine { root: c.clone() };
        if e.ok() {
            return Some(e);
        }
        // Le dossier « phaselimiter » choisi directement (celui qui contient bin/).
        if let Some(parent) = c.parent() {
            let e = Engine { root: parent.to_path_buf() };
            if c.join("bin").join("phase_limiter.exe").exists() && e.ok() {
                return Some(e);
            }
        }
    }
    None
}

pub const ENGINE_HINT: &str = "Le mastering utilise le moteur PhaseLimiter (logiciel libre, licence MIT). Le bouton « Installer le moteur » de cette page s'en occupe. Si tu l'as déjà, indique son dossier (celui qui contient ffmpeg.exe et le sous-dossier phaselimiter\\bin).";

/// « titre (master).wav » à côté de l'original (ou dans le dossier choisi), sans écraser.
pub fn output_path(src: &Path, s: &Settings) -> PathBuf {
    let out_dir = str_of(s, "out_dir");
    let folder = if out_dir.is_empty() { src.parent().map(Path::to_path_buf).unwrap_or_default() } else { PathBuf::from(out_dir) };
    let ext = match str_of(s, "format").as_str() {
        "mp3" => ".mp3",
        "aac" => ".aac",
        _ => ".wav",
    };
    let stem = src.file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = folder.join(format!("{stem} (master){ext}"));
    let mut n = 2;
    while out.exists() {
        out = folder.join(format!("{stem} (master {n}){ext}"));
        n += 1;
    }
    out
}

fn f(v: f64) -> String {
    // Comme `float()` de Python écrit dans une chaîne : « -9.0 », « 0.5 ».
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{v:.1}") } else { format!("{v}") }
}

fn tf(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

/// Les arguments de `phase_limiter.exe`, comme l'application d'origine.
pub fn build_args(e: &Engine, src: &str, dst: &str, s: &Settings, tmp: &str, ref_json: &str) -> Vec<String> {
    let intensity = f(num(s, "intensity"));
    let mut a = vec![
        format!("--input={src}"),
        format!("--output={dst}"),
        format!("--ffmpeg={}", e.ffmpeg().display()),
        format!("--tmp={tmp}"),
        format!("--sound_quality2_cache={}", e.cache().display()),
        format!("--mastering={}", tf(flag(s, "mastering"))),
        "--mastering_mode=mastering5".into(),
        format!("--mastering_matching_level={intensity}"),
        format!("--mastering_ms_matching_level={intensity}"),
        format!("--mastering5_mastering_level={intensity}"),
        format!("--mastering5_optimization_algorithm={}", str_of(s, "solver")),
        format!("--mastering5_optimization_max_eval_count={}", num(s, "evaluations") as i64),
        format!("--mastering5_optimization_seed={}", num(s, "seed") as i64),
        format!("--reference={}", f(num(s, "loudness"))),
        format!("--reference_mode={}", str_of(s, "reference_mode")),
        format!("--ceiling={}", f(num(s, "ceiling"))),
        format!("--ceiling_mode={}", str_of(s, "ceiling_mode")),
        format!("--lowpass_true_peak_cut_freq={}", f(num(s, "lowpass_cut"))),
        format!("--low_cut_freq={}", f(num(s, "low_cut"))),
        format!("--high_cut_freq={}", f(num(s, "high_cut"))),
        format!("--pre_compression={}", tf(flag(s, "pre_compression"))),
        format!("--erb_eval_func_weighting={}", tf(flag(s, "preserve_bass"))),
        "--limiting_mode=phase".into(),
        format!("--limiter_external_oversample={}", num(s, "oversample") as i64),
        "--limiter_internal_oversample=1".into(),
        format!("--max_iter1={}", num(s, "max_iter1") as i64),
        format!("--max_iter2={}", num(s, "max_iter2") as i64),
        format!("--noise_update_mode={}", str_of(s, "noise_mode")),
        format!("--output_format={}", str_of(s, "format")),
        format!("--sample_rate={}", num(s, "sample_rate") as i64),
        format!("--bit_depth={}", num(s, "bit_depth") as i64),
        format!("--worker_count={}", num(s, "workers") as i64),
    ];
    if flag(s, "mastering") && flag(s, "use_reference") && !ref_json.is_empty() {
        a.push(format!("--mastering5_mastering_reference_file={ref_json}"));
    }
    a
}

/// ffmpeg : WAV stéréo 32 bits flottants (que l'analyseur lit quel que soit le format d'origine).
pub fn decode_args(src: &str, wav: &str) -> Vec<String> {
    ["-nostdin", "-v", "error", "-y", "-i", src, "-vn", "-ac", "2", "-c:a", "pcm_f32le", "-f", "wav", wav].map(String::from).to_vec()
}

/// La conversion que le moteur fait avant de traiter : stéréo, 44,1 kHz, flottant.
pub fn decode44_args(src: &str, wav: &str) -> Vec<String> {
    ["-nostdin", "-v", "error", "-y", "-i", src, "-vn", "-ac", "2", "-ar", "44100", "-c:a", "pcm_f32le", "-f", "wav", wav].map(String::from).to_vec()
}

/// Nombre d'images d'un WAV stéréo 32 bits flottants (lecture de l'en-tête seulement).
pub fn wav_frames(path: &Path) -> Result<u64, String> {
    let mut fh = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let len = fh.metadata().map(|m| m.len()).unwrap_or(0);
    let mut head = [0u8; 12];
    fh.read_exact(&mut head).map_err(|_| "WAV invalide")?;
    if &head[..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return Err("WAV invalide".into());
    }
    let mut pos = 12u64;
    loop {
        let mut ck = [0u8; 8];
        if fh.read_exact(&mut ck).is_err() {
            return Err("WAV sans données".into());
        }
        pos += 8;
        let size = u32::from_le_bytes([ck[4], ck[5], ck[6], ck[7]]) as u64;
        if &ck[..4] == b"data" {
            let size = if size == 0 || size == 0xFFFF_FFFF { len.saturating_sub(pos) } else { size };
            return Ok(size / 8);
        }
        let skip = size + (size & 1);
        std::io::Seek::seek(&mut fh, std::io::SeekFrom::Current(skip as i64)).map_err(|e| e.to_string())?;
        pos += skip;
    }
}

const BLOCK: u64 = 8192;

fn smooth(mut x: u64) -> bool {
    for p in [2, 3, 5, 7] {
        while x % p == 0 && x > 0 {
            x /= p;
        }
    }
    x == 1
}

/// Le limiteur calcule une transformée de Fourier de 8192 × suréchantillonnage × k, avec
/// k = 2 + ⌈n / 8192⌉ ; si k a un grand facteur premier, sa bibliothèque déborde. La longueur
/// (≥ n) qu'il traite sans débordement (n lui-même si elle convient).
pub fn safe_length(n: u64) -> u64 {
    let padded8 = n.div_ceil(8) * 8;
    let mut j = padded8.div_ceil(BLOCK);
    if smooth(j + 2) {
        return n;
    }
    j += 1;
    while !smooth(j + 2) {
        j += 1;
    }
    (j - 1) * BLOCK + 1
}

/// Recoupe le master à la longueur de l'original et l'encode dans le format demandé.
pub fn trim_encode_args(src: &str, dst: &str, frames: u64, s: &Settings) -> Vec<String> {
    let mut a: Vec<String> = ["-nostdin", "-v", "error", "-y", "-i", src, "-af"].map(String::from).to_vec();
    a.push(format!("atrim=end_sample={frames}"));
    let codec: Vec<&str> = match str_of(s, "format").as_str() {
        "mp3" => vec!["-c:a", "libmp3lame", "-b:a", "320k", "-f", "mp3"],
        "aac" => vec!["-c:a", "aac", "-b:a", "256k", "-f", "adts"],
        _ => vec![
            "-c:a",
            match str_of(s, "bit_depth").as_str() {
                "24" => "pcm_s24le",
                "32" => "pcm_f32le",
                _ => "pcm_s16le",
            },
            "-f",
            "wav",
        ],
    };
    a.extend(codec.into_iter().map(String::from));
    a.push(dst.into());
    a
}

pub fn analyzer_args(e: &Engine, src: &str, tmp: &str, full: bool) -> Vec<String> {
    vec![
        format!("--input={src}"),
        format!("--ffmpeg={}", e.ffmpeg().display()),
        format!("--tmp={tmp}"),
        "--sound_quality2=true".into(),
        "--sound_quality2_cache_archiver=binary".into(),
        format!("--sound_quality2_cache={}", e.cache().display()),
        "--drr=true".into(),
        "--sound_quality=false".into(),
        format!("--analysis_for_visualization={}", tf(full)),
    ]
}

/// Les mesures de l'analyseur, en valeurs prêtes à montrer.
pub fn extract_measures(data: &Json) -> Vec<(&'static str, Option<f64>)> {
    MEASURES
        .iter()
        .map(|(key, ..)| {
            let v = match *key {
                "youtube_compensation" => data["youtube_loudness"].as_f64().map(|yl| (-10.3 - yl).min(0.0)),
                "sound_quality2" => data[*key].as_f64().map(|v| v * 100.0),
                k => data[k].as_f64(),
            };
            (*key, v)
        })
        .collect()
}

pub fn format_measure(key: &str, value: Option<f64>) -> String {
    let Some(v) = value else { return "—".into() };
    let Some(m) = MEASURES.iter().find(|m| m.0 == key) else { return v.to_string() };
    let t = format!("{v:.prec$}", prec = m.3).replace('.', ",");
    format!("{t} {}", m.2).trim().to_string()
}

// ---------------------------------------------------------------------- un mastering

/// Ce que l'interface suit pendant un mastering.
pub trait Progress {
    fn step(&mut self, label: &str, progress: f64);
    fn log(&mut self, line: &str);
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub dst: PathBuf,
    pub before: Vec<(&'static str, Option<f64>)>,
    pub after: Vec<(&'static str, Option<f64>)>,
    pub note: String,
    pub command: String,
    pub log: Vec<String>,
}

fn command(exe: &Path, args: &[String], cwd: &Path, low_priority: bool) -> Command {
    let mut c = Command::new(exe);
    c.args(args).current_dir(cwd);
    // Pas de fenêtre ; « inférieure à la normale » si demandé : la musique et le reste passent d'abord.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000 | if low_priority { 0x0000_4000 } else { 0 });
    }
    // Ailleurs (la base se teste aussi hors de Windows) : rien à cacher.
    #[cfg(not(windows))]
    let _ = low_priority;
    c
}

/// Lance un programme ; rend sa sortie standard, ou l'erreur. `on_line` reçoit chaque ligne
/// (fusionnée avec les erreurs) quand on suit l'avancement.
fn run(exe: &Path, args: &[String], cwd: &Path, low: bool, cancel: &AtomicBool, mut on_line: Option<&mut dyn FnMut(&str)>) -> Result<Vec<u8>, String> {
    let mut c = command(exe, args, cwd, low);
    c.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let mut child = c.spawn().map_err(|e| format!("impossible de lancer {} ({e})", exe.display()))?;
    let mut stderr = child.stderr.take().expect("stderr");
    let err_thread = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = stderr.read_to_end(&mut v);
        v
    });
    let mut out = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                break;
            }
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    out.extend_from_slice(&line);
                    if let Some(cb) = on_line.as_deref_mut() {
                        cb(String::from_utf8_lossy(&line).trim_end());
                    }
                }
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let err = err_thread.join().unwrap_or_default();
    if cancel.load(Ordering::Relaxed) {
        return Err("annulé".into());
    }
    if !status.success() {
        let (e_txt, o_txt) = (String::from_utf8_lossy(&err).into_owned(), String::from_utf8_lossy(&out).into_owned());
        let lines: Vec<&str> = e_txt.lines().chain(o_txt.lines()).filter(|l| !l.contains("RSS")).collect();
        let tail = lines[lines.len().saturating_sub(8)..].join("\n");
        return Err(format!("code {}{}", status.code().unwrap_or(-1), if tail.is_empty() { String::new() } else { format!("\n{tail}") }));
    }
    Ok(out)
}

/// Un mastering complet : référence (si demandée), préparation, mastering (avec le
/// contournement de longueur si besoin), puis mesures de l'original et du master.
pub fn master(e: &Engine, src: &Path, s: &Settings, tmp_root: &Path, ref_cache: &mut Vec<(String, PathBuf)>, cancel: &AtomicBool, p: &mut dyn Progress) -> Result<Outcome, String> {
    let dst = output_path(src, s);
    std::fs::create_dir_all(tmp_root).map_err(|e| e.to_string())?;
    let tmp = tmp_root.join(format!("job_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = master_in(e, src, &dst, s, tmp_root, &tmp, ref_cache, cancel, p);
    // Les copies WAV temporaires pèsent plusieurs centaines de Mo.
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[allow(clippy::too_many_arguments)]
fn master_in(e: &Engine, src: &Path, dst: &Path, s: &Settings, tmp_root: &Path, tmp: &Path, ref_cache: &mut Vec<(String, PathBuf)>, cancel: &AtomicBool, p: &mut dyn Progress) -> Result<Outcome, String> {
    let low = flag(s, "low_priority");
    let ts = tmp.to_string_lossy().into_owned();
    let mut out = Outcome { dst: dst.to_path_buf(), ..Default::default() };
    let src_s = src.to_string_lossy().into_owned();
    // 1. Le morceau de référence, analysé une fois par session.
    let mut ref_json = String::new();
    if flag(s, "mastering") && flag(s, "use_reference") {
        let r = str_of(s, "reference_audio");
        if let Some((_, j)) = ref_cache.iter().find(|(k, j)| *k == r && j.exists()) {
            ref_json = j.to_string_lossy().into_owned();
        } else {
            p.step("Lecture du morceau de référence", 0.0);
            let ref_wav = tmp.join("reference.wav");
            run(&e.ffmpeg(), &decode_args(&r, &ref_wav.to_string_lossy()), tmp, low, cancel, None).map_err(|_| "La référence n'a pas pu être lue ou analysée.".to_string())?;
            p.step("Analyse du morceau de référence", 0.02);
            let data = run(&e.analyzer(), &analyzer_args(e, &ref_wav.to_string_lossy(), &ts, true), tmp, low, cancel, None).map_err(|_| "La référence n'a pas pu être lue ou analysée.".to_string())?;
            serde_json::from_slice::<Json>(&data).map_err(|_| "La référence n'a pas pu être analysée.".to_string())?;
            let refs = tmp_root.join("references");
            let _ = std::fs::create_dir_all(&refs);
            let j = refs.join(format!("ref{}_{}.json", ref_cache.len() + 1, std::process::id()));
            std::fs::write(&j, &data).map_err(|e| e.to_string())?;
            ref_cache.push((r, j.clone()));
            ref_json = j.to_string_lossy().into_owned();
        }
    }
    let lo = if ref_json.is_empty() { 0.0 } else { 0.08 };
    // 2. La conversion que fera le moteur, pour connaître la longueur exacte.
    p.step("Préparation du morceau", lo);
    let source = tmp.join("source.wav");
    run(&e.ffmpeg(), &decode44_args(&src_s, &source.to_string_lossy()), tmp, low, cancel, None).map_err(|_| "Le morceau n'a pas pu être lu (format non reconnu ou fichier abîmé).".to_string())?;
    let n = wav_frames(&source).map_err(|e| format!("Le morceau n'a pas pu être préparé ({e})."))?;
    if n == 0 {
        return Err("Le morceau n'a pas pu être préparé (morceau vide).".into());
    }
    // 3. Le mastering.
    let target = safe_length(n);
    let (m_lo, m_hi) = (lo + 0.02, 0.86);
    let dst_s = dst.to_string_lossy().into_owned();
    let on_line = |line: &str, lo: f64, hi: f64, p: &mut dyn Progress, log: &mut Vec<String>| {
        if let Some(v) = line.split("progression:").nth(1).and_then(|x| x.trim().split_whitespace().next()).and_then(|x| x.parse::<f64>().ok()) {
            p.step("Mastering", lo + (hi - lo) * v.min(1.0));
        } else if !line.trim().is_empty() {
            log.push(line.to_string());
            p.log(line);
        }
    };
    let mut log: Vec<String> = Vec::new();
    if target == n {
        let mut args = build_args(e, &source.to_string_lossy(), &dst_s, s, &ts, &ref_json);
        args.push("--disable_input_encode=true".into());
        out.command = format!("\"{}\" {}", e.phase_limiter().display(), args.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(" "));
        p.step("Mastering", m_lo);
        let mut cb = |l: &str| on_line(l, m_lo, m_hi, p, &mut log);
        run(&e.phase_limiter(), &args, tmp, low, cancel, Some(&mut cb)).map_err(|err| if err == "annulé" { err } else { format!("Le moteur s'est arrêté avec une erreur.\n{err}") })?;
    } else {
        // Longueur défavorable : silence ajouté pendant le calcul, retiré ensuite.
        let (padded, limited) = (tmp.join("padded.wav"), tmp.join("limited.wav"));
        let mut s_wav = s.clone();
        s_wav.insert("format".into(), json!("wav"));
        if str_of(s, "format") != "wav" {
            s_wav.insert("bit_depth".into(), json!("32"));
        }
        p.step("Préparation du morceau", m_lo);
        let pad = vec![
            "-nostdin".into(),
            "-v".into(),
            "error".into(),
            "-y".into(),
            "-i".into(),
            source.to_string_lossy().into_owned(),
            "-af".into(),
            format!("apad=whole_len={target}"),
            "-c:a".into(),
            "pcm_f32le".into(),
            "-f".into(),
            "wav".into(),
            padded.to_string_lossy().into_owned(),
        ];
        run(&e.ffmpeg(), &pad, tmp, low, cancel, None).map_err(|_| "Le morceau n'a pas pu être préparé.".to_string())?;
        let mut args = build_args(e, &padded.to_string_lossy(), &limited.to_string_lossy(), &s_wav, &ts, &ref_json);
        args.push("--disable_input_encode=true".into());
        out.command = format!("\"{}\" {}", e.phase_limiter().display(), args.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(" "));
        let mut cb = |l: &str| on_line(l, m_lo, m_hi - 0.01, p, &mut log);
        run(&e.phase_limiter(), &args, tmp, low, cancel, Some(&mut cb)).map_err(|err| if err == "annulé" { err } else { format!("Le moteur s'est arrêté avec une erreur.\n{err}") })?;
        p.step("Enregistrement du master", m_hi - 0.01);
        let frames_out = (n as f64 * num(s, "sample_rate") / 44100.0).round() as u64;
        run(&e.ffmpeg(), &trim_encode_args(&limited.to_string_lossy(), &dst_s, frames_out, s), tmp, low, cancel, None)
            .map_err(|_| "Le master n'a pas pu être enregistré dans le format demandé.".to_string())?;
        out.note = format!(
            "Le moteur PhaseLimiter n'accepte pas cette longueur exacte : {} s de silence ont été ajoutées pendant le calcul, puis retirées. Le master a exactement la durée de l'original.",
            format!("{:.1}", (target - n) as f64 / 44100.0).replace('.', ",")
        );
    }
    if !dst.exists() {
        return Err("Aucun fichier produit.".into());
    }
    // 4. Les mesures de l'original et du master (facultatives : le master est bien là).
    for (label, file, wav, range, into_before) in [("Mesures de l'original", src_s.clone(), "original.wav", 0.86, true), ("Mesures du master", dst_s.clone(), "master.wav", 0.93, false)] {
        p.step(label, range);
        let w = tmp.join(wav);
        if run(&e.ffmpeg(), &decode_args(&file, &w.to_string_lossy()), tmp, low, cancel, None).is_err() {
            log.push(format!("Mesure impossible ({label})."));
            continue;
        }
        match run(&e.analyzer(), &analyzer_args(e, &w.to_string_lossy(), &ts, false), tmp, low, cancel, None).ok().and_then(|d| serde_json::from_slice::<Json>(&d).ok()) {
            Some(data) => {
                if into_before {
                    out.before = extract_measures(&data);
                } else {
                    out.after = extract_measures(&data);
                }
            }
            None => log.push(format!("Mesure impossible ({label}).")),
        }
    }
    p.step("", 1.0);
    out.log = log;
    Ok(out)
}

// ---------------------------------------------------------------------- la bibliothèque

/// Le morceau de la bibliothèque qui a ce fichier.
pub fn track_of(conn: &Connection, path: &Path) -> Option<i64> {
    let p = super::scan::normalize(path).to_string_lossy().into_owned();
    conn.query_row("SELECT id FROM tracks WHERE path = ?1 COLLATE NOCASE", [p], |r| r.get(0)).optional().ok().flatten()
}

/// Range le master avec son morceau : version « master », titre, artiste, genre, tempo et
/// tonalité repris du morceau, sonie et crête mesurées. Le morceau qui attendait son master
/// (« À masteriser ») passe à « Terminé » ; sa sonie, si elle n'avait pas été analysée, est celle
/// mesurée avant le mastering (pour les comparer à volume égal). Rend l'identifiant du master et
/// le changement d'état.
pub fn register_master(conn: &Connection, parent: i64, dst: &Path, before: &[(&'static str, Option<f64>)], after: &[(&'static str, Option<f64>)]) -> rusqlite::Result<(i64, bool)> {
    let (_, id) = super::scan::upsert(conn, dst, &[])?;
    if id == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    super::files::attach(conn, id, parent, false)?;
    let m = |k: &str| after.iter().find(|x| x.0 == k).and_then(|x| x.1);
    conn.execute(
        "UPDATE tracks SET version_label = 'master',
            title = CASE WHEN IFNULL(p.title, '') = '' THEN tracks.title ELSE p.title || ' (master)' END,
            artist = COALESCE(NULLIF(tracks.artist, ''), p.artist), album = COALESCE(NULLIF(tracks.album, ''), p.album),
            genre = COALESCE(NULLIF(tracks.genre, ''), p.genre), year = COALESCE(tracks.year, p.year),
            bpm = COALESCE(tracks.bpm, p.bpm), music_key = COALESCE(NULLIF(tracks.music_key, ''), p.music_key),
            nature = p.nature, nature_meta = p.nature_meta, status = 'done',
            lufs = COALESCE(?2, tracks.lufs), true_peak = COALESCE(?3, tracks.true_peak)
         FROM (SELECT title, artist, album, genre, year, bpm, music_key, nature, nature_meta FROM tracks WHERE id = ?4) AS p
         WHERE tracks.id = ?1",
        params![id, m("loudness"), m("true_peak"), parent],
    )?;
    let b = |k: &str| before.iter().find(|x| x.0 == k).and_then(|x| x.1);
    conn.execute("UPDATE tracks SET lufs = ?2 WHERE id = ?1 AND lufs IS NULL AND ?2 IS NOT NULL", params![parent, b("loudness")])?;
    let finished = conn.execute("UPDATE tracks SET status = 'done', modified_at = datetime('now') WHERE id = ?1 AND status = 'to_master'", [parent])? > 0;
    Ok((id, finished))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Print;
    impl Progress for Print {
        fn step(&mut self, label: &str, progress: f64) {
            println!("{:>5.1} % {label}", progress * 100.0);
        }
        fn log(&mut self, _line: &str) {}
    }

    /// Un vrai mastering avec le moteur de l'utilisateur (`OTOKO_PHASELIMITER`), sur 12 s de synthèse.
    #[test]
    #[ignore]
    fn real_engine() {
        let Some(root) = std::env::var_os("OTOKO_PHASELIMITER") else { return };
        let e = find_engine(&root.to_string_lossy(), &[]).expect("moteur");
        let dir = std::env::temp_dir().join(format!("otoko-mastering-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("essai.wav");
        // 12 s : une basse et un charleston, en WAV 16 bits écrit à la main.
        let rate = 44_100u32;
        let n = rate * 12;
        let mut data = Vec::with_capacity(n as usize * 4);
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let v = (t * 55.0 * std::f32::consts::TAU).sin() * 0.3 + if (i % 11_025) < 600 { ((i * 7919) % 200) as f32 / 400.0 - 0.25 } else { 0.0 };
            let s = (v.clamp(-1.0, 1.0) * 20_000.0) as i16;
            data.extend_from_slice(&s.to_le_bytes());
            data.extend_from_slice(&s.to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(&src, wav).unwrap();
        let mut s = defaults();
        apply_preset(&mut s, "Essai rapide");
        s.insert("use_reference".into(), json!(false));
        let t = std::time::Instant::now();
        let out = master(&e, &src, &s, &dir.join("tmp"), &mut Vec::new(), &AtomicBool::new(false), &mut Print).unwrap();
        println!("{:?} en {:.1} s ; {}", out.dst, t.elapsed().as_secs_f64(), out.note);
        for (k, v) in &out.after {
            println!("  {k:<26} {} → {}", format_measure(k, out.before.iter().find(|b| b.0 == *k).and_then(|b| b.1)), format_measure(k, *v));
        }
        assert!(out.dst.exists());
        assert!(out.after.iter().any(|m| m.0 == "loudness" && m.1.is_some_and(|l| (l + 9.0).abs() < 1.5)), "volume visé -9 LUFS");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Le master rejoint son morceau : version « master », infos reprises, « À masteriser » → « Terminé ».
    #[test]
    fn master_joins_its_track() {
        let dir = std::env::temp_dir().join(format!("otoko-master-biblio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wav = |p: &Path| {
            let mut b = b"RIFF".to_vec();
            b.extend_from_slice(&(36u32 + 400).to_le_bytes());
            b.extend_from_slice(b"WAVEfmt ");
            for v in [16u32.to_le_bytes().to_vec(), 1u16.to_le_bytes().to_vec(), 1u16.to_le_bytes().to_vec(), 44100u32.to_le_bytes().to_vec(), 88200u32.to_le_bytes().to_vec(), 2u16.to_le_bytes().to_vec(), 16u16.to_le_bytes().to_vec()] {
                b.extend_from_slice(&v);
            }
            b.extend_from_slice(b"data");
            b.extend_from_slice(&400u32.to_le_bytes());
            b.extend_from_slice(&[0u8; 400]);
            std::fs::write(p, b).unwrap();
        };
        let (src, dst) = (dir.join("Nuit.wav"), dir.join("Nuit (master).wav"));
        wav(&src);
        wav(&dst);
        let c = super::super::Career::open(&dir.join("c.db")).unwrap();
        c.conn()
            .execute(
                "INSERT INTO tracks (path, filename, title, artist, genre, bpm, music_key, status) VALUES (?1, 'Nuit.wav', 'Nuit', 'Otoko', 'House', 124.0, 'Am', 'to_master')",
                [src.to_string_lossy()],
            )
            .unwrap();
        let parent = c.conn().last_insert_rowid();
        assert_eq!(track_of(c.conn(), &src), Some(parent));
        let (id, finished) = register_master(c.conn(), parent, &dst, &[("loudness", Some(-17.5))], &[("loudness", Some(-9.1)), ("true_peak", Some(-1.0))]).unwrap();
        assert!(finished && id != parent);
        let row: (String, String, i64, String, f64, String, String) = c
            .conn()
            .query_row("SELECT title, artist, parent_id, role, lufs, version_label, music_key FROM tracks WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
            })
            .unwrap();
        assert_eq!(row, ("Nuit (master)".into(), "Otoko".into(), parent, "version".into(), -9.1, "master".into(), "Am".into()));
        let (status, lufs): (String, f64) = c.conn().query_row("SELECT status, lufs FROM tracks WHERE id = ?1", [parent], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((status.as_str(), lufs), ("done", -17.5));
        // Une deuxième fois : rien ne change d'état.
        assert!(!register_master(c.conn(), parent, &dst, &[], &[]).unwrap().1);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_and_commands() {
        let s = defaults();
        assert_eq!(s.len(), PARAMS.len());
        assert!(validate(&s).is_empty(), "{:?}", validate(&s));
        assert_eq!(preset_of(&s), None);
        let mut q = s.clone();
        apply_preset(&mut q, "Essai rapide");
        assert_eq!(preset_of(&q), Some("Essai rapide"));
        assert!(is_disabled(param("bit_depth").unwrap(), &{ let mut m = s.clone(); m.insert("format".into(), json!("mp3")); m }));
        assert!(!is_disabled(param("bit_depth").unwrap(), &s));
        assert!(is_disabled(param("loudness").unwrap(), &{ let mut m = s.clone(); m.insert("reference_mode".into(), json!("zero")); m }));
        let mut bad = s.clone();
        bad.insert("format".into(), json!("mp3"));
        bad.insert("sample_rate".into(), json!("96000"));
        assert_eq!(validate(&bad), vec!["Le format MP3 n'accepte pas 96 kHz."]);
        let e = Engine { root: PathBuf::from(r"C:\PL") };
        let a = build_args(&e, "in.wav", "out.wav", &s, "T", "");
        assert!(a.contains(&"--reference=-9.0".to_string()) && a.contains(&"--mastering_matching_level=1.0".to_string()) && a.contains(&"--bit_depth=16".to_string()));
        assert!(a.contains(&"--mastering5_optimization_max_eval_count=40000".to_string()));
        // Le contournement de longueur, comme en Python.
        // Valeurs de Python (Studio Vault).
        for (n, want) in [(8379000, 8404993), (8246823, 8364033), (90113, 90113), (122877, 122881), (368640, 368641), (1000003, 1000003), (8380417, 8404993)] {
            assert_eq!(safe_length(n), want, "n = {n}");
        }
        assert_eq!(format_measure("loudness", Some(-9.04)), "-9,0 LUFS");
        assert_eq!(format_measure("sound_quality2", Some(71.4)), "71 %");
        let m = extract_measures(&json!({"loudness": -9.0, "youtube_loudness": -8.0, "sound_quality2": 0.5}));
        assert_eq!(m.iter().find(|x| x.0 == "youtube_compensation").unwrap().1, Some(-2.3000000000000007));
        assert!(estimate_seconds(&q, 190.0) > 40.0);
        let src = Path::new(r"C:\Musique\Nuit.flac");
        assert_eq!(output_path(src, &s), PathBuf::from(r"C:\Musique\Nuit (master).wav"));
    }
}
