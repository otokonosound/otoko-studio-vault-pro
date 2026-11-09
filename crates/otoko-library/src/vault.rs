//! Lecture de la base Studio Vault (ComposerVault), sans jamais la modifier : la base et son
//! journal (-wal) sont copiés dans un dossier temporaire et c'est la copie qu'on ouvre. SQLite
//! rejoue le journal dans la copie ; l'original et son -shm ne sont pas touchés.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

/// Le dossier de Studio Vault de l'utilisateur, s'il existe.
pub fn default_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("APPDATA")?).join("ComposerVault");
    dir.join("vault.db").exists().then_some(dir)
}

/// Copie la base (et son journal) et ouvre la copie en lecture seule.
pub fn snapshot(dir: &Path) -> Result<(Connection, PathBuf), String> {
    let src = dir.join("vault.db");
    if !src.exists() {
        return Err(format!("pas de base Studio Vault dans {}", dir.display()));
    }
    let tmp = std::env::temp_dir().join(format!("otoko-vault-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let dst = tmp.join("vault.db");
    std::fs::copy(&src, &dst).map_err(|e| format!("copie de la base : {e}"))?;
    let wal = dir.join("vault.db-wal");
    if wal.exists() {
        std::fs::copy(&wal, tmp.join("vault.db-wal")).map_err(|e| format!("copie du journal : {e}"))?;
    }
    // La copie s'ouvre en écriture le temps de rejouer le journal, puis on ne fait que lire.
    let conn = Connection::open_with_flags(&dst, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|e| e.to_string())?;
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok((conn, tmp))
}

/// Copie une autre base de Studio Vault (`partages.db`…) et son journal dans `tmp` ; rend le
/// chemin de la copie, ou rien si Studio Vault n'a pas cette base.
pub fn copy_db(dir: &Path, name: &str, tmp: &Path) -> Result<Option<PathBuf>, String> {
    let src = dir.join(name);
    if !src.exists() {
        return Ok(None);
    }
    let dst = tmp.join(name);
    std::fs::copy(&src, &dst).map_err(|e| format!("copie de {name} : {e}"))?;
    let wal = dir.join(format!("{name}-wal"));
    if wal.exists() {
        std::fs::copy(&wal, tmp.join(format!("{name}-wal"))).map_err(|e| format!("copie du journal de {name} : {e}"))?;
    }
    // Rejoue le journal dans la copie.
    let conn = Connection::open_with_flags(&dst, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|e| e.to_string())?;
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(Some(dst))
}

/// Les tables et leurs colonnes (pour l'import, et pour le diagnostic).
pub fn schema(conn: &Connection) -> Result<Vec<(String, Vec<String>, u64)>, String> {
    let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").map_err(|e| e.to_string())?;
    let names: Vec<String> = st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?.flatten().collect();
    let mut out = Vec::new();
    for n in names {
        let mut c = conn.prepare(&format!("PRAGMA table_info(\"{n}\")")).map_err(|e| e.to_string())?;
        let cols: Vec<String> = c.query_map([], |r| Ok(format!("{} {}", r.get::<_, String>(1)?, r.get::<_, String>(2)?))).map_err(|e| e.to_string())?.flatten().collect();
        let count: u64 = conn.query_row(&format!("SELECT COUNT(*) FROM \"{n}\""), [], |r| r.get(0)).unwrap_or(0);
        out.push((n, cols, count));
    }
    Ok(out)
}
