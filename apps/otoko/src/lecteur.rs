//! Le lecteur (comme celui de Studio Vault) : un morceau entier, décodé au fil de l'eau dans un fil à part ; forme d'onde,
//! repères, boucle A-B, file de lecture et enchaînement, comparateur A/B à sonie égale. La carte son ne s'ouvre que le
//! temps d'écouter (udio::Ecoute).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use otoko_library::career::library as lib;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{SeekMode, SeekTo};
use symphonia::core::units::Time;

use crate::state::State;
use crate::{AppWindow, Lib, MarkUi};

/// Le comparateur : deux morceaux, chacun avec son gain pour qu'ils sonnent aussi fort.
#[derive(Clone, Debug, Default)]
pub struct Compare {
    pub a: (i64, f32),
    pub b: (i64, f32),
    pub on_b: bool,
}

#[derive(Default)]
pub struct Player {
    pub id: Option<i64>,
    pub path: String,
    pub title: String,
    pub sub: String,
    pub duration: f64,
    pub paused: bool,
    /// Le fil de décodage en cours : l'arrêter, savoir s'il a tout lu.
    stop: Option<Arc<AtomicBool>>,
    finished: Option<Arc<AtomicBool>>,
    pub loop_a: Option<f64>,
    pub loop_b: Option<f64>,
    pub queue: Vec<i64>,
    pub peaks: Vec<f32>,
    pub markers: Vec<(f64, String)>,
    pub compare: Option<Compare>,
    pub gain: f32,
    /// Position affichée (secondes).
    pub pos: f64,
    pub timer: Option<slint::Timer>,
}

/// Rééchantillonneur d'Hermite à 4 points qui garde son état d'un paquet à l'autre.
struct Resampler {
    step: f64,
    pos: f64,
    hist: Vec<[f32; 2]>,
}

impl Resampler {
    fn new(from: f64, to: f64) -> Resampler {
        Resampler { step: from / to, pos: 1.0, hist: vec![[0.0; 2]; 1] }
    }

    /// Ajoute des images (stéréo) et rend celles qu'on peut déjà calculer, entrelacées.
    fn push(&mut self, input: &[[f32; 2]], out: &mut Vec<f32>) {
        self.hist.extend_from_slice(input);
        // Il faut i-1, i, i+1, i+2 : on calcule tant que i+2 existe.
        while (self.pos.floor() as usize) + 2 < self.hist.len() {
            let i = self.pos.floor() as usize;
            let t = (self.pos - i as f64) as f32;
            for ch in 0..2 {
                let y0 = self.hist[i.saturating_sub(1)][ch];
                let (y1, y2, y3) = (self.hist[i][ch], self.hist[i + 1][ch], self.hist[i + 2][ch]);
                let c1 = 0.5 * (y2 - y0);
                let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
                let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
                out.push(((c3 * t + c2) * t + c1) * t + y1);
            }
            self.pos += self.step;
        }
        // On ne garde que ce qui sert encore.
        let keep_from = (self.pos.floor() as usize).saturating_sub(1);
        if keep_from > 0 {
            self.hist.drain(..keep_from);
            self.pos -= keep_from as f64;
        }
    }
}

/// Décode `path` depuis `start` secondes, à la fréquence `rate`, dans l'anneau.
fn decode(path: String, start: f64, rate: f64, mut ring: rtrb::Producer<f32>, stop: Arc<AtomicBool>, finished: Arc<AtomicBool>) {
    let mut run = || -> Result<(), String> {
        let mut format = crate::samples::open(&path)?;
        let track = format.default_track().ok_or("aucune piste audio")?;
        let id = track.id;
        let src = track.codec_params.sample_rate.ok_or("fréquence inconnue")? as f64;
        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| e.to_string())?;
        if start > 0.0 {
            let _ = format.seek(SeekMode::Accurate, SeekTo::Time { time: Time::from(start), track_id: Some(id) });
        }
        let mut rs = ((src - rate).abs() > 0.5).then(|| Resampler::new(src, rate));
        let mut out: Vec<f32> = Vec::new();
        let mut frames: Vec<[f32; 2]> = Vec::new();
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(SymError::ResetRequired) => return Ok(()),
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id() != id {
                continue;
            }
            let decoded = match decoder.decode(&packet) {
                Ok(d) => d,
                Err(SymError::DecodeError(_)) => continue,
                Err(e) => return Err(e.to_string()),
            };
            let spec = *decoded.spec();
            let ch = spec.channels.count().max(1);
            let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            buf.copy_interleaved_ref(decoded);
            frames.clear();
            frames.extend(buf.samples().chunks(ch).map(|f| [f[0], if ch > 1 { f[1] } else { f[0] }]));
            out.clear();
            match rs.as_mut() {
                Some(r) => r.push(&frames, &mut out),
                None => out.extend(frames.iter().flat_map(|f| [f[0], f[1]])),
            }
            // Verse dans l'anneau en attendant qu'il se vide.
            let mut k = 0;
            while k < out.len() {
                if stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let n = ring.slots().min(out.len() - k) & !1;
                if n == 0 {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                for v in &out[k..k + n] {
                    let _ = ring.push(*v);
                }
                k += n;
            }
        }
    };
    let _ = run();
    finished.store(true, Ordering::Relaxed);
}

