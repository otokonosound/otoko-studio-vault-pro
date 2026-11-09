//! Les dossiers de la bibliothèque, comme Studio Vault : dossiers surveillés (actifs ou non,
//! introuvables au démarrage), dossiers d'archives, dossiers en double (`dupe_folders.py`), et
//! changement d'emplacement (`relocate.py` : les fichiers ont été déplacés, seul le début des
//! chemins change dans la base ; les fichiers eux-mêmes ne sont jamais touchés).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use rusqlite::{Connection, params};

// ---------------------------------------------------------------------- dossiers surveillés

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Watch {
    pub id: i64,
    pub path: String,
    pub enabled: bool,
    pub exists: bool,
    pub tracks: i64,
}

pub fn watch_list(conn: &Connection) -> Vec<Watch> {
    let rows: Vec<(i64, String, bool)> = conn
        .prepare("SELECT id, path, IFNULL(enabled, 1) FROM watch_folders ORDER BY path COLLATE NOCASE")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))?.collect())
        .unwrap_or_default();
    rows.into_iter()
        .map(|(id, path, enabled)| {
            // Un dossier de Studio Vault peut être noté avec des « / » ; les morceaux, eux, avec des « \ ».
            let like = format!("{}\\%", path.trim_end_matches(['\\', '/']).replace('/', "\\"));
            let tracks = conn.query_row("SELECT COUNT(*) FROM tracks WHERE missing = 0 AND REPLACE(path, '/', '\\') LIKE ?1", [&like], |r| r.get(0)).unwrap_or(0);
            Watch { id, exists: Path::new(&path).is_dir(), path, enabled, tracks }
        })
        .collect()
}

pub fn set_watch_enabled(conn: &Connection, id: i64, on: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE watch_folders SET enabled = ?2 WHERE id = ?1", params![id, on as i64])?;
    Ok(())
}

/// Les dossiers surveillés actifs qu'on ne trouve plus (disque débranché, dossier déplacé…), sauf
/// ceux pour lesquels on a demandé à ne plus être prévenu (réglage `ignore_missing_folders`).
pub fn missing_watch(conn: &Connection) -> Vec<Watch> {
    let quiet: Vec<String> = conn
        .query_row("SELECT value FROM settings WHERE key = 'ignore_missing_folders'", [], |r| r.get::<_, String>(0))
        .map(|v| v.split('|').map(str::to_string).collect())
        .unwrap_or_default();
    watch_list(conn).into_iter().filter(|w| w.enabled && !w.exists && !quiet.contains(&w.path)).collect()
}

/// Ne plus être prévenu pour ces dossiers introuvables.
pub fn quiet_missing(conn: &Connection, paths: &[String]) -> rusqlite::Result<()> {
    conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('ignore_missing_folders', ?1)", [paths.join("|")])?;
    Ok(())
}

/// Ne surveille plus un dossier ; ses morceaux restent dans la bibliothèque.
pub fn unwatch(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM watch_folders WHERE id = ?1", [id])?;
    Ok(())
}

/// Les noms de dossiers d'archives (réglage `archive_patterns`, liste JSON de Studio Vault).
pub fn set_archive_patterns(conn: &Connection, list: &[String]) -> rusqlite::Result<()> {
    let clean: Vec<serde_json::Value> = list.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).map(|p| serde_json::Value::String(p.into())).collect();
    conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('archive_patterns', ?1)", [serde_json::Value::Array(clean).to_string()])?;
    Ok(())
}

// ---------------------------------------------------------------------- dossiers en double

/// Part des morceaux d'un dossier déjà présents ailleurs pour le dire « en double ».
pub const MIN_SHARE: f64 = 0.8;

#[derive(Clone, Debug, PartialEq)]
pub struct Dupe {
    pub folder: String,
    pub other: String,
    pub n: usize,
    pub common: usize,
    /// Les morceaux de `folder` qu'on retrouve dans `other`.
    pub ids: Vec<i64>,
}

