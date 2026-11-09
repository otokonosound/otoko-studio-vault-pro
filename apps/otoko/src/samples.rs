//! Lire les fichiers audio : durée, décodage, et l'estimation du tempo.

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Un son décodé en mémoire, stéréo.
#[derive(Default)]
pub struct SampleData {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// Ouvre un fichier audio (WAV, AIFF, FLAC, MP3, OGG, AAC/M4A).
pub fn open(path: &str) -> Result<Box<dyn FormatReader>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("format non reconnu ({e})"))?;
    Ok(probed.format)
}

/// Durée (s), fréquence et nombre de canaux d'un fichier, lus dans l'en-tête.
pub fn probe(path: &str) -> Option<(f64, u32, u16)> {
    let format = open(path).ok()?;
    let track = format.default_track()?;
    let p = &track.codec_params;
    let rate = p.sample_rate?;
    let channels = p.channels.map(|c| c.count() as u16).unwrap_or(2);
    let secs = match p.n_frames {
        Some(n) => n as f64 / rate as f64,
        None => duration(path).ok()?,
    };
    Some((secs, rate, channels))
}

/// Durée d'un fichier en secondes, lue dans l'en-tête quand c'est possible.
pub fn duration(path: &str) -> Result<f64, String> {
    let format = open(path)?;
    let track = format.default_track().ok_or("aucune piste audio")?;
    let rate = track.codec_params.sample_rate.ok_or("fréquence inconnue")? as f64;
    match track.codec_params.n_frames {
        Some(n) => Ok(n as f64 / rate),
        // Certains MP3 n'indiquent pas leur durée : on décode pour la connaître.
        None => Ok(load(path, rate, None)?.left.len() as f64 / rate),
    }
}

/// Charge un fichier audio à la fréquence `rate`. `max_secs` limite la lecture (écoute rapide).
pub fn load(path: &str, rate: f64, max_secs: Option<f64>) -> Result<SampleData, String> {
    let mut format = open(path)?;
    let track = format.default_track().ok_or("aucune piste audio")?;
    let id = track.id;
    let src = track.codec_params.sample_rate.ok_or("fréquence inconnue")? as f64;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("décodeur indisponible ({e})"))?;
    let limit = max_secs.map(|s| (s * src) as usize).unwrap_or(usize::MAX);
    let (mut l, mut r) = (Vec::new(), Vec::new());
    while l.len() < limit {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue, // paquet abîmé : on passe au suivant
            Err(e) => return Err(e.to_string()),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        for f in buf.samples().chunks(channels) {
            l.push(f[0]);
            r.push(if channels > 1 { f[1] } else { f[0] });
        }
    }
    l.truncate(limit);
    r.truncate(limit);
    if (src - rate).abs() < 0.5 {
        return Ok(SampleData { left: l, right: r });
    }
    Ok(SampleData { left: resample(&l, src, rate), right: resample(&r, src, rate) })
}

