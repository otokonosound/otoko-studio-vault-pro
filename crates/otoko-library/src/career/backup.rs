//! Sauvegarde de la Carrière (comme `utils/backup.py` de Studio Vault) : une copie par jour, les
//! 15 dernières gardées, et une seconde copie dans un autre dossier (Google Drive…), les 10
//! dernières, pour ne rien perdre si l'ordinateur lâche.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

pub const KEEP: usize = 15;
pub const KEEP_EXTRA: usize = 10;
pub const EXTRA_SETTING: &str = "backup_extra_dir";

/// Les sauvegardes d'un dossier, de la plus récente à la plus ancienne.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("carriere-") && n.ends_with(".db")))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

fn prune(dir: &Path, keep: usize) {
    for old in list(dir).into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
}

/// Copie une sauvegarde dans le second dossier.
pub fn copy_to_extra(src: &Path, extra: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(extra).map_err(|e| format!("{} : {e}", extra.display()))?;
    let dst = extra.join(src.file_name().unwrap_or_default());
    std::fs::copy(src, &dst).map_err(|e| format!("copie vers {} : {e}", extra.display()))?;
    prune(extra, KEEP_EXTRA);
    Ok(dst)
}

/// La date et l'heure, pour nommer une sauvegarde.
pub fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Copie cohérente de la base ouverte, maintenant.
pub fn now(conn: &Connection, dir: &Path, extra: Option<&Path>) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dst = dir.join(format!("carriere-{}.db", stamp()));
    conn.execute("VACUUM INTO ?1", [dst.to_string_lossy()]).map_err(|e| format!("sauvegarde : {e}"))?;
    prune(dir, KEEP);
    if let Some(x) = extra {
        copy_to_extra(&dst, x)?;
    }
    Ok(dst)
}

/// Sauvegarde si aucune n'a été faite aujourd'hui (et vérifie que la copie est partie).
pub fn daily(conn: &Connection, dir: &Path, extra: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let today = format!("carriere-{}-", chrono::Local::now().format("%Y%m%d"));
    if let Some(done) = list(dir).into_iter().find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&today))) {
        if let Some(x) = extra {
            if !x.join(done.file_name().unwrap_or_default()).exists() {
                copy_to_extra(&done, x)?;
            }
        }
        return Ok(None);
    }
    now(conn, dir, extra).map(Some)
}

/// Ce que contient une sauvegarde (morceaux, sorties), si c'est bien une base saine de ce logiciel.
pub fn inspect(path: &Path) -> Result<(i64, i64), String> {
    let not_ours = || "Ce fichier n'est pas une sauvegarde d'Otoko Studio Vault Pro.".to_string();
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|_| not_ours())?;
    let check: String = c.query_row("PRAGMA quick_check(1)", [], |r| r.get(0)).map_err(|_| not_ours())?;
    if check != "ok" {
        return Err("Cette sauvegarde est abîmée : choisis-en une autre.".into());
    }
    let count = |table: &str| c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).map_err(|_| not_ours());
    Ok((count("tracks")?, count("releases")?))
}

