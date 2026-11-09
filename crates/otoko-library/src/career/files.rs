//! Ce qui touche aux fichiers des morceaux, toujours à la demande de l'utilisateur (comme Studio
//! Vault) : renommer en lot et réorganiser en dossiers (annulables, journal `file_ops`), repérer
//! les doublons par le contenu, écrire les tags dans les fichiers, proposer genres et artistes,
//! rattacher une version ou un stem à son morceau principal.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};

use super::scan::normalize;
use super::status_label;

// ---------------------------------------------------------------------- renommer, réorganiser

fn sanitize(s: &str) -> String {
    let out: String = s.chars().map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    out.trim().trim_end_matches('.').to_string()
}

/// Les champs d'un morceau utiles aux modèles de noms.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileRow {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub year: Option<i64>,
    pub track_no: Option<i64>,
    pub bpm: Option<f64>,
    pub key: String,
    pub status: String,
    pub role: String,
    pub version: String,
}

pub fn file_rows(conn: &Connection, ids: &[i64]) -> Vec<FileRow> {
    let mut out = Vec::new();
    for id in ids {
        let r = conn.query_row(
            "SELECT id, path, IFNULL(title, ''), IFNULL(artist, ''), IFNULL(album, ''), IFNULL(genre, ''), year, track_no, bpm, IFNULL(music_key, ''),
                    IFNULL(status, ''), IFNULL(role, ''), IFNULL(version_label, '') FROM tracks WHERE id = ?1",
            [id],
            |r| {
                Ok(FileRow {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    title: r.get(2)?,
                    artist: r.get(3)?,
                    album: r.get(4)?,
                    genre: r.get(5)?,
                    year: r.get(6)?,
                    track_no: r.get(7)?,
                    bpm: r.get(8)?,
                    key: r.get(9)?,
                    status: r.get(10)?,
                    role: r.get(11)?,
                    version: r.get(12)?,
                })
            },
        );
        if let Ok(r) = r {
            out.push(r);
        }
    }
    out
}

/// Un déplacement prévu : « ok », « unchanged », « missing », « invalid », « outside_root ».
#[derive(Clone, Debug, PartialEq)]
pub struct Move {
    pub id: i64,
    pub src: PathBuf,
    pub dst: PathBuf,
    pub reason: &'static str,
}

fn free_name(dst: PathBuf, seen: &HashSet<String>) -> PathBuf {
    if !seen.contains(&dst.to_string_lossy().to_lowercase()) && !dst.exists() {
        return dst;
    }
    let stem = dst.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = dst.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let parent = dst.parent().map(Path::to_path_buf).unwrap_or_default();
    (2..)
        .map(|i| parent.join(format!("{stem} ({i}){ext}")))
        .find(|c| !seen.contains(&c.to_string_lossy().to_lowercase()) && !c.exists())
        .unwrap_or(dst)
}

/// Placeholders : {title} {artist} {album} {genre} {year} {track_no} {bpm} {key} {status} {ext} {orig}
pub fn rename_preview(rows: &[FileRow], pattern: &str) -> Vec<Move> {
    let mut seen = HashSet::new();
    rows.iter()
        .map(|f| {
            let src = PathBuf::from(&f.path);
            if !src.exists() {
                return Move { id: f.id, dst: src.clone(), src, reason: "missing" };
            }
            let stem0 = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let ext = src.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
            let ctx = [
                ("title", if f.title.is_empty() { stem0.clone() } else { f.title.clone() }),
                ("artist", f.artist.clone()),
                ("album", f.album.clone()),
                ("genre", f.genre.clone()),
                ("year", f.year.map(|y| y.to_string()).unwrap_or_default()),
                ("track_no", f.track_no.map(|n| n.to_string()).unwrap_or_default()),
                ("bpm", f.bpm.map(|b| format!("{}", b.round() as i64)).unwrap_or_default()),
                ("key", f.key.clone()),
                ("status", f.status.clone()),
                ("ext", ext.clone()),
                ("orig", stem0.clone()),
            ];
            let mut stem = pattern.to_string();
            for (k, v) in &ctx {
                stem = stem.replace(&format!("{{{k}}}"), &sanitize(v));
            }
            let stem = stem.trim_end().to_string();
            if stem.is_empty() {
                return Move { id: f.id, dst: src.clone(), src, reason: "invalid" };
            }
            let name = if stem.to_lowercase().contains(&format!(".{}", ext.to_lowercase())) { stem } else { format!("{stem}.{ext}") };
            let dst = src.with_file_name(name);
            if dst == src {
                return Move { id: f.id, dst, src, reason: "unchanged" };
            }
            let dst = free_name(dst, &seen);
            seen.insert(dst.to_string_lossy().to_lowercase());
            Move { id: f.id, src, dst, reason: "ok" }
        })
        .collect()
}

