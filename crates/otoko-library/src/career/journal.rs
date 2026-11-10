//! L'historique des fiches : ce qui a changé sur un morceau, quand, par quoi, et l'annulation.
//!
//! Une opération faite à la main s'enregistre entre `record` (ou `begin`) et la fin : des déclencheurs SQLite, posés sur la
//! connexion seulement (TEMP), notent chaque champ suivi qui change, ses tags et ses verrous. Hors de ces moments (analyse,
//! scan, travaux en fond), rien n'est noté. Les frappes successives dans un même champ forment un seul lot (`grp`), annulé
//! d'un coup. `undo` ne remet un champ que s'il a gardé la valeur écrite : ce qui a changé depuis n'est pas écrasé.

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};

use super::{NATURES, STATUSES};

/// Les champs suivis, avec leur nom à l'écran. Ni le chemin (les déplacements ont leur propre annulation, `files`), ni
/// l'analyse (sonie, énergie…), ni les écoutes.
pub const FIELDS: [(&str, &str); 26] = [
    ("title", "Titre"),
    ("artist", "Artiste"),
    ("album", "Album"),
    ("genre", "Genre"),
    ("year", "Année"),
    ("track_no", "N° de piste"),
    ("bpm", "Tempo"),
    ("music_key", "Tonalité"),
    ("mood_auto", "Ambiance"),
    ("status", "Statut"),
    ("nature", "Nature"),
    ("nature_meta", "Fiche projet"),
    ("role", "Type de fichier"),
    ("version_label", "Version"),
    ("notes", "Notes"),
    ("sc_url", "Lien SoundCloud"),
    ("published_at", "Publié le"),
    ("release_type", "Type de sortie"),
    ("to_publish", "À publier"),
    ("daw_project", "Projet"),
    ("parent_id", "Rattaché à"),
    ("pick", "Choix"),
    ("rating", "Note"),
    ("archived", "Archivé"),
    ("isrc", "ISRC"),
    ("cover_path", "Pochette"),
];

/// D'où vient une modification.
pub const PC: &str = "pc";
pub const PHONE: &str = "téléphone";
pub const AUTO: &str = "auto";

/// Au-delà, les plus anciennes lignes s'effacent.
const KEEP: i64 = 50_000;

/// Les déclencheurs de cette connexion (une fois par connexion ; les colonnes absentes d'une vieille base sont sautées).
pub fn install(conn: &Connection) -> rusqlite::Result<()> {
    let here = conn.query_row("SELECT 1 FROM temp.sqlite_master WHERE type = 'trigger' AND name = 'journal_tracks'", [], |_| Ok(())).optional()?.is_some();
    if here {
        return Ok(());
    }
    conn.execute_batch(super::schema::JOURNAL)?;
    let cols: Vec<String> = conn.prepare("PRAGMA main.table_info(tracks)")?.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<_>>()?;
    let watched: Vec<&str> = FIELDS.iter().map(|f| f.0).filter(|f| cols.iter().any(|c| c == f)).collect();
    let rows: Vec<String> = watched.iter().map(|f| format!("SELECT '{f}' AS field, OLD.{f} AS o, NEW.{f} AS n WHERE OLD.{f} IS NOT NEW.{f}")).collect();
    let ctx = "SELECT c.batch_id, c.label, c.source, c.grp";
    let insert = "INSERT INTO main.edit_log (batch_id, label, source, grp, track_id, field, old_value, new_value)";
    let on = "WHEN EXISTS (SELECT 1 FROM temp.journal_ctx)";
    conn.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS journal_ctx (batch_id TEXT, label TEXT, source TEXT, grp TEXT);
         CREATE TEMP TRIGGER IF NOT EXISTS journal_tracks AFTER UPDATE OF {cols} ON main.tracks {on} BEGIN
            {insert} {ctx}, NEW.id, f.field, f.o, f.n FROM temp.journal_ctx c JOIN ({rows}) f;
         END;
         CREATE TEMP TRIGGER IF NOT EXISTS journal_tag_on AFTER INSERT ON main.track_tags {on} BEGIN
            {insert} {ctx}, NEW.track_id, 'tag', NULL, (SELECT name FROM main.tags WHERE id = NEW.tag_id) FROM temp.journal_ctx c;
         END;
         CREATE TEMP TRIGGER IF NOT EXISTS journal_tag_off AFTER DELETE ON main.track_tags {on} BEGIN
            {insert} {ctx}, OLD.track_id, 'tag', (SELECT name FROM main.tags WHERE id = OLD.tag_id), NULL FROM temp.journal_ctx c;
         END;
         CREATE TEMP TRIGGER IF NOT EXISTS journal_lock_on AFTER INSERT ON main.field_locks {on} BEGIN
            {insert} {ctx}, NEW.track_id, 'lock:' || NEW.field, NULL, 1 FROM temp.journal_ctx c;
         END;
         CREATE TEMP TRIGGER IF NOT EXISTS journal_lock_off AFTER DELETE ON main.field_locks {on} BEGIN
            {insert} {ctx}, OLD.track_id, 'lock:' || OLD.field, 1, NULL FROM temp.journal_ctx c;
         END;",
        cols = watched.join(", "),
        rows = rows.join(" UNION ALL "),
    ))
}