/// Met la base de côté, avec ses fichiers d'accompagnement. Windows refuse si un autre programme l'a encore ouverte (le
/// serveur mobile qui prépare ses fichiers, par exemple) : on réessaie quelques secondes, puis rien n'a changé.
fn set_aside(db: &Path) -> Result<PathBuf, String> {
    let old = db.with_extension("remplacee");
    let _ = std::fs::remove_file(&old);
    if db.exists() {
        let mut tries = 0;
        while let Err(e) = std::fs::rename(db, &old) {
            tries += 1;
            if tries >= 25 {
                return Err(format!("La base est encore utilisée par un autre programme : réessaie dans une minute ({e})."));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    for side in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{side}", db.display()));
    }
    Ok(old)
}

/// Remplace la base (refermée par l'appelant) par une sauvegarde. Si quelque chose échoue, la base reste celle d'avant.
pub fn restore(db: &Path, src: &Path) -> Result<(), String> {
    inspect(src)?;
    let incoming = db.with_extension("restauration");
    std::fs::copy(src, &incoming).map_err(|e| format!("Restauration impossible : {e}"))?;
    let old = set_aside(db).inspect_err(|_| {
        let _ = std::fs::remove_file(&incoming);
    })?;
    if let Err(e) = std::fs::rename(&incoming, db) {
        let _ = std::fs::rename(&old, db);
        let _ = std::fs::remove_file(&incoming);
        return Err(format!("Restauration impossible : {e}"));
    }
    let _ = std::fs::remove_file(&old);
    Ok(())
}

/// Retire la base (refermée par l'appelant) : le prochain lancement en crée une neuve.
pub fn clear(db: &Path) -> Result<(), String> {
    let old = set_aside(db)?;
    let _ = std::fs::remove_file(old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backup_comes_back_and_a_base_starts_over() {
        let root = std::env::temp_dir().join(format!("otoko-restauration-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (db, saved) = (root.join("carriere.db"), root.join("sauvegardes"));
        let c = Connection::open(&db).unwrap();
        c.execute_batch("PRAGMA journal_mode = WAL; CREATE TABLE tracks (id); CREATE TABLE releases (id); INSERT INTO tracks VALUES (1), (2); INSERT INTO releases VALUES (1);").unwrap();
        let backup = now(&c, &saved, None).unwrap();
        c.execute_batch("DELETE FROM tracks;").unwrap();
        assert_eq!(inspect(&backup).unwrap(), (2, 1));
        // Encore ouverte (par un autre programme, par exemple) : refus, et la base n'a pas bougé.
        // (L'attente de quelques secondes est le comportement voulu.)
        assert!(restore(&db, &backup).is_err(), "base encore ouverte");
        assert_eq!(c.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        drop(c);
        // Un fichier qui n'en est pas une, ou abîmé : refusé.
        let bad = root.join("autre.db");
        std::fs::write(&bad, b"pas une base").unwrap();
        assert!(inspect(&bad).is_err());
        assert!(restore(&db, &bad).is_err());
        assert!(db.exists(), "la base reste en place");
        let empty = root.join("vide.db");
        Connection::open(&empty).unwrap().execute_batch("CREATE TABLE autre (x);").unwrap();
        assert!(inspect(&empty).is_err(), "une base sans morceaux n'est pas la nôtre");
        // La vraie restauration.
        restore(&db, &backup).unwrap();
        assert_eq!(Connection::open(&db).unwrap().query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert!(!db.with_extension("remplacee").exists() && !db.with_extension("restauration").exists(), "rien ne traîne");
        // Repartir de zéro.
        clear(&db).unwrap();
        assert!(!db.exists());
        assert!(backup.exists(), "la sauvegarde reste");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn daily_then_pruned() {
        let root = std::env::temp_dir().join(format!("otoko-sauvegardes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (dir, extra) = (root.join("ici"), root.join("drive"));
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (x); INSERT INTO t VALUES (42);").unwrap();
        let first = daily(&conn, &dir, Some(&extra)).unwrap().unwrap();
        assert!(daily(&conn, &dir, Some(&extra)).unwrap().is_none(), "une seule par jour");
        assert!(extra.join(first.file_name().unwrap()).exists());
        let back = Connection::open(&first).unwrap();
        assert_eq!(back.query_row("SELECT x FROM t", [], |r| r.get::<_, i64>(0)).unwrap(), 42);
        drop(back);
        for k in 0..20 {
            std::fs::write(dir.join(format!("carriere-20200101-0000{k:02}.db")), b"").unwrap();
        }
        prune(&dir, KEEP);
        assert_eq!(list(&dir).len(), KEEP);
        assert_eq!(list(&dir)[0], first, "la plus récente reste");
        let _ = std::fs::remove_dir_all(&root);
    }
}
