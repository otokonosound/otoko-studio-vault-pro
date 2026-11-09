//! Pochettes, formes d'onde et versions allégées pour l'app mobile (comme `composer_vault/mobile/
//! media.py`). Tout est calculé à la demande puis gardé en cache disque ; les fichiers d'origine
//! ne sont jamais modifiés.

use std::collections::HashMap;
use std::io::Cursor;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::library::Sources;

const COVER_SIZES: [u32; 2] = [160, 600];
const NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL: u32 = 0x0000_4000;

/// Versions pour le téléphone : clé → (extension, type MIME, arguments ffmpeg).
pub fn quality(q: &str) -> Option<(&'static str, &'static str, &'static [&'static str])> {
    Some(match q {
        // FLAC sans perte : le lecteur d'Android lit mal les WAV 32 bits flottants.
        "lossless" => (".flac", "audio/flac", &["-c:a", "flac", "-compression_level", "5"][..]),
        "high" => (".ogg", "audio/ogg", &["-c:a", "libopus", "-b:a", "256k", "-vbr", "on"][..]),
        "eco" => (".ogg", "audio/ogg", &["-c:a", "libopus", "-b:a", "128k", "-vbr", "on"][..]),
        // Pour les navigateurs qui ne lisent pas l'Ogg Opus (Safari, iPhone).
        "high_aac" => (".m4a", "audio/mp4", &["-c:a", "aac", "-b:a", "256k", "-movflags", "+faststart"][..]),
        "eco_aac" => (".m4a", "audio/mp4", &["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"][..]),
        _ => return None,
    })
}

pub fn mime(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "aif" | "aiff" => "audio/aiff",
        "ogg" | "opus" => "audio/ogg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        _ => "application/octet-stream",
    }
}

pub fn path_mime(p: &Path) -> &'static str {
    mime(&p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default())
}

pub const LOSSLESS_SOURCES: [&str; 3] = ["wav", "aif", "aiff"];

