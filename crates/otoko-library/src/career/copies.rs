//! Le contrôle des copies : la base est sauvegardée chaque jour, pas la musique. Ce module dit quels morceaux n'existent
//! qu'à un seul endroit. Une copie est un fichier de même nom et de même taille, sur un autre disque que l'original :
//! rien n'est lu dans le fichier, le contrôle reste rapide même sur Google Drive.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{Connection, params};

use super::scan;

/// Les endroits où chercher (liste JSON).
pub const ROOTS_KEY: &str = "copies_roots";
/// Le dernier contrôle.
pub const LAST_KEY: &str = "copies_last";

pub fn roots(conn: &Connection) -> Vec<String> {
    let raw: String = conn.query_row("SELECT value FROM settings WHERE key = ?1", [ROOTS_KEY], |r| r.get(0)).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn set_roots(conn: &Connection, list: &[String]) -> rusqlite::Result<()> {
    conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", params![ROOTS_KEY, serde_json::to_string(list).unwrap_or_default()])?;
    Ok(())
}

/// Le disque d'un chemin (« z: »).
fn drive(p: &str) -> String {
    p.chars().take(2).collect::<String>().to_lowercase()
}

/// Les fichiers audio trouvés : (nom en minuscules, taille) → les disques où ils sont.
pub fn index(list: &[String], stop: &AtomicBool) -> HashMap<(String, u64), Vec<String>> {
    let mut out: HashMap<(String, u64), Vec<String>> = HashMap::new();
    for root in list {
        for e in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            if stop.load(Ordering::Relaxed) {
                return out;
            }
            if !e.file_type().is_file() || !scan::is_audio(e.path()) {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            let key = (e.file_name().to_string_lossy().to_lowercase(), meta.len());
            let d = drive(&e.path().to_string_lossy());
            let places = out.entry(key).or_default();
            if !places.contains(&d) {
                places.push(d);
            }
        }
    }
    out
}

/// Ce que le contrôle a trouvé.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Report {
    pub checked: usize,
    pub safe: usize,
    pub single: usize,
}

/// Compare la bibliothèque à l'index et note le résultat de chaque morceau.
pub fn record(conn: &Connection, found: &HashMap<(String, u64), Vec<String>>) -> rusqlite::Result<Report> {
    let rows: Vec<(i64, String, Option<i64>)> = conn
        .prepare("SELECT id, path, file_size FROM tracks WHERE missing = 0 AND COALESCE(archived, 0) = 0")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM track_copies", [])?;
    let mut rep = Report::default();
    for (id, path, size) in rows {
        let name = Path::new(&path.replace('/', "\\")).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let here = drive(&path);
        let copies = size.and_then(|s| found.get(&(name, s as u64))).map(|d| d.iter().filter(|x| **x != here).count()).unwrap_or(0);
        tx.execute("INSERT INTO track_copies (track_id, copies, checked_at) VALUES (?1, ?2, datetime('now', 'localtime'))", params![id, copies as i64])?;
        rep.checked += 1;
        if copies == 0 {
            rep.single += 1;
        } else {
            rep.safe += 1;
        }
    }
    tx.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, datetime('now', 'localtime'))", [LAST_KEY])?;
    tx.commit()?;
    Ok(rep)
}

/// La condition SQL des morceaux sans copie, pour la bibliothèque.
pub const SINGLE_SQL: &str = "id IN (SELECT track_id FROM track_copies WHERE copies = 0)";

/// Le dernier résultat : (morceaux sans copie, date du contrôle), s'il y en a eu un.
pub fn last(conn: &Connection) -> Option<(i64, String)> {
    let when: String = conn.query_row("SELECT value FROM settings WHERE key = ?1", [LAST_KEY], |r| r.get(0)).ok()?;
    let single = conn.query_row("SELECT COUNT(*) FROM track_copies c JOIN tracks t ON t.id = c.track_id WHERE c.copies = 0 AND t.missing = 0", [], |r| r.get(0)).unwrap_or(0);
    Some((single, when))
}

/// Le dernier contrôle date-t-il de plus d'une semaine ?
pub fn due(conn: &Connection) -> bool {
    match last(conn) {
        None => !roots(conn).is_empty(),
        Some((_, when)) => !roots(conn).is_empty() && chrono::NaiveDateTime::parse_from_str(&when, "%Y-%m-%d %H:%M:%S").map(|d| (chrono::Local::now().naive_local() - d).num_days() >= 7).unwrap_or(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_lives_on_another_disk() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, file_size) VALUES
                (1, 'Z:\\Drive\\a.wav', 'a.wav', 100),
                (2, 'Z:\\Drive\\b.wav', 'b.wav', 200),
                (3, 'Z:\\Drive\\c.wav', 'c.wav', 300);",
        )
        .unwrap();
        let mut found = HashMap::new();
        found.insert(("a.wav".to_string(), 100u64), vec!["z:".to_string(), "g:".to_string()]);
        found.insert(("b.wav".to_string(), 200u64), vec!["z:".to_string()]);
        found.insert(("c.wav".to_string(), 999u64), vec!["g:".to_string()]);
        let r = record(&conn, &found).unwrap();
        assert_eq!((r.checked, r.safe, r.single), (3, 1, 2), "même disque ou autre taille : ce n'est pas une copie");
        assert_eq!(last(&conn).map(|l| l.0), Some(2));
        assert!(!due(&conn), "sans emplacement choisi, rien à refaire");
    }

    #[test]
    fn the_index_walks_folders() {
        let dir = std::env::temp_dir().join(format!("otoko-copies-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sous")).unwrap();
        std::fs::write(dir.join("sous").join("A.wav"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("note.txt"), [0u8; 10]).unwrap();
        let i = index(&[dir.to_string_lossy().into_owned()], &AtomicBool::new(false));
        assert_eq!(i.len(), 1);
        assert!(i.contains_key(&("a.wav".to_string(), 10)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