/// Placeholders, avec « / » pour les sous-dossiers : {title} {artist} {album} {genre} {year}
/// {track_no} (sur 2 chiffres) {bpm} {key} {status} {status_fr} {role} {version} {format} {ext} {orig}
pub fn reorganize_preview(rows: &[FileRow], root: &Path, pattern: &str) -> Vec<Move> {
    let root = normalize(root);
    let mut seen = HashSet::new();
    rows.iter()
        .map(|f| {
            let src = PathBuf::from(&f.path);
            if !src.exists() {
                return Move { id: f.id, dst: src.clone(), src, reason: "missing" };
            }
            let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
            let orig = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let ctx = [
                ("title", if f.title.is_empty() { orig.clone() } else { f.title.clone() }),
                ("artist", if f.artist.is_empty() { "Inconnu".into() } else { f.artist.clone() }),
                ("album", if f.album.is_empty() { "Sans album".into() } else { f.album.clone() }),
                ("genre", if f.genre.is_empty() { "Sans genre".into() } else { f.genre.clone() }),
                ("year", f.year.map(|y| y.to_string()).unwrap_or_else(|| "Sans année".into())),
                ("track_no", f.track_no.map(|n| format!("{n:02}")).unwrap_or_default()),
                ("bpm", f.bpm.map(|b| format!("{}", b.round() as i64)).unwrap_or_else(|| "SansBPM".into())),
                ("key", if f.key.is_empty() { "SansKey".into() } else { f.key.clone() }),
                ("status", if f.status.is_empty() { "unknown".into() } else { f.status.clone() }),
                ("status_fr", if f.status.is_empty() { "Inconnu".into() } else { status_label(&f.status).to_string() }),
                ("role", if f.role.is_empty() { "mixdown".into() } else { f.role.clone() }),
                ("version", f.version.clone()),
                ("format", ext.trim_start_matches('.').to_uppercase()),
                ("ext", ext.trim_start_matches('.').to_string()),
                ("orig", orig.clone()),
            ];
            let parts: Vec<String> = pattern
                .split('/')
                .map(|part| {
                    let mut s = part.to_string();
                    for (k, v) in &ctx {
                        s = s.replace(&format!("{{{k}}}"), &sanitize(v));
                    }
                    s.trim().trim_matches('.').to_string()
                })
                .filter(|s| !s.is_empty())
                .collect();
            if parts.is_empty() {
                return Move { id: f.id, dst: src.clone(), src, reason: "invalid" };
            }
            let mut rel = parts.join("\\");
            if !rel.to_lowercase().ends_with(&ext) {
                rel.push_str(&ext);
            }
            if rel.split('\\').any(|p| p == "..") {
                return Move { id: f.id, dst: src.clone(), src, reason: "outside_root" };
            }
            let dst = root.join(rel);
            if normalize(&dst).to_string_lossy().to_lowercase() == normalize(&src).to_string_lossy().to_lowercase() {
                return Move { id: f.id, dst, src, reason: "unchanged" };
            }
            let dst = free_name(dst, &seen);
            seen.insert(dst.to_string_lossy().to_lowercase());
            Move { id: f.id, src, dst, reason: "ok" }
        })
        .collect()
}

fn batch_id() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    crate::hex(&crate::sha1(format!("{t}-{}", std::process::id()).as_bytes())[..6])
}

/// Applique les déplacements « ok » (ou les copies), met la Carrière à jour et note le lot pour
/// l'annuler. Rend (réussis, échecs).
pub fn apply(conn: &Connection, moves: &[Move], label: &str, copy: bool) -> (usize, usize) {
    let batch = batch_id();
    let (mut ok, mut err) = (0, 0);
    for m in moves.iter().filter(|m| m.reason == "ok") {
        let done = (|| -> std::io::Result<()> {
            if let Some(d) = m.dst.parent() {
                std::fs::create_dir_all(d)?;
            }
            if copy {
                std::fs::copy(&m.src, &m.dst)?;
            } else if std::fs::rename(&m.src, &m.dst).is_err() {
                // Autre disque : copie puis suppression de l'original (déplacement).
                std::fs::copy(&m.src, &m.dst)?;
                std::fs::remove_file(&m.src)?;
            }
            Ok(())
        })();
        match done {
            Ok(()) => {
                ok += 1;
                if !copy {
                    let dst = m.dst.to_string_lossy().into_owned();
                    let name = m.dst.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let _ = conn.execute("UPDATE tracks SET path = ?1, filename = ?2, modified_at = datetime('now') WHERE id = ?3", params![dst, name, m.id]);
                    let _ = conn.execute(
                        "INSERT INTO file_ops (batch_id, label, track_id, src, dst) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![batch, label, m.id, m.src.to_string_lossy(), dst],
                    );
                }
            }
            Err(_) => err += 1,
        }
    }
    (ok, err)
}