/// Commence à noter. `grp` non vide : la suite d'une même saisie (même champ, même morceau) rejoint le lot précédent s'il
/// date de moins de 90 s et n'a pas été annulé.
pub fn begin(conn: &Connection, label: &str, source: &str, grp: &str) -> rusqlite::Result<()> {
    install(conn)?;
    let again: Option<String> = if grp.is_empty() {
        None
    } else {
        conn.query_row(
            "SELECT batch_id FROM edit_log WHERE id = (SELECT MAX(id) FROM edit_log) AND grp = ?1 AND source = ?2 AND undone = 0
                AND at > strftime('%Y-%m-%d %H:%M:%f', 'now', '-90 seconds')",
            params![grp, source],
            |r| r.get(0),
        )
        .optional()?
    };
    let batch = match again {
        Some(b) => b,
        None => conn.query_row("SELECT lower(hex(randomblob(8)))", [], |r| r.get::<_, String>(0))?,
    };
    conn.execute("DELETE FROM temp.journal_ctx", [])?;
    conn.execute("INSERT INTO temp.journal_ctx (batch_id, label, source, grp) VALUES (?1, ?2, ?3, ?4)", params![batch, label, source, grp])?;
    Ok(())
}

/// Arrête de noter (et fait un peu de place de temps en temps).
pub fn end(conn: &Connection) {
    let _ = conn.execute("DELETE FROM temp.journal_ctx", []);
    let _ = conn.execute("DELETE FROM edit_log WHERE id <= (SELECT MAX(id) FROM edit_log) - ?1", [KEEP]);
}

/// `f` est noté sous ce libellé (depuis le PC).
pub fn record<T>(conn: &Connection, label: &str, grp: &str, f: impl FnOnce() -> T) -> T {
    record_as(conn, label, PC, grp, f)
}

pub fn record_as<T>(conn: &Connection, label: &str, source: &str, grp: &str, f: impl FnOnce() -> T) -> T {
    struct Stop<'a>(&'a Connection);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            end(self.0);
        }
    }
    let on = begin(conn, label, source, grp).is_ok();
    let _stop = on.then(|| Stop(conn));
    f()
}

/// Le dernier lot annulable d'une source : (lot, libellé, morceaux touchés, quand, en UTC).
#[derive(Clone, Debug, PartialEq)]
pub struct Last {
    pub batch: String,
    pub label: String,
    pub tracks: i64,
    pub at: String,
}

pub fn last(conn: &Connection, source: &str) -> Option<Last> {
    conn.query_row(
        "SELECT batch_id, label, COUNT(DISTINCT track_id), MAX(at) FROM edit_log
         WHERE batch_id = (SELECT batch_id FROM edit_log WHERE source = ?1 AND undone = 0 ORDER BY id DESC LIMIT 1)",
        [source],
        |r| Ok(Last { batch: r.get(0)?, label: r.get(1)?, tracks: r.get(2)?, at: r.get(3)? }),
    )
    .optional()
    .ok()
    .flatten()
}

