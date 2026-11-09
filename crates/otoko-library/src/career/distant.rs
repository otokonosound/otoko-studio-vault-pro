//! Le client distant : le PC principal reste le centre et un autre PC travaille sur une copie de sa base. Les lectures sont
//! locales, donc instantanées. Chaque écriture de la copie est notée par des déclencheurs dans `_journal`, puis envoyée
//! au PC principal, qui l'applique. Aucun autre module n'a besoin de savoir qu'il travaille sur une copie.

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

/// Prépare une copie fraîche : le journal, les déclencheurs, les identifiants à part.
pub fn install(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS _journal (n INTEGER PRIMARY KEY AUTOINCREMENT, tbl TEXT NOT NULL, op TEXT NOT NULL, k1, k2);")?;
    for (table, keys) in TABLES {
        let (k1, k2) = (keys[0], keys.get(1).copied());
        let vals = |row: &str| format!("{row}.{k1}, {}", k2.map(|k| format!("{row}.{k}")).unwrap_or_else(|| "NULL".into()));
        let only = |row: &str| if table == "settings" { format!(" WHEN {row}.key LIKE 'pub\\_%' ESCAPE '\\' OR {row}.key LIKE 'artist\\_%' ESCAPE '\\'") } else { String::new() };
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS _ji_{table} AFTER INSERT ON {table}{w} BEGIN INSERT INTO _journal (tbl, op, k1, k2) VALUES ('{table}', 'set', {new}); END;
             CREATE TRIGGER IF NOT EXISTS _ju_{table} AFTER UPDATE ON {table}{w} BEGIN INSERT INTO _journal (tbl, op, k1, k2) VALUES ('{table}', 'set', {new}); END;
             CREATE TRIGGER IF NOT EXISTS _jd_{table} AFTER DELETE ON {table}{wo} BEGIN INSERT INTO _journal (tbl, op, k1, k2) VALUES ('{table}', 'del', {old}); END;",
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

/// Une ligne de la copie, en JSON.
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
        if op == "del" {
            let key: Map<String, Json> = keys.iter().zip(&k).map(|(c, v)| (c.to_string(), value_json(v))).collect();
            out.push(json!({ "table": table, "op": "del", "key": key }));
        } else if let Some(row) = row_json(conn, &table, keys, &k)? {
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

/// Les écritures sont arrivées : elles sortent du journal.
pub fn sent(conn: &Connection, last: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM _journal WHERE n <= ?1", [last])?;
    Ok(())
}

/// Les colonnes d'une table : seules celles-ci sont acceptées dans une requête.
fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare(&format!("PRAGMA table_info({table})"))?.query_map([], |r| r.get::<_, String>(1))?.collect()
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
    fn a_request_cannot_reach_other_tables_or_columns() {
        let mini = base();
        assert!(apply(&mini, &[json!({ "table": "sqlite_master", "op": "set", "row": { "name": "x" } })]).is_err());
        assert!(apply(&mini, &[json!({ "table": "tracks; DROP TABLE tracks", "op": "del", "key": { "id": 1 } })]).is_err());
        // Une colonne inconnue est ignorée : son nom n'entre jamais dans la requête.
        apply(&mini, &[json!({ "table": "tracks", "op": "set", "row": { "id": 1, "path": "a.wav", "filename": "a.wav", "x = 1; --": "y", "title": "Ok" } })]).unwrap();
        assert_eq!(mini.query_row("SELECT title FROM tracks WHERE id = 1", [], |r| r.get::<_, String>(0)).unwrap(), "Ok");
    }
}
