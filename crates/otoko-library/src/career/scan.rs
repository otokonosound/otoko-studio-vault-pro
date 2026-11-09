//! Indexation des dossiers surveillés de la Carrière (comme `composer_vault/library/scanner.py`) :
//! les fichiers ne bougent jamais ; on note leur chemin, leurs tags, une empreinte du contenu
//! (une copie renommée est reconnue) et leur date. Un fichier déjà connu, de même taille et même
//! date, n'est pas relu : un nouveau passage ne coûte presque rien.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};

use super::nature;

/// Les formats que Studio Vault indexe.
pub const AUDIO_EXTENSIONS: [&str; 14] = ["wav", "flac", "mp3", "aiff", "aif", "ogg", "opus", "m4a", "mp4", "aac", "wma", "ape", "wv", "alac"];

pub fn is_audio(p: &Path) -> bool {
    p.extension().is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScanStats {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub missing: usize,
    pub back: usize,
    pub folders: usize,
}

/// Ce que les tags d'un fichier disent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub track_no: Option<i64>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub duration: Option<f64>,
    pub bitrate: Option<i64>,
    pub samplerate: Option<i64>,
    pub channels: Option<i64>,
    pub comment: Option<String>,
}

fn first_int(s: &str) -> Option<i64> {
    let start = s.find(|c: char| c.is_ascii_digit() || c == '-')?;
    let tail: String = s[start..].chars().enumerate().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-')).map(|x| x.1).collect();
    tail.parse().ok()
}

fn first_float(s: &str) -> Option<f64> {
    let start = s.find(|c: char| c.is_ascii_digit() || c == '-')?;
    let tail: String = s[start..].chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')).collect();
    tail.replace(',', ".").trim_end_matches('.').parse().ok()
}

/// Les tags d'un fichier (titre : le nom du fichier s'il n'en a pas).
pub fn read_tags(path: &Path) -> TagInfo {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::tag::{Accessor, ItemKey};
    let mut info = TagInfo::default();
    if let Ok(f) = lofty::read_from_path(path) {
        let p = f.properties();
        let d = p.duration().as_secs_f64();
        info.duration = (d > 0.0).then_some(d);
        info.bitrate = p.audio_bitrate().or(p.overall_bitrate()).map(|b| b as i64).filter(|b| *b > 0);
        info.samplerate = p.sample_rate().map(|s| s as i64);
        info.channels = p.channels().map(|c| c as i64);
        // La première étiquette d'abord (ID3 d'un WAV), puis les autres (bloc INFO de Windows).
        let mut tags: Vec<&lofty::tag::Tag> = f.primary_tag().into_iter().collect();
        tags.extend(f.tags().iter().filter(|t| Some(t.tag_type()) != f.primary_tag().map(|p| p.tag_type())));
        let text = |get: &dyn Fn(&lofty::tag::Tag) -> Option<String>| tags.iter().find_map(|t| get(t).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
        info.title = text(&|t| t.title().map(|s| s.into_owned()));
        info.artist = text(&|t| t.artist().map(|s| s.into_owned()).or_else(|| t.get_string(&ItemKey::AlbumArtist).map(str::to_string)));
        info.album = text(&|t| t.album().map(|s| s.into_owned()));
        info.genre = text(&|t| t.genre().map(|s| s.into_owned()));
        info.comment = text(&|t| t.comment().map(|s| s.into_owned()));
        info.year = tags.iter().find_map(|t| t.year().map(|y| y as i64).or_else(|| t.get_string(&ItemKey::RecordingDate).and_then(first_int)));
        info.track_no = tags.iter().find_map(|t| t.track().map(|n| n as i64));
        info.bpm = text(&|t| t.get_string(&ItemKey::Bpm).or(t.get_string(&ItemKey::IntegerBpm)).map(str::to_string)).and_then(|s| first_float(&s)).filter(|b| *b > 0.0);
        info.key = text(&|t| t.get_string(&ItemKey::InitialKey).map(str::to_string));
    }
    if info.title.is_none() {
        info.title = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    }
    info
}

/// Empreinte rapide du contenu : taille + 4 blocs de 4 Ko (SHA-1, comme Studio Vault). Le nom
/// n'y entre pas : une copie renommée est reconnue comme doublon.
pub fn quick_fingerprint(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(meta) = std::fs::metadata(path) else { return String::new() };
    let size = meta.len();
    let mut data = size.to_string().into_bytes();
    if let Ok(mut f) = std::fs::File::open(path) {
        for off in [0, size / 4, size / 2, size.saturating_sub(4096)] {
            let mut buf = vec![0u8; 4096];
            if f.seek(SeekFrom::Start(off)).is_ok() {
                let mut n = 0;
                while n < buf.len() {
                    match f.read(&mut buf[n..]) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => n += k,
                    }
                }
                data.extend_from_slice(&buf[..n]);
            }
        }
    }
    crate::hex(&crate::sha1(&data))
}