/// Le dernier lot non annulé : (lot, libellé, nombre de fichiers).
pub fn last_batch(conn: &Connection) -> Option<(String, String, i64)> {
    conn.query_row("SELECT batch_id, label, COUNT(*) FROM file_ops WHERE undone = 0 GROUP BY batch_id ORDER BY MAX(id) DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()
        .ok()
        .flatten()
}

/// Remet chaque fichier du lot à sa place. Rend (réussis, échecs).
pub fn undo(conn: &Connection, batch: &str) -> (usize, usize) {
    let rows: Vec<(i64, String, String)> = conn
        .prepare("SELECT id, src, dst FROM file_ops WHERE batch_id = ?1 AND undone = 0 ORDER BY id DESC")
        .and_then(|mut s| s.query_map([batch], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
        .unwrap_or_default();
    let (mut ok, mut err) = (0, 0);
    for (id, src, dst) in rows {
        let (s, d) = (Path::new(&src), Path::new(&dst));
        let r = (|| -> std::io::Result<()> {
            if !d.exists() {
                return Err(std::io::Error::other("fichier déplacé introuvable"));
            }
            if s.exists() {
                return Err(std::io::Error::other("l'ancien emplacement est occupé"));
            }
            if let Some(p) = s.parent() {
                std::fs::create_dir_all(p)?;
            }
            if std::fs::rename(d, s).is_err() {
                std::fs::copy(d, s)?;
                std::fs::remove_file(d)?;
            }
            Ok(())
        })();
        match r {
            Ok(()) => {
                let name = s.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                let _ = conn.execute("UPDATE tracks SET path = ?1, filename = ?2, modified_at = datetime('now') WHERE path = ?3", params![src, name, dst]);
                let _ = conn.execute("UPDATE file_ops SET undone = 1 WHERE id = ?1", [id]);
                ok += 1;
            }
            Err(_) => err += 1,
        }
    }
    (ok, err)
}

/// Retire les dossiers d'origine devenus vides (en remontant).
pub fn clean_empty_dirs(paths: &[PathBuf]) -> usize {
    let mut parents: Vec<PathBuf> = paths.iter().filter_map(|p| p.parent().map(Path::to_path_buf)).collect();
    parents.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    parents.dedup();
    let mut removed = 0;
    for p in parents {
        let mut cur = Some(p);
        while let Some(d) = cur {
            let empty = std::fs::read_dir(&d).map(|mut r| r.next().is_none()).unwrap_or(false);
            if empty && std::fs::remove_dir(&d).is_ok() {
                removed += 1;
                cur = d.parent().map(Path::to_path_buf);
            } else {
                break;
            }
        }
    }
    removed
}

// ---------------------------------------------------------------------- doublons

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DupRow {
    pub id: i64,
    pub path: String,
    pub filename: String,
    pub size: i64,
    pub format: String,
    pub duration: Option<f64>,
    pub samplerate: i64,
    pub pick: i64,
}

/// Groupes de doublons : copie exacte du fichier (même empreinte de contenu, même renommée), ou
/// même audio d'après l'analyse (une copie convertie en MP3, par exemple).
pub fn dupe_groups(conn: &Connection) -> Vec<Vec<DupRow>> {
    let rows: Vec<(DupRow, String, String)> = conn
        .prepare(
            "SELECT id, path, filename, IFNULL(file_size, 0), IFNULL(file_format, ''), duration_s, IFNULL(samplerate, 0), IFNULL(pick, 0),
                    IFNULL(fingerprint, ''), IFNULL(audio_fp, '')
             FROM tracks WHERE missing = 0",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok((
                    DupRow { id: r.get(0)?, path: r.get(1)?, filename: r.get(2)?, size: r.get(3)?, format: r.get(4)?, duration: r.get(5)?, samplerate: r.get(6)?, pick: r.get(7)? },
                    r.get(8)?,
                    r.get(9)?,
                ))
            })?
            .collect()
        })
        .unwrap_or_default();
    // Union-find sur les indices.
    let mut parent: Vec<usize> = (0..rows.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let union = |p: &mut Vec<usize>, a: usize, b: usize| {
        let (ra, rb) = (find(p, a), find(p, b));
        if ra != rb {
            p[rb] = ra;
        }
    };
    let mut by_fp: HashMap<&str, usize> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        if !r.1.is_empty() {
            match by_fp.get(r.1.as_str()) {
                Some(&j) => union(&mut parent, j, i),
                None => {
                    by_fp.insert(r.1.as_str(), i);
                }
            }
        }
    }
    let mut audio: Vec<(i64, usize)> = rows.iter().enumerate().filter(|(_, r)| !r.2.is_empty()).filter_map(|(i, r)| Some((r.2.split(':').next()?.parse().ok()?, i))).collect();
    audio.sort();
    for (k, &(da, a)) in audio.iter().enumerate() {
        for &(db, b) in &audio[k + 1..] {
            if db - da > 1 {
                break;
            }
            if super::library::fingerprint_distance(&rows[a].2, &rows[b].2) <= 3 {
                union(&mut parent, a, b);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<DupRow>> = BTreeMap::new();
    for i in 0..rows.len() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(rows[i].0.clone());
    }
    groups.into_values().filter(|g| g.len() > 1).collect()
}

/// Le fichier conseillé à garder : gardé ✓, puis sans perte, puis meilleure qualité, puis le plus ancien.
pub fn best_of(group: &[DupRow]) -> i64 {
    group
        .iter()
        .max_by_key(|r| (r.pick == 1, ["wav", "flac", "aiff", "aif"].contains(&r.format.to_lowercase().as_str()), r.samplerate, r.size, -r.id))
        .map(|r| r.id)
        .unwrap_or(0)
}

/// Retire des morceaux de la bibliothèque (les fichiers ne sont pas touchés).
pub fn forget(conn: &Connection, ids: &[i64]) -> usize {
    ids.iter().map(|id| conn.execute("DELETE FROM tracks WHERE id = ?1", [id]).unwrap_or(0)).sum()
}

// ---------------------------------------------------------------------- tags dans les fichiers

/// Ce qu'on écrit : `None` ne touche pas au champ, `Some("")` l'efface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagValues {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<String>,
    pub track_no: Option<String>,
    pub bpm: Option<String>,
    pub key: Option<String>,
    pub comment: Option<String>,
}

