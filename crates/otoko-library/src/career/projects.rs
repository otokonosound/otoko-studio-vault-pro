//! Fiche projet (comme `ui/project_dialog.py`) : ce qui est propre à la nature du morceau — remix
//! et edit (titre d'origine, autorisation, stems, échéance), mashup (titres sources), set (liste
//! minutée, .cue, enchaînements), prod pour un artiste (client, commande, livrables, rémunération,
//! accord, versions) — et le carnet de clients (`ui/clients_dialog.py`).

use rusqlite::{Connection, params};
use serde_json::{Value as Json, json};

use super::camelot::{camelot, compatibles};
use super::nature::{self, Entry};

/// Une ligne de la liste minutée d'un set, avec le morceau de la bibliothèque qu'elle désigne.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetEntry {
    pub time: f64,
    pub artist: String,
    pub title: String,
    pub track: Option<i64>,
    pub bpm: Option<f64>,
    pub key: String,
}

impl SetEntry {
    pub fn entry(&self) -> Entry {
        Entry { time: self.time, artist: self.artist.clone(), title: self.title.clone(), bpm: self.bpm, key: self.key.clone() }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub id: i64,
    /// Le titre lisible (`nettoyer_titre`).
    pub title: String,
    pub raw_title: String,
    pub path: String,
    pub duration: Option<f64>,
    pub nature: String,
    pub meta: Json,
    pub entries: Vec<SetEntry>,
}

/// Titre lisible : tag titre, sinon nom de fichier sans numéro ni artistes (`textes.nettoyer_titre`).
pub fn clean_title(title: &str, filename: &str) -> String {
    use std::sync::OnceLock;
    static EXT: OnceLock<regex::Regex> = OnceLock::new();
    static NUM: OnceLock<regex::Regex> = OnceLock::new();
    let mut t = title.trim().to_string();
    if t.is_empty() {
        t = EXT.get_or_init(|| regex::Regex::new(r"\.[A-Za-z0-9]{2,4}$").unwrap()).replace(filename, "").into_owned();
    }
    t = NUM.get_or_init(|| regex::Regex::new(r"^\s*\d{1,3}\s*[.\-_)]\s*").unwrap()).replace(&t, "").into_owned();
    if let Some((_, last)) = t.rsplit_once(" - ") {
        t = last.to_string();
    }
    let t = t.trim();
    if t.is_empty() { "ce morceau".into() } else { t.to_string() }
}

pub fn load(conn: &Connection, id: i64) -> Option<Project> {
    let mut p = conn
        .query_row(
            "SELECT id, IFNULL(title, ''), IFNULL(filename, ''), path, duration_s, IFNULL(NULLIF(nature, ''), 'original'), IFNULL(nature_meta, '{}') FROM tracks WHERE id = ?1",
            [id],
            |r| {
                let (title, filename): (String, String) = (r.get(1)?, r.get(2)?);
                Ok(Project {
                    id: r.get(0)?,
                    title: clean_title(&title, &filename),
                    raw_title: title,
                    path: r.get(3)?,
                    duration: r.get(4)?,
                    nature: r.get(5)?,
                    meta: nature::load_meta(&r.get::<_, String>(6)?),
                    entries: Vec::new(),
                })
            },
        )
        .ok()?;
    p.entries = conn
        .prepare(
            "SELECT e.time_s, IFNULL(e.artist, ''), IFNULL(e.title, ''), e.ref_track_id, t.bpm, IFNULL(t.music_key, '') FROM set_entries e
             LEFT JOIN tracks t ON t.id = e.ref_track_id WHERE e.set_id = ?1 ORDER BY e.time_s",
        )
        .and_then(|mut s| s.query_map([id], |r| Ok(SetEntry { time: r.get::<_, Option<f64>>(0)?.unwrap_or(0.0), artist: r.get(1)?, title: r.get(2)?, track: r.get(3)?, bpm: r.get(4)?, key: r.get(5)? }))?.collect())
        .unwrap_or_default();
    Some(p)
}

/// Enregistre la nature, ses informations et (pour un set) la liste minutée. Payé ⇒ commande
/// « Payé ».
pub fn save(conn: &Connection, p: &Project) -> rusqlite::Result<()> {
    let mut meta = p.meta.clone();
    if p.nature == "prod" && meta["paid"].as_bool().unwrap_or(false) {
        meta["order_status"] = json!("paid");
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE tracks SET nature = ?2, nature_meta = ?3, modified_at = datetime('now') WHERE id = ?1", params![p.id, p.nature, meta.to_string()])?;
    if p.nature == "set" {
        tx.execute("DELETE FROM set_entries WHERE set_id = ?1", [p.id])?;
        let mut sorted = p.entries.clone();
        sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
        for e in sorted {
            tx.execute("INSERT INTO set_entries (set_id, time_s, artist, title, ref_track_id) VALUES (?1, ?2, ?3, ?4, ?5)", params![p.id, e.time, e.artist, e.title, e.track])?;
        }
    }
    tx.commit()
}

/// Le titre du morceau dans la bibliothèque (« Titre (Artiste Remix) »).
pub fn set_title(conn: &Connection, id: i64, title: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE tracks SET title = ?2, modified_at = datetime('now') WHERE id = ?1", params![id, title])?;
    Ok(())
}

/// « 1:02:03 », « 62:03 », « 3723 » → secondes.
pub fn parse_time(text: &str) -> f64 {
    let parts: Vec<&str> = text.trim().split(':').filter(|p| !p.is_empty()).collect();
    let mut sec = 0.0;
    for p in parts {
        match p.replace(',', ".").trim().parse::<f64>() {
            Ok(v) => sec = sec * 60.0 + v,
            Err(_) => return 0.0,
        }
    }
    sec
}

/// « Titre (Artiste Remix) » (ou Edit).
pub fn remix_title(base: &str, artist: &str, nature: &str) -> String {
    let label = if nature == "remix" { "Remix" } else { "Edit" };
    format!("{} ({} {label})", base.trim(), artist.trim()).replace("( ", "(")
}

/// Le contrôle des sources d'un mashup, par rapport à la première (tempo à 6 % près, tonalité).
pub fn mashup_check(sources: &[Json]) -> String {
    let with: Vec<&Json> = sources.iter().filter(|s| s["bpm"].as_f64().is_some() || !s["key"].as_str().unwrap_or("").is_empty()).collect();
    if with.len() < 2 {
        return String::new();
    }
    let r = with[0];
    let mut lines = Vec::new();
    for s in &with[1..] {
        let mut bits = vec![format!("{} :", s["title"].as_str().filter(|t| !t.is_empty()).unwrap_or("?"))];
        if let (Some(a), Some(b)) = (r["bpm"].as_f64().filter(|x| *x > 0.0), s["bpm"].as_f64()) {
            let d = (b - a) / a * 100.0;
            bits.push(format!("tempo {d:+.1} %{}", if d.abs() <= 6.0 { " ✓" } else { " — écart important" }));
        }
        let (ca, cb) = (camelot(r["key"].as_str().unwrap_or("")), camelot(s["key"].as_str().unwrap_or("")));
        if !ca.is_empty() && !cb.is_empty() {
            bits.push(if compatibles(ca).iter().any(|c| c == cb) { "tonalité compatible ✓".into() } else { "tonalité en conflit".into() });
        }
        lines.push(bits.join(" "));
    }
    format!("Par rapport à « {} » :\n{}", r["title"].as_str().filter(|t| !t.is_empty()).unwrap_or("la première source"), lines.join("\n"))
}

/// Le rapport des enchaînements d'un set.
pub fn transitions_report(entries: &[SetEntry]) -> String {
    let mut sorted: Vec<&SetEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
    let list: Vec<Entry> = sorted.iter().map(|e| e.entry()).collect();
    let res = nature::transitions(&list);
    if res.is_empty() {
        return "Il faut au moins deux titres pour vérifier les enchaînements.".into();
    }
    res.iter()
        .map(|(i, bpm, ok)| {
            let (a, b) = (&list[*i], &list[*i + 1]);
            let mut bits = Vec::new();
            if let Some(d) = bpm {
                bits.push(format!("tempo {d:+.1} %{}", if d.abs() <= 6.0 { "" } else { " ⚠ écart important" }));
            }
            if let Some(ok) = ok {
                bits.push(if *ok { "tonalités compatibles".into() } else { "⚠ tonalités en conflit".into() });
            }
            if bits.is_empty() {
                bits.push("tempo et tonalité inconnus (titres externes ou non analysés)".into());
            }
            format!("{} → {} : {}", if a.title.is_empty() { "?" } else { &a.title }, if b.title.is_empty() { "?" } else { &b.title }, bits.join(", "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Les marqueurs du morceau qui ne sont pas déjà dans la liste (« Artiste - Titre » coupé en deux).
pub fn markers_as_entries(conn: &Connection, id: i64, existing: &[SetEntry]) -> Vec<SetEntry> {
    let have: Vec<i64> = existing.iter().map(|e| e.time.round() as i64).collect();
    conn.prepare("SELECT time_s, IFNULL(label, '') FROM markers WHERE track_id = ?1 ORDER BY time_s")
        .and_then(|mut s| s.query_map([id], |r| Ok((r.get::<_, f64>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|(t, _)| !have.contains(&(t.round() as i64)))
        .map(|(t, label)| {
            let (artist, title) = match label.split_once(" - ") {
                Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
                None => (String::new(), label.trim().to_string()),
            };
            SetEntry { time: t, artist, title, ..Default::default() }
        })
        .collect()
}

/// Un morceau de la bibliothèque à ajouter (source de mashup, titre d'un set) : (id, titre lisible,
/// artiste, tempo, tonalité).
pub fn pickable(conn: &Connection) -> Vec<(i64, String, String, Option<f64>, String)> {
    conn.prepare("SELECT id, IFNULL(title, ''), IFNULL(filename, ''), IFNULL(artist, ''), bpm, IFNULL(music_key, '') FROM tracks WHERE missing = 0 ORDER BY title COLLATE NOCASE")
        .and_then(|mut s| {
            s.query_map([], |r| {
                let (t, f): (String, String) = (r.get(1)?, r.get(2)?);
                Ok((r.get(0)?, clean_title(&t, &f), r.get(3)?, r.get(4)?, r.get(5)?))
            })?
            .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------- clients

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Client {
    pub id: i64,
    pub name: String,
    pub contact: String,
    pub notes: String,
}

pub fn clients(conn: &Connection) -> Vec<Client> {
    conn.prepare("SELECT id, name, IFNULL(contact, ''), IFNULL(notes, '') FROM clients ORDER BY name COLLATE NOCASE")
        .and_then(|mut s| s.query_map([], |r| Ok(Client { id: r.get(0)?, name: r.get(1)?, contact: r.get(2)?, notes: r.get(3)? }))?.collect())
        .unwrap_or_default()
}

pub fn new_client(conn: &Connection) -> rusqlite::Result<i64> {
    conn.execute("INSERT INTO clients (name) VALUES ('Nouveau client')", [])?;
    Ok(conn.last_insert_rowid())
}

/// Enregistre le client ; le nom suit dans les prods qui le citent.
pub fn save_client(conn: &Connection, c: &Client) -> rusqlite::Result<()> {
    let name = if c.name.trim().is_empty() { "Sans nom" } else { c.name.trim() };
    conn.execute("UPDATE clients SET name = ?2, contact = ?3, notes = ?4 WHERE id = ?1", params![c.id, name, c.contact.trim(), c.notes.trim()])?;
    for (id, raw) in prods_of(conn, c.id) {
        let mut m = nature::load_meta(&raw);
        if m["client_name"].as_str() != Some(name) {
            m["client_name"] = json!(name);
            conn.execute("UPDATE tracks SET nature_meta = ?2 WHERE id = ?1", params![id, m.to_string()])?;
        }
    }
    Ok(())
}

/// Retire le client du carnet ; les morceaux liés gardent leurs informations.
pub fn delete_client(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM clients WHERE id = ?1", [id])?;
    Ok(())
}

fn prods_of(conn: &Connection, client: i64) -> Vec<(i64, String)> {
    conn.prepare("SELECT id, IFNULL(nature_meta, '{}') FROM tracks WHERE nature = 'prod'")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, raw)| nature::load_meta(raw)["client_id"].as_i64() == Some(client))
        .collect()
}

/// Nombre de prods pour ce client dans la bibliothèque.
pub fn client_prods(conn: &Connection, client: i64) -> usize {
    prods_of(conn, client).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, title, bpm, music_key) VALUES (1, 'D:\\set.wav', '03 - Otoko - Set de nuit.wav', '', NULL, ''),
                (2, 'D:\\a.wav', 'a.wav', 'Nuit', 124, 'Am'), (3, 'D:\\b.wav', 'b.wav', 'Jour', 126, 'Em');
             INSERT INTO markers (track_id, time_s, label) VALUES (1, 0, 'Daft Punk - One More Time'), (1, 300.4, 'Intro');",
        )
        .unwrap();
        let mut p = load(&conn, 1).unwrap();
        assert_eq!(p.title, "Set de nuit");
        p.nature = "set".into();
        p.entries = markers_as_entries(&conn, 1, &[]);
        assert_eq!((p.entries[0].artist.as_str(), p.entries[0].title.as_str(), p.entries[1].title.as_str()), ("Daft Punk", "One More Time", "Intro"));
        p.entries.push(SetEntry { time: parse_time("10:00"), title: "Nuit".into(), track: Some(2), ..Default::default() });
        save(&conn, &p).unwrap();
        let back = load(&conn, 1).unwrap();
        assert_eq!(back.entries.len(), 3);
        assert_eq!((back.entries[2].bpm, back.entries[2].key.as_str()), (Some(124.0), "Am"));
        assert!(markers_as_entries(&conn, 1, &back.entries).is_empty(), "déjà importés");
        assert!(transitions_report(&back.entries).contains("Intro → Nuit : tempo et tonalité inconnus"));
        assert_eq!(parse_time("1:02:03"), 3723.0);
        assert_eq!(parse_time("2,5"), 2.5);
        assert_eq!(remix_title("Nuit", "Otoko", "remix"), "Nuit (Otoko Remix)");
        assert_eq!(remix_title("Nuit", "", "edit"), "Nuit (Edit)");
        let srcs = vec![json!({"title": "Nuit", "bpm": 124.0, "key": "Am"}), json!({"title": "Jour", "bpm": 126.0, "key": "Em"}), json!({"title": "Loin", "bpm": 140.0, "key": "F#"})];
        assert_eq!(mashup_check(&srcs), "Par rapport à « Nuit » :\nJour : tempo +1.6 % ✓ tonalité compatible ✓\nLoin : tempo +12.9 % — écart important tonalité en conflit");
        // Prod : payée ⇒ commande « Payé » ; le nom du client suit.
        let c = new_client(&conn).unwrap();
        let mut p2 = load(&conn, 2).unwrap();
        p2.nature = "prod".into();
        p2.meta = json!({"client_id": c, "client_name": "Nouveau client", "paid": true, "order_status": "delivered"});
        save(&conn, &p2).unwrap();
        assert_eq!(load(&conn, 2).unwrap().meta["order_status"], "paid");
        save_client(&conn, &Client { id: c, name: "Léa".into(), contact: "lea@exemple.fr".into(), notes: String::new() }).unwrap();
        assert_eq!(load(&conn, 2).unwrap().meta["client_name"], "Léa");
        assert_eq!(client_prods(&conn, c), 1);
        assert_eq!(clients(&conn)[0].name, "Léa");
        delete_client(&conn, c).unwrap();
        assert!(clients(&conn).is_empty());
        assert_eq!(clean_title("", "05.Titre.mp3"), "Titre");
        assert_eq!(clean_title("Artiste X Autre - Titre", ""), "Titre");
    }
}