pub fn is_lossless_source(p: &Path) -> bool {
    p.extension().is_some_and(|e| LOSSLESS_SOURCES.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

/// Le cache, à côté des données d'OTOKO.
pub struct Cache {
    pub root: PathBuf,
    ffmpeg: Option<PathBuf>,
    /// Les versions allégées déjà faites par le serveur de Studio Vault (lues, jamais modifiées).
    borrowed: Option<PathBuf>,
    locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
    cover_lock: Mutex<()>,
}

/// La signature d'un fichier (chemin, taille, date) : change si le fichier change. Calculée
/// comme Studio Vault, pour retrouver ses conversions sous le même nom.
fn sig(p: &Path) -> String {
    let shown = p.display().to_string().replace('/', "\\");
    let raw = match std::fs::metadata(p) {
        Ok(m) => format!("{shown}|{}|{}", m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)),
        Err(_) => shown,
    };
    otoko_library::hex(&otoko_library::sha1(raw.as_bytes())[..10])
}

/// ffmpeg : `OTOKO_FFMPEG`, à côté de l'exe, installé par le logiciel, dans le PATH, ou installé par winget.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OTOKO_FFMPEG").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    if let Some(p) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("ffmpeg.exe"))).filter(|p| p.exists()) {
        return Some(p);
    }
    let installed = crate::complements::ffmpeg_path();
    if installed.exists() {
        return Some(installed);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            let p = d.join("ffmpeg.exe");
            if p.exists() {
                return Some(p);
            }
        }
    }
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Microsoft").join("WinGet");
    let link = local.join("Links").join("ffmpeg.exe");
    if link.exists() {
        return Some(link);
    }
    for e in std::fs::read_dir(local.join("Packages")).ok()?.flatten() {
        if e.file_name().to_string_lossy().starts_with("Gyan.FFmpeg") {
            for sub in std::fs::read_dir(e.path()).ok()?.flatten() {
                let p = sub.path().join("bin").join("ffmpeg.exe");
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

impl Cache {
    pub fn new(root: PathBuf) -> Cache {
        Cache { root, ffmpeg: find_ffmpeg(), borrowed: None, locks: Mutex::new(HashMap::new()), cover_lock: Mutex::new(()) }
    }

    /// Les conversions d'un autre cache (celui de Studio Vault) servies telles quelles.
    pub fn borrowing(mut self, allege: Option<PathBuf>) -> Cache {
        self.borrowed = allege.filter(|d| d.is_dir());
        self
    }

    fn borrowed(&self, path: &Path, q: &str) -> Option<PathBuf> {
        let (ext, ..) = quality(q)?;
        self.borrowed.as_ref().map(|d| d.join(format!("{}_{q}{ext}", sig(path)))).filter(|p| p.is_file())
    }

    pub fn has_ffmpeg(&self) -> bool {
        self.ffmpeg.is_some()
    }

    fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    // ------------------------------------------------------------------ pochettes

    /// (JPEG carré de côté `size`, couleur marquante) depuis la première source qui a une image.
    pub fn cover_jpeg(&self, sources: &Sources, size: u32) -> Option<(Vec<u8>, Option<String>)> {
        let size = *COVER_SIZES.iter().min_by_key(|s| (**s as i64 - size as i64).abs()).unwrap_or(&600);
        for (kind, path) in sources {
            let key = format!("{kind}_{}_{size}", sig(path));
            let dir = self.dir("pochettes");
            let (jpg, none, col) = (dir.join(format!("{key}.jpg")), dir.join(format!("{key}.none")), dir.join(format!("{key}.color")));
            if none.exists() {
                continue;
            }
            if !jpg.exists() {
                let _g = self.cover_lock.lock();
                if !jpg.exists() && !make_cover(kind, path, size, &jpg, &col) {
                    let _ = std::fs::write(&none, b"");
                    continue;
                }
            }
            let color = std::fs::read_to_string(&col).ok().map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            return std::fs::read(&jpg).ok().map(|b| (b, color));
        }
        None
    }

    pub fn cover_color(&self, sources: &Sources) -> Option<String> {
        self.cover_jpeg(sources, 160).and_then(|r| r.1)
    }

    // ------------------------------------------------------------------ forme d'onde

    /// Crêtes normalisées (0 à 1), réduites à `points` barres, plus la durée.
    pub fn waveform(&self, path: &Path, points: usize) -> Option<serde_json::Value> {
        let file = self.dir("ondes").join(format!("{}.txt", sig(path)));
        let (dur, peaks) = match std::fs::read_to_string(&file).ok().and_then(|t| parse_peaks(&t)) {
            Some(x) => x,
            None => {
                let x = compute_peaks(path)?;
                let _ = std::fs::write(&file, format!("{}\n{}", x.0, x.1.iter().map(|p| format!("{p:.4}")).collect::<Vec<_>>().join(",")));
                x
            }
        };
        let n = peaks.len();
        let points = points.clamp(16, n.max(16));
        let out: Vec<f64> = (0..points)
            .map(|i| {
                let (a, b) = (i * n / points, ((i + 1) * n / points).max(i * n / points + 1).min(n));
                let m = peaks[a.min(n.saturating_sub(1))..b].iter().fold(0.0f32, |m, v| m.max(*v));
                ((m as f64) * 1000.0).round() / 1000.0
            })
            .collect();
        Some(serde_json::json!({ "duration": (dur * 100.0).round() / 100.0, "peaks": out }))
    }

    // ------------------------------------------------------------------ versions allégées

    fn out_path(&self, path: &Path, q: &str) -> Option<PathBuf> {
        let (ext, ..) = quality(q)?;
        Some(self.dir("allege").join(format!("{}_{q}{ext}", sig(path))))
    }

    /// Version déjà convertie (sans lancer de conversion).
    pub fn cached(&self, path: &Path, q: &str) -> Option<PathBuf> {
        self.out_path(path, q).filter(|p| p.exists()).or_else(|| self.borrowed(path, q))
    }

    /// Version allégée du fichier (créée au premier appel, puis servie depuis le cache).
    pub fn transcoded(&self, path: &Path, q: &str) -> Option<PathBuf> {
        let (ext, _, args) = quality(q)?;
        let out = self.out_path(path, q)?;
        if out.exists() {
            let _ = std::fs::File::options().append(true).open(&out).and_then(|f| f.set_modified(std::time::SystemTime::now()));
            return Some(out);
        }
        if let Some(b) = self.borrowed(path, q) {
            return Some(b);
        }
        let exe = self.ffmpeg.as_ref()?;
        if !path.exists() {
            return None;
        }
        let lock = self.locks.lock().ok()?.entry(out.clone()).or_default().clone();
        let _g = lock.lock().ok()?;
        if out.exists() {
            return Some(out);
        }
        let tmp = out.with_extension(format!("part{ext}"));
        let mut cmd = std::process::Command::new(exe);
        cmd.args(["-v", "error", "-y", "-i"]).arg(path).args(["-vn", "-map_metadata", "0"]).args(args);
        if q == "lossless" {
            cmd.args(if bits16(path) { &["-sample_fmt", "s16"][..] } else { &["-sample_fmt", "s32", "-bits_per_raw_sample", "24"][..] });
        }
        let ok = cmd.arg(&tmp).creation_flags(NO_WINDOW | BELOW_NORMAL).output().map(|o| o.status.success()).unwrap_or(false);
        if !ok || !tmp.exists() {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        std::fs::rename(&tmp, &out).ok()?;
        self.trim();
        Some(out)
    }

    /// Copie temporaire du fichier avec titre, artiste, album, année et, si possible, la pochette
    /// (sans réencoder l'audio). À supprimer après l'envoi.
    pub fn tagged_copy(&self, path: &Path, tags: &[(&str, String)], cover: Option<&[u8]>) -> Option<PathBuf> {
        let exe = self.ffmpeg.as_ref()?;
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        let dir = self.dir("tags").join(format!("t{}", rand_hex(6)));
        std::fs::create_dir_all(&dir).ok()?;
        let out = dir.join(format!("morceau.{ext}"));
        let mut cmd = std::process::Command::new(exe);
        cmd.args(["-v", "error", "-y", "-i"]).arg(path);
        let with_cover = cover.is_some() && ["flac", "mp3", "m4a"].contains(&ext.as_str());
        if with_cover {
            let jpg = dir.join("pochette.jpg");
            std::fs::write(&jpg, cover?).ok()?;
            cmd.arg("-i").arg(&jpg).args(["-map", "0:a", "-map", "1:0", "-c:v", "copy", "-disposition:v:0", "attached_pic", "-metadata:s:v", "title=Album cover", "-metadata:s:v", "comment=Cover (front)"]);
        } else {
            cmd.args(["-map", "0:a"]);
        }
        cmd.args(["-c:a", "copy", "-map_metadata", "0"]);
        for (k, v) in tags {
            if !v.is_empty() {
                cmd.arg("-metadata").arg(format!("{k}={v}"));
            }
        }
        if ext == "mp3" {
            cmd.args(["-id3v2_version", "3"]);
        }
        let ok = cmd.arg(&out).creation_flags(NO_WINDOW | BELOW_NORMAL).output().map(|o| o.status.success()).unwrap_or(false);
        let _ = std::fs::remove_file(dir.join("pochette.jpg"));
        if !ok || std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0) == 0 {
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        let fin = self.dir("tags").join(format!("{}.{ext}", dir.file_name()?.to_string_lossy()));
        std::fs::rename(&out, &fin).ok()?;
        let _ = std::fs::remove_dir_all(&dir);
        Some(fin)
    }

    /// 60 Go au plus de versions allégées : les plus anciennement servies partent d'abord.
    fn trim(&self) {
        const MAX: u64 = 60 * 1024 * 1024 * 1024;
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = std::fs::read_dir(self.dir("allege"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                let ext = e.path().extension()?.to_string_lossy().to_lowercase();
                ["ogg", "flac", "m4a"].contains(&ext.as_str()).then(|| (m.modified().ok().unwrap_or(std::time::UNIX_EPOCH), m.len(), e.path()))
            })
            .collect();
        files.sort();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        for (_, len, p) in files {
            if total <= MAX {
                break;
            }
            let _ = std::fs::remove_file(&p);
            total = total.saturating_sub(len);
        }
    }
}

pub fn rand_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Nom de fichier lisible partout (Android, Windows).
pub fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 32 { '_' } else { c }).collect();
    let s = s.trim_matches(|c| c == ' ' || c == '.').chars().take(120).collect::<String>();
    if s.is_empty() { "Morceau".into() } else { s }
}

/// Un fichier 16 bits reste en 16 bits ; tout le reste passe en 24 bits (le maximum du FLAC).
fn bits16(path: &Path) -> bool {
    use symphonia::core::{formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint};
    let Ok(f) = std::fs::File::open(path) else { return false };
    let mss = MediaSourceStream::new(Box::new(f), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = path.extension() {
        hint.with_extension(&e.to_string_lossy());
    }
    symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .ok()
        .and_then(|p| p.format.default_track().and_then(|t| t.codec_params.bits_per_sample))
        .is_some_and(|b| b <= 16)
}

/// L'image (pochette) d'un fichier ou celle intégrée à un son, recadrée, réduite, en JPEG, et sa
/// couleur marquante écrite à côté.
fn make_cover(kind: &str, path: &Path, size: u32, out: &Path, color_out: &Path) -> bool {
    let data = if kind == "img" || kind == "rel" {
        match std::fs::read(path) {
            Ok(d) => d,
            Err(_) => return false,
        }
    } else {
        match embedded_picture(path) {
            Some(d) => d,
            None => return false,
        }
    };
    let Ok(img) = image::load_from_memory(&data) else { return false };
    let side = img.width().min(img.height());
    let img = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side).resize_exact(size, size, image::imageops::FilterType::Lanczos3);
    let _ = std::fs::write(color_out, dominant_color(&img).unwrap_or_default());
    let mut buf = Cursor::new(Vec::new());
    let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
    if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90).encode_image(&rgb).is_err() {
        return false;
    }
    std::fs::write(out, buf.into_inner()).is_ok()
}

/// L'image intégrée à un fichier audio (la pochette de face d'abord).
pub fn embedded_picture(path: &Path) -> Option<Vec<u8>> {
    use lofty::file::TaggedFileExt;
    use lofty::picture::PictureType;
    let tagged = lofty::read_from_path(path).ok()?;
    let pics: Vec<&lofty::picture::Picture> = tagged.tags().iter().flat_map(|t| t.pictures()).collect();
    pics.iter().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| pics.first()).map(|p| p.data().to_vec())
}

