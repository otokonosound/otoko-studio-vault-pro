//! Le client distant : le PC principal reste le centre et un autre PC travaille sur une copie de sa base. Les lectures sont
//! locales, donc instantanées. Chaque écriture de la copie est notée par des déclencheurs dans `_journal`, puis envoyée
//! au PC principal, qui l'applique. Aucun autre module n'a besoin de savoir qu'il travaille sur une copie.
//!
//! Seules les colonnes modifiées sur la copie partent : la ligne d'avant la première modification est gardée dans
//! `_avant_<table>`, et une ligne créée sur la copie est notée dans `_nouvelles`. Le PC principal ne perd donc pas ce qu'il a
//! changé de son côté entre-temps (une analyse, un statut), et une ligne qu'il a supprimée ne revient pas.

use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, params_from_iter};
use serde_json::{Map, Value as Json, json};

/// Les tables suivies et leur clé. Les ondes et les copies ne sont calculées que sur le PC principal.
pub const TABLES: [(&str, &[&str]); 15] = [
    ("tracks", &["id"]),
    ("releases", &["id"]),
    ("release_tracks", &["release_id", "track_id"]),
    ("tags", &["id"]),
    ("track_tags", &["track_id", "tag_id"]),
    ("markers", &["id"]),
    ("post_drafts", &["id"]),
    ("text_models", &["id"]),
    ("prompts", &["id"]),
    ("prompt_versions", &["id"]),
    ("clients", &["id"]),
    ("saved_views", &["id"]),
    ("set_entries", &["id"]),
    ("field_locks", &["track_id", "field"]),
    ("settings", &["key"]),
];

/// Seuls les réglages de l'artiste voyagent : ceux d'une machine (dossiers, audio, apparence) restent sur elle.
fn shared_setting(key: &str) -> bool {
    key.starts_with("pub_") || key.starts_with("artist_")
}

/// Les nouvelles lignes de la copie partent de là, pour ne pas prendre les identifiants du PC principal.
const LOCAL_IDS: i64 = 1_000_000_000;

fn key_of(table: &str) -> Option<&'static [&'static str]> {
    TABLES.iter().find(|t| t.0 == table).map(|t| t.1)
}

/// Prépare une copie : le journal, les déclencheurs, les lignes d'avant, les identifiants à part. Se relance sans risque sur
/// une copie déjà prête (au démarrage, après une mise à jour du logiciel) : les déclencheurs sont refaits avec les colonnes
/// du moment.
pub fn install(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _journal (n INTEGER PRIMARY KEY AUTOINCREMENT, tbl TEXT NOT NULL, op TEXT NOT NULL, k1, k2);
         CREATE TABLE IF NOT EXISTS _nouvelles (tbl TEXT NOT NULL, k1, k2, UNIQUE (tbl, k1, k2));",
    )?;
    for (table, keys) in TABLES {
        let (k1, k2) = (keys[0], keys.get(1).copied());
        let vals = |row: &str| format!("{row}.{k1}, {}", k2.map(|k| format!("{row}.{k}")).unwrap_or_else(|| "NULL".into()));
        let only = |row: &str| if table == "settings" { format!(" WHEN {row}.key LIKE 'pub\\_%' ESCAPE '\\' OR {row}.key LIKE 'artist\\_%' ESCAPE '\\'") } else { String::new() };
        // La table des lignes d'avant suit les colonnes de la table (une mise à jour du logiciel peut en ajouter).
        let cols = columns(conn, table)?;
        let before = format!("_avant_{table}");
        conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS {before} AS SELECT * FROM {table} WHERE 0;"))?;
        let have = columns(conn, &before)?;
        for c in cols.iter().filter(|c| !have.contains(c)) {
            conn.execute_batch(&format!("ALTER TABLE {before} ADD COLUMN {c};"))?;
        }
        conn.execute_batch(&format!("CREATE UNIQUE INDEX IF NOT EXISTS {before}_cle ON {before} ({});", keys.join(", ")))?;
        let list = cols.join(", ");
        let olds = cols.iter().map(|c| format!("OLD.{c}")).collect::<Vec<_>>().join(", ");
        conn.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS _ji_{table}; DROP TRIGGER IF EXISTS _ju_{table}; DROP TRIGGER IF EXISTS _jd_{table};
             CREATE TRIGGER _ji_{table} AFTER INSERT ON {table}{w} BEGIN
                 INSERT OR IGNORE INTO _nouvelles (tbl, k1, k2) VALUES ('{table}', {new});
                 INSERT INTO _journal (tbl, op, k1, k2) VALUES ('{table}', 'set', {new});
             END;
             CREATE TRIGGER _ju_{table} AFTER UPDATE ON {table}{w} BEGIN
                 INSERT OR IGNORE INTO {before} ({list}) VALUES ({olds});
                 INSERT INTO _journal (tbl, op, k1, k2) VALUES ('{table}', 'set', {new});
             END;
             CREATE TRIGGER _jd_{table} AFTER DELETE ON {table}{wo} BEGIN INSERT INTO _journal (tbl, op, k1, k2) VALUES ('{table}', 'del', {old}); END;",
            w = only("NEW"),
            wo = only("OLD"),
            new = vals("NEW"),
            old = vals("OLD"),
        ))?;
    }
    conn.execute("UPDATE sqlite_sequence SET seq = ?1 WHERE seq < ?1", [LOCAL_IDS])?;
    Ok(())
}