fn parent(p: &str) -> String {
    Path::new(p).parent().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Les dossiers dont (presque) tous les morceaux existent déjà ailleurs : même empreinte rapide
/// (taille + extraits) ou, à défaut, même empreinte audio. Les deux sens d'une paire.
pub fn dupe_folders(conn: &Connection) -> Vec<Dupe> {
    let rows: Vec<(i64, String, String, String)> = conn
        .prepare("SELECT id, path, IFNULL(fingerprint, ''), IFNULL(audio_fp, '') FROM tracks WHERE missing = 0")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect())
        .unwrap_or_default();
    let keys = |r: &(i64, String, String, String)| -> Vec<String> {
        let mut k = Vec::new();
        if !r.2.is_empty() {
            k.push(format!("f:{}", r.2));
        }
        if !r.3.is_empty() {
            k.push(format!("a:{}", r.3));
        }
        k
    };
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
    let mut folders: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        for k in keys(r) {
            by_key.entry(k).or_default().push(i);
        }
        folders.entry(parent(&r.1)).or_default().push(i);
    }
    let mut out = Vec::new();
    for (folder, tracks) in &folders {
        if tracks.len() < 2 {
            continue;
        }
        let mut shared: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
        for &t in tracks {
            for k in keys(&rows[t]) {
                for &o in by_key.get(&k).into_iter().flatten() {
                    let of = parent(&rows[o].1);
                    if &of != folder {
                        shared.entry(of).or_default().insert(rows[t].0);
                    }
                }
            }
        }
        for (other, ids) in shared {
            if ids.len() as f64 >= MIN_SHARE * tracks.len() as f64 {
                out.push(Dupe { folder: folder.clone(), other, n: tracks.len(), common: ids.len(), ids: ids.into_iter().collect() });
            }
        }
    }
    out.sort_by(|a, b| b.common.cmp(&a.common).then(a.folder.cmp(&b.folder)));
    out
}

/// Retire de la bibliothèque les morceaux d'un dossier et ne le réindexe plus (dossiers ignorés).
/// Les fichiers restent sur le disque. Rend le nombre de morceaux retirés.
pub fn ignore_folder(conn: &Connection, folder: &str) -> rusqlite::Result<usize> {
    let mut list = super::scan::ignored_folders(conn);
    if !list.iter().any(|f| f == folder) {
        list.push(folder.to_string());
        let json = serde_json::Value::Array(list.into_iter().map(serde_json::Value::String).collect()).to_string();
        conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('ignored_folders', ?1)", [json])?;
    }
    let like = format!("{}\\%", folder.trim_end_matches(['\\', '/']));
    conn.execute("DELETE FROM tracks WHERE path LIKE ?1", [like])
}

// ---------------------------------------------------------------------- changer d'emplacement

/// (table, colonne) qui contiennent des chemins de fichiers ou de dossiers.
const PATH_COLUMNS: [(&str, &str); 7] =
    [("tracks", "path"), ("watch_folders", "path"), ("releases", "cover_path"), ("release_kits", "cover_path"), ("release_kits", "teaser_path"), ("file_ops", "src"), ("file_ops", "dst")];
/// Colonnes uniques : une entrée déjà présente au nouvel emplacement est remplacée par l'ancienne.
const UNIQUE_COLUMNS: [(&str, &str); 2] = [("tracks", "path"), ("watch_folders", "path")];

fn clean(p: &str) -> String {
    p.trim().trim_end_matches(['\\', '/']).to_string()
}

/// Forme de comparaison : Windows ne distingue ni la casse ni « / » de « \ ».
fn key(p: &str) -> String {
    p.replace('/', "\\").to_lowercase()
}

/// `path` est `old` ou un fichier / dossier dedans (« G:\a » ne contient pas « G:\ab »).
fn under(path: &str, old: &str) -> bool {
    let (k, o) = (key(path), key(old));
    k == o || (k.starts_with(&o) && k[o.len()..].starts_with('\\'))
}