/// Couleur marquante d'une pochette : moyenne pondérée par la saturation (couleurs vives, ni
/// noires ni blanches). Rien pour une pochette en noir et blanc.
pub fn dominant_color(img: &image::DynamicImage) -> Option<String> {
    let small = img.resize_exact(24, 24, image::imageops::FilterType::Triangle).to_rgb8();
    let (mut tot, mut r, mut g, mut b) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for p in small.pixels() {
        let (pr, pg, pb) = (p[0] as f64 / 255.0, p[1] as f64 / 255.0, p[2] as f64 / 255.0);
        let (mx, mn) = (pr.max(pg).max(pb), pr.min(pg).min(pb));
        let l = (mx + mn) / 2.0;
        let s = if mx == mn { 0.0 } else if l > 0.5 { (mx - mn) / (2.0 - mx - mn) } else { (mx - mn) / (mx + mn) };
        let w = s * (1.0 - (l - 0.5).abs() * 2.0) + 0.02;
        tot += w;
        r += p[0] as f64 * w;
        g += p[1] as f64 * w;
        b += p[2] as f64 * w;
    }
    if tot <= 0.0 {
        return None;
    }
    let (r, g, b) = ((r / tot) as u8, (g / tot) as u8, (b / tot) as u8);
    let (fr, fg, fb) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let (mx, mn) = (fr.max(fg).max(fb), fr.min(fg).min(fb));
    let l = (mx + mn) / 2.0;
    let s = if mx == mn { 0.0 } else if l > 0.5 { (mx - mn) / (2.0 - mx - mn) } else { (mx - mn) / (mx + mn) };
    (s >= 0.15).then(|| format!("#{r:02x}{g:02x}{b:02x}"))
}