/// Le nombre d'écritures à envoyer.
pub fn waiting(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM _journal", [], |r| r.get(0)).unwrap_or(0)
}

fn to_json(v: ValueRef) -> Json {
    match v {
        ValueRef::Null | ValueRef::Blob(_) => Json::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
    }
}

fn from_json(v: &Json) -> Value {
    match v {
        Json::Bool(b) => Value::Integer(*b as i64),
        Json::Number(n) => n.as_i64().map(Value::Integer).or(n.as_f64().map(Value::Real)).unwrap_or(Value::Null),
        Json::String(s) => Value::Text(s.clone()),
        _ => Value::Null,
    }
}

/// Une ligne de la copie (ou sa version d'avant, dans `_avant_<table>`), en JSON.
fn row_json(conn: &Connection, table: &str, keys: &[&str], k: &[Value]) -> rusqlite::Result<Option<Json>> {
    let wh: Vec<String> = keys.iter().enumerate().map(|(i, c)| format!("{c} = ?{}", i + 1)).collect();
    let mut st = conn.prepare(&format!("SELECT * FROM {table} WHERE {}", wh.join(" AND ")))?;
    let names: Vec<String> = st.column_names().map(|s| s.to_string()).collect();
    let mut rows = st.query(params_from_iter(k.iter()))?;
    let Some(row) = rows.next()? else { return Ok(None) };
    let mut m = Map::new();
    for (i, n) in names.iter().enumerate() {
        m.insert(n.clone(), to_json(row.get_ref(i)?));
    }
    Ok(Some(Json::Object(m)))
}