/// Date du fichier, heure locale : la plus ancienne entre création et modification (une copie
/// ou un téléchargement rajeunit la création, la modification garde la vraie date).
pub fn file_date(meta: &std::fs::Metadata) -> String {
    let t = [meta.created().ok(), meta.modified().ok()].into_iter().flatten().min();
    match t {
        Some(t) => chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string(),
        None => String::new(),
    }
}

/// Fichier probablement sorti de Suno.
fn is_suno(path: &Path, tags: &TagInfo) -> bool {
    path.components().any(|c| c.as_os_str().to_string_lossy().to_lowercase().contains("suno"))
        || tags.comment.as_deref().is_some_and(|c| {
            let c = c.to_lowercase();
            c.contains("suno.ai") || c.contains("suno.com")
        })
}

/// Dossiers retirés de la bibliothèque par l'utilisateur (copies…) : jamais réindexés.
pub fn ignored_folders(conn: &Connection) -> Vec<String> {
    let raw: String = conn.query_row("SELECT value FROM settings WHERE key = 'ignored_folders'", [], |r| r.get(0)).unwrap_or_else(|_| "[]".into());
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

fn under(path: &str, folder: &str) -> bool {
    let p = path.to_lowercase().replace('/', "\\");
    let f = folder.to_lowercase().replace('/', "\\");
    p.starts_with(&format!("{}\\", f.trim_end_matches('\\')))
}

pub enum Action {
    Added,
    Updated,
    Unchanged,
    Skipped,
}

/// Le chemin écrit comme Windows (et Python) l'écrit : « Z:\a\b.wav », jamais « Z:\a/b.wav »
/// (des dossiers surveillés de Studio Vault sont notés avec des « / »).
pub fn normalize(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().replace('/', "\\"))
}