/// (durée, crêtes de 1 000 tranches) du fichier.
fn compute_peaks(path: &Path) -> Option<(f64, Vec<f32>)> {
    use symphonia::core::{audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint};
    let f = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(f), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = path.extension() {
        hint.with_extension(&e.to_string_lossy());
    }
    let mut format = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).ok()?.format;
    let track = format.default_track()?;
    let (id, rate) = (track.id, track.codec_params.sample_rate? as f64);
    let mut dec = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).ok()?;
    let mut abs: Vec<f32> = Vec::new();
    // Une crête par bloc de 512 images, puis réduction à 1 000 tranches.
    let (mut cur, mut count) = (0.0f32, 0usize);
    let mut frames = 0usize;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != id {
            continue;
        }
        let Ok(decoded) = dec.decode(&packet) else { continue };
        let spec = *decoded.spec();
        let ch = spec.channels.count().max(1);
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        for fr in buf.samples().chunks(ch) {
            let m = fr.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            cur = cur.max(m);
            count += 1;
            if count == 512 {
                abs.push(cur);
                cur = 0.0;
                count = 0;
            }
        }
        frames += buf.samples().len() / ch;
    }
    if count > 0 {
        abs.push(cur);
    }
    if abs.is_empty() {
        return None;
    }
    let n = 1000.min(abs.len());
    let peaks = (0..n).map(|i| abs[i * abs.len() / n..((i + 1) * abs.len() / n).max(i * abs.len() / n + 1)].iter().fold(0.0f32, |m, v| m.max(*v))).collect();
    Some((frames as f64 / rate, peaks))
}