/// Les écritures à envoyer, sans doublons : (les changements, le dernier numéro du journal qu'ils couvrent).
pub fn pending(conn: &Connection, limit: i64) -> rusqlite::Result<(Vec<Json>, i64)> {
    let list: Vec<(i64, String, String, Value, Value)> = conn
        .prepare("SELECT n, tbl, op, k1, k2 FROM _journal ORDER BY n LIMIT ?1")?
        .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let last = list.last().map(|l| l.0).unwrap_or(0);
    let mut seen: Vec<(String, Value, Value)> = Vec::new();
    let mut out = Vec::new();
    // Du plus récent au plus ancien : seule compte la dernière écriture d'une ligne.
    for (_, table, op, k1, k2) in list.into_iter().rev() {
        let id = (table.clone(), k1.clone(), k2.clone());
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let Some(keys) = key_of(&table) else { continue };
        let k: Vec<Value> = if keys.len() > 1 { vec![k1, k2] } else { vec![k1] };
        let key: Map<String, Json> = keys.iter().zip(&k).map(|(c, v)| (c.to_string(), value_json(v))).collect();
        if op == "del" {
            out.push(json!({ "table": table, "op": "del", "key": key }));
            continue;
        }
        let Some(row) = row_json(conn, &table, keys, &k)? else { continue };
        if is_new(conn, &table, &k)? {
            // Créée ici : elle part entière.
            out.push(json!({ "table": table, "op": "set", "row": row }));
        } else if let Some(before) = has_table(conn, &format!("_avant_{table}"))?.then(|| row_json(conn, &format!("_avant_{table}"), keys, &k)).transpose()?.flatten() {
            // Modifiée ici : seulement ce qui a changé depuis la ligne d'avant. « row » reste pour un PC principal d'avant
            // cette version, qui ne connaît que la ligne entière.
            let changes: Map<String, Json> = row
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(c, v)| !keys.contains(&c.as_str()) && before.get(c.as_str()) != Some(*v))
                .map(|(c, v)| (c.clone(), v.clone()))
                .collect();
            if !changes.is_empty() {
                out.push(json!({ "table": table, "op": "set", "key": key, "changes": changes, "row": row }));
            }
        } else {
            // Notée avant cette version (pas de ligne d'avant) : la ligne entière, comme avant.
            out.push(json!({ "table": table, "op": "set", "row": row }));
        }
    }
    // Dans l'ordre d'origine : une sortie avant ses morceaux.
    out.reverse();
    Ok((out, last))
}

fn value_json(v: &Value) -> Json {
    match v {
        Value::Integer(i) => json!(i),
        Value::Real(f) => json!(f),
        Value::Text(t) => json!(t),
        _ => Json::Null,
    }
}

fn has_table(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |r| r.get::<_, i64>(0)).map(|n| n > 0)
}

/// La ligne a-t-elle été créée sur la copie (et pas encore envoyée) ?
fn is_new(conn: &Connection, table: &str, k: &[Value]) -> rusqlite::Result<bool> {
    if !has_table(conn, "_nouvelles")? {
        return Ok(false);
    }
    let k2 = k.get(1).cloned().unwrap_or(Value::Null);
    conn.query_row("SELECT COUNT(*) FROM _nouvelles WHERE tbl = ?1 AND k1 IS ?2 AND k2 IS ?3", rusqlite::params![table, k[0], k2], |r| r.get::<_, i64>(0)).map(|n| n > 0)
}

/// Les écritures sont arrivées : elles sortent du journal, avec les lignes d'avant et les marques de création des lignes
/// qui n'attendent plus rien.
pub fn sent(conn: &Connection, last: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM _journal WHERE n <= ?1", [last])?;
    if has_table(conn, "_nouvelles")? {
        conn.execute("DELETE FROM _nouvelles WHERE NOT EXISTS (SELECT 1 FROM _journal j WHERE j.tbl = _nouvelles.tbl AND j.k1 IS _nouvelles.k1 AND j.k2 IS _nouvelles.k2)", [])?;
    }
    for (table, keys) in TABLES {
        let before = format!("_avant_{table}");
        if !has_table(conn, &before)? {
            continue;
        }
        let k2 = keys.get(1).map(|k| format!("{before}.{k}")).unwrap_or_else(|| "NULL".into());
        conn.execute(&format!("DELETE FROM {before} WHERE NOT EXISTS (SELECT 1 FROM _journal j WHERE j.tbl = '{table}' AND j.k1 IS {before}.{} AND j.k2 IS {k2})", keys[0]), [])?;
    }
    Ok(())
}

/// Les colonnes d'une table : seules celles-ci sont acceptées dans une requête.
fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare(&format!("PRAGMA table_info({table})"))?.query_map([], |r| r.get::<_, String>(1))?.collect()
}