fn moved(path: &str, old: &str, new: &str) -> String {
    // La suite du chemin est gardée telle quelle (mêmes barres qu'avant).
    format!("{new}{}", path.get(old.len()..).unwrap_or(""))
}

fn columns(conn: &Connection) -> Vec<(&'static str, &'static str)> {
    PATH_COLUMNS
        .iter()
        .copied()
        .filter(|(t, c)| {
            conn.prepare(&format!("SELECT {c} FROM {t} LIMIT 0")).is_ok()
        })
        .collect()
}

fn values(conn: &Connection, t: &str, c: &str) -> Vec<(i64, String)> {
    conn.prepare(&format!("SELECT rowid, {c} FROM {t} WHERE {c} IS NOT NULL AND {c} <> ''"))
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .unwrap_or_default()
}

/// Débuts de chemin possibles (dossiers surveillés et leurs parents), les plus fournis d'abord.
pub fn suggestions(conn: &Connection) -> Vec<(String, usize)> {
    let paths: Vec<String> = values(conn, "tracks", "path").into_iter().map(|v| v.1).collect();
    // Écrits comme Windows (et `PureWindowsPath` de Studio Vault) : « Z:\Drive », « Z:\ ».
    let mut cands: BTreeMap<String, String> = BTreeMap::new();
    for (_, root) in values(conn, "watch_folders", "path") {
        let mut p = Some(Path::new(&clean(&root).replace('/', "\\")).to_path_buf());
        while let Some(x) = p {
            let s = x.to_string_lossy().into_owned();
            if !clean(&s).is_empty() {
                cands.entry(key(&clean(&s))).or_insert(s);
            }
            p = x.parent().map(Path::to_path_buf);
        }
    }
    let mut out: Vec<(String, usize)> = cands.into_values().map(|s| (s.clone(), paths.iter().filter(|t| under(t, &clean(&s))).count())).filter(|o| o.1 > 0).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.len().cmp(&a.0.len())));
    out
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    pub tracks: usize,
    pub others: usize,
    pub found: usize,
    pub absent: Vec<String>,
    pub conflicts: usize,
}

/// Ce que changerait `relocate`, et combien de fichiers existent bien au nouvel emplacement.
pub fn preview(conn: &Connection, old: &str, new: &str) -> Preview {
    let (old, new) = (clean(old), clean(new).replace('/', "\\"));
    let mut p = Preview::default();
    if old.is_empty() || new.is_empty() {
        return p;
    }
    for (t, c) in columns(conn) {
        let n = values(conn, t, c).iter().filter(|(_, v)| under(v, &old)).count();
        if (t, c) == ("tracks", "path") {
            p.tracks = n;
        } else {
            p.others += n;
        }
    }
    let rows = values(conn, "tracks", "path");
    let by_key: HashMap<String, i64> = rows.iter().map(|(i, v)| (key(v), *i)).collect();
    for (i, v) in &rows {
        if !under(v, &old) {
            continue;
        }
        let q = moved(v, &old, &new);
        if Path::new(&q).exists() {
            p.found += 1;
        } else {
            p.absent.push(q.clone());
        }
        if by_key.get(&key(&q)).is_some_and(|j| j != i) {
            p.conflicts += 1;
        }
    }
    p
}

