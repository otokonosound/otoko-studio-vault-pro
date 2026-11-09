//! Import d'une base Studio Vault (sa copie, ouverte en lecture seule) : toutes les tables, dans
//! l'ordre des dépendances. Une ligne déjà présente (même chemin de fichier, même nom…) n'est pas
//! recopiée ; une nouvelle garde son identifiant s'il est libre (dans une Carrière neuve, tous le
//! gardent : les liens de partage et les vues enregistrées restent valides), sinon en reçoit un
//! autre et les références suivent.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, Transaction, params_from_iter};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub songs: usize,
    pub releases: usize,
    pub clients: usize,
    pub skipped: usize,
    pub drafts: usize,
    pub prompts: usize,
    pub tags: usize,
    pub markers: usize,
    pub views: usize,
    pub sets: usize,
    pub projects: usize,
    pub folders: usize,
    /// Liens de partage repris (base `partages.db` à part, importée par l'application).
    pub shares: usize,
    /// Correspondance des identifiants de morceaux et de sorties (ancien → nouveau), pour les partages.
    pub track_ids: HashMap<i64, i64>,
    pub release_ids: HashMap<i64, i64>,
}

type Map = HashMap<i64, i64>;

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut st = conn.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
    st.query_map([], |r| r.get::<_, String>(1))?.collect()
}

fn has_table(conn: &Connection, table: &str) -> bool {
    conn.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |_| Ok(())).is_ok()
}

/// Ce que fait une table : les colonnes qui identifient une ligne déjà là, et le traitement des
/// références (`None` : la ligne ne peut pas venir).
struct Copy<'a> {
    table: &'a str,
    key: &'a [&'a str],
    remap: &'a dyn Fn(&str, Value) -> Option<Value>,
}

/// Recopie une table ; rend (lignes ajoutées, lignes déjà présentes) et remplit `map`.
fn copy(tx: &Transaction, src: &Connection, c: Copy, map: &mut Map) -> rusqlite::Result<(usize, usize)> {
    if !has_table(src, c.table) {
        return Ok((0, 0));
    }
    let dst_cols = columns(tx, c.table)?;
    let src_cols = columns(src, c.table)?;
    let common: Vec<&String> = src_cols.iter().filter(|s| dst_cols.contains(s)).collect();
    let has_id = common.iter().any(|c| c.as_str() == "id");
    let mut read = src.prepare(&format!("SELECT {} FROM \"{}\"", common.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(","), c.table))?;
    let n = common.len();
    let rows: Vec<Vec<Value>> = read.query_map([], |r| (0..n).map(|i| r.get::<_, Value>(i)).collect())?.collect::<rusqlite::Result<_>>()?;
    let (mut added, mut skipped) = (0, 0);
    'rows: for row in rows {
        let old_id = if has_id { match row[common.iter().position(|c| c.as_str() == "id").unwrap()] { Value::Integer(i) => Some(i), _ => None } } else { None };
        let mut vals: Vec<(String, Value)> = Vec::with_capacity(n);
        for (col, v) in common.iter().zip(row) {
            if col.as_str() == "id" {
                continue;
            }
            match (c.remap)(col, v) {
                Some(v) => vals.push((col.to_string(), v)),
                None => continue 'rows,
            }
        }
        // Déjà là ?
        if !c.key.is_empty() {
            let cond: Vec<String> = c.key.iter().map(|k| format!("\"{k}\" IS ?")).collect();
            let kv: Vec<Value> = c.key.iter().map(|k| vals.iter().find(|(c, _)| c == k).map(|(_, v)| v.clone()).unwrap_or(Value::Null)).collect();
            let sel = if has_id { "id" } else { "1" };
            let found: Option<i64> = tx
                .query_row(&format!("SELECT {sel} FROM \"{}\" WHERE {} LIMIT 1", c.table, cond.join(" AND ")), params_from_iter(kv.iter()), |r| r.get(0))
                .ok();
            if let Some(existing) = found {
                if let Some(o) = old_id {
                    map.insert(o, existing);
                }
                skipped += 1;
                continue;
            }
        }
        // L'identifiant d'origine s'il est libre.
        if let Some(o) = old_id {
            let free = tx.query_row(&format!("SELECT 1 FROM \"{}\" WHERE id = ?1", c.table), [o], |_| Ok(())).is_err();
            if free {
                vals.push(("id".into(), Value::Integer(o)));
            }
        }
        let names: Vec<String> = vals.iter().map(|(c, _)| format!("\"{c}\"")).collect();
        let marks: Vec<&str> = vals.iter().map(|_| "?").collect();
        let n = tx.execute(
            &format!("INSERT OR IGNORE INTO \"{}\" ({}) VALUES ({})", c.table, names.join(","), marks.join(",")),
            params_from_iter(vals.iter().map(|(_, v)| v)),
        )?;
        if n == 0 {
            skipped += 1;
            continue;
        }
        if let Some(o) = old_id {
            map.insert(o, tx.last_insert_rowid());
        }
        added += 1;
    }
    Ok((added, skipped))
}

fn through(map: &Map, v: Value) -> Option<Value> {
    match v {
        Value::Integer(i) => map.get(&i).map(|&n| Value::Integer(n)),
        Value::Null => Some(Value::Null),
        _ => None,
    }
}

/// Une référence facultative : suit la correspondance, ou devient vide.
fn optional(map: &Map, v: Value) -> Value {
    through(map, v).unwrap_or(Value::Null)
}