fn set_tag(tag: &mut lofty::tag::Tag, v: &TagValues) {
    use lofty::tag::{Accessor, ItemKey};
    let text = |tag: &mut lofty::tag::Tag, key: ItemKey, val: &Option<String>| {
        if let Some(x) = val {
            tag.remove_key(&key);
            if !x.trim().is_empty() {
                tag.insert_text(key, x.trim().to_string());
            }
        }
    };
    if let Some(x) = &v.title {
        if x.trim().is_empty() { tag.remove_title() } else { tag.set_title(x.trim().to_string()) }
    }
    if let Some(x) = &v.artist {
        if x.trim().is_empty() { tag.remove_artist() } else { tag.set_artist(x.trim().to_string()) }
    }
    if let Some(x) = &v.album {
        if x.trim().is_empty() { tag.remove_album() } else { tag.set_album(x.trim().to_string()) }
    }
    if let Some(x) = &v.genre {
        if x.trim().is_empty() { tag.remove_genre() } else { tag.set_genre(x.trim().to_string()) }
    }
    if let Some(x) = &v.comment {
        if x.trim().is_empty() { tag.remove_comment() } else { tag.set_comment(x.trim().to_string()) }
    }
    if let Some(x) = &v.year {
        match x.trim().parse::<u32>() {
            Ok(y) => tag.set_year(y),
            Err(_) => tag.remove_year(),
        }
    }
    if let Some(x) = &v.track_no {
        match x.trim().split('/').next().unwrap_or("").parse::<u32>() {
            Ok(n) => tag.set_track(n),
            Err(_) => tag.remove_track(),
        }
    }
    let bpm = v.bpm.as_ref().map(|b| b.trim().replace(',', ".").parse::<f64>().map(|f| format!("{}", f.round() as i64)).unwrap_or_default());
    // ID3 et MP4 gardent un tempo entier (TBPM, tmpo), Vorbis un tempo décimal (BPM).
    text(tag, ItemKey::IntegerBpm, &bpm);
    text(tag, ItemKey::Bpm, &bpm);
    text(tag, ItemKey::InitialKey, &v.key);
}

/// Écrit les champs dans le fichier (ID3 pour MP3, WAV et AIFF — plus le bloc INFO de Windows
/// pour un WAV —, commentaires Vorbis pour FLAC et OGG, MP4 pour M4A), puis relit pour vérifier.
pub fn write_tags(path: &Path, v: &TagValues) -> Result<(), String> {
    use lofty::config::WriteOptions;
    use lofty::file::{AudioFile, FileType, TaggedFileExt};
    use lofty::tag::{Tag, TagType};
    let mut f = lofty::read_from_path(path).map_err(|e| format!("fichier illisible : {e}"))?;
    let primary = f.primary_tag_type();
    if f.tag(primary).is_none() {
        f.insert_tag(Tag::new(primary));
    }
    if let Some(t) = f.tag_mut(primary) {
        set_tag(t, v);
    }
    if f.file_type() == FileType::Wav && f.supports_tag_type(TagType::RiffInfo) {
        if f.tag(TagType::RiffInfo).is_none() {
            f.insert_tag(Tag::new(TagType::RiffInfo));
        }
        if let Some(t) = f.tag_mut(TagType::RiffInfo) {
            // Le bloc INFO n'a ni tempo ni tonalité.
            set_tag(t, &TagValues { bpm: None, key: None, ..v.clone() });
        }
    }
    f.save_to_path(path, WriteOptions::default()).map_err(|e| match e.kind() {
        lofty::error::ErrorKind::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied => "fichier ouvert ailleurs ou en lecture seule".to_string(),
        _ => format!("écriture impossible : {e}"),
    })?;
    // Relecture : les valeurs y sont vraiment.
    let back = super::scan::read_tags(path);
    for (want, got) in [(&v.title, &back.title), (&v.album, &back.album), (&v.genre, &back.genre)] {
        if let Some(w) = want.as_ref().filter(|w| !w.trim().is_empty()) {
            if got.as_deref() != Some(w.trim()) {
                return Err(format!("vérification : « {} » attendu, « {} » lu", w.trim(), got.clone().unwrap_or_default()));
            }
        }
    }
    Ok(())
}

/// Les fichiers qui peuvent recevoir une pochette.
pub const COVER_EXTS: [&str; 12] = ["mp3", "wav", "aif", "aiff", "flac", "ogg", "oga", "opus", "m4a", "mp4", "aac", "alac"];

/// Remplace la pochette du fichier (« couverture avant » : image ID3 pour MP3, WAV et AIFF, bloc
/// PICTURE pour FLAC, METADATA_BLOCK_PICTURE pour OGG, atome covr pour M4A), puis la relit.
pub fn embed_cover(path: &Path, jpeg: &[u8]) -> Result<(), String> {
    use lofty::config::WriteOptions;
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::tag::Tag;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !COVER_EXTS.contains(&ext.as_str()) {
        return Err(format!("format {ext} non pris en charge"));
    }
    let mut f = lofty::read_from_path(path).map_err(|e| format!("fichier illisible : {e}"))?;
    let primary = f.primary_tag_type();
    if f.tag(primary).is_none() {
        f.insert_tag(Tag::new(primary));
    }
    if let Some(t) = f.tag_mut(primary) {
        t.remove_picture_type(PictureType::CoverFront);
        t.push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(MimeType::Jpeg), Some("Cover".into()), jpeg.to_vec()));
    }
    f.save_to_path(path, WriteOptions::default()).map_err(|e| match e.kind() {
        lofty::error::ErrorKind::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied => "fichier ouvert ailleurs ou en lecture seule".to_string(),
        _ => format!("écriture impossible : {e}"),
    })?;
    match read_cover(path) {
        Some(back) if back == jpeg => Ok(()),
        _ => Err("vérification : la pochette relue n'est pas celle écrite".into()),
    }
}