fn column(field: &str) -> Option<&'static str> {
    FIELDS.iter().map(|f| f.0).find(|f| *f == field)
}

/// Remet une ligne : rend vrai si elle a pu l'être (le champ avait toujours la valeur écrite).
fn put_back(conn: &Connection, track: i64, field: &str, old: &Value, new: &Value) -> rusqlite::Result<bool> {
    if let Some(col) = column(field) {
        return Ok(conn.execute(&format!("UPDATE tracks SET {col} = ?2 WHERE id = ?1 AND {col} IS ?3"), params![track, old, new])? > 0);
    }
    let text = |v: &Value| if let Value::Text(t) = v { Some(t.clone()) } else { None };
    if field == "tag" {
        return Ok(match (text(old), text(new)) {
            (None, Some(added)) => conn.execute("DELETE FROM track_tags WHERE track_id = ?1 AND tag_id = (SELECT id FROM tags WHERE name = ?2)", params![track, added])? > 0,
            (Some(removed), None) => {
                conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [&removed])?;
                conn.execute("INSERT OR IGNORE INTO track_tags (track_id, tag_id) SELECT ?1, id FROM tags WHERE name = ?2", params![track, removed])? > 0
            }
            _ => false,
        });
    }
    if let Some(f) = field.strip_prefix("lock:") {
        return Ok(if *old == Value::Null {
            conn.execute("DELETE FROM field_locks WHERE track_id = ?1 AND field = ?2", params![track, f])? > 0
        } else {
            conn.execute("INSERT OR IGNORE INTO field_locks (track_id, field) VALUES (?1, ?2)", params![track, f])? > 0
        });
    }
    Ok(false)
}

/// Annule un lot, de la dernière ligne à la première. Rend (champs remis, champs laissés parce qu'ils ont changé depuis).
pub fn undo(conn: &Connection, batch: &str) -> rusqlite::Result<(usize, usize)> {
    end(conn);
    let rows: Vec<(i64, i64, String, Value, Value)> = conn
        .prepare("SELECT id, track_id, field, old_value, new_value FROM edit_log WHERE batch_id = ?1 AND undone = 0 ORDER BY id DESC")?
        .query_map([batch], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let tx = conn.unchecked_transaction()?;
    let (mut ok, mut kept) = (0, 0);
    for (_, track, field, old, new) in &rows {
        let done = put_back(&tx, *track, field, old, new)?;
        if !field.starts_with("lock:") {
            if done { ok += 1 } else { kept += 1 }
        }
    }
    tx.execute("UPDATE edit_log SET undone = 1 WHERE batch_id = ?1", [batch])?;
    tx.commit()?;
    Ok((ok, kept))
}

/// Une ligne de l'historique d'un morceau : un champ changé par un lot, de sa première valeur à sa dernière.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub batch: String,
    pub field: String,
    /// « Genre », « Tags »…
    pub name: String,
    pub before: String,
    pub after: String,
    /// La ligne à afficher : « Genre : House → Deep House », « Tags : + club · − nuit ».
    pub text: String,
    /// Ce qui l'a fait : « Modification », « Genre et ambiance », « Depuis le téléphone »…
    pub label: String,
    pub source: String,
    /// « aujourd'hui à 14:03 », « le 08/11 à 21:40 ».
    pub when: String,
    pub undone: bool,
}

fn shown(conn: &Connection, field: &str, v: &Value) -> String {
    let raw = match v {
        Value::Null => String::new(),
        Value::Integer(i) => i.to_string(),
        Value::Real(r) => {
            let s = format!("{r:.1}");
            s.strip_suffix(".0").unwrap_or(&s).replace('.', ",")
        }
        Value::Text(t) => t.trim().to_string(),
        Value::Blob(_) => "…".into(),
    };
    let label = |list: &[(&str, &str)]| list.iter().find(|x| x.0 == raw).map(|x| x.1.to_string());
    let out = match field {
        _ if raw.is_empty() && !matches!(field, "pick" | "archived" | "to_publish") => return "(vide)".into(),
        "status" => label(&STATUSES).unwrap_or(raw),
        "release_type" => super::release_label(&raw),
        "nature" => NATURES.iter().find(|x| x.0 == raw).map(|x| x.1.to_string()).unwrap_or(raw),
        "role" => super::library::role_label(&raw),
        "pick" => match raw.as_str() {
            "1" => "Gardé".into(),
            "-1" => "Rejeté".into(),
            _ => "Neutre".into(),
        },
        "archived" | "to_publish" => if raw == "1" { "oui" } else { "non" }.into(),
        "parent_id" => conn.query_row("SELECT COALESCE(NULLIF(title, ''), filename) FROM tracks WHERE id = ?1", [&raw], |r| r.get(0)).unwrap_or(raw),
        "nature_meta" => "modifiée".into(),
        _ => raw,
    };
    let short: String = out.chars().take(60).collect();
    if short.len() < out.len() { format!("{short}…") } else { short }
}