/// Remplace l'ancien début de chemin par le nouveau partout dans la base (et dans les réglages).
/// Rend (chemins changés, réglages changés, entrées doublées retirées).
pub fn relocate(conn: &Connection, old: &str, new: &str) -> Result<(usize, usize, usize), String> {
    let (old, new) = (clean(old), clean(new).replace('/', "\\"));
    if old.is_empty() || new.is_empty() || key(&old) == key(&new) {
        return Err("Ancien et nouvel emplacement identiques ou vides.".into());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    // Un scan fait après la copie a pu indexer les fichiers au nouvel emplacement : ces entrées-là
    // sont vides (ni analyse ni fiche), on les retire au profit des anciennes.
    let mut replaced = 0;
    for (t, c) in UNIQUE_COLUMNS {
        let rows = values(&tx, t, c);
        let moving: BTreeSet<String> = rows.iter().filter(|(_, v)| under(v, &old)).map(|(_, v)| key(&moved(v, &old, &new))).collect();
        let dup: Vec<i64> = rows.iter().filter(|(_, v)| !under(v, &old) && moving.contains(&key(v))).map(|(r, _)| *r).collect();
        for r in &dup {
            tx.execute(&format!("DELETE FROM {t} WHERE rowid = ?1"), [r]).map_err(|e| e.to_string())?;
        }
        if t == "tracks" {
            replaced = dup.len();
        }
    }
    let mut changed = 0;
    for (t, c) in columns(&tx) {
        for (r, v) in values(&tx, t, c) {
            if under(&v, &old) {
                tx.execute(&format!("UPDATE {t} SET {c} = ?1 WHERE rowid = ?2"), params![moved(&v, &old, &new), r]).map_err(|e| e.to_string())?;
                changed += 1;
            }
        }
    }
    // Réglages : dossier de sauvegarde, dossiers ignorés, import Suno, référence de mastering…
    let settings: Vec<(String, String)> = tx
        .prepare("SELECT key, value FROM settings WHERE value IS NOT NULL AND value <> ''")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .map_err(|e| e.to_string())?;
    let mut n = 0;
    for (k, v) in settings {
        let nv = replace_in_text(&v, &old, &new);
        if nv != v {
            tx.execute("UPDATE settings SET value = ?1 WHERE key = ?2", params![nv, k]).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok((changed, n, replaced))
}

/// `old` dans un réglage texte : seul, en début de valeur, dans une liste JSON ou « a|b » ; les
/// barres peuvent être « \\ » (JSON), « \ » ou « / ». Comme le motif de Studio Vault.
fn replace_in_text(text: &str, old: &str, new: &str) -> String {
    let parts: Vec<String> = old.split(['\\', '/']).map(str::to_lowercase).collect();
    let lower = text.to_lowercase();
    let (b, l) = (text.as_bytes(), lower.as_bytes());
    let mut out = String::new();
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        let starts = i == 0 || matches!(b[i - 1], b'"' | b'|');
        if starts {
            if let Some((end, json)) = match_parts(l, i, &parts) {
                let next_ok = end == b.len() || matches!(b[end], b'\\' | b'/' | b'"' | b'|');
                if next_ok {
                    out.push_str(&text[last..i]);
                    out.push_str(&if json { new.replace('\\', "\\\\") } else { new.to_string() });
                    i = end;
                    last = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.push_str(&text[last..]);
    out
}

/// Les morceaux du chemin à partir de `at`, séparés par « \\ », « \ » ou « / ». Rend la fin et
/// si les barres étaient doublées (JSON).
fn match_parts(l: &[u8], at: usize, parts: &[String]) -> Option<(usize, bool)> {
    let mut i = at;
    let mut json = false;
    for (k, p) in parts.iter().enumerate() {
        if k > 0 {
            if l.get(i..i + 2) == Some(b"\\\\") {
                json = true;
                i += 2;
            } else if matches!(l.get(i), Some(b'\\') | Some(b'/')) {
                i += 1;
            } else {
                return None;
            }
        }
        if l.get(i..i + p.len()) != Some(p.as_bytes()) {
            return None;
        }
        i += p.len();
    }
    Some((i, json))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE tracks (id INTEGER PRIMARY KEY, path TEXT UNIQUE, fingerprint TEXT, audio_fp TEXT, missing INTEGER DEFAULT 0, lufs REAL);
             CREATE TABLE watch_folders (id INTEGER PRIMARY KEY, path TEXT UNIQUE, enabled INTEGER DEFAULT 1);
             CREATE TABLE releases (id INTEGER PRIMARY KEY, cover_path TEXT);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);",
        )
        .unwrap();
        c
    }

    #[test]
    fn relocation_moves_every_path() {
        let c = db();
        c.execute_batch(
            r#"INSERT INTO tracks (id, path, lufs) VALUES (1, 'G:\Musique\A\a.wav', -9), (2, 'G:\Musique\B\b.wav', -8), (3, 'G:\MusiqueBis\c.wav', NULL),
                                                  (4, 'Z:\Drive\Musique\A\a.wav', NULL);
               INSERT INTO watch_folders (id, path) VALUES (1, 'G:\Musique');
               INSERT INTO releases (id, cover_path) VALUES (1, 'G:/Musique/A/cover.jpg');
               INSERT INTO settings (key, value) VALUES ('ignored_folders', '["G:\\Musique\\Old"]'), ('mastering_engine_dir', 'G:\Musique|x'), ('other', 'G:\MusiqueBis');"#,
        )
        .unwrap();
        // Comme Studio Vault : les plus fournis d'abord (le disque entier), puis le dossier surveillé.
        assert_eq!(suggestions(&c), vec![(r"G:\".to_string(), 3), (r"G:\Musique".to_string(), 2)]);
        let p = preview(&c, r"G:\Musique", r"Z:\Drive\Musique\");
        assert_eq!((p.tracks, p.others, p.conflicts, p.found), (2, 2, 1, 0));
        let (changed, settings, replaced) = relocate(&c, r"G:\Musique", r"Z:\Drive\Musique").unwrap();
        assert_eq!((changed, settings, replaced), (4, 2, 1));
        let path = |id: i64| c.query_row("SELECT path FROM tracks WHERE id = ?1", [id], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(path(1), r"Z:\Drive\Musique\A\a.wav");
        assert_eq!(path(3), r"G:\MusiqueBis\c.wav");
        // L'entrée vide indexée au nouvel endroit a laissé la place à l'ancienne (et son analyse).
        assert!(c.query_row("SELECT 1 FROM tracks WHERE id = 4", [], |_| Ok(())).is_err());
        let cover: String = c.query_row("SELECT cover_path FROM releases", [], |r| r.get(0)).unwrap();
        assert_eq!(cover, r"Z:\Drive\Musique/A/cover.jpg");
        let set = |k: &str| c.query_row("SELECT value FROM settings WHERE key = ?1", [k], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(set("ignored_folders"), r#"["Z:\\Drive\\Musique\\Old"]"#);
        assert_eq!(set("mastering_engine_dir"), r"Z:\Drive\Musique|x");
        assert_eq!(set("other"), r"G:\MusiqueBis");
        assert!(relocate(&c, r"Z:\a", r"z:/A/").is_err());
    }

    #[test]
    fn duplicate_folders() {
        let c = db();
        c.execute_batch(
            r#"INSERT INTO tracks (id, path, fingerprint) VALUES (1, 'C:\A\1.wav', 'x1'), (2, 'C:\A\2.wav', 'x2'), (3, 'C:\B\1.wav', 'x1'), (4, 'C:\B\2.wav', 'x2'),
                                                       (5, 'C:\B\3.wav', 'x3'), (6, 'C:\C\9.wav', 'x9'), (7, 'C:\C\8.wav', 'x1');"#,
        )
        .unwrap();
        let d = dupe_folders(&c);
        // A est entièrement dans B ; B (3 morceaux, 2 communs) n'est pas « en double » de A ; C non plus.
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].folder.as_str(), d[0].other.as_str(), d[0].n, d[0].common, d[0].ids.clone()), (r"C:\A", r"C:\B", 2, 2, vec![1, 2]));
        assert_eq!(ignore_folder(&c, r"C:\A").unwrap(), 2);
        assert!(super::super::scan::ignored_folders(&c).contains(&r"C:\A".to_string()));
    }
}