/// La pochette intégrée au fichier (« couverture avant », sinon la première image).
pub fn read_cover(path: &Path) -> Option<Vec<u8>> {
    use lofty::file::TaggedFileExt;
    use lofty::picture::PictureType;
    let f = lofty::read_from_path(path).ok()?;
    let tag = f.primary_tag().or_else(|| f.first_tag())?;
    let pic = tag.pictures().iter().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| tag.pictures().first())?;
    Some(pic.data().to_vec())
}

// ---------------------------------------------------------------------- genres et artistes

const GENRE_WORDS: [&str; 48] = [
    "afro house", "afro bass", "tribal house", "tribal tech", "tribal trance", "bass house", "tech house", "deep house", "french house", "disco house",
    "melodic house", "progressive house", "fast house", "melodic techno", "deep techno", "hard techno", "minimal techno", "progressive trance", "psytrance",
    "future bass", "drum & bass", "drum and bass", "uk garage", "speed garage", "2-step", "breakbeat", "tech groove", "folktronica", "downtempo", "ambient",
    "synthwave", "dark-synth", "darksynth", "dubstep", "trap", "jungle", "rave", "house", "techno", "trance", "minimal", "organic", "disco", "garage",
    "electro", "hardstyle", "lo-fi", "lofi",
];

fn display(g: &str) -> String {
    match g {
        "drum and bass" | "drum & bass" => "Drum & Bass".into(),
        "uk garage" => "UK Garage".into(),
        "lo-fi" | "lofi" => "Lo-fi".into(),
        "2-step" => "2-Step".into(),
        "dark-synth" | "darksynth" => "Dark-synth".into(),
        "psytrance" => "Psytrance".into(),
        _ => g
            .split(' ')
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Premier style reconnu dans un texte (le plus précis d'abord).
pub fn genre_from_text(text: &str) -> String {
    let low = format!(" {} ", text.to_lowercase());
    for g in GENRE_WORDS {
        let mut start = 0;
        while let Some(i) = low[start..].find(g) {
            let a = start + i;
            let b = a + g.len();
            let before = low[..a].chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_');
            let after = low[b..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_');
            if !before && !after {
                return display(g);
            }
            start = a + 1;
        }
    }
    String::new()
}

/// Style entre parenthèses : « (Tribal House Remix) ».
pub fn genre_from_title(title: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)\(([^)]*?)\b(remix|edit|mix|version|rework|bootleg)\)").expect("expression"));
    re.captures_iter(title).map(|c| genre_from_text(&c[1])).find(|g| !g.is_empty()).unwrap_or_default()
}

pub fn genre_from_folder(path: &str) -> String {
    static NOT: OnceLock<Regex> = OnceLock::new();
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    let name = Path::new(&path.replace('/', "\\")).parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if NOT.get_or_init(|| Regex::new(r"(?i)^\d\d_|demos?|unreleased|albums?|singles?|eps?$|collab|remix|mashup").expect("expression")).is_match(&name) {
        return String::new();
    }
    let first = SPLIT.get_or_init(|| Regex::new(r"\s+-\s+|,|/").expect("expression")).split(&name).next().unwrap_or("").to_string();
    genre_from_text(&first)
}

pub fn genre_from_tempo(bpm: Option<f64>) -> String {
    let Some(b) = bpm else { return String::new() };
    match b {
        b if b < 100.0 => "Downtempo",
        b if b < 118.0 => {
            if b >= 110.0 {
                "Deep House"
            } else {
                "Downtempo"
            }
        }
        b if b <= 128.5 => "House",
        b if b <= 136.0 => "Techno",
        b if b <= 150.0 => {
            if b >= 138.0 {
                "Trance"
            } else {
                "Techno"
            }
        }
        b if (160.0..=180.0).contains(&b) => "Drum & Bass",
        _ => "",
    }
    .into()
}

pub fn artist_from_filename(filename: &str) -> String {
    static NUM: OnceLock<Regex> = OnceLock::new();
    static LAB: OnceLock<Regex> = OnceLock::new();
    let stem = Path::new(filename).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let Some((artist, _)) = stem.split_once(" - ") else { return String::new() };
    let artist = NUM.get_or_init(|| Regex::new(r"^\d+\s*[.\-_)]\s*").expect("expression")).replace(artist.trim(), "").to_string();
    if artist.is_empty() || artist.chars().all(|c| c.is_ascii_digit()) || artist.chars().count() > 60 || LAB.get_or_init(|| Regex::new(r"(?i)^lab\s*\d").expect("expression")).is_match(&artist) {
        return String::new();
    }
    artist
}

/// Une proposition : (morceau, champ « genre » ou « artist », valeur, raison, titre).
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub id: i64,
    pub field: &'static str,
    pub value: String,
    pub reason: String,
    pub title: String,
    pub path: String,
}

/// Propositions de genre et d'artiste pour les morceaux qui n'en ont pas (rien n'est écrit).
pub fn suggest(conn: &Connection, artist_name: &str) -> Vec<Suggestion> {
    struct R {
        id: i64,
        path: String,
        filename: String,
        title: String,
        artist: String,
        genre: String,
        bpm: Option<f64>,
        rel: Option<i64>,
    }
    let rows: Vec<R> = conn
        .prepare(
            "SELECT t.id, t.path, t.filename, IFNULL(t.title, ''), IFNULL(t.artist, ''), IFNULL(t.genre, ''), t.bpm,
                    (SELECT release_id FROM release_tracks rt WHERE rt.track_id = t.id LIMIT 1) FROM tracks t WHERE t.missing = 0",
        )
        .and_then(|mut s| s.query_map([], |r| Ok(R { id: r.get(0)?, path: r.get(1)?, filename: r.get(2)?, title: r.get(3)?, artist: r.get(4)?, genre: r.get(5)?, bpm: r.get(6)?, rel: r.get(7)? }))?.collect())
        .unwrap_or_default();
    let dir = |p: &str| Path::new(&p.replace('/', "\\")).parent().map(|d| d.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut by_rel: HashMap<i64, BTreeMap<String, i64>> = HashMap::new();
    let mut by_dir: HashMap<String, BTreeMap<String, i64>> = HashMap::new();
    for r in rows.iter().filter(|r| !r.genre.is_empty()) {
        if let Some(rel) = r.rel {
            *by_rel.entry(rel).or_default().entry(r.genre.clone()).or_default() += 1;
        }
        *by_dir.entry(dir(&r.path)).or_default().entry(r.genre.clone()).or_default() += 1;
    }
    let most = |m: &BTreeMap<String, i64>| m.iter().max_by_key(|(_, n)| **n).map(|(g, _)| g.clone());
    // Dossiers où la majorité des morceaux portent déjà le nom de l'artiste.
    let mut own: HashMap<String, (i64, i64)> = HashMap::new();
    if !artist_name.is_empty() {
        for r in &rows {
            let e = own.entry(dir(&r.path)).or_default();
            if r.artist.to_lowercase().contains(&artist_name.to_lowercase()) {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
    }
    let mut out = Vec::new();
    for r in &rows {
        let title = if r.title.is_empty() { Path::new(&r.filename).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { r.title.clone() };
        let d = dir(&r.path);
        if r.genre.is_empty() {
            let mut g = (genre_from_title(&format!("{} {}", r.filename, r.title)), "style écrit dans le titre".to_string());
            if g.0.is_empty() {
                if let Some(x) = r.rel.and_then(|rel| by_rel.get(&rel)).and_then(most) {
                    g = (x, "genre des autres morceaux de la sortie".into());
                }
            }
            if g.0.is_empty() {
                if let Some(x) = by_dir.get(&d).and_then(most) {
                    g = (x, "genre des autres morceaux du dossier".into());
                }
            }
            if g.0.is_empty() {
                g = (genre_from_folder(&r.path), "nom du dossier".into());
            }
            if g.0.is_empty() {
                g = (genre_from_tempo(r.bpm), r.bpm.map(|b| format!("tempo ({b:.0} BPM), à vérifier")).unwrap_or_default());
            }
            if !g.0.is_empty() {
                out.push(Suggestion { id: r.id, field: "genre", value: g.0, reason: g.1, title: title.clone(), path: r.path.clone() });
            }
        }
        if r.artist.is_empty() {
            let mut a = (artist_from_filename(&r.filename), "nom du fichier".to_string());
            if a.0.is_empty() && !artist_name.is_empty() && own.get(&d).is_some_and(|(y, n)| *y >= (*n).max(1)) {
                a = (artist_name.to_string(), "rangé avec tes morceaux".into());
            }
            if !a.0.is_empty() {
                out.push(Suggestion { id: r.id, field: "artist", value: a.0, reason: a.1, title, path: r.path.clone() });
            }
        }
    }
    out
}

/// Un lot de morceaux sans genre, à valider d'un clic.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub label: String,
    pub genre: String,
    pub why: String,
    pub ids: Vec<i64>,
    /// Une proposition sûre : les autres titres du lot ou le titre lui-même le disent. Sinon, à vérifier.
    pub sure: bool,
}

/// Les morceaux sans genre, regroupés : par sortie d'abord (les titres d'une sortie partagent souvent leur genre), puis
/// par artiste invité, puis par dossier. Rien n'est écrit.
pub fn batches(conn: &Connection, own_names: &[String]) -> Vec<Batch> {
    struct R {
        id: i64,
        path: String,
        filename: String,
        title: String,
        artist: String,
        genre: String,
        rel: Option<(i64, String)>,
    }
    let rows: Vec<R> = conn
        .prepare(
            "SELECT t.id, t.path, t.filename, IFNULL(t.title, ''), IFNULL(t.artist, ''), IFNULL(t.genre, ''), r.id, r.title
             FROM tracks t LEFT JOIN release_tracks rt ON rt.track_id = t.id LEFT JOIN releases r ON r.id = rt.release_id
             WHERE t.missing = 0 AND COALESCE(t.archived, 0) = 0 GROUP BY t.id",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                let rel: Option<i64> = r.get(6)?;
                let name: Option<String> = r.get(7)?;
                Ok(R { id: r.get(0)?, path: r.get(1)?, filename: r.get(2)?, title: r.get(3)?, artist: r.get(4)?, genre: r.get(5)?, rel: rel.zip(name) })
            })?
            .collect()
        })
        .unwrap_or_default();
    let dir = |p: &str| Path::new(&p.replace('/', "\\")).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let own = |a: &str| {
        let a = a.to_lowercase();
        a.is_empty() || own_names.iter().any(|n| !n.is_empty() && a.contains(&n.to_lowercase()))
    };
    // Ce que disent les morceaux qui ont déjà un genre.
    let (mut by_rel, mut by_artist, mut by_dir): (HashMap<i64, BTreeMap<String, i64>>, HashMap<String, BTreeMap<String, i64>>, HashMap<String, BTreeMap<String, i64>>) = Default::default();
    for r in rows.iter().filter(|r| !r.genre.is_empty()) {
        if let Some((rel, _)) = &r.rel {
            *by_rel.entry(*rel).or_default().entry(r.genre.clone()).or_default() += 1;
        }
        if !own(&r.artist) {
            *by_artist.entry(r.artist.to_lowercase()).or_default().entry(r.genre.clone()).or_default() += 1;
        }
        *by_dir.entry(dir(&r.path).to_lowercase()).or_default().entry(r.genre.clone()).or_default() += 1;
    }
    let most = |m: &BTreeMap<String, i64>| m.iter().max_by_key(|(_, n)| **n).map(|(g, _)| g.clone());
    let mut out: Vec<Batch> = Vec::new();
    let mut add = |label: String, genre: String, why: String, id: i64, sure: bool| match out.iter_mut().find(|b| b.label == label && b.genre == genre) {
        Some(b) => b.ids.push(id),
        None => out.push(Batch { label, genre, why, ids: vec![id], sure }),
    };
    for r in rows.iter().filter(|r| r.genre.is_empty()) {
        let name = format!("{} {}", r.filename, r.title);
        let written = genre_from_title(&name);
        if !written.is_empty() {
            add("Style écrit dans le titre".into(), written, "le titre le dit".into(), r.id, true);
            continue;
        }
        if let Some((rel, title)) = &r.rel {
            if let Some(g) = by_rel.get(rel).and_then(most) {
                add(format!("Sortie « {title} »"), g, "genre des autres titres de la sortie".into(), r.id, true);
                continue;
            }
        }
        if let Some(g) = by_artist.get(&r.artist.to_lowercase()).and_then(most) {
            add(format!("Artiste {}", r.artist), g, "genre de ses autres morceaux".into(), r.id, true);
            continue;
        }
        let d = dir(&r.path);
        let folder = Path::new(&d).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        // Un dossier mélange souvent les styles : la proposition reste à vérifier.
        if let Some(g) = by_dir.get(&d.to_lowercase()).and_then(most) {
            add(format!("Dossier {folder}"), g, "genre le plus présent dans le dossier : à vérifier".into(), r.id, false);
            continue;
        }
        let g = genre_from_folder(&r.path);
        if !g.is_empty() {
            add(format!("Dossier {folder}"), g, "nom du dossier".into(), r.id, true);
            continue;
        }
        // Une sortie entière sans genre : un seul choix pour tout le lot.
        match &r.rel {
            Some((_, title)) => add(format!("Sortie « {title} »"), String::new(), "aucun titre de la sortie n'a de genre : à choisir".into(), r.id, false),
            None => add(format!("Dossier {folder}"), String::new(), "aucun indice : à choisir".into(), r.id, false),
        }
    }
    out.sort_by(|a, b| b.sure.cmp(&a.sure).then(b.ids.len().cmp(&a.ids.len())));
    out
}

/// Donne ce genre à ces morceaux (seulement à ceux qui n'en ont pas).
pub fn apply_genre(conn: &Connection, ids: &[i64], genre: &str) -> usize {
    ids.iter().map(|id| conn.execute("UPDATE tracks SET genre = ?1, modified_at = datetime('now') WHERE id = ?2 AND IFNULL(genre, '') = ''", params![genre.trim(), id]).unwrap_or(0)).sum()
}

/// Applique les propositions retenues (seulement sur les champs encore vides).
pub fn apply_suggestions(conn: &Connection, items: &[Suggestion]) -> usize {
    items
        .iter()
        .map(|s| {
            conn.execute(
                &format!("UPDATE tracks SET {0} = ?1, modified_at = datetime('now') WHERE id = ?2 AND ({0} IS NULL OR {0} = '')", s.field),
                params![s.value, s.id],
            )
            .unwrap_or(0)
        })
        .sum()
}

// ---------------------------------------------------------------------- versions

/// Rattache un fichier à son morceau principal, comme version ou comme stem.
pub fn attach(conn: &Connection, id: i64, parent: i64, stem: bool) -> rusqlite::Result<()> {
    if id == parent {
        return Ok(());
    }
    conn.execute(
        "UPDATE tracks SET parent_id = ?2, role = ?3, modified_at = datetime('now') WHERE id = ?1",
        params![id, parent, if stem { "stem" } else { "version" }],
    )?;
    Ok(())
}

pub fn detach(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE tracks SET parent_id = NULL, role = 'mixdown', modified_at = datetime('now') WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_genres_artists() {
        let dir = std::env::temp_dir().join(format!("otoko-fichiers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("vieux nom.wav");
        std::fs::write(&src, b"RIFF").unwrap();
        let row = FileRow { id: 1, path: src.to_string_lossy().into(), title: "Nuit".into(), artist: "Otoko".into(), bpm: Some(124.4), key: "Am".into(), track_no: Some(3), ..Default::default() };
        let m = rename_preview(std::slice::from_ref(&row), "{artist} - {title} ({bpm} {key})");
        assert_eq!(m[0].dst.file_name().unwrap().to_string_lossy(), "Otoko - Nuit (124 Am).wav");
        let r = reorganize_preview(std::slice::from_ref(&row), &dir, "{artist}/{track_no} - {title}");
        assert!(r[0].dst.ends_with(r"Otoko\03 - Nuit.wav"), "{:?}", r[0].dst);
        // « .. » est retiré du modèle : on reste dans le dossier choisi.
        let up = reorganize_preview(std::slice::from_ref(&row), &dir, "../{title}");
        assert!(up[0].reason == "ok" && up[0].dst.starts_with(normalize(&dir)), "{:?}", up[0]);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(genre_from_title("Massalia (Tribal House Remix)"), "Tribal House");
        assert_eq!(genre_from_text("best of house music"), "House");
        assert_eq!(genre_from_text("housewarming"), "");
        assert_eq!(genre_from_folder(r"Z:\Musique\Minimal - Organic - Deep Techno\a.wav"), "Minimal");
        assert_eq!(genre_from_folder(r"Z:\Musique\02_SINGLES_EP\a.wav"), "");
        assert_eq!(genre_from_tempo(Some(174.0)), "Drum & Bass");
        assert_eq!(artist_from_filename("01. Otoko X D.Cave - Gate.wav"), "Otoko X D.Cave");
        assert_eq!(artist_from_filename("LAB 7 - 01 - Skipped.wav"), "");
        let g = vec![
            DupRow { id: 1, format: "mp3".into(), size: 5, ..Default::default() },
            DupRow { id: 2, format: "wav".into(), size: 50, ..Default::default() },
            DupRow { id: 3, format: "mp3".into(), pick: 1, ..Default::default() },
        ];
        assert_eq!(best_of(&g), 3);
    }

    /// Un petit WAV (16 bits, mono, 0,1 s de silence).
    fn wav(path: &Path) {
        let n: u32 = 4410;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&44100u32.to_le_bytes());
        b.extend_from_slice(&88200u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(n * 2).to_le_bytes());
        b.extend(std::iter::repeat_n(0u8, (n * 2) as usize));
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn tags_written_and_read_back() {
        let p = std::env::temp_dir().join(format!("otoko-tags-{}.wav", std::process::id()));
        wav(&p);
        let v = TagValues {
            title: Some("Nuit blanche".into()),
            artist: Some("Otoko".into()),
            album: Some("Esprit Autonome".into()),
            genre: Some("UK Garage".into()),
            year: Some("2026".into()),
            track_no: Some("3".into()),
            bpm: Some("132,4".into()),
            key: Some("Am".into()),
            comment: None,
        };
        write_tags(&p, &v).unwrap();
        let t = super::super::scan::read_tags(&p);
        assert_eq!((t.title.as_deref(), t.artist.as_deref(), t.genre.as_deref(), t.year, t.track_no), (Some("Nuit blanche"), Some("Otoko"), Some("UK Garage"), Some(2026), Some(3)));
        assert_eq!((t.bpm, t.key.as_deref()), (Some(132.0), Some("Am")));
        assert!((t.duration.unwrap_or(0.0) - 0.1).abs() < 0.01, "le son n'a pas bougé");
        // Effacer un champ.
        write_tags(&p, &TagValues { genre: Some(String::new()), ..Default::default() }).unwrap();
        assert_eq!(super::super::scan::read_tags(&p).genre, None);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn cover_embedded_and_read_back() {
        let p = std::env::temp_dir().join(format!("otoko-pochette-{}.wav", std::process::id()));
        wav(&p);
        write_tags(&p, &TagValues { title: Some("Nuit".into()), ..Default::default() }).unwrap();
        // Un JPEG n'a pas à être valide pour être rangé : l'en-tête suffit au test.
        let jpeg = [&[0xFF, 0xD8, 0xFF, 0xE0][..], &[7u8; 300][..], &[0xFF, 0xD9][..]].concat();
        embed_cover(&p, &jpeg).unwrap();
        assert_eq!(read_cover(&p), Some(jpeg.clone()));
        // Une seconde fois : remplacée, pas ajoutée ; les tags restent.
        let other = [&[0xFF, 0xD8, 0xFF, 0xE0][..], &[9u8; 100][..], &[0xFF, 0xD9][..]].concat();
        embed_cover(&p, &other).unwrap();
        assert_eq!(read_cover(&p), Some(other));
        assert_eq!(super::super::scan::read_tags(&p).title.as_deref(), Some("Nuit"));
        assert!(embed_cover(&p.with_extension("txt"), &jpeg).is_err());
        let _ = std::fs::remove_file(&p);
    }
}
