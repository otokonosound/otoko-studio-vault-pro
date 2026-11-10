//! Ce que le téléphone peut changer sur un morceau, en écoutant : le garder ou le rejeter, son statut, une note posée à
//! un instant précis (un repère, le même que ceux du lecteur du PC) et l'écoute elle-même (compteur et date). Rien
//! d'autre : ni fichiers, ni titres, ni tags. Chaque valeur est vérifiée avant d'être écrite.

use otoko_library::career::{STATUSES, library as lib};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value as Json;

/// La longueur maximale d'une note (une dictée vocale tient largement dedans).
pub const NOTE_MAX: usize = 300;

fn duration(con: &Connection, id: i64) -> Result<f64, String> {
    con.query_row("SELECT IFNULL(duration_s, 0) FROM tracks WHERE id = ?1", [id], |r| r.get::<_, f64>(0))
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Morceau introuvable.".to_string())
}

/// `{"pick": -1|0|1, "status": "wip"}` : les champs donnés sont écrits, les autres ne bougent pas.
pub fn edit(con: &Connection, id: i64, body: &Json) -> Result<(), String> {
    duration(con, id)?;
    let pick = match &body["pick"] {
        Json::Null => None,
        v => Some(v.as_i64().filter(|p| (-1..=1).contains(p)).ok_or("« pick » vaut -1, 0 ou 1.")?),
    };
    let status = match &body["status"] {
        Json::Null => None,
        v => Some(v.as_str().filter(|s| STATUSES.iter().any(|x| x.0 == *s)).ok_or("Statut inconnu.")?),
    };
    if pick.is_none() && status.is_none() {
        return Err("Rien à changer.".into());
    }
    if let Some(p) = pick {
        lib::set_pick(con, &[id], p).map_err(|e| e.to_string())?;
    }
    if let Some(s) = status {
        lib::update(con, id, "status", s)?;
    }
    Ok(())
}

/// Le texte d'une note : sans espaces autour, sans retours à la ligne, coupé à `NOTE_MAX` caractères ; « Note » si vide.
pub fn note_label(text: &str) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = t.chars().take(NOTE_MAX).collect();
    if t.is_empty() { "Note".into() } else { t }
}

/// L'instant d'une note : dans le morceau (au dixième de seconde).
fn note_time(con: &Connection, id: i64, body: &Json) -> Result<f64, String> {
    let d = duration(con, id)?;
    let t = body["time"].as_f64().filter(|t| t.is_finite() && *t >= 0.0).ok_or("Instant manquant.")?;
    let t = if d > 0.0 { t.min(d) } else { t };
    Ok((t * 10.0).round() / 10.0)
}

/// `{"time": 92.4, "label": "kick trop fort"}` : un repère sur le morceau.
pub fn add_note(con: &Connection, id: i64, body: &Json) -> Result<(), String> {
    let t = note_time(con, id, body)?;
    lib::add_marker(con, id, t, &note_label(body["label"].as_str().unwrap_or(""))).map_err(|e| e.to_string())
}

/// `{"time": 92.4, "label": "kick trop fort"}` : retire ce repère.
pub fn remove_note(con: &Connection, id: i64, body: &Json) -> Result<(), String> {
    let t = note_time(con, id, body)?;
    let label = body["label"].as_str().ok_or("Note manquante.")?;
    lib::remove_marker(con, id, t, label).map_err(|e| e.to_string())
}

/// Le réglage que le logiciel du PC surveille pour relire ce qui est à l'écran après une écriture du téléphone.
pub const STAMP: &str = "mobile_edit";
/// Le même, pour une réponse arrivée par un lien de partage (un « j'aime », un commentaire).
pub const FEEDBACK_STAMP: &str = "share_feedback";

/// Signale au logiciel du PC qu'un morceau vient de changer depuis le téléphone (le morceau, puis un compteur).
pub fn stamp(con: &Connection, id: i64) {
    stamp_as(con, STAMP, id);
}

/// `stamp`, sous le réglage donné.
pub fn stamp_as(con: &Connection, key: &str, id: i64) {
    let _ = con.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2 || ':1')
         ON CONFLICT(key) DO UPDATE SET value = ?2 || ':' || (CAST(substr(value, instr(value, ':') + 1) AS INTEGER) + 1)",
        rusqlite::params![key, id.to_string()],
    );
}

/// Une écoute sur le téléphone compte comme sur le PC.
pub fn played(con: &Connection, id: i64) -> Result<(), String> {
    duration(con, id)?;
    lib::played(con, id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> Connection {
        let con = Connection::open_in_memory().unwrap();
        con.execute_batch(otoko_library::career::schema::SCHEMA).unwrap();
        con.execute("INSERT INTO tracks (id, path, filename, duration_s, status) VALUES (1, 'a.wav', 'a.wav', 200, 'idea')", []).unwrap();
        con
    }

    #[test]
    fn the_phone_keeps_rejects_and_moves_a_track_along() {
        let con = base();
        edit(&con, 1, &json!({ "pick": 1, "status": "wip" })).unwrap();
        let (pick, status): (i64, String) = con.query_row("SELECT pick, status FROM tracks WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((pick, status.as_str()), (1, "wip"));
        // Rien d'inventé : statut inconnu, choix hors bornes, morceau absent, requête vide.
        assert!(edit(&con, 1, &json!({ "status": "génial" })).is_err());
        assert!(edit(&con, 1, &json!({ "pick": 4 })).is_err());
        assert!(edit(&con, 9, &json!({ "pick": 1 })).is_err());
        assert!(edit(&con, 1, &json!({ "title": "x" })).is_err());
    }

    #[test]
    fn a_note_lands_on_the_waveform_and_can_be_taken_back() {
        let con = base();
        add_note(&con, 1, &json!({ "time": 92.43, "label": "  kick\ntrop   fort " })).unwrap();
        add_note(&con, 1, &json!({ "time": 999, "label": "" })).unwrap();
        let notes: Vec<(f64, String)> =
            con.prepare("SELECT time_s, label FROM markers ORDER BY time_s").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
        // Au dixième, sans retours à la ligne ; au-delà de la fin, posée à la fin ; vide, « Note ».
        assert_eq!(notes, vec![(92.4, "kick trop fort".to_string()), (200.0, "Note".to_string())]);
        remove_note(&con, 1, &json!({ "time": 92.4, "label": "kick trop fort" })).unwrap();
        assert_eq!(con.query_row("SELECT COUNT(*) FROM markers", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert!(add_note(&con, 1, &json!({ "label": "sans instant" })).is_err());
        assert_eq!(note_label(&"é".repeat(400)).chars().count(), NOTE_MAX);
    }

    #[test]
    fn each_phone_edit_changes_the_stamp() {
        let con = base();
        let read = |c: &Connection| c.query_row("SELECT value FROM settings WHERE key = ?1", [STAMP], |r| r.get::<_, String>(0)).unwrap();
        stamp(&con, 1);
        assert_eq!(read(&con), "1:1");
        stamp(&con, 1);
        assert_eq!(read(&con), "1:2");
    }

    #[test]
    fn a_phone_listen_counts() {
        let con = base();
        played(&con, 1).unwrap();
        played(&con, 1).unwrap();
        assert_eq!(con.query_row("SELECT play_count FROM tracks WHERE id = 1", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert!(played(&con, 7).is_err());
    }
}