fn when(conn: &Connection, at: &str) -> String {
    conn.query_row(
        "SELECT CASE WHEN date(?1, 'localtime') = date('now', 'localtime') THEN 'aujourd''hui à ' || strftime('%H:%M', ?1, 'localtime')
                     WHEN date(?1, 'localtime') = date('now', 'localtime', '-1 day') THEN 'hier à ' || strftime('%H:%M', ?1, 'localtime')
                     ELSE 'le ' || strftime('%d/%m/%Y à %H:%M', ?1, 'localtime') END",
        [at],
        |r| r.get(0),
    )
    .unwrap_or_default()
}

/// L'historique d'un morceau, le plus récent d'abord (au plus `limit` lignes). Les frappes d'une même saisie sont réunies, les
/// tags ajoutés puis retirés dans le même lot disparaissent, un verrou ne s'affiche que seul (« Rendu à l'analyse »), pas à côté
/// de la correction qui l'a posé.
pub fn history(conn: &Connection, track: i64, limit: usize) -> Vec<Entry> {
    let rows: Vec<(String, String, Value, Value, String, String, String, bool)> = conn
        .prepare(
            "SELECT batch_id, field, old_value, new_value, label, source, at, undone FROM edit_log
             WHERE track_id = ?1 ORDER BY id DESC LIMIT 2000",
        )
        .and_then(|mut s| s.query_map([track], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get::<_, i64>(7)? != 0)))?.collect())
        .unwrap_or_default();
    // (lot, champ) → (première valeur, dernière valeur, ligne la plus récente). Les lignes arrivent de la plus récente à la plus ancienne.
    let mut groups: Vec<(String, String, Value, Value, usize)> = Vec::new();
    // Lot → mouvements des tags (nom, ajouté ?), du plus récent au plus ancien, et sa ligne la plus récente.
    let mut tags: Vec<(String, Vec<(String, bool)>, usize)> = Vec::new();
    for (i, (batch, field, old, new, ..)) in rows.iter().enumerate() {
        if field == "tag" {
            let moved = match (old, new) {
                (Value::Null, Value::Text(t)) => (t.clone(), true),
                (Value::Text(t), Value::Null) => (t.clone(), false),
                _ => continue,
            };
            match tags.iter_mut().find(|t| t.0 == *batch) {
                Some(t) => t.1.push(moved),
                None => tags.push((batch.clone(), vec![moved], i)),
            }
            continue;
        }
        match groups.iter_mut().find(|g| g.0 == *batch && g.1 == *field) {
            Some(g) => g.2 = old.clone(),
            None => groups.push((batch.clone(), field.clone(), old.clone(), new.clone(), i)),
        }
    }
    let mut out: Vec<(usize, Entry)> = Vec::new();
    let meta = |i: usize| {
        let r = &rows[i];
        (r.4.clone(), r.5.clone(), when(conn, &r.6), r.7)
    };
    let name_of = |f: &str| FIELDS.iter().find(|x| x.0 == f).map(|x| x.1).unwrap_or("Champ").to_string();
    for (batch, field, first, last, i) in groups.iter().cloned() {
        if first == last {
            continue;
        }
        let (label, source, when, undone) = meta(i);
        let (name, before, after, text) = match field.strip_prefix("lock:") {
            // Le verrou va avec la correction du même lot : il ne se montre que seul.
            Some(f) if groups.iter().any(|g| g.0 == batch && g.1 == f) => continue,
            Some(f) => {
                let name = name_of(f);
                let text = if last == Value::Null { format!("{name} : rendu à l'analyse") } else { format!("{name} : gardé contre l'analyse") };
                (name, String::new(), String::new(), text)
            }
            None => {
                let name = name_of(&field);
                let (before, after) = (shown(conn, &field, &first), shown(conn, &field, &last));
                let text = if field == "nature_meta" { format!("{name} modifiée") } else { format!("{name} : {before} → {after}") };
                (name, before, after, text)
            }
        };
        out.push((i, Entry { batch, field, name, before, after, text, label, source, when, undone }));
    }
    for (batch, moves, i) in tags {
        // Du plus ancien au plus récent : le premier mouvement d'un tag dit s'il était là avant, le dernier s'il y est après.
        let mut state: Vec<(String, bool, bool)> = Vec::new();
        for (name, added) in moves.iter().rev() {
            match state.iter_mut().find(|s| s.0 == *name) {
                Some(s) => s.2 = *added,
                None => state.push((name.clone(), !added, *added)),
            }
        }
        let added: Vec<String> = state.iter().filter(|s| !s.1 && s.2).map(|s| s.0.clone()).collect();
        let removed: Vec<String> = state.iter().filter(|s| s.1 && !s.2).map(|s| s.0.clone()).collect();
        if added.is_empty() && removed.is_empty() {
            continue;
        }
        let (label, source, when, undone) = meta(i);
        let (before, after) = (removed.join(", "), added.join(", "));
        let mut bits = Vec::new();
        if !after.is_empty() {
            bits.push(format!("+ {after}"));
        }
        if !before.is_empty() {
            bits.push(format!("− {before}"));
        }
        let text = format!("Tags : {}", bits.join(" · "));
        out.push((i, Entry { batch, field: "tag".into(), name: "Tags".into(), before, after, text, label, source, when, undone }));
    }
    out.sort_by_key(|e| e.0);
    out.into_iter().take(limit).map(|e| e.1).collect()
}

