//! Analyse audio locale des morceaux de la Carrière (comme `composer_vault/audio/analyzer.py`) :
//! tempo, tonalité (profils de Krumhansl-Schmuckler sur un chroma), sonie intégrée (UIT-R BS.1770),
//! énergie, brillance et dynamique (0 à 1), mode, crête réelle, part d'échantillons saturés,
//! silences au début et à la fin, empreinte du contenu (doublons convertis). Les 5 premières
//! minutes sont lues en pleine résolution, comme dans Studio Vault.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use otoko_library::career::files::{self, TagValues};
use otoko_library::career::library::{self as lib, Analysis};

use crate::state::{Done, State};
use crate::{AppWindow, Lib};
use slint::ComponentHandle;

const PITCH: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const MAJOR: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
const SILENCE_DB: f64 = -50.0;
const FP_SEGMENTS: usize = 64;
const NFFT: usize = 4096;

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

fn round(x: f64, d: i32) -> f64 {
    let p = 10f64.powi(d);
    (x * p).round() / p
}

/// Niveau (dB) de chaque fenêtre de `win` secondes.
fn frame_db(y: &[f32], sr: f64, win: f64) -> Vec<f64> {
    let n = ((sr * win) as usize).max(1);
    let frames: Vec<f64> = y
        .chunks_exact(n)
        .map(|f| {
            let ms = f.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / n as f64;
            20.0 * ((ms + 1e-12).sqrt() + 1e-12).log10()
        })
        .collect();
    if frames.is_empty() { vec![-120.0] } else { frames }
}

/// Centile avec interpolation linéaire (comme `numpy.percentile`).
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = p / 100.0 * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

fn pearson(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let (ma, mb) = (a.iter().sum::<f64>() / 12.0, b.iter().sum::<f64>() / 12.0);
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        num += (a[i] - ma) * (b[i] - mb);
        da += (a[i] - ma).powi(2);
        db += (b[i] - mb).powi(2);
    }
    if da <= 0.0 || db <= 0.0 { f64::NEG_INFINITY } else { num / (da * db).sqrt() }
}

/// Transformée de Fourier (radix 2), sur place.
struct Fft {
    n: usize,
    rev: Vec<usize>,
    tw: Vec<(f32, f32)>,
    window: Vec<f32>,
}

impl Fft {
    fn new(n: usize) -> Fft {
        let bits = n.trailing_zeros();
        Fft {
            n,
            rev: (0..n).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect(),
            tw: (0..n / 2)
                .map(|k| {
                    let a = -2.0 * std::f64::consts::PI * k as f64 / n as f64;
                    (a.cos() as f32, a.sin() as f32)
                })
                .collect(),
            window: (0..n).map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()) as f32).collect(),
        }
    }

    /// Amplitudes des fréquences 0..n/2 d'une trame (fenêtre de Hann).
    fn magnitudes(&self, frame: &[f32], out: &mut [f32]) {
        let n = self.n;
        let mut re: Vec<f32> = frame.iter().zip(&self.window).map(|(x, w)| x * w).collect();
        re.resize(n, 0.0);
        let mut im = vec![0.0f32; n];
        for i in 0..n {
            let j = self.rev[i];
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = self.tw[k * step];
                    let (a, b) = (start + k, start + k + half);
                    let (xr, xi) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                    re[b] = re[a] - xr;
                    im[b] = im[a] - xi;
                    re[a] += xr;
                    im[a] += xi;
                }
            }
            len *= 2;
        }
        for k in 0..n / 2 {
            out[k] = (re[k] * re[k] + im[k] * im[k]).sqrt();
        }
    }
}

/// Tonalité (sur 4 minutes) et brillance moyenne (centroïde spectral, sur tout ce qui est lu).
///
/// Le chroma suit `librosa.feature.chroma_cqt` : énergie par demi-ton du do 1 (32,7 Hz) au do 8,
/// amplitude (pas puissance), chaque trame ramenée à son maximum, puis moyenne. Sans cela, le
/// kick (vers 55-60 Hz) écrase tout et la tonalité tombe presque toujours sur la ou la dièse.
fn key_and_brightness(y: &[f32], sr: f64) -> (Option<String>, f64) {
    (key(y, sr), brightness(y, sr))
}