/// Rééchantillonnage par interpolation d'Hermite à 4 points : propre et rapide.
fn resample(x: &[f32], from: f64, to: f64) -> Vec<f32> {
    let n = ((x.len() as f64) * to / from) as usize;
    let step = from / to;
    let at = |i: isize| x[i.clamp(0, x.len() as isize - 1) as usize];
    (0..n)
        .map(|k| {
            let pos = k as f64 * step;
            let i = pos.floor() as isize;
            let t = (pos - i as f64) as f32;
            let (y0, y1, y2, y3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
            ((c3 * t + c2) * t + c1) * t + y1
        })
        .collect()
}

/// Estime le tempo d'un son rythmé : autocorrélation du flux d'énergie, affinée sur plusieurs
/// temps, puis calée sur une longueur de boucle (1 à 16 mesures) quand le son en est une.
pub fn estimate_bpm(data: &SampleData, rate: f64) -> Option<f64> {
    let hop = 256usize;
    let n = data.left.len().min(data.right.len()) / hop;
    if n < 128 {
        return None;
    }
    // Flux positif de l'amplitude : il monte à chaque attaque, et garde l'accent des temps forts
    // (en logarithme, un charleston pèserait autant qu'un kick).
    let mut env = Vec::with_capacity(n);
    let mut prev = 0.0f32;
    for k in 0..n {
        let e: f32 = (k * hop..(k + 1) * hop).map(|i| (data.left[i] + data.right[i]).powi(2)).sum();
        let a = (e / hop as f32).sqrt();
        env.push(if k == 0 { 0.0 } else { (a - prev).max(0.0) });
        prev = a;
    }
    // Lissage triangulaire : une période qui ne tombe pas sur un nombre entier de trames
    // corrèle quand même avec elle-même.
    let smooth: Vec<f32> = (0..n)
        .map(|i| {
            let at = |d: isize| env[(i as isize + d).clamp(0, n as isize - 1) as usize];
            (at(-2) + 2.0 * at(-1) + 3.0 * at(0) + 2.0 * at(1) + at(2)) / 9.0
        })
        .collect();
    let mut env = smooth;
    let mean = env.iter().sum::<f32>() / n as f32;
    env.iter_mut().for_each(|v| *v -= mean);
    let fps = rate / hop as f64;
    let ac = |lag: usize| -> f64 {
        if lag == 0 || lag >= n {
            return 0.0;
        }
        (0..n - lag).map(|i| (env[i] * env[i + lag]) as f64).sum::<f64>() / (n - lag) as f64
    };
    let (lo, hi) = ((fps * 60.0 / 180.0).floor() as usize, (fps * 60.0 / 70.0).ceil() as usize);
    let mut best = (0usize, f64::MIN);
    for lag in lo.max(2)..=hi.min(n / 3) {
        // Les multiples du temps renforcent le bon candidat (et départagent la noire de la croche) ;
        // une légère préférence va aux tempos courants, autour de 110.
        let bpm = 60.0 * fps / lag as f64;
        let prior = (-0.5 * ((bpm / 110.0).log2() / 0.8).powi(2)).exp();
        let s = (ac(lag) + 0.5 * ac(2 * lag) + 0.25 * ac(4 * lag)) * (0.6 + 0.4 * prior);
        if s > best.1 {
            best = (lag, s);
        }
    }
    if best.0 == 0 || best.1 <= 0.0 {
        return None;
    }
    // Affinage de proche en proche : le pic de l'autocorrélation vers k temps donne k fois plus
    // de précision ; chaque étape part de la précédente pour ne pas sauter sur un autre pic.
    let mut beat = best.0 as f64;
    for k in [2usize, 4, 8, 16, 32, 64, 128] {
        let center = (beat * k as f64).round() as usize;
        let win = (k / 4).max(2);
        if center + win + 1 >= n * 2 / 3 {
            break;
        }
        let peak = (center - win.min(center)..=center + win).max_by(|a, b| ac(*a).total_cmp(&ac(*b))).unwrap_or(center);
        let (y0, y1, y2) = (ac(peak.saturating_sub(1)), ac(peak), ac(peak + 1));
        let d = y0 - 2.0 * y1 + y2;
        let off = if d.abs() > 1e-12 { 0.5 * (y0 - y2) / d } else { 0.0 };
        beat = (peak as f64 + off.clamp(-0.5, 0.5)) / k as f64;
    }
    let mut bpm = 60.0 * fps / beat;
    // Une boucle tombe juste sur un nombre de mesures : on s'y cale.
    let secs = data.left.len() as f64 / rate;
    if secs < 45.0 {
        for bars in [1.0, 2.0, 4.0, 8.0, 16.0] {
            let exact = bars * 4.0 * 60.0 / secs;
            if (exact - bpm).abs() / bpm < 0.03 {
                bpm = exact;
                break;
            }
        }
    }
    if (bpm - bpm.round()).abs() < 0.12 {
        bpm = bpm.round();
    }
    Some((bpm * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une boucle de batterie synthétique : un coup grave par temps, un plus léger sur la croche.
    fn beat_loop(bpm: f64, beats: usize, rate: f64) -> SampleData {
        let frames = (beats as f64 * 60.0 / bpm * rate) as usize;
        let mut x = vec![0.0f32; frames];
        for b in 0..beats * 2 {
            let at = (b as f64 * 30.0 / bpm * rate) as usize;
            let amp = if b % 2 == 0 { 0.8 } else { 0.3 };
            for i in 0..2000.min(frames - at.min(frames)) {
                let t = i as f64 / rate;
                x[at + i] += (amp * (-t * 40.0).exp() * (std::f64::consts::TAU * 70.0 * t).sin()) as f32;
            }
        }
        SampleData { left: x.clone(), right: x }
    }

    #[test]
    fn finds_the_tempo_of_loops_and_longer_grooves() {
        let rate = 48000.0;
        assert_eq!(estimate_bpm(&beat_loop(124.0, 8, rate), rate), Some(124.0));
        assert_eq!(estimate_bpm(&beat_loop(97.0, 16, rate), rate), Some(97.0));
        let long = estimate_bpm(&beat_loop(132.5, 96, rate), rate).unwrap();
        assert!((long - 132.5).abs() < 0.3, "{long}");
        let slow = estimate_bpm(&beat_loop(85.0, 32, rate), rate).unwrap();
        assert!((slow - 85.0).abs() < 0.3, "{slow}");
    }
}
