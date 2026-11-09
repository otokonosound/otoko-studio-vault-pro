//! Le projet d'un morceau (FL Studio, Ableton…), noté dans sa fiche : le logiciel l'ouvre avec son programme.

use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension};

/// Le titre du morceau, et son projet s'il existe encore sur le disque.
pub fn project_of(conn: &Connection, id: i64) -> Option<(String, Option<PathBuf>)> {
    let (title, project): (String, String) = conn
        .query_row("SELECT COALESCE(NULLIF(title, ''), filename, ''), IFNULL(daw_project, '') FROM tracks WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .ok()
        .flatten()?;
    let path = PathBuf::from(project.trim());
    Some((title, path.is_file().then_some(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn un_projet_absent_du_disque_ne_s_ouvre_pas() {
        let dir = std::env::temp_dir().join(format!("otoko-projet-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let flp = dir.join("Nuit.flp");
        std::fs::write(&flp, b"x").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::career::schema::SCHEMA).unwrap();
        for (path, project) in [("C:/a.wav", flp.to_string_lossy().into_owned()), ("C:/b.wav", "C:/rien.flp".to_string())] {
            conn.execute("INSERT INTO tracks (path, filename, title, daw_project) VALUES (?1, ?1, 'Nuit', ?2)", params![path, project]).unwrap();
        }
        assert_eq!(project_of(&conn, 1), Some(("Nuit".to_string(), Some(flp))));
        assert_eq!(project_of(&conn, 2), Some(("Nuit".to_string(), None)));
        assert_eq!(project_of(&conn, 3), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