/// Centroïde spectral moyen, ramené de 800 Hz (0) à 3,5 kHz (1) : fenêtres de 2048 points au pas
/// de 512, comme `librosa.feature.spectral_centroid` (la taille de fenêtre change le résultat).
fn brightness(y: &[f32], sr: f64) -> f64 {
    const N: usize = 2048;
    let fft = Fft::new(N);
    let mut mag = vec![0.0f32; N / 2];
    let bin_hz = sr / N as f64;
    let (mut sum, mut frames) = (0.0f64, 0usize);
    let mut start = 0;
    while start + N <= y.len() {
        fft.magnitudes(&y[start..start + N], &mut mag);
        let total: f64 = mag.iter().map(|m| *m as f64).sum();
        let weighted: f64 = mag.iter().enumerate().map(|(k, m)| *m as f64 * k as f64 * bin_hz).sum();
        sum += if total > 1e-9 { weighted / total } else { 0.0 };
        frames += 1;
        start += 512;
    }
    if frames > 0 { clamp01((sum / frames as f64 - 800.0) / 2700.0) } else { 0.5 }
}

fn key(y: &[f32], sr: f64) -> Option<String> {
    let fft = Fft::new(NFFT);
    let mut mag = vec![0.0f32; NFFT / 2];
    let mut chroma = [0.0f64; 12];
    let bin_hz = sr / NFFT as f64;
    // Pour chaque demi-ton (MIDI 24 à 107) : les cases de fréquence de sa bande, ou, pour les
    // notes graves plus fines qu'une case, la case la plus proche.
    let bands: Vec<(usize, Vec<usize>)> = (24..108)
        .filter_map(|m: i64| {
            let f = |x: f64| 440.0 * 2f64.powf((x - 69.0) / 12.0);
            let (lo, hi) = (f(m as f64 - 0.5) / bin_hz, f(m as f64 + 0.5) / bin_hz);
            if hi >= (NFFT / 2) as f64 {
                return None;
            }
            let mut bins: Vec<usize> = (lo.ceil() as usize..hi.ceil() as usize).filter(|k| *k > 0).collect();
            if bins.is_empty() {
                bins.push((f(m as f64) / bin_hz).round() as usize);
            }
            Some((m.rem_euclid(12) as usize, bins))
        })
        .collect();
    // Les 4 premières minutes.
    let end = y.len().min((240.0 * sr) as usize);
    let mut start = 0;
    while start + NFFT <= end {
        fft.magnitudes(&y[start..start + NFFT], &mut mag);
        let mut frame = [0.0f64; 12];
        for (pc, bins) in &bands {
            frame[*pc] += bins.iter().map(|k| mag[*k] as f64).fold(0.0, f64::max);
        }
        let max = frame.iter().fold(0.0f64, |a, b| a.max(*b));
        if max > 1e-9 {
            for i in 0..12 {
                chroma[i] += frame[i] / max;
            }
        }
        start += NFFT / 2;
    }
    let sum: f64 = chroma.iter().sum();
    if sum <= 0.0 {
        return None;
    }
    chroma.iter_mut().for_each(|c| *c /= sum);
    let mut best = (f64::NEG_INFINITY, 0usize, false);
    for i in 0..12 {
        for (minor, profile) in [(false, &MAJOR), (true, &MINOR)] {
            // np.roll(profile, i) : la tonique en i.
            let rolled: [f64; 12] = std::array::from_fn(|j| profile[(j + 12 - i) % 12]);
            let s = pearson(&chroma, &rolled);
            if s > best.0 {
                best = (s, i, minor);
            }
        }
    }
    Some(format!("{}{}", PITCH[best.1], if best.2 { "m" } else { "" }))
}