/// Ajoute ou met à jour un fichier. Un fichier connu de même taille et de même date n'est pas relu.
pub fn upsert(conn: &Connection, path: &Path, ignored: &[String]) -> rusqlite::Result<(Action, i64)> {
    let path = &normalize(path);
    let p = path.to_string_lossy().into_owned();
    if ignored.iter().any(|f| under(&p, f)) {
        return Ok((Action::Skipped, 0));
    }
    let Ok(meta) = std::fs::metadata(path) else { return Ok((Action::Skipped, 0)) };
    let size = meta.len() as i64;
    let date = file_date(&meta);
    let row: Option<(i64, Option<i64>, Option<String>, i64)> = conn
        .query_row("SELECT id, file_size, file_date, IFNULL(missing, 0) FROM tracks WHERE path = ?1", [&p], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .optional()?;
    if let Some((id, s, d, missing)) = &row {
        if *s == Some(size) && d.as_deref() == Some(date.as_str()) {
            if *missing != 0 {
                conn.execute("UPDATE tracks SET missing = 0 WHERE id = ?1", [id])?;
            }
            return Ok((Action::Unchanged, *id));
        }
    }
    let tags = read_tags(path);
    let fp = quick_fingerprint(path);
    let filename = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let format = path.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let opt_s = |v: &Option<String>| v.clone().map(Value::Text).unwrap_or(Value::Null);
    let opt_i = |v: Option<i64>| v.map(Value::Integer).unwrap_or(Value::Null);
    let opt_f = |v: Option<f64>| v.map(Value::Real).unwrap_or(Value::Null);
    let fields: Vec<(&str, Value)> = vec![
        ("filename", Value::Text(filename.clone())),
        ("title", opt_s(&tags.title)),
        ("artist", opt_s(&tags.artist)),
        ("album", opt_s(&tags.album)),
        ("genre", opt_s(&tags.genre)),
        ("year", opt_i(tags.year)),
        ("track_no", opt_i(tags.track_no)),
        ("duration_s", opt_f(tags.duration)),
        ("bitrate_kbps", opt_i(tags.bitrate)),
        ("samplerate", opt_i(tags.samplerate)),
        ("channels", opt_i(tags.channels)),
        ("file_format", Value::Text(format)),
        ("file_size", Value::Integer(size)),
        ("bpm", opt_f(tags.bpm)),
        ("music_key", opt_s(&tags.key)),
        ("fingerprint", Value::Text(fp)),
        ("file_date", Value::Text(date)),
        ("missing", Value::Integer(0)),
    ];
    match row {
        None => {
            let mut fields = fields;
            if is_suno(path, &tags) {
                fields.push(("role", Value::Text("suno_generation".into())));
            }
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let nat = nature::detect(&format!("{stem} {}", tags.title.as_deref().unwrap_or("")));
            fields.push(("nature", Value::Text(nat.into())));
            if nat == "remix" || nat == "edit" {
                fields.push(("nature_meta", Value::Text(nature::parse_remix_title(&filename).to_string())));
            }
            let cols: Vec<&str> = fields.iter().map(|f| f.0).collect();
            let sql = format!("INSERT INTO tracks (path, {}) VALUES (?{})", cols.join(", "), ", ?".repeat(cols.len()));
            let mut vals = vec![Value::Text(p)];
            vals.extend(fields.into_iter().map(|f| f.1));
            conn.execute(&sql, rusqlite::params_from_iter(vals.iter()))?;
            Ok((Action::Added, conn.last_insert_rowid()))
        }
        Some((id, ..)) => {
            // Un tag vide n'efface pas ce que l'utilisateur ou l'analyse ont rempli.
            const KEEP: [&str; 9] = ["title", "artist", "album", "genre", "year", "track_no", "bpm", "music_key", "duration_s"];
            let sets: Vec<String> = fields.iter().map(|(k, _)| if KEEP.contains(k) { format!("{k} = COALESCE(?, {k})") } else { format!("{k} = ?") }).collect();
            let sql = format!("UPDATE tracks SET {}, modified_at = datetime('now') WHERE id = ?", sets.join(", "));
            let mut vals: Vec<Value> = fields.into_iter().map(|f| f.1).collect();
            vals.push(Value::Integer(id));
            conn.execute(&sql, rusqlite::params_from_iter(vals.iter()))?;
            Ok((Action::Updated, id))
        }
    }
}

/// Les dossiers surveillés actifs.
pub fn watch_folders(conn: &Connection) -> Vec<(i64, String, bool)> {
    conn.prepare("SELECT id, path, IFNULL(enabled, 1) FROM watch_folders ORDER BY path COLLATE NOCASE")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))?.collect())
        .unwrap_or_default()
}

/// Parcourt les dossiers surveillés (ou ceux donnés), indexe les nouveautés, puis note les
/// fichiers disparus. `progress(fichiers vus)` ; `stop` interrompt proprement.
pub fn scan(conn: &Connection, only: Option<&[PathBuf]>, stop: &AtomicBool, progress: &mut dyn FnMut(usize)) -> rusqlite::Result<ScanStats> {
    let folders: Vec<PathBuf> = match only {
        Some(f) => f.to_vec(),
        None => watch_folders(conn).into_iter().filter(|f| f.2).map(|f| normalize(Path::new(&f.1))).collect(),
    };
    let ignored = ignored_folders(conn);
    let mut stats = ScanStats { folders: folders.len(), ..Default::default() };
    let mut seen = 0;
    for root in &folders {
        if !root.is_dir() {
            continue;
        }
        // Sans transaction : l'interface écrit dans la même base (garder un morceau, enregistrer une vue) et ne doit
        // jamais attendre la lecture des tags, parfois lente sur un disque réseau. Chaque morceau s'écrit à part.
        for e in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            if !e.file_type().is_file() || !is_audio(e.path()) {
                continue;
            }
            seen += 1;
            if seen % 50 == 0 {
                progress(seen);
            }
            match upsert(conn, e.path(), &ignored) {
                Ok((Action::Added, _)) => stats.added += 1,
                Ok((Action::Updated, _)) => stats.updated += 1,
                Ok((Action::Unchanged, _)) => stats.unchanged += 1,
                _ => stats.skipped += 1,
            }
        }
    }
    let (gone, back) = refresh_missing(conn)?;
    stats.missing = gone;
    stats.back = back;
    progress(seen);
    Ok(stats)
}