/// Joue (ou reprend) le morceau à partir de `start` secondes. Rend faux si la carte son ne s'ouvre pas.
fn start_stream(app: &AppWindow, st: &mut State, start: f64) -> bool {
    if let Some(s) = st.player.stop.take() {
        s.store(true, Ordering::Relaxed);
    }
    let rate = match st.ecoute.ouvrir() {
        Ok(r) => r,
        Err(e) => {
            app.set_message(format!("Pas de son : {e}. Réglages › Sortie d'écoute.").into());
            return false;
        }
    };
    let (prod, cons) = rtrb::RingBuffer::new((rate * 2.0) as usize * 2);
    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    st.player.stop = Some(stop.clone());
    st.player.finished = Some(finished.clone());
    let path = st.player.path.clone();
    let _ = std::thread::Builder::new().name("lecteur".into()).spawn(move || decode(path, start.max(0.0), rate, prod, stop, finished));
    st.ecoute.lancer(cons, (start.max(0.0) * rate) as u64);
    st.ecoute.gain(st.player.gain);
    st.player.paused = false;
    st.player.pos = start;
    true
}

/// Ouvre un morceau dans le lecteur et le joue.
pub fn play_id(app: &AppWindow, st: &mut State, id: i64, at: f64) {
    let Some(c) = st.career.as_ref() else { return };
    let Some(r) = lib::row_by_id(c.conn(), id).ok().flatten() else { return };
    let markers = lib::markers(c.conn(), id);
    // Relié au PC principal, la musique reste chez lui : elle arrive à la demande.
    let mut r = r;
    if !Path::new(&r.path).exists() && st.distant.is_some() {
        match crate::distant::cached_audio(id) {
            Some(p) => r.path = p.to_string_lossy().into_owned(),
            None => return crate::distant::fetch_audio(app, st, id, at),
        }
    }
    if !Path::new(&r.path).exists() {
        app.set_message(format!("« {} » est introuvable sur le disque (lecteur débranché, dossier renommé ?).", r.title).into());
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    let _ = lib::played(c.conn(), id);
    st.player.id = Some(id);
    st.player.path = r.path.clone();
    st.player.title = r.title.clone();
    st.player.sub = [r.artist.clone(), lib::duration_text(r.duration), r.cell("camelot"), r.cell("bpm")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    st.player.duration = r.duration.or_else(|| crate::samples::probe(&r.path).map(|p| p.0)).unwrap_or(0.0);
    st.player.markers = markers;
    st.player.peaks.clear();
    if st.player.compare.as_ref().is_none_or(|cmp| cmp.a.0 != id && cmp.b.0 != id) {
        st.player.compare = None;
        st.player.loop_a = None;
        st.player.loop_b = None;
        st.player.gain = 1.0;
    }
    st.lib.playing = Some(id);
    start_stream(app, st, at);
    // La forme d'onde, calculée une fois (cache du serveur mobile).
    let path = r.path.clone();
    let cache = if st.career_path == crate::career::default_path() { crate::donnees::data_dir().join("mobile") } else { std::env::temp_dir().join("otoko-essai-pochettes") };
    st.spawn(move || {
        let w = otoko_mobile::media::Cache::new(cache).waveform(Path::new(&path), 400);
        let peaks: Vec<f32> = w.as_ref().and_then(|v| v["peaks"].as_array().cloned()).unwrap_or_default().iter().filter_map(|p| p.as_f64()).map(|p| p as f32).collect();
        crate::state::Done::Waveform { id, peaks }
    });
    ensure_timer(app, st);
    push(app, st);
    crate::bibliotheque::push_rows(app, st);
}

/// Identifiant du lecteur pour un fichier hors de la bibliothèque (un master tout juste créé).
pub const OUTSIDE: i64 = -1;

/// Joue un fichier qui n'est pas (encore) dans la bibliothèque.
pub fn play_path(app: &AppWindow, st: &mut State, path: &Path, title: &str) {
    if !path.exists() {
        app.set_message(format!("« {} » est introuvable sur le disque.", path.display()).into());
        return;
    }
    let p = path.to_string_lossy().into_owned();
    st.player.id = Some(OUTSIDE);
    st.player.path = p.clone();
    st.player.title = title.to_string();
    st.player.duration = crate::samples::probe(&p).map(|x| x.0).unwrap_or(0.0);
    st.player.sub = lib::duration_text(Some(st.player.duration));
    st.player.markers.clear();
    st.player.peaks.clear();
    st.player.compare = None;
    st.player.loop_a = None;
    st.player.loop_b = None;
    st.player.gain = 1.0;
    st.lib.playing = None;
    start_stream(app, st, 0.0);
    ensure_timer(app, st);
    push(app, st);
}

pub fn waveform(app: &AppWindow, st: &mut State, id: i64, peaks: Vec<f32>) {
    if st.player.id == Some(id) {
        st.player.peaks = peaks;
        push(app, st);
    }
}

pub fn toggle(app: &AppWindow, st: &mut State) {
    if st.player.id.is_none() {
        return;
    }
    st.player.paused = !st.player.paused;
    st.ecoute.pause(st.player.paused);
    push(app, st);
}

pub fn stop(app: &AppWindow, st: &mut State) {
    if let Some(s) = st.player.stop.take() {
        s.store(true, Ordering::Relaxed);
    }
    // La carte est rendue aux autres logiciels.
    st.ecoute.fermer();
    st.player.id = None;
    st.player.compare = None;
    st.lib.playing = None;
    push(app, st);
    crate::bibliotheque::push_rows(app, st);
}

/// Va à une position (fraction 0..1 de la durée).
pub fn seek(app: &AppWindow, st: &mut State, frac: f64) {
    if st.player.id.is_none() || st.player.duration <= 0.0 {
        return;
    }
    let t = frac.clamp(0.0, 1.0) * st.player.duration;
    let paused = st.player.paused;
    if start_stream(app, st, t) && paused {
        st.player.paused = true;
        st.ecoute.pause(true);
    }
    push(app, st);
}

/// Le morceau suivant : la file d'abord, sinon la ligne suivante du tableau.
pub fn next(app: &AppWindow, st: &mut State) {
    if !st.player.queue.is_empty() {
        let id = st.player.queue.remove(0);
        return play_id(app, st, id, 0.0);
    }
    let cur = st.player.id;
    if cur == Some(OUTSIDE) {
        return stop(app, st);
    }
    let i = st.lib.rows.iter().position(|r| Some(r.id) == cur).map(|i| i + 1).unwrap_or(0);
    match st.lib.rows.iter().skip(i).find(|r| !r.missing).map(|r| r.id) {
        Some(id) => play_id(app, st, id, 0.0),
        None => stop(app, st),
    }
}

pub fn previous(app: &AppWindow, st: &mut State) {
    // Plus de 3 s : retour au début du morceau.
    if st.player.pos > 3.0 {
        return seek(app, st, 0.0);
    }
    let cur = st.player.id;
    if let Some(i) = st.lib.rows.iter().position(|r| Some(r.id) == cur) {
        if let Some(id) = st.lib.rows[..i].iter().rev().find(|r| !r.missing).map(|r| r.id) {
            return play_id(app, st, id, 0.0);
        }
    }
    seek(app, st, 0.0);
}

/// Ajoute des morceaux à la file (Q).
pub fn enqueue(app: &AppWindow, st: &mut State, ids: &[i64]) {
    st.player.queue.extend_from_slice(ids);
    app.set_message(format!("{} morceau(x) dans la file de lecture.", st.player.queue.len()).into());
    push(app, st);
}

/// Boucle A-B : « a », « b » (à la position actuelle) ou « clear ».
pub fn set_loop(app: &AppWindow, st: &mut State, what: &str) {
    match what {
        "a" => st.player.loop_a = Some(st.player.pos),
        "b" => {
            st.player.loop_b = Some(st.player.pos);
            if let (Some(a), Some(b)) = (st.player.loop_a, st.player.loop_b) {
                if b <= a {
                    st.player.loop_a = Some(b);
                    st.player.loop_b = Some(a);
                }
            }
        }
        _ => {
            st.player.loop_a = None;
            st.player.loop_b = None;
        }
    }
    push(app, st);
}

/// Repère à une position (fraction), gardé avec le morceau (table `markers`).
pub fn add_marker(app: &AppWindow, st: &mut State, frac: f64) {
    let Some(id) = st.player.id else { return };
    let t = (frac.clamp(0.0, 1.0) * st.player.duration * 10.0).round() / 10.0;
    let label = format!("Repère {}", st.player.markers.len() + 1);
    if let Some(c) = st.career.as_ref() {
        let _ = lib::add_marker(c.conn(), id, t, &label);
    }
    st.player.markers.push((t, label));
    st.player.markers.sort_by(|a, b| a.0.total_cmp(&b.0));
    push(app, st);
}

pub fn remove_marker(app: &AppWindow, st: &mut State, k: usize) {
    let Some(id) = st.player.id else { return };
    if k >= st.player.markers.len() {
        return;
    }
    let (t, label) = st.player.markers.remove(k);
    if let Some(c) = st.career.as_ref() {
        let _ = lib::remove_marker(c.conn(), id, t, &label);
    }
    push(app, st);
}

/// Comparateur A/B : les deux premiers morceaux de la sélection, à sonie égale.
pub fn compare(app: &AppWindow, st: &mut State, ids: &[i64]) {
    if ids.len() != 2 {
        app.set_message("Sélectionne deux morceaux pour les comparer (Ctrl+clic), puis Ctrl+D.".into());
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    let lufs = |id: i64| lib::field(c.conn(), id, "lufs").parse::<f64>().ok();
    let (la, lb) = (lufs(ids[0]), lufs(ids[1]));
    // Le plus fort est baissé au niveau de l'autre.
    let (ga, gb) = match (la, lb) {
        (Some(a), Some(b)) => {
            let target = a.min(b);
            (10f64.powf((target - a) / 20.0) as f32, 10f64.powf((target - b) / 20.0) as f32)
        }
        _ => (1.0, 1.0),
    };
    let note = match (la, lb) {
        (Some(a), Some(b)) => format!("Comparaison à sonie égale ({a:.1} et {b:.1} LUFS ramenés à {:.1}). Tab ou « A | B » pour passer de l'un à l'autre au même endroit.", a.min(b)),
        _ => "Comparaison A/B : analyse les deux morceaux pour les entendre à sonie égale. Tab pour passer de l'un à l'autre.".into(),
    };
    st.player.compare = Some(Compare { a: (ids[0], ga), b: (ids[1], gb), on_b: false });
    st.player.gain = ga;
    play_id(app, st, ids[0], 0.0);
    app.set_message(note.into());
}

/// Passe de A à B (ou l'inverse) au même endroit.
pub fn switch_ab(app: &AppWindow, st: &mut State) {
    let Some(mut cmp) = st.player.compare.clone() else { return };
    cmp.on_b = !cmp.on_b;
    let (id, gain) = if cmp.on_b { cmp.b } else { cmp.a };
    let at = st.player.pos;
    st.player.compare = Some(cmp);
    st.player.gain = gain;
    play_id(app, st, id, at);
}

/// L'horloge du lecteur (créée par `wire`) tourne tant qu'un morceau est ouvert.
fn ensure_timer(app: &AppWindow, st: &mut State) {
    let _ = app;
    if let Some(t) = st.player.timer.as_ref() {
        if !t.running() {
            t.restart();
        }
    }
}

/// Crée l'horloge du lecteur (100 ms), arrêtée tant que rien ne joue.
pub fn wire(app: &AppWindow, state: &std::rc::Rc<std::cell::RefCell<State>>) {
    let timer = slint::Timer::default();
    let weak = app.as_weak();
    let s = state.clone();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(app) = weak.upgrade() else { return };
        if let Ok(mut st) = s.try_borrow_mut() {
            tick(&app, &mut st);
            if st.player.id.is_none() {
                if let Some(t) = st.player.timer.as_ref() {
                    t.stop();
                }
            }
        }
    });
    timer.stop();
    state.borrow_mut().player.timer = Some(timer);
}

/// Avance de la position (appelée toutes les 100 ms tant qu'un morceau est ouvert).
pub fn tick(app: &AppWindow, st: &mut State) {
    if st.player.id.is_none() {
        return;
    }
    st.player.pos = st.ecoute.position() as f64 / st.ecoute.rate();
    // Boucle A-B.
    if let (Some(a), Some(b)) = (st.player.loop_a, st.player.loop_b) {
        if st.player.pos >= b && b > a {
            start_stream(app, st, a);
        }
    }
    // Fin du morceau : le suivant (la file d'abord), sauf si « Enchaîner » est décoché dans les préférences.
    let done = st.player.finished.as_ref().is_some_and(|f| f.load(Ordering::Relaxed)) && st.ecoute.a_sec() && !st.player.paused;
    if done {
        let continuous = st.career.as_ref().is_none_or(|c| lib::flag(c.conn(), "continuous_play", true));
        return if continuous || !st.player.queue.is_empty() { next(app, st) } else { stop(app, st) };
    }
    let g = app.global::<Lib>();
    g.set_player_pos(if st.player.duration > 0.0 { (st.player.pos / st.player.duration).clamp(0.0, 1.0) as f32 } else { 0.0 });
    g.set_player_time(format!("{} / {}", lib::duration_text(Some(st.player.pos)), lib::duration_text(Some(st.player.duration))).into());
}

/// Les notes du morceau, écrites depuis le lecteur ouvert.
pub fn notes_edited(app: &AppWindow, st: &mut State, text: &str) {
    let Some(id) = st.player.id.filter(|id| *id != OUTSIDE) else { return };
    let Some(c) = st.career.as_ref() else { return };
    if let Err(e) = lib::update(c.conn(), id, "notes", text) {
        app.set_message(format!("Notes non enregistrées : {e}").into());
    }
}

/// Le lecteur tel qu'il est.
pub fn push(app: &AppWindow, st: &State) {
    let g = app.global::<Lib>();
    let p = &st.player;
    g.set_player_open(p.id.is_some());
    if let Some(c) = st.career.as_ref() {
        let title = |id: &i64| lib::row_by_id(c.conn(), *id).ok().flatten().map(|r| r.title).unwrap_or_default();
        g.set_player_queue_list(ModelRc::new(VecModel::from(p.queue.iter().take(3).map(|id| SharedString::from(title(id))).collect::<Vec<_>>())));
        let notes = p.id.filter(|id| *id != OUTSIDE).map(|id| lib::field(c.conn(), id, "notes")).unwrap_or_default();
        if g.get_player_notes() != notes.as_str() {
            g.set_player_notes(notes.into());
        }
    }
    g.set_player_title(p.title.as_str().into());
    g.set_player_sub(p.sub.as_str().into());
    g.set_player_paused(p.paused);
    g.set_player_peaks(ModelRc::new(VecModel::from(p.peaks.clone())));
    let d = p.duration.max(0.001);
    g.set_player_markers(ModelRc::new(VecModel::from(p.markers.iter().map(|(t, l)| MarkUi { x: (*t / d).clamp(0.0, 1.0) as f32, label: SharedString::from(l.as_str()) }).collect::<Vec<_>>())));
    g.set_player_loop_a(p.loop_a.map(|a| (a / d) as f32).unwrap_or(-1.0));
    g.set_player_loop_b(p.loop_b.map(|b| (b / d) as f32).unwrap_or(-1.0));
    g.set_player_queue(p.queue.len() as i32);
    g.set_player_compare(match &p.compare {
        Some(c) => if c.on_b { "B".into() } else { "A".into() },
        None => "".into(),
    });
    let cover = p.id.and_then(|id| st.lib.covers.get(&id).cloned().flatten());
    crate::preferences::cover_accent(app, st, cover.as_ref());
    g.set_player_has_cover(cover.is_some());
    g.set_player_cover(cover.unwrap_or_default());
    g.set_player_pos(if p.duration > 0.0 { (p.pos / p.duration).clamp(0.0, 1.0) as f32 } else { 0.0 });
    g.set_player_time(format!("{} / {}", lib::duration_text(Some(p.pos)), lib::duration_text(Some(p.duration))).into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resampler_keeps_its_place_between_packets() {
        // Une sinusoïde 44,1 → 48 kHz, envoyée en petits paquets : pas de saut aux jointures.
        let x: Vec<[f32; 2]> = (0..44_100).map(|i| { let v = (i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin(); [v, v] }).collect();
        let mut r = Resampler::new(44_100.0, 48_000.0);
        let mut out = Vec::new();
        for chunk in x.chunks(1000) {
            r.push(chunk, &mut out);
        }
        let n = out.len() / 2;
        assert!((n as i64 - 48_000).abs() < 10, "{n} images");
        let worst = (0..n).map(|k| (out[2 * k] - (k as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin()).abs()).take(n - 10).fold(0.0f32, f32::max);
        assert!(worst < 0.01, "écart {worst}");
    }
}