/// Une vue enregistrée qui arrive : sa condition (nouvelle, ou celle déjà là si seuls les paramètres changent) doit rester
/// une simple lecture des morceaux (`library::view_condition_ok`).
fn view_ok(conn: &Connection, c: &Json) -> bool {
    let sent = if c["changes"].is_object() { &c["changes"] } else { &c["row"] };
    if sent.get("where_sql").is_none() && sent.get("params").is_none() {
        return true;
    }
    let id = c["key"]["id"].as_i64().or(c["row"]["id"].as_i64());
    let now: Option<(String, String)> = id.and_then(|id| conn.query_row("SELECT where_sql, IFNULL(params, '[]') FROM saved_views WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).ok());
    let w = sent["where_sql"].as_str().map(str::to_string).or(now.as_ref().map(|n| n.0.clone())).unwrap_or_default();
    let p = sent["params"].as_str().map(str::to_string).or(now.map(|n| n.1)).unwrap_or_else(|| "[]".into());
    super::library::view_condition_ok(conn, &w, &p)
}

/// Sur le PC principal : applique ce qu'envoie la copie. La dernière écriture gagne. Rend le nombre de lignes appliquées.
pub fn apply(conn: &Connection, changes: &[Json]) -> Result<usize, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let tx = conn.unchecked_transaction().map_err(e)?;
    let mut n = 0;
    for c in changes {
        let table = c["table"].as_str().ok_or("table absente")?;
        let keys = key_of(table).ok_or_else(|| format!("table refusée : {table}"))?;
        let known = columns(&tx, table).map_err(e)?;
        if c["op"] == "del" {
            let key = c["key"].as_object().ok_or("clé absente")?;
            if table == "settings" && !key.get("key").and_then(|k| k.as_str()).is_some_and(shared_setting) {
                continue;
            }
            let vals: Vec<Value> = keys.iter().map(|k| key.get(*k).map(from_json).unwrap_or(Value::Null)).collect();
            let wh: Vec<String> = keys.iter().enumerate().map(|(i, k)| format!("{k} = ?{}", i + 1)).collect();
            n += tx.execute(&format!("DELETE FROM {table} WHERE {}", wh.join(" AND ")), params_from_iter(vals.iter())).map_err(e)?;
            continue;
        }
        if table == "saved_views" && !view_ok(&tx, c) {
            // Une condition douteuse n'entre pas dans la base ; le reste de l'envoi passe.
            continue;
        }
        if let Some(changes) = c["changes"].as_object() {
            // Seulement les colonnes changées sur la copie, et seulement si la ligne existe encore ici : ce que le PC
            // principal a modifié d'autre est gardé, et une ligne qu'il a supprimée ne revient pas.
            let key = c["key"].as_object().ok_or("clé absente")?;
            if table == "settings" && !key.get("key").and_then(|k| k.as_str()).is_some_and(shared_setting) {
                continue;
            }
            if !keys.iter().all(|k| key.contains_key(*k)) {
                return Err(format!("clé incomplète pour {table}"));
            }
            let cols: Vec<&String> = changes.keys().filter(|k| known.contains(k) && !keys.contains(&k.as_str())).collect();
            if cols.is_empty() {
                continue;
            }
            let sets = cols.iter().enumerate().map(|(i, c)| format!("{c} = ?{}", i + 1)).collect::<Vec<_>>().join(", ");
            let wh = keys.iter().enumerate().map(|(i, k)| format!("{k} = ?{}", cols.len() + i + 1)).collect::<Vec<_>>().join(" AND ");
            let vals: Vec<Value> = cols.iter().map(|c| from_json(&changes[c.as_str()])).chain(keys.iter().map(|k| from_json(&key[*k]))).collect();
            n += tx.execute(&format!("UPDATE {table} SET {sets} WHERE {wh}"), params_from_iter(vals.iter())).map_err(e)?;
            continue;
        }
        let row = c["row"].as_object().ok_or("ligne absente")?;
        if table == "settings" && !row.get("key").and_then(|k| k.as_str()).is_some_and(shared_setting) {
            continue;
        }
        let cols: Vec<&String> = row.keys().filter(|k| known.contains(k)).collect();
        if !keys.iter().all(|k| cols.iter().any(|c| c.as_str() == *k)) {
            return Err(format!("clé incomplète pour {table}"));
        }
        let names = cols.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ");
        let marks = (1..=cols.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
        let sets: Vec<String> = cols.iter().filter(|c| !keys.contains(&c.as_str())).map(|c| format!("{c} = excluded.{c}")).collect();
        let tail = if sets.is_empty() { "DO NOTHING".to_string() } else { format!("DO UPDATE SET {}", sets.join(", ")) };
        let sql = format!("INSERT INTO {table} ({names}) VALUES ({marks}) ON CONFLICT({}) {tail}", keys.join(", "));
        let vals: Vec<Value> = cols.iter().map(|c| from_json(&row[c.as_str()])).collect();
        n += tx.execute(&sql, params_from_iter(vals.iter())).map_err(e)?;
    }
    tx.commit().map_err(e)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, title, status) VALUES (1, 'a.wav', 'a.wav', 'Nuit', 'idea'), (2, 'b.wav', 'b.wav', 'Jour', 'idea');
             INSERT INTO tags (id, name) VALUES (1, 'club');
             INSERT INTO track_tags (track_id, tag_id) VALUES (1, 1);
             INSERT INTO releases (id, title) VALUES (1, 'Sortie');
             INSERT INTO release_tracks (release_id, track_id, position) VALUES (1, 1, 1);
             INSERT INTO settings (key, value) VALUES ('pub_city', 'Marseille'), ('accent_color', '#123456');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn writes_travel_to_the_mini() {
        let (mini, copy) = (base(), base());
        install(&copy).unwrap();
        assert_eq!(waiting(&copy), 0);
        copy.execute("UPDATE tracks SET status = 'wip' WHERE id = 1", []).unwrap();
        copy.execute("UPDATE tracks SET status = 'done', notes = 'prêt' WHERE id = 1", []).unwrap();
        copy.execute("DELETE FROM tracks WHERE id = 2", []).unwrap();
        copy.execute("INSERT INTO releases (title) VALUES ('Nouvelle')", []).unwrap();
        let new_release = copy.last_insert_rowid();
        assert!(new_release >= LOCAL_IDS, "les identifiants de la copie ne prennent pas ceux du PC principal");
        copy.execute("INSERT INTO release_tracks (release_id, track_id, position) VALUES (?1, 1, 1)", [new_release]).unwrap();
        copy.execute("UPDATE settings SET value = 'Lyon' WHERE key = 'pub_city'", []).unwrap();
        copy.execute("UPDATE settings SET value = '#654321' WHERE key = 'accent_color'", []).unwrap();
        let (changes, last) = pending(&copy, 1000).unwrap();
        assert_eq!(changes.len(), 5, "deux écritures de la même ligne n'en font qu'une : {changes:?}");
        apply(&mini, &changes).unwrap();
        sent(&copy, last).unwrap();
        assert_eq!(waiting(&copy), 0);
        let (status, notes): (String, String) = mini.query_row("SELECT status, notes FROM tracks WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((status.as_str(), notes.as_str()), ("done", "prêt"));
        assert_eq!(mini.query_row("SELECT COUNT(*) FROM tracks WHERE id = 2", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(mini.query_row("SELECT COUNT(*) FROM release_tracks WHERE release_id = ?1", [new_release], |r| r.get::<_, i64>(0)).unwrap(), 1);
        let set = |k: &str| mini.query_row("SELECT value FROM settings WHERE key = ?1", [k], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(set("pub_city"), "Lyon");
        assert_eq!(set("accent_color"), "#123456", "le réglage d'une machine reste sur elle");
        // Une écriture de la ligne ne vide pas ce qui en dépend (pas de suppression en cascade).
        assert_eq!(mini.query_row("SELECT COUNT(*) FROM track_tags WHERE track_id = 1", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn what_the_mini_changed_meanwhile_stays() {
        let (mini, copy) = (base(), base());
        install(&copy).unwrap();
        // Pendant que l'autre PC travaille, le PC principal analyse le morceau et en supprime un autre.
        mini.execute("UPDATE tracks SET bpm = 124, status = 'wip' WHERE id = 1", []).unwrap();
        mini.execute("DELETE FROM tracks WHERE id = 2", []).unwrap();
        // L'autre PC renomme le premier et annote le second.
        copy.execute("UPDATE tracks SET title = 'Nuit blanche' WHERE id = 1", []).unwrap();
        copy.execute("UPDATE tracks SET title = 'Jour (v2)' WHERE id = 1", []).unwrap();
        copy.execute("UPDATE tracks SET notes = 'à revoir' WHERE id = 2", []).unwrap();
        let (changes, last) = pending(&copy, 1000).unwrap();
        assert_eq!(changes[0]["changes"], json!({ "title": "Jour (v2)" }), "seulement la colonne changée : {changes:?}");
        assert_eq!(apply(&mini, &changes).unwrap(), 1, "la ligne supprimée n'est pas comptée");
        sent(&copy, last).unwrap();
        let (title, bpm, status): (String, f64, String) = mini.query_row("SELECT title, bpm, status FROM tracks WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!((title.as_str(), bpm, status.as_str()), ("Jour (v2)", 124.0, "wip"), "l'analyse et le statut du PC principal restent");
        assert_eq!(mini.query_row("SELECT COUNT(*) FROM tracks WHERE id = 2", [], |r| r.get::<_, i64>(0)).unwrap(), 0, "une ligne supprimée ne revient pas");
        // Tout est parti : plus rien d'avant à garder.
        assert_eq!(copy.query_row("SELECT COUNT(*) FROM _avant_tracks", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        // La modification suivante part de la ligne telle qu'elle est maintenant.
        copy.execute("UPDATE tracks SET notes = 'fini' WHERE id = 1", []).unwrap();
        let (changes, _) = pending(&copy, 1000).unwrap();
        assert_eq!(changes[0]["changes"], json!({ "notes": "fini" }));
    }

    #[test]
    fn a_row_made_on_the_copy_arrives_whole() {
        let (mini, copy) = (base(), base());
        install(&copy).unwrap();
        copy.execute("INSERT INTO tags (name) VALUES ('nuit')", []).unwrap();
        let tag = copy.last_insert_rowid();
        copy.execute("UPDATE tags SET name = 'nuit blanche' WHERE id = ?1", [tag]).unwrap();
        copy.execute("INSERT INTO track_tags (track_id, tag_id) VALUES (1, ?1)", [tag]).unwrap();
        let (changes, last) = pending(&copy, 1000).unwrap();
        assert!(changes.iter().all(|c| c.get("changes").is_none()), "créée ici : la ligne entière, même modifiée ensuite");
        apply(&mini, &changes).unwrap();
        sent(&copy, last).unwrap();
        assert_eq!(mini.query_row("SELECT name FROM tags WHERE id = ?1", [tag], |r| r.get::<_, String>(0)).unwrap(), "nuit blanche");
        assert_eq!(copy.query_row("SELECT COUNT(*) FROM _nouvelles", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        // Envoyée une fois : la suite ne renvoie que ce qui change.
        copy.execute("UPDATE tags SET name = 'nuit' WHERE id = ?1", [tag]).unwrap();
        let (changes, _) = pending(&copy, 1000).unwrap();
        assert_eq!(changes[0]["changes"], json!({ "name": "nuit" }));
    }

    #[test]
    fn a_copy_from_an_older_version_is_brought_up_to_date() {
        let (mini, copy) = (base(), base());
        // Les déclencheurs d'avant : le journal seul, sans lignes d'avant.
        copy.execute_batch(
            "CREATE TABLE _journal (n INTEGER PRIMARY KEY AUTOINCREMENT, tbl TEXT NOT NULL, op TEXT NOT NULL, k1, k2);
             CREATE TRIGGER _ju_tracks AFTER UPDATE ON tracks BEGIN INSERT INTO _journal (tbl, op, k1, k2) VALUES ('tracks', 'set', NEW.id, NULL); END;",
        )
        .unwrap();
        copy.execute("UPDATE tracks SET title = 'Ancienne' WHERE id = 1", []).unwrap();
        // Au démarrage suivant, la copie est remise à jour : l'écriture en attente part comme avant, ligne entière.
        install(&copy).unwrap();
        install(&copy).unwrap();
        copy.execute("UPDATE tracks SET notes = 'nouvelle' WHERE id = 2", []).unwrap();
        let (changes, last) = pending(&copy, 1000).unwrap();
        assert_eq!(changes.len(), 2);
        assert!(changes[0].get("changes").is_none() && changes[0]["row"]["title"] == "Ancienne");
        assert_eq!(changes[1]["changes"], json!({ "notes": "nouvelle" }));
        apply(&mini, &changes).unwrap();
        sent(&copy, last).unwrap();
        assert_eq!(waiting(&copy), 0);
        assert_eq!(mini.query_row("SELECT notes FROM tracks WHERE id = 2", [], |r| r.get::<_, String>(0)).unwrap(), "nouvelle");
    }

    #[test]
    fn a_saved_view_from_the_copy_stays_a_simple_read() {
        let mini = base();
        let view = |id: i64, w: &str, p: &str| json!({ "table": "saved_views", "op": "set", "row": { "id": id, "name": "Vue", "where_sql": w, "params": p } });
        let good = [view(1, "bpm BETWEEN ? AND ? AND LOWER(genre) = LOWER(?)", "[120, 128, \"house\"]"), view(2, "file_format = 'wav' AND id IN (SELECT track_id FROM release_tracks)", "[]")];
        assert_eq!(apply(&mini, &good).unwrap(), 2);
        let bad = [
            view(3, "1); DELETE FROM tracks; --", "[]"),
            view(4, "id IN (WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT x FROM c)", "[]"),
            view(5, "length(randomblob(1000000000)) > 0", "[]"),
            view(6, "bpm > ?", "[]"),
            view(7, "no_such_column = 1", "[]"),
        ];
        assert_eq!(apply(&mini, &bad).unwrap(), 0, "aucune condition douteuse n'entre");
        // Modifier seulement les paramètres d'une vue : vérifiés avec sa condition déjà là.
        let params = |p: &str| json!({ "table": "saved_views", "op": "set", "key": { "id": 1 }, "changes": { "params": p } });
        assert_eq!(apply(&mini, &[params("[100, 110, \"techno\"]")]).unwrap(), 1);
        assert_eq!(apply(&mini, &[params("[100]")]).unwrap(), 0, "un paramètre manque");
        assert_eq!(mini.query_row("SELECT COUNT(*) FROM saved_views", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    }

    #[test]
    fn a_request_cannot_reach_other_tables_or_columns() {
        let mini = base();
        assert!(apply(&mini, &[json!({ "table": "sqlite_master", "op": "set", "row": { "name": "x" } })]).is_err());
        assert!(apply(&mini, &[json!({ "table": "tracks; DROP TABLE tracks", "op": "del", "key": { "id": 1 } })]).is_err());
        // Une colonne inconnue est ignorée : son nom n'entre jamais dans la requête.
        apply(&mini, &[json!({ "table": "tracks", "op": "set", "row": { "id": 1, "path": "a.wav", "filename": "a.wav", "x = 1; --": "y", "title": "Ok" } })]).unwrap();
        assert_eq!(mini.query_row("SELECT title FROM tracks WHERE id = 1", [], |r| r.get::<_, String>(0)).unwrap(), "Ok");
        // Pareil pour les colonnes changées : la clé ne bouge pas, une colonne inconnue n'entre pas dans la requête.
        apply(&mini, &[json!({ "table": "tracks", "op": "set", "key": { "id": 1 }, "changes": { "id": 9, "x = 1; --": "y", "title": "Bien" } })]).unwrap();
        assert_eq!(mini.query_row("SELECT title FROM tracks WHERE id = 1", [], |r| r.get::<_, String>(0)).unwrap(), "Bien");
        assert!(apply(&mini, &[json!({ "table": "release_tracks", "op": "set", "key": { "release_id": 1 }, "changes": { "position": 2 } })]).is_err(), "clé incomplète");
        // Un réglage propre à une machine ne passe pas non plus par là.
        apply(&mini, &[json!({ "table": "settings", "op": "set", "key": { "key": "accent_color" }, "changes": { "value": "#000000" } })]).unwrap();
        assert_eq!(mini.query_row("SELECT value FROM settings WHERE key = 'accent_color'", [], |r| r.get::<_, String>(0)).unwrap(), "#123456");
    }
}