/// L'état « introuvable » d'après le disque : (disparus, revenus). Si le lecteur lui-même est
/// absent (disque débranché, Drive arrêté), rien ne change : les fichiers ne sont pas perdus.
pub fn refresh_missing(conn: &Connection) -> rusqlite::Result<(usize, usize)> {
    let rows: Vec<(i64, String, i64)> = conn
        .prepare("SELECT id, path, IFNULL(missing, 0) FROM tracks WHERE path NOT LIKE 'otoko:%'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut drives: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let (mut gone, mut back) = (Vec::new(), Vec::new());
    for (id, p, missing) in rows {
        let path = Path::new(&p);
        let anchor: String = path.components().take(2).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("");
        let ok = *drives.entry(anchor.clone()).or_insert_with(|| !anchor.is_empty() && Path::new(&anchor).exists());
        if !ok {
            continue;
        }
        let exists = path.exists();
        if !exists && missing == 0 {
            gone.push(id);
        } else if exists && missing != 0 {
            back.push(id);
        }
    }
    for id in &gone {
        conn.execute("UPDATE tracks SET missing = 1 WHERE id = ?1", [id])?;
    }
    for id in &back {
        conn.execute("UPDATE tracks SET missing = 0 WHERE id = ?1", [id])?;
    }
    Ok((gone.len(), back.len()))
}

/// Ajoute un dossier surveillé (un même dossier écrit autrement n'est pas ajouté deux fois).
pub fn add_folder(conn: &Connection, path: &str) -> rusqlite::Result<bool> {
    let key = path.trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase();
    if watch_folders(conn).iter().any(|f| f.1.trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase() == key) {
        return Ok(false);
    }
    conn.execute("INSERT INTO watch_folders (path, enabled) VALUES (?1, 1)", [normalize(Path::new(path)).to_string_lossy().trim_end_matches('\\')])?;
    // Un dossier rajouté n'est plus ignoré.
    let ignored: Vec<String> = ignored_folders(conn).into_iter().filter(|f| f.replace('/', "\\").to_lowercase() != key).collect();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('ignored_folders', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [serde_json::to_string(&ignored).unwrap_or_else(|_| "[]".into())],
    )?;
    Ok(true)
}

/// Retire un dossier surveillé : ses morceaux sortent de la bibliothèque (pas du disque).
pub fn remove_folder(conn: &Connection, id: i64) -> rusqlite::Result<usize> {
    let Some(path): Option<String> = conn.query_row("SELECT path FROM watch_folders WHERE id = ?1", [id], |r| r.get(0)).optional()? else { return Ok(0) };
    conn.execute("DELETE FROM watch_folders WHERE id = ?1", [id])?;
    let prefix = format!("{}\\%", normalize(Path::new(&path)).to_string_lossy().trim_end_matches('\\'));
    conn.execute("DELETE FROM tracks WHERE REPLACE(path, '/', '\\') LIKE ?1 AND id NOT IN (SELECT track_id FROM release_tracks)", params![prefix])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_paths() {
        assert_eq!(first_int("2024-05-01"), Some(2024));
        assert_eq!(first_int("3/12"), Some(3));
        assert_eq!(first_float("124,5 BPM"), Some(124.5));
        assert!(under(r"Z:\Drive\Audio\copie\a.wav", "Z:/Drive/Audio/copie/"));
        assert!(!under(r"Z:\Drive\Audio\copies\a.wav", r"Z:\Drive\Audio\copie"));
    }
}
