//! Les statuts automatiques : le tableau de production suit le travail. Un morceau n'avance que si les données le
//! prouvent, il ne recule jamais, et un statut choisi à la main n'est plus touché (`field_locks`).

use std::path::Path;

use rusqlite::{Connection, params};

use super::status_label;

/// Le réglage (vrai par défaut).
pub const FLAG: &str = "auto_status";

fn rank(s: &str) -> usize {
    super::STATUSES.iter().position(|x| x.0 == s).unwrap_or(0)
}

/// Un morceau qui change de statut.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub id: i64,
    pub title: String,
    pub to: &'static str,
    pub why: &'static str,
}

impl Change {
    pub fn text(&self) -> String {
        format!("« {} » → {} ({})", self.title, status_label(self.to), self.why)
    }
}

/// Le statut est-il verrouillé par un choix à la main ?
fn locked(conn: &Connection, id: i64) -> bool {
    conn.query_row("SELECT 1 FROM field_locks WHERE track_id = ?1 AND field = 'status'", [id], |_| Ok(())).is_ok()
}

/// Un nouvel export du morceau : il passe « En cours » s'il était encore une idée.
pub fn touched(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    if !locked(conn, id) && super::library::flag(conn, FLAG, true) {
        conn.execute("UPDATE tracks SET status = 'wip', modified_at = datetime('now') WHERE id = ?1 AND status IN ('idea', 'draft')", [id])?;
    }
    Ok(())
}

/// Le projet relié a-t-il bougé ces sept derniers jours ?
fn project_moved(path: &str) -> bool {
    let p = Path::new(path);
    if path.is_empty() || !p.is_file() {
        return false;
    }
    std::fs::metadata(p).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() < 7 * 86_400)
}

/// Ce qui changerait, d'après la base.
pub fn plan(conn: &Connection) -> Vec<Change> {
    struct R {
        id: i64,
        title: String,
        status: String,
        linked: bool,
        master: bool,
        out: bool,
        recent: bool,
        project: String,
    }
    let rows: Vec<R> = conn
        .prepare(
            "SELECT t.id, COALESCE(NULLIF(t.title, ''), t.filename), IFNULL(t.status, 'idea'),
                    IFNULL(t.sc_url, '') <> '' OR IFNULL(t.published_at, '') <> '',
                    IFNULL(t.version_label, '') = 'master' OR EXISTS (SELECT 1 FROM tracks m WHERE m.parent_id = t.id AND m.version_label = 'master' AND m.missing = 0),
                    EXISTS (SELECT 1 FROM release_tracks rt JOIN releases r ON r.id = rt.release_id
                            WHERE rt.track_id = t.id AND IFNULL(r.sc_link, '') <> '' AND r.release_date <> '' AND r.release_date <= date('now', 'localtime')),
                    EXISTS (SELECT 1 FROM tracks v WHERE v.parent_id = t.id AND v.role = 'version' AND v.added_at > datetime('now', '-7 days')),
                    IFNULL(t.daw_project, '')
             FROM tracks t
             WHERE t.missing = 0 AND COALESCE(t.archived, 0) = 0 AND t.parent_id IS NULL AND IFNULL(t.role, 'mixdown') IN ('mixdown', 'render')
               AND NOT EXISTS (SELECT 1 FROM field_locks l WHERE l.track_id = t.id AND l.field = 'status')",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok(R { id: r.get(0)?, title: r.get(1)?, status: r.get(2)?, linked: r.get(3)?, master: r.get(4)?, out: r.get(5)?, recent: r.get(6)?, project: r.get(7)? })
            })?
            .collect()
        })
        .unwrap_or_default();
    let mut out = Vec::new();
    for r in rows {
        let cur = rank(&r.status);
        let (to, why) = if r.linked || r.out {
            ("released", if r.linked { "il a un lien" } else { "sa sortie est en ligne" })
        } else if r.master {
            ("done", "son master est là")
        } else if r.recent {
            ("wip", "nouvel export cette semaine")
        } else if project_moved(&r.project) {
            ("wip", "projet modifié cette semaine")
        } else {
            continue;
        };
        if rank(to) > cur {
            out.push(Change { id: r.id, title: r.title, to, why });
        }
    }
    out
}

/// Applique le plan ; rend ce qui a changé.
pub fn apply(conn: &Connection) -> Vec<Change> {
    if !super::library::flag(conn, FLAG, true) {
        return Vec::new();
    }
    let list = plan(conn);
    for c in &list {
        let _ = conn.execute("UPDATE tracks SET status = ?2, modified_at = datetime('now') WHERE id = ?1", params![c.id, c.to]);
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, title, status, parent_id, version_label, role, added_at, sc_url) VALUES
                (1, 'a', 'a', 'Avec master', 'idea', NULL, '', 'mixdown', '2020-01-01', ''),
                (2, 'b', 'b', 'Le master', 'done', 1, 'master', 'version', '2020-01-01', ''),
                (3, 'c', 'c', 'Avec lien', 'done', NULL, '', 'mixdown', '2020-01-01', 'https://x'),
                (4, 'd', 'd', 'Nouvel export', 'idea', NULL, '', 'mixdown', '2020-01-01', ''),
                (5, 'e', 'e', 'L''export', 'idea', 4, 'v2', 'version', datetime('now'), ''),
                (6, 'f', 'f', 'En ligne', 'done', NULL, '', 'mixdown', '2020-01-01', ''),
                (7, 'g', 'g', 'Rien', 'idea', NULL, '', 'mixdown', '2020-01-01', ''),
                (8, 'h', 'h', 'Choisi à la main', 'idea', NULL, 'master', 'mixdown', '2020-01-01', '');
             INSERT INTO releases (id, title, release_date, sc_link) VALUES (1, 'Sortie', '2020-02-01', 'https://x/set');
             INSERT INTO release_tracks (release_id, track_id, position) VALUES (1, 6, 1);
             INSERT INTO field_locks (track_id, field) VALUES (8, 'status');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn tracks_move_forward_on_proof() {
        let conn = base();
        let list = apply(&conn);
        let to = |id: i64| list.iter().find(|c| c.id == id).map(|c| c.to);
        assert_eq!(to(1), Some("done"));
        assert_eq!(to(3), Some("released"));
        assert_eq!(to(4), Some("wip"));
        assert_eq!(to(6), Some("released"));
        assert_eq!(to(7), None, "rien ne prouve qu'il avance");
        assert_eq!(to(8), None, "un choix à la main n'est plus touché");
        assert!(apply(&conn).is_empty(), "un second passage ne change rien");
    }

    #[test]
    fn nothing_goes_back_and_the_setting_stops_it() {
        let conn = base();
        conn.execute("UPDATE tracks SET status = 'released' WHERE id = 1", []).unwrap();
        assert!(plan(&conn).iter().all(|c| c.id != 1));
        conn.execute("INSERT INTO settings (key, value) VALUES ('auto_status', '0')", []).unwrap();
        assert!(apply(&conn).is_empty());
    }
}
