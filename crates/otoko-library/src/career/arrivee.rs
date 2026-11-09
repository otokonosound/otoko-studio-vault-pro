//! Le dossier d'arrivée des exports : un WAV y entre, l'application reconnaît le morceau d'après son nom et le range
//! comme nouvelle version. Le fichier ne bouge pas ; seule la base change. Quand le nom désigne plusieurs morceaux, rien
//! n'est décidé à la place de l'artiste : l'export attend d'être rangé à la main.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use regex::Regex;
use rusqlite::{Connection, params};

use super::{files, releases, scan, statuts};

/// Le dossier choisi.
pub const DIR_KEY: &str = "arrival_dir";
/// Les exports à ranger à la main (liste d'identifiants).
pub const TODO_KEY: &str = "arrival_todo";

/// Un export se pose quand il n'a plus bougé depuis ce temps (FL a fini d'écrire).
const SETTLED: Duration = Duration::from_secs(15);

/// Ce qui dit la version dans un nom : « v3 », « master », « final », « mix 2 », une date.
fn version_words() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?ix)
            [\s_\-.]*[(\[]?
            (?P<tag>
                v(?:ersion)?[\s_.]?\d+ | (?:mix|master|edit|export|bounce|render|final|take|rough)(?:[\s_.]?\d+)? | mixdown |
                \d{4}[\-_.]\d{2}[\-_.]\d{2} | \d{2}[\-_.]\d{2}[\-_.]\d{2,4} | \(\d+\) | copie
            )
            [)\]]?\s*$",
        )
        .expect("expression")
    })
}

/// Le nom d'un morceau sans ce qui dit sa version ni son numéro de piste.
pub fn base_name(stem: &str) -> String {
    let mut s = stem.trim().to_string();
    // « Morceau v3 final » : plusieurs mots de version de suite.
    while let Some(m) = version_words().find(&s) {
        if m.start() == 0 {
            break;
        }
        s.truncate(m.start());
    }
    releases::name_key(&s)
}

/// Ce que le nom dit de la version : « v3 », « master »… (vide s'il ne dit rien).
pub fn version_tag(stem: &str) -> String {
    let mut s = stem.trim().to_string();
    let mut tags = Vec::new();
    while let Some(c) = version_words().captures(&s) {
        let m = c.get(0).expect("toute la prise");
        if m.start() == 0 {
            break;
        }
        tags.push(c["tag"].trim_matches(['(', ')']).to_lowercase().replace(['_', '.'], " "));
        s.truncate(m.start());
    }
    tags.reverse();
    tags.join(" ")
}

/// Ce qui est arrivé d'un export.
#[derive(Clone, Debug, PartialEq)]
pub enum Landing {
    /// Rangé comme version d'un morceau (identifiant de la version, morceau, son titre, libellé).
    Version { id: i64, parent: i64, title: String, label: String },
    /// Aucun morceau de ce nom : c'est un nouveau morceau.
    New { id: i64, title: String },
    /// Plusieurs morceaux portent ce nom : à ranger à la main.
    Unsure { id: i64, title: String, matches: usize },
}

impl Landing {
    pub fn id(&self) -> i64 {
        match self {
            Landing::Version { id, .. } | Landing::New { id, .. } | Landing::Unsure { id, .. } => *id,
        }
    }
}