/// Empreinte : durée arrondie et sens des variations d'énergie sur 64 tranches.
fn fingerprint(y: &[f32], duration: f64) -> Option<String> {
    if y.len() < FP_SEGMENTS {
        return None;
    }
    let seg = y.len() / FP_SEGMENTS;
    let env: Vec<f64> = (0..FP_SEGMENTS).map(|s| (y[s * seg..(s + 1) * seg].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / seg as f64 + 1e-12).sqrt()).collect();
    let mut value: u64 = 0;
    for w in env.windows(2) {
        value = (value << 1) | (w[1] > w[0]) as u64;
    }
    Some(format!("{}:{value:016x}", duration.round_ties_even() as i64))
}

/// Analyse complète d'un fichier.
pub fn analyze(path: &str) -> Result<Analysis, String> {
    let (secs, rate, _) = crate::samples::probe(path).ok_or("lecture impossible")?;
    let data = crate::samples::load(path, rate as f64, Some(300.0))?;
    let sr = rate as f64;
    let (l, r) = (&data.left, &data.right);
    let n = l.len().min(r.len());
    if n == 0 {
        return Err("fichier vide".into());
    }
    let mut a = Analysis { samplerate: Some(rate as i64), duration: Some(secs), ..Default::default() };
    // Mono à ~22 kHz (un échantillon sur deux à 44,1 kHz, comme Studio Vault).
    let step = (rate as usize / 22_050).max(1);
    let y: Vec<f32> = (0..n).step_by(step).map(|i| 0.5 * (l[i] + r[i])).collect();
    let ysr = sr / step as f64;
    // Tempo sur 4 minutes, ramené entre 60 et 200.
    let cut = n.min((240.0 * sr) as usize);
    let head = crate::samples::SampleData { left: l[..cut].to_vec(), right: r[..cut].to_vec() };
    a.bpm = crate::samples::estimate_bpm(&head, sr).map(|mut b| {
        while b < 60.0 {
            b *= 2.0;
        }
        while b > 200.0 {
            b /= 2.0;
        }
        round(b, 1)
    });
    let (key, brightness) = key_and_brightness(&y, ysr);
    a.mode = key.as_ref().map(|k| if k.ends_with('m') { "mineur".to_string() } else { "majeur".to_string() });
    a.key = key;
    let mut meter = crate::loudness::Meter::new(sr);
    meter.add(&l[..n], &r[..n]);
    let lufs = meter.integrated();
    a.lufs = (lufs > -70.0).then(|| round(lufs, 1));
    // Caractère : niveau des passages actifs, écart fort / faible, brillance.
    let db = frame_db(&y, ysr, 0.4);
    let mut active: Vec<f64> = db.into_iter().filter(|d| *d > SILENCE_DB).collect();
    if active.is_empty() {
        a.energy = Some(0.0);
        a.brightness = Some(0.0);
        a.dynamics = Some(0.0);
    } else {
        let mean = active.iter().sum::<f64>() / active.len() as f64;
        active.sort_by(f64::total_cmp);
        a.energy = Some(round(clamp01((mean + 30.0) / 22.0), 3));
        a.dynamics = Some(round(clamp01((percentile(&active, 95.0) - percentile(&active, 10.0) - 3.0) / 17.0), 3));
        a.brightness = Some(round(brightness, 3));
    }
    let tp = crate::publish::true_peak(&l[..n]).max(crate::publish::true_peak(&r[..n])) as f64;
    a.true_peak = Some(round(20.0 * (tp + 1e-12).log10(), 2));
    let clipped = (0..n).filter(|&i| l[i].abs().max(r[i].abs()) >= 0.999).count();
    a.clip_ratio = Some(round(clipped as f64 / n as f64, 6));
    // Silences (fenêtres de 50 ms).
    let db = frame_db(&y, ysr, 0.05);
    let above: Vec<usize> = db.iter().enumerate().filter(|(_, d)| **d > SILENCE_DB).map(|(i, _)| i).collect();
    match (above.first(), above.last()) {
        (Some(&first), Some(&last)) => {
            a.silence_start = Some(round(first as f64 * 0.05, 2));
            // La fin n'est mesurée que si tout le morceau a été lu.
            a.silence_end = (secs <= n as f64 / sr + 1.0).then(|| round((db.len() - 1 - last) as f64 * 0.05, 2));
        }
        _ => {
            a.silence_start = Some(round(y.len() as f64 / ysr, 2));
            a.silence_end = Some(0.0);
        }
    }
    a.audio_fp = fingerprint(&y, secs);
    Ok(a)
}