/// Importe la base `src_path` (une copie : elle n'est que lue) dans `dst`.
pub fn import(dst: &mut Connection, src_path: &Path) -> Result<ImportReport, String> {
    let src = Connection::open_with_flags(src_path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    let tx = dst.transaction().map_err(|e| e.to_string())?;
    let mut r = ImportReport::default();
    let same = |_: &str, v: Value| Some(v);
    let (mut projects, mut tags, mut clients, mut prompts, mut tracks, mut releases) = (Map::new(), Map::new(), Map::new(), Map::new(), Map::new(), Map::new());
    let e = |e: rusqlite::Error| e.to_string();
    (r.projects, _) = copy(&tx, &src, Copy { table: "projects", key: &["name"], remap: &same }, &mut projects).map_err(e)?;
    (r.tags, _) = copy(&tx, &src, Copy { table: "tags", key: &["name"], remap: &same }, &mut tags).map_err(e)?;
    (r.clients, _) = copy(&tx, &src, Copy { table: "clients", key: &["name"], remap: &same }, &mut clients).map_err(e)?;
    // Les prompts, puis leurs « parents » (versions dérivées) une fois tous là.
    let no_parent = |c: &str, v: Value| Some(if c == "parent_id" { Value::Null } else { v });
    (r.prompts, _) = copy(&tx, &src, Copy { table: "prompts", key: &["name", "created_at"], remap: &no_parent }, &mut prompts).map_err(e)?;
    relink_parents(&tx, &src, "prompts", &prompts).map_err(e)?;
    let pv = |c: &str, v: Value| if c == "prompt_id" { through(&prompts, v) } else { Some(v) };
    copy(&tx, &src, Copy { table: "prompt_versions", key: &["prompt_id", "version_no"], remap: &pv }, &mut Map::new()).map_err(e)?;
    // Les morceaux : même chemin = même morceau. Les morceaux archivés viennent aussi (vue « Archives »).
    let tr = |c: &str, v: Value| {
        Some(match c {
            "parent_id" => Value::Null,
            "project_id" => optional(&projects, v),
            "prompt_id" => optional(&prompts, v),
            _ => v,
        })
    };
    (r.songs, r.skipped) = copy(&tx, &src, Copy { table: "tracks", key: &["path"], remap: &tr }, &mut tracks).map_err(e)?;
    relink_parents(&tx, &src, "tracks", &tracks).map_err(e)?;
    let tt = |c: &str, v: Value| match c {
        "track_id" => through(&tracks, v),
        "tag_id" => through(&tags, v),
        _ => Some(v),
    };
    copy(&tx, &src, Copy { table: "track_tags", key: &["track_id", "tag_id"], remap: &tt }, &mut Map::new()).map_err(e)?;
    let by_track = |c: &str, v: Value| if c == "track_id" { through(&tracks, v) } else { Some(v) };
    (r.markers, _) = copy(&tx, &src, Copy { table: "markers", key: &["track_id", "time_s", "label"], remap: &by_track }, &mut Map::new()).map_err(e)?;
    (r.releases, _) = copy(&tx, &src, Copy { table: "releases", key: &["title", "release_date"], remap: &same }, &mut releases).map_err(e)?;
    let rt = |c: &str, v: Value| match c {
        "release_id" => through(&releases, v),
        "track_id" => through(&tracks, v),
        _ => Some(v),
    };
    copy(&tx, &src, Copy { table: "release_tracks", key: &["release_id", "track_id"], remap: &rt }, &mut Map::new()).map_err(e)?;
    copy(&tx, &src, Copy { table: "release_kits", key: &["track_id"], remap: &by_track }, &mut Map::new()).map_err(e)?;
    (r.drafts, _) = copy(&tx, &src, Copy { table: "post_drafts", key: &["track_id", "platform", "created_at", "body"], remap: &by_track }, &mut Map::new()).map_err(e)?;
    (r.views, _) = copy(&tx, &src, Copy { table: "saved_views", key: &["name"], remap: &same }, &mut Map::new()).map_err(e)?;
    (r.folders, _) = copy(&tx, &src, Copy { table: "watch_folders", key: &["path"], remap: &same }, &mut Map::new()).map_err(e)?;
    let se = |c: &str, v: Value| match c {
        "set_id" => through(&tracks, v),
        "ref_track_id" => Some(optional(&tracks, v)),
        _ => Some(v),
    };
    (r.sets, _) = copy(&tx, &src, Copy { table: "set_entries", key: &["set_id", "time_s", "title"], remap: &se }, &mut Map::new()).map_err(e)?;
    // Les réglages de Studio Vault (univers de marque, colonnes…) qui n'existent pas encore ici.
    copy(&tx, &src, Copy { table: "settings", key: &["key"], remap: &same }, &mut Map::new()).map_err(e)?;
    tx.commit().map_err(|e| e.to_string())?;
    r.track_ids = tracks;
    r.release_ids = releases;
    Ok(r)
}

/// Les liens « parent » d'une table (versions d'un morceau, prompts dérivés), une fois toutes les
/// lignes là : seulement pour celles qui viennent d'arriver et n'ont pas encore de parent.
fn relink_parents(tx: &Transaction, src: &Connection, table: &str, map: &Map) -> rusqlite::Result<()> {
    if !has_table(src, table) || !columns(src, table)?.iter().any(|c| c == "parent_id") {
        return Ok(());
    }
    let mut st = src.prepare(&format!("SELECT id, parent_id FROM \"{table}\" WHERE parent_id IS NOT NULL"))?;
    let pairs: Vec<(i64, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for (child, parent) in pairs {
        if let (Some(c), Some(p)) = (map.get(&child), map.get(&parent)) {
            tx.execute(&format!("UPDATE \"{table}\" SET parent_id = ?2 WHERE id = ?1 AND parent_id IS NULL"), [c, p])?;
        }
    }
    Ok(())
}