/// Les morceaux principaux dont le nom correspond.
fn matches(conn: &Connection, base: &str, except: i64) -> Vec<(i64, String)> {
    let rows: Vec<(i64, String, String)> = conn
        .prepare(
            "SELECT id, IFNULL(title, ''), IFNULL(filename, '') FROM tracks
             WHERE id <> ?1 AND parent_id IS NULL AND missing = 0 AND COALESCE(archived, 0) = 0 AND IFNULL(role, 'mixdown') IN ('mixdown', 'render')",
        )
        .and_then(|mut s| s.query_map([except], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
        .unwrap_or_default();
    rows.into_iter()
        .filter(|(_, title, file)| {
            let stem = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            base_name(title) == base || base_name(&stem) == base
        })
        .map(|(id, title, file)| (id, if title.is_empty() { file } else { title }))
        .collect()
}

/// Range un export.
pub fn land(conn: &Connection, path: &Path) -> rusqlite::Result<Landing> {
    let (_, id) = scan::upsert(conn, path, &[])?;
    if id == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let base = base_name(&stem);
    let found = matches(conn, &base, id);
    let tag = version_tag(&stem);
    match found.as_slice() {
        [(parent, title)] => {
            files::attach(conn, id, *parent, false)?;
            let label = if tag.is_empty() {
                let n: i64 = conn.query_row("SELECT COUNT(*) FROM tracks WHERE parent_id = ?1 AND role = 'version'", [parent], |r| r.get(0))?;
                format!("v{}", n + 1)
            } else {
                tag
            };
            conn.execute(
                "UPDATE tracks SET version_label = ?2,
                    title = CASE WHEN IFNULL(p.title, '') = '' THEN tracks.title ELSE p.title || ' (' || ?2 || ')' END,
                    artist = COALESCE(NULLIF(tracks.artist, ''), p.artist), album = COALESCE(NULLIF(tracks.album, ''), p.album),
                    genre = COALESCE(NULLIF(tracks.genre, ''), p.genre), nature = p.nature, nature_meta = p.nature_meta
                 FROM (SELECT title, artist, album, genre, nature, nature_meta FROM tracks WHERE id = ?3) AS p
                 WHERE tracks.id = ?1",
                params![id, label, parent],
            )?;
            statuts::touched(conn, *parent)?;
            Ok(Landing::Version { id, parent: *parent, title: title.clone(), label })
        }
        [] => {
            conn.execute("UPDATE tracks SET status = 'wip' WHERE id = ?1 AND status IN ('idea', 'draft')", [id])?;
            Ok(Landing::New { id, title: stem })
        }
        many => {
            let mut todo = todo(conn);
            todo.push(id);
            conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", params![TODO_KEY, serde_json::to_string(&todo).unwrap_or_default()])?;
            Ok(Landing::Unsure { id, title: stem, matches: many.len() })
        }
    }
}

/// Les exports à ranger à la main (ceux qui sont encore seuls).
pub fn todo(conn: &Connection) -> Vec<i64> {
    let raw: String = conn.query_row("SELECT value FROM settings WHERE key = ?1", [TODO_KEY], |r| r.get(0)).unwrap_or_default();
    let ids: Vec<i64> = serde_json::from_str(&raw).unwrap_or_default();
    ids.into_iter().filter(|id| conn.query_row("SELECT 1 FROM tracks WHERE id = ?1 AND parent_id IS NULL AND missing = 0", [id], |_| Ok(())).is_ok()).collect()
}

/// Les fichiers posés dans le dossier et pas encore connus. Un fichier encore en cours d'écriture attend le passage
/// suivant.
pub fn pending(conn: &Connection, dir: &Path, now: SystemTime) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(dir).max_depth(2).into_iter().filter_map(|e| e.ok()) {
        if !e.file_type().is_file() || !scan::is_audio(e.path()) {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        let settled = meta.modified().ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|d| d >= SETTLED);
        if !settled {
            continue;
        }
        let p = scan::normalize(e.path());
        let known = conn.query_row("SELECT 1 FROM tracks WHERE path = ?1 COLLATE NOCASE", [p.to_string_lossy()], |_| Ok(())).is_ok();
        if !known {
            out.push(p);
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_their_version() {
        assert_eq!(base_name("I'm Alive v3"), "i m alive");
        assert_eq!(base_name("I'm Alive (master)"), "i m alive");
        assert_eq!(base_name("I'm Alive_mix2 final"), "i m alive");
        assert_eq!(base_name("03.I'm Alive 2026-11-06"), "i m alive");
        assert_eq!(base_name("Master Plan"), "master plan");
        assert_eq!(base_name("Final"), "final");
        assert_eq!(version_tag("I'm Alive v3"), "v3");
        assert_eq!(version_tag("I'm Alive (Master)"), "master");
        assert_eq!(version_tag("I'm Alive_mix2 final"), "mix2 final");
        assert_eq!(version_tag("I'm Alive"), "");
    }

    fn wav(p: &Path) {
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
    }

    #[test]
    fn an_export_finds_its_track() {
        let dir = std::env::temp_dir().join(format!("otoko-arrivee-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let c = super::super::Career::open(&dir.join("c.db")).unwrap();
        c.conn()
            .execute_batch(
                "INSERT INTO tracks (id, path, filename, title, artist, genre, status) VALUES
                    (1, 'Z:\\a\\Im Alive.wav', 'Im Alive.wav', 'Im Alive', 'Otoko', 'UK Garage', 'idea'),
                    (2, 'Z:\\a\\Nuit.wav', 'Nuit.wav', 'Nuit', 'Otoko', '', 'idea'),
                    (3, 'Z:\\b\\Nuit.wav', 'Nuit.wav', 'Nuit', 'Otoko', '', 'idea');",
            )
            .unwrap();
        let (a, b, n) = (dir.join("Im Alive v3.wav"), dir.join("Inconnu.wav"), dir.join("Nuit v2.wav"));
        for p in [&a, &b, &n] {
            wav(p);
        }
        let later = SystemTime::now() + Duration::from_secs(60);
        assert_eq!(pending(c.conn(), &dir, later).len(), 3);
        assert!(pending(c.conn(), &dir, SystemTime::now()).is_empty(), "un export tout frais attend");
        match land(c.conn(), &a).unwrap() {
            Landing::Version { parent, label, .. } => assert_eq!((parent, label.as_str()), (1, "v3")),
            other => panic!("{other:?}"),
        }
        let (title, genre, role): (String, String, String) = c.conn().query_row("SELECT title, genre, role FROM tracks WHERE parent_id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!((title.as_str(), genre.as_str(), role.as_str()), ("Im Alive (v3)", "UK Garage", "version"));
        let status: String = c.conn().query_row("SELECT status FROM tracks WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(status, "wip", "un nouvel export veut dire qu'on y travaille");
        assert!(matches!(land(c.conn(), &b).unwrap(), Landing::New { .. }));
        assert!(matches!(land(c.conn(), &n).unwrap(), Landing::Unsure { matches: 2, .. }));
        assert_eq!(todo(c.conn()).len(), 1);
        assert_eq!(pending(c.conn(), &dir, later).len(), 0);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