/// Remet un champ d'un morceau comme avant ce lot (une nouvelle modification, elle-même annulable). Rend le nom du champ.
pub fn restore(conn: &Connection, track: i64, batch: &str, field: &str) -> Result<String, String> {
    let rows: Vec<(Value, Value)> = conn
        .prepare("SELECT old_value, new_value FROM edit_log WHERE track_id = ?1 AND batch_id = ?2 AND field = ?3 ORDER BY id DESC")
        .and_then(|mut s| s.query_map(params![track, batch, field], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Err("Cette modification n'est plus dans l'historique.".into());
    }
    let key = field.strip_prefix("lock:").unwrap_or(field);
    let name = FIELDS.iter().find(|f| f.0 == key).map(|f| f.1).unwrap_or("Tags").to_string();
    record(conn, &format!("{name} remis comme avant"), "", || -> rusqlite::Result<()> {
        if let Some(col) = column(field) {
            let first = &rows.last().unwrap().0;
            conn.execute(&format!("UPDATE tracks SET {col} = ?2, modified_at = datetime('now') WHERE id = ?1"), params![track, first])?;
        } else {
            // Tags et verrous : chaque mouvement défait, du plus récent au plus ancien, sans exiger que le reste n'ait pas bougé.
            for (old, new) in &rows {
                put_back(conn, track, field, old, new)?;
            }
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, title, genre, bpm, status) VALUES
                (1, 'a.wav', 'a.wav', 'Nuit', 'House', 120.0, 'idea'),
                (2, 'b.wav', 'b.wav', 'Jour', '', NULL, 'wip');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn only_what_is_recorded_is_noted() {
        let conn = base();
        // Hors d'un enregistrement (analyse, scan…) : rien.
        install(&conn).unwrap();
        conn.execute("UPDATE tracks SET bpm = 124 WHERE id = 1", []).unwrap();
        let n = |c: &Connection| c.query_row("SELECT COUNT(*) FROM edit_log", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(n(&conn), 0);
        // Une édition en lot : genre et statut sur deux morceaux, un seul lot.
        record(&conn, "Édition en lot", "", || {
            for id in [1, 2] {
                super::super::library::update(&conn, id, "genre", "Deep House").unwrap();
                super::super::library::update(&conn, id, "status", "done").unwrap();
            }
        });
        assert_eq!(n(&conn), 6, "genre, statut et verrou du statut, pour chacun des deux");
        let last = last(&conn, PC).unwrap();
        assert_eq!((last.label.as_str(), last.tracks), ("Édition en lot", 2));
        // Un changement fait depuis n'est pas écrasé par l'annulation.
        conn.execute("UPDATE tracks SET genre = 'Techno' WHERE id = 2", []).unwrap();
        assert_eq!(undo(&conn, &last.batch).unwrap(), (3, 1));
        let row = |id: i64| conn.query_row("SELECT genre, status, bpm FROM tracks WHERE id = ?1", [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<f64>>(2)?))).unwrap();
        assert_eq!(row(1), ("House".into(), "idea".into(), Some(124.0)));
        assert_eq!(row(2), ("Techno".into(), "wip".into(), None));
        // Les verrous posés par l'édition sont retirés avec elle ; le lot ne se propose plus.
        assert!(!super::super::library::locked(&conn, 1, "status"));
        assert!(super::last(&conn, PC).is_none());
        // Un tempo corrigé puis rendu à l'analyse : deux lignes, la correction et le verrou retiré seul.
        record(&conn, "Tempo × 2", "", || super::super::library::scale_bpm(&conn, &[1], 2.0).unwrap());
        record(&conn, "Rendu à l'analyse", "", || super::super::library::lock(&conn, 1, "bpm", false).unwrap());
        let h: Vec<String> = history(&conn, 1, 10).iter().map(|e| e.text.clone()).collect();
        assert_eq!(h[..2], ["Tempo : rendu à l'analyse".to_string(), "Tempo : 124 → 248".into()]);
        assert_eq!(super::last(&conn, PC).unwrap().label, "Rendu à l'analyse");
        undo(&conn, &super::last(&conn, PC).unwrap().batch).unwrap();
        assert!(super::super::library::locked(&conn, 1, "bpm"));
    }

    #[test]
    fn typing_makes_one_change_and_history_reads_well() {
        let conn = base();
        for t in ["N", "Nu", "Nui", "Nuit blanche"] {
            record(&conn, "Modification", "field:1:title", || super::super::library::update(&conn, 1, "title", t).unwrap());
        }
        record(&conn, "Tags", "tags:1", || {
            super::super::library::set_tags(&conn, 1, "club, n").unwrap();
            super::super::library::set_tags(&conn, 1, "club, nuit").unwrap();
        });
        record_as(&conn, "Depuis le téléphone", PHONE, "", || super::super::library::set_pick(&conn, &[1], 1).unwrap());
        let h = history(&conn, 1, 10);
        let lines: Vec<String> = h.iter().map(|e| format!("{} ({}, {})", e.text, e.label, e.source)).collect();
        assert_eq!(
            lines,
            vec![
                "Choix : Neutre → Gardé (Depuis le téléphone, téléphone)".to_string(),
                "Tags : + club, nuit (Tags, pc)".into(),
                "Titre : Nuit → Nuit blanche (Modification, pc)".into(),
            ]
        );
        assert!(h[0].when.starts_with("aujourd'hui à "), "{}", h[0].when);
        // Ctrl+Z ne défait que ce qui a été fait sur ce PC : les tags d'abord, puis le titre en une fois.
        undo(&conn, &last(&conn, PC).unwrap().batch).unwrap();
        assert!(super::super::library::tags_of(&conn, 1).is_empty());
        undo(&conn, &last(&conn, PC).unwrap().batch).unwrap();
        assert_eq!(super::super::library::field(&conn, 1, "title"), "Nuit");
        assert!(history(&conn, 1, 10)[1].undone);
        // « Remettre » une ligne : le choix du téléphone, comme avant, et c'est annulable à son tour.
        assert_eq!(restore(&conn, 1, &h[0].batch, "pick").unwrap(), "Choix");
        assert_eq!(conn.query_row("SELECT pick FROM tracks WHERE id = 1", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(last(&conn, PC).unwrap().label, "Choix remis comme avant");
    }
}