fn parse_peaks(t: &str) -> Option<(f64, Vec<f32>)> {
    let (d, p) = t.split_once('\n')?;
    Some((d.trim().parse().ok()?, p.split(',').filter_map(|x| x.parse().ok()).collect()))
}

/// Les fils d'arrière-plan qui préparent pochettes et conversions, une seule fois chacun.
pub fn once(name: &'static str) -> bool {
    static DONE: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let m = DONE.get_or_init(|| Mutex::new(Vec::new()));
    let mut v = m.lock().unwrap_or_else(|e| e.into_inner());
    if v.contains(&name) {
        return false;
    }
    v.push(name);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_names_and_qualities() {
        let red = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(40, 40, image::Rgb([220, 40, 30])));
        assert_eq!(dominant_color(&red).as_deref(), Some("#dc281e"));
        let grey = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(40, 40, image::Rgb([120, 120, 120])));
        assert_eq!(dominant_color(&grey), None, "noir et blanc : pas d'accent");
        assert_eq!(safe_name("Otoko - A/B: test?"), "Otoko - A_B_ test_");
        assert_eq!(quality("eco").map(|q| q.0), Some(".ogg"));
        assert!(quality("bidon").is_none());
    }

    #[test]
    fn sha1_like_python() {
        use otoko_library::{hex, sha1};
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&sha1(&[b'a'; 1000])), "291e9a6c66994949b57ba5e650361e98fc36b1ba");
    }
}