// ---------------------------------------------------------------------- en arrière-plan

/// Lance l'analyse de morceaux (sélection, ou ceux jamais analysés en fond).
pub fn start(app: &AppWindow, st: &mut State, todo: Vec<(i64, String)>, quiet: bool) {
    if st.distant.is_some() {
        if !quiet {
            app.set_message("L'analyse se fait sur le PC principal, où sont les fichiers.".into());
        }
        return;
    }
    if st.lib.analysing.is_some() || todo.is_empty() {
        if !quiet && todo.is_empty() {
            app.set_message("Tous ces morceaux sont déjà analysés.".into());
        }
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    st.lib.analysing = Some(stop.clone());
    // Les tags ne s'écrivent dans les fichiers que pour la vraie Carrière, jamais pendant un essai
    // (autotest, vitrines sur une base temporaire : les fichiers, eux, sont ceux de l'utilisateur).
    // QCM du 4 novembre 2026 : l'analyse n'écrit jamais dans les fichiers audio. L'outil « Écrire
    // les tags dans les fichiers » (menu des outils) reste, à la demande.
    let write = false;
    let db = st.career_path.clone();
    let tx = st.jobs.clone();
    let total = todo.len();
    app.global::<Lib>().set_analysis_text(format!("Analyse : 0 sur {total}…").into());
    app.global::<Lib>().set_analysing(true);
    st.spawn(move || {
        let Ok(c) = otoko_library::career::Career::open(&db) else { return Done::Analysed { done: 0, total, ok: 0, failed: total, last: true, quiet } };
        let (mut ok, mut failed_ids) = (0, Vec::new());
        for (k, (id, path)) in todo.iter().enumerate() {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            if !Path::new(path).exists() {
                continue;
            }
            match analyze(path) {
                Ok(a) => {
                    if lib::save_analysis(c.conn(), *id, &a).is_ok() {
                        ok += 1;
                        if write && (a.bpm.is_some() || a.key.is_some()) {
                            let _ = files::write_tags(Path::new(path), &TagValues { bpm: a.bpm.map(|b| format!("{b:.0}")), key: a.key.clone(), ..Default::default() });
                        }
                    }
                }
                Err(_) => failed_ids.push(*id),
            }
            let _ = tx.send(Done::Analysed { done: k + 1, total, ok, failed: failed_ids.len(), last: false, quiet });
        }
        lib::mark_failed(c.conn(), &failed_ids);
        Done::Analysed { done: total, total, ok, failed: failed_ids.len(), last: true, quiet }
    });
}

pub fn progress(app: &AppWindow, st: &mut State, done: usize, total: usize, ok: usize, failed: usize, last: bool, quiet: bool) {
    let g = app.global::<Lib>();
    if !last {
        st.pending += 1;
        g.set_analysis_text(format!("Analyse : {done} sur {total}…").into());
        return;
    }
    st.lib.analysing = None;
    g.set_analysing(false);
    let text = format!("Analyse terminée : {ok} morceau{}{}.", if ok > 1 { "x" } else { "" }, if failed > 0 { format!(", {failed} illisible(s)") } else { String::new() });
    g.set_analysis_text(if quiet && ok == 0 { "".into() } else { text.as_str().into() });
    if !quiet {
        app.set_message(text.into());
    }
    // La base a été écrite par une autre connexion.
    st.career = crate::career::open(&st.career_path);
    crate::bibliotheque::fill(app, st);
}

pub fn stop(st: &mut State) {
    if let Some(s) = &st.lib.analysing {
        s.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compare à l'analyse de Studio Vault sur de vrais morceaux (copie de sa base, fichiers lus).
    /// `cargo test -p otoko --release -- --ignored --nocapture compare_with_studio_vault`
    #[test]
    #[ignore]
    fn compare_with_studio_vault() {
        let Some(dir) = otoko_library::vault::default_dir() else { return };
        let (conn, tmp) = otoko_library::vault::snapshot(&dir).unwrap();
        let rows: Vec<(String, Option<f64>, Option<String>, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<String>)> = conn
            .prepare(
                "SELECT path, bpm, music_key, lufs, energy, brightness, dynamics, true_peak, mood_auto FROM tracks
                 WHERE analyzed_at IS NOT NULL AND missing = 0 AND duration_s < 400 ORDER BY id DESC LIMIT 15",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))
            .unwrap()
            .flatten()
            .collect();
        let (mut bpm_ok, mut key_ok, mut lufs_d) = (0, 0, Vec::new());
        for (path, bpm, key, lufs, energy, bright, dyn_, tp, mood) in &rows {
            let t = std::time::Instant::now();
            let Ok(a) = analyze(path) else { continue };
            let same_bpm = match (bpm, a.bpm) {
                (Some(x), Some(y)) => (x - y).abs() < 2.0 || (x * 2.0 - y).abs() < 2.0 || (x - y * 2.0).abs() < 2.0,
                _ => false,
            };
            bpm_ok += same_bpm as usize;
            key_ok += (key.as_deref() == a.key.as_deref()) as usize;
            if let (Some(x), Some(y)) = (lufs, a.lufs) {
                lufs_d.push((x - y).abs());
            }
            let mood2 = otoko_library::career::humeur::texte(&otoko_library::career::humeur::deduire(a.mode.as_deref(), a.bpm, a.energy, a.brightness, a.dynamics));
            println!(
                "{:<38} bpm {:>5.1}/{:<5.1} clé {:>4}/{:<4} lufs {:>5.1}/{:<5.1} én {:.2}/{:.2} bril {:.2}/{:.2} dyn {:.2}/{:.2} crête {:+.1}/{:+.1} « {} » / « {} » ({:.1} s)",
                std::path::Path::new(path).file_stem().unwrap().to_string_lossy().chars().take(38).collect::<String>(),
                bpm.unwrap_or(0.0),
                a.bpm.unwrap_or(0.0),
                key.clone().unwrap_or_default(),
                a.key.clone().unwrap_or_default(),
                lufs.unwrap_or(0.0),
                a.lufs.unwrap_or(0.0),
                energy.unwrap_or(0.0),
                a.energy.unwrap_or(0.0),
                bright.unwrap_or(0.0),
                a.brightness.unwrap_or(0.0),
                dyn_.unwrap_or(0.0),
                a.dynamics.unwrap_or(0.0),
                tp.unwrap_or(0.0),
                a.true_peak.unwrap_or(0.0),
                mood.clone().unwrap_or_default(),
                mood2,
                t.elapsed().as_secs_f64()
            );
        }
        println!("tempo identique (ou double / moitié) : {bpm_ok}/{} ; tonalité identique : {key_ok}/{} ; écart moyen de sonie : {:.2} LU", rows.len(), rows.len(), lufs_d.iter().sum::<f64>() / lufs_d.len().max(1) as f64);
        drop(conn);
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn key_of_a_minor_chord() {
        // Accord de la mineur (A, C, E) tenu, à 22 050 Hz.
        let sr = 22_050.0;
        let y: Vec<f32> = (0..(sr as usize * 6))
            .map(|i| {
                let t = i as f64 / sr;
                ([220.0, 261.63, 329.63, 440.0].iter().map(|f| (std::f64::consts::TAU * f * t).sin()).sum::<f64>() * 0.2) as f32
            })
            .collect();
        let (key, bright) = key_and_brightness(&y, sr);
        assert_eq!(key.as_deref(), Some("Am"));
        assert!(bright < 0.1, "un son grave et doux n'est pas brillant : {bright}");
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 50.0), 2.5);
        assert!(fingerprint(&y, 6.4).unwrap().starts_with("6:"));
    }
}
