//! Lecture de la Carrière pour l'app mobile : même requêtes et même JSON que Studio Vault
//! (`composer_vault/mobile/library.py`), pour que l'app Android s'y retrouve à l'identique.
//! La base est ouverte en lecture seule, à chaque requête.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use otoko_library::career::{camelot, nature_short, release_label, status_label};
use rusqlite::functions::FunctionFlags;
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, Row, params_from_iter};
use serde_json::{Value as Json, json};

/// Ordres de tri proposés par l'app.
fn sort_sql(sort: &str) -> &'static str {
    match sort {
        "title" => "LOWER(COALESCE(NULLIF(t.title, ''), t.filename)) ASC",
        "bpm" => "t.bpm IS NULL, t.bpm ASC",
        "key" => "camelot_order IS NULL, camelot_order ASC, t.bpm ASC",
        "lufs" => "t.lufs IS NULL, t.lufs DESC",
        "duration" => "t.duration_s IS NULL, t.duration_s DESC",
        "plays" => "t.play_count DESC, t.last_played DESC",
        "recent" => "t.last_played IS NULL, t.last_played DESC",
        "published" => {
            "published_date IS NULL, published_date DESC, (SELECT MIN(rt.position) FROM release_tracks rt WHERE rt.track_id = t.id), t.added_at DESC"
        }
        _ => "t.added_at DESC, t.id DESC",
    }
}

/// Date de publication (AAAA-MM-JJ) : `published_at` s'il est rempli, sinon la première sortie déjà parue.
const PUB_DATE: &str = "COALESCE(NULLIF(SUBSTR(COALESCE(t.published_at, ''), 1, 10), ''),
    (SELECT MIN(SUBSTR(r.release_date, 1, 10)) FROM release_tracks rt JOIN releases r ON r.id = rt.release_id
     WHERE rt.track_id = t.id AND COALESCE(r.release_date, '') != ''
       AND SUBSTR(r.release_date, 1, 10) <= date('now', 'localtime')))";

fn only_upcoming() -> String {
    format!(
        "({PUB_DATE} IS NULL AND EXISTS (SELECT 1 FROM release_tracks rt JOIN releases r ON r.id = rt.release_id
         WHERE rt.track_id = t.id AND SUBSTR(r.release_date, 1, 10) > date('now', 'localtime')))"
    )
}

fn track_cols() -> String {
    format!(
        "t.id, t.path, t.filename, t.title, t.artist, t.album, t.genre, t.year, t.duration_s, t.samplerate, t.channels, t.file_format,
         t.file_size, t.bitrate_kbps, t.bpm, t.music_key, t.lufs, t.status, t.role, t.parent_id, t.version_label, t.nature, t.mood_auto,
         t.energy, t.added_at, t.last_played, t.play_count, t.published_at, t.sc_url, IFNULL(t.pick, 0) AS pick, {PUB_DATE} AS published_date"
    )
}

pub const MAIN_ROLES: [&str; 2] = ["mixdown", "render"];

/// Ouvre la Carrière en lecture seule, avec la fonction `camelot_rank` des requêtes.
pub fn connect(db: &Path) -> rusqlite::Result<Connection> {
    let con = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    con.busy_timeout(std::time::Duration::from_secs(5))?;
    con.create_scalar_function("camelot_rank", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
        let k: Option<String> = ctx.get(0)?;
        Ok(k.and_then(|k| camelot::rank(camelot::camelot(&k))))
    })?;
    Ok(con)
}

fn s(r: &Row, k: &str) -> String {
    r.get::<_, Option<String>>(k).ok().flatten().unwrap_or_default()
}

fn f(r: &Row, k: &str) -> Option<f64> {
    r.get::<_, Option<f64>>(k).ok().flatten()
}

fn i(r: &Row, k: &str) -> Option<i64> {
    r.get::<_, Option<i64>>(k).ok().flatten()
}

fn round(x: f64, d: i32) -> f64 {
    let p = 10f64.powi(d);
    (x * p).round() / p
}

fn release_by_track(con: &Connection) -> rusqlite::Result<HashMap<i64, Json>> {
    let mut out = HashMap::new();
    let mut st = con.prepare(
        "SELECT rt.track_id, r.id, r.title, r.rtype, r.release_date, rt.position FROM release_tracks rt JOIN releases r ON r.id = rt.release_id ORDER BY r.release_date DESC",
    )?;
    let mut q = st.query([])?;
    while let Some(r) = q.next()? {
        let tid: i64 = r.get(0)?;
        out.entry(tid).or_insert_with(|| {
            let t = s(r, "rtype");
            // La place du morceau dans la sortie : le téléphone range ainsi ce qu'il garde hors ligne.
            json!({ "id": r.get::<_, i64>(1).unwrap_or(0), "title": s(r, "title"), "type": t, "type_label": if t.is_empty() { "Sortie".into() } else { release_label(&t) }, "date": s(r, "release_date"), "position": i(r, "position") })
        });
    }
    Ok(out)
}

pub fn track_json(r: &Row, releases: &HashMap<i64, Json>) -> Json {
    let id = i(r, "id").unwrap_or(0);
    let filename = s(r, "filename");
    let mut title = s(r, "title").trim().to_string();
    if title.is_empty() {
        title = Path::new(&filename).file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
    }
    let nature = Some(s(r, "nature")).filter(|n| !n.is_empty()).unwrap_or_else(|| "original".into());
    let mood = s(r, "mood_auto").trim().to_string();
    let status = Some(s(r, "status")).filter(|x| !x.is_empty()).unwrap_or_else(|| "idea".into());
    let key = s(r, "music_key");
    let format = Some(s(r, "file_format")).filter(|x| !x.is_empty()).unwrap_or_else(|| Path::new(&filename).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()).to_lowercase();
    json!({
        "id": id,
        "title": title,
        "artist": s(r, "artist"),
        "album": s(r, "album"),
        "genre": s(r, "genre"),
        "year": i(r, "year"),
        "duration": f(r, "duration_s").filter(|d| *d != 0.0).map(|d| round(d, 2)),
        "bpm": f(r, "bpm").filter(|b| *b != 0.0).map(|b| round(b, 1)),
        "key": key,
        "camelot": camelot::camelot(&key),
        "lufs": f(r, "lufs").map(|l| round(l, 1)),
        "mood": mood,
        "moods": mood.split(',').map(|m| m.trim()).filter(|m| !m.is_empty()).collect::<Vec<_>>(),
        "energy": f(r, "energy"),
        "nature": nature,
        "nature_label": nature_short(&nature),
        "status": status,
        "status_label": status_label(&status),
        "role": r.get::<_, Option<String>>("role").ok().flatten(),
        "parent_id": i(r, "parent_id"),
        "version_label": s(r, "version_label"),
        "format": format,
        "samplerate": i(r, "samplerate"),
        "channels": i(r, "channels"),
        "size": i(r, "file_size"),
        "added_at": r.get::<_, Option<String>>("added_at").ok().flatten(),
        "last_played": r.get::<_, Option<String>>("last_played").ok().flatten(),
        "play_count": i(r, "play_count").unwrap_or(0),
        "pick": i(r, "pick").unwrap_or(0),
        "published_at": s(r, "published_at"),
        "published_date": s(r, "published_date"),
        "soundcloud": s(r, "sc_url"),
        "release": releases.get(&id).cloned().unwrap_or(Json::Null),
    })
}

/// Filtres de la liste des morceaux (les mêmes que l'app envoie).
#[derive(Default, Clone)]
pub struct List {
    pub q: String,
    pub sort: String,
    /// « main » (morceaux principaux) ou « all ».
    pub scope: String,
    pub nature: String,
    pub status: String,
    pub mood: String,
    pub key: String,
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    pub compatible_with: Option<i64>,
    pub release: Option<i64>,
    pub ids: Option<Vec<i64>>,
    pub limit: i64,
    pub offset: i64,
}

pub fn list_tracks(con: &Connection, a: &List) -> rusqlite::Result<Vec<Json>> {
    let mut wh: Vec<String> = vec!["t.archived = 0".into(), "t.missing = 0".into()];
    let mut p: Vec<Value> = Vec::new();
    let marks = |n: usize| vec!["?"; n].join(",");
    if a.scope.is_empty() || a.scope == "main" {
        wh.push(format!("t.role IN ({})", marks(MAIN_ROLES.len())));
        p.extend(MAIN_ROLES.iter().map(|r| Value::Text(r.to_string())));
        wh.push("t.parent_id IS NULL".into());
    }
    let list = |s: &str| s.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let natures = list(&a.nature);
    if !natures.is_empty() {
        wh.push(format!("COALESCE(t.nature, 'original') IN ({})", marks(natures.len())));
        p.extend(natures.into_iter().map(Value::Text));
    }
    let sts = list(&a.status);
    if !sts.is_empty() {
        wh.push(format!("t.status IN ({})", marks(sts.len())));
        p.extend(sts.into_iter().map(Value::Text));
    }
    if !a.mood.is_empty() {
        wh.push("t.mood_auto LIKE ?".into());
        p.push(Value::Text(format!("%{}%", a.mood)));
    }
    let keys = list(&a.key);
    if !keys.is_empty() {
        wh.push(format!("camelot_rank(t.music_key) IN ({})", marks(keys.len())));
        p.extend(keys.iter().map(|k| Value::Integer(camelot::rank(k).unwrap_or(-1))));
    }
    if let Some(b) = a.bpm_min {
        wh.push("t.bpm >= ?".into());
        p.push(Value::Real(b));
    }
    if let Some(b) = a.bpm_max {
        wh.push("t.bpm <= ?".into());
        p.push(Value::Real(b));
    }
    if let Some(rid) = a.release {
        wh.push("t.id IN (SELECT track_id FROM release_tracks WHERE release_id = ?)".into());
        p.push(Value::Integer(rid));
    }
    if let Some(ids) = &a.ids {
        wh.push(if ids.is_empty() { "t.id IN (NULL)".into() } else { format!("t.id IN ({})", marks(ids.len())) });
        p.extend(ids.iter().map(|i| Value::Integer(*i)));
    }
    if let Some(cw) = a.compatible_with {
        let reference: Option<(i64, Option<f64>, Option<String>)> = con.query_row("SELECT id, bpm, music_key FROM tracks WHERE id = ?1", [cw], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok();
        if let Some((rid, bpm, key)) = reference {
            wh.push("t.id != ?".into());
            p.push(Value::Integer(rid));
            let keys = camelot::compatible_keys(key.as_deref().unwrap_or(""));
            if !keys.is_empty() {
                wh.push(format!("t.music_key IN ({})", marks(keys.len())));
                p.extend(keys.iter().map(|k| Value::Text(k.to_string())));
            }
            if let Some(b) = bpm.filter(|b| *b > 0.0) {
                // même tempo à 6 % près, demi et double tempo compris
                let mut tempo = Vec::new();
                for target in [b, b / 2.0, b * 2.0] {
                    tempo.push("(t.bpm BETWEEN ? AND ?)");
                    p.push(Value::Real(target * 0.94));
                    p.push(Value::Real(target * 1.06));
                }
                wh.push(format!("({})", tempo.join(" OR ")));
            }
        }
    }
    for word in a.q.split_whitespace() {
        let like = format!("%{word}%");
        let mut clause = "(t.title LIKE ? OR t.filename LIKE ? OR t.artist LIKE ? OR t.album LIKE ? OR t.genre LIKE ? OR t.mood_auto LIKE ? OR t.music_key = ? OR CAST(ROUND(t.bpm) AS INT) = ?".to_string();
        for _ in 0..6 {
            p.push(Value::Text(like.clone()));
        }
        p.push(Value::Text(word.to_string()));
        p.push(Value::Integer(word.parse::<i64>().unwrap_or(-1)));
        if let Some(r) = camelot::rank(word) {
            clause.push_str(" OR camelot_rank(t.music_key) = ?");
            p.push(Value::Integer(r));
        }
        wh.push(clause + ")");
    }
    if a.sort == "published" {
        wh.push(format!("NOT {}", only_upcoming()));
    }
    let sql = format!(
        "SELECT {}, camelot_rank(t.music_key) AS camelot_order FROM tracks t WHERE {} ORDER BY {} LIMIT ? OFFSET ?",
        track_cols(),
        wh.join(" AND "),
        sort_sql(&a.sort)
    );
    p.push(Value::Integer(if a.limit > 0 { a.limit } else { 5000 }));
    p.push(Value::Integer(a.offset.max(0)));
    let rels = release_by_track(con)?;
    let mut st = con.prepare(&sql)?;
    let rows = st.query_map(params_from_iter(p.iter()), |r| Ok(track_json(r, &rels)))?;
    rows.collect()
}

pub fn get_track(con: &Connection, id: i64) -> rusqlite::Result<Option<Json>> {
    let rels = release_by_track(con)?;
    let mut st = con.prepare(&format!("SELECT {} FROM tracks t WHERE t.id = ?1", track_cols()))?;
    let Some(mut out) = st.query_map([id], |r| Ok(track_json(r, &rels)))?.next().transpose()? else { return Ok(None) };
    let mut st = con.prepare(&format!("SELECT {} FROM tracks t WHERE t.parent_id = ?1 AND t.archived = 0 AND t.missing = 0 ORDER BY t.added_at", track_cols()))?;
    let versions: Vec<Json> = st.query_map([id], |r| Ok(track_json(r, &rels)))?.collect::<rusqlite::Result<_>>()?;
    let mut st = con.prepare("SELECT time_s, label FROM markers WHERE track_id = ?1 ORDER BY time_s")?;
    let markers: Vec<Json> = st.query_map([id], |r| Ok(json!({ "time": r.get::<_, f64>(0)?, "label": r.get::<_, String>(1)? })))?.collect::<rusqlite::Result<_>>()?;
    out["versions"] = Json::Array(versions);
    out["markers"] = Json::Array(markers);
    Ok(Some(out))
}

/// Le fichier d'un morceau. Seul un fichier audio sort d'ici : même une base modifiée (un chemin écrit depuis un autre
/// appareil, par exemple) ne peut pas faire lire autre chose sur ce PC.
pub fn track_path(con: &Connection, id: i64) -> Option<PathBuf> {
    con.query_row("SELECT path FROM tracks WHERE id = ?1", [id], |r| r.get::<_, String>(0)).ok().map(PathBuf::from).filter(|p| otoko_library::career::scan::is_audio(p))
}

/// Le nom d'artiste réglé dans le logiciel (vide s'il ne l'est pas).
pub fn artist_name(con: &Connection) -> String {
    otoko_library::career::textes::brand_load(con).artist_name.trim().to_string()
}

fn today() -> String {
    // date('now', 'localtime') de SQLite : la même horloge que les requêtes.
    Connection::open_in_memory().and_then(|c| c.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))).unwrap_or_default()
}

/// Sorties, les plus récentes d'abord. « upcoming » : date de sortie pas encore atteinte.
pub fn list_releases(con: &Connection) -> rusqlite::Result<Vec<Json>> {
    let today = today();
    let mut st = con.prepare(
        "SELECT r.id, r.title, r.rtype, r.release_date, r.description, r.sc_link,
            (SELECT COUNT(*) FROM release_tracks rt JOIN tracks t ON t.id = rt.track_id WHERE rt.release_id = r.id AND t.archived = 0) AS n,
            (SELECT COALESCE(SUM(t.duration_s), 0) FROM release_tracks rt JOIN tracks t ON t.id = rt.track_id WHERE rt.release_id = r.id AND t.archived = 0) AS dur
         FROM releases r ORDER BY r.release_date DESC, r.id DESC",
    )?;
    let rows = st.query_map([], |r| {
        let d: String = s(r, "release_date").chars().take(10).collect();
        let t = s(r, "rtype");
        Ok(json!({
            "id": i(r, "id").unwrap_or(0),
            "title": s(r, "title"),
            "type": t,
            "type_label": if t.is_empty() { "Sortie".to_string() } else { release_label(&t) },
            "date": d,
            "description": s(r, "description"),
            "soundcloud": s(r, "sc_link"),
            "tracks": i(r, "n").unwrap_or(0),
            "duration": round(f(r, "dur").unwrap_or(0.0), 1),
            "upcoming": !d.is_empty() && d > today,
        }))
    })?;
    rows.collect()
}

pub fn get_release(con: &Connection, id: i64) -> rusqlite::Result<Option<Json>> {
    Ok(list_releases(con)?.into_iter().find(|r| r["id"] == id))
}

/// Morceaux d'une sortie dans l'ordre de l'album : le numéro de piste s'ils en ont tous un, tous
/// différents ; sinon l'ordre de la fiche de sortie.
pub fn release_tracks(con: &Connection, rid: i64) -> rusqlite::Result<Vec<Json>> {
    let rels = release_by_track(con)?;
    let mut st = con.prepare(&format!(
        "SELECT {}, rt.position AS rt_position, t.track_no AS t_track_no FROM release_tracks rt JOIN tracks t ON t.id = rt.track_id
         WHERE rt.release_id = ?1 AND t.archived = 0 ORDER BY rt.position, t.track_no, t.id",
        track_cols()
    ))?;
    let mut rows: Vec<(Option<i64>, i64, Json)> = st
        .query_map([rid], |r| Ok((i(r, "t_track_no"), i(r, "rt_position").unwrap_or(0), track_json(r, &rels))))?
        .collect::<rusqlite::Result<_>>()?;
    let nos: Vec<Option<i64>> = rows.iter().map(|r| r.0).collect();
    let distinct: std::collections::HashSet<i64> = nos.iter().flatten().copied().collect();
    if !rows.is_empty() && nos.iter().all(|n| n.is_some_and(|n| n > 0)) && distinct.len() == nos.len() {
        rows.sort_by_key(|r| (r.0, r.1));
    }
    Ok(rows.into_iter().map(|r| r.2).collect())
}

/// Sources d'une pochette, dans l'ordre : (« img », image) puis (« emb », fichier audio).
pub type Sources = Vec<(&'static str, PathBuf)>;

/// Pochette d'un morceau : la sienne, celle de sa sortie (la plus récente qui en a une), puis
/// l'image intégrée au fichier.
pub fn cover_sources(con: &Connection, tid: i64) -> Sources {
    let row: Option<(String, String)> = con.query_row("SELECT path, IFNULL(cover_path, '') FROM tracks WHERE id = ?1", [tid], |r| Ok((r.get(0)?, r.get(1)?))).ok();
    let Some((path, own)) = row else { return Vec::new() };
    let rel: String = con
        .query_row(
            "SELECT r.cover_path FROM release_tracks rt JOIN releases r ON r.id = rt.release_id WHERE rt.track_id = ?1 AND r.cover_path != '' ORDER BY r.release_date DESC LIMIT 1",
            [tid],
            |r| r.get(0),
        )
        .unwrap_or_default();
    let mut out: Sources = Vec::new();
    for p in [own, rel] {
        if !p.is_empty() && Path::new(&p).exists() {
            out.push(("img", PathBuf::from(p)));
        }
    }
    if !path.is_empty() {
        out.push(("emb", PathBuf::from(path)));
    }
    out
}

/// Pochette d'une sortie : celle de sa fiche (« rel »), sinon celle de son premier morceau.
pub fn release_cover_sources(con: &Connection, rid: i64) -> Sources {
    let mut out: Sources = Vec::new();
    if let Ok(p) = con.query_row("SELECT IFNULL(cover_path, '') FROM releases WHERE id = ?1", [rid], |r| r.get::<_, String>(0)) {
        if !p.is_empty() {
            out.push(("rel", PathBuf::from(p)));
        }
    }
    let first: Option<i64> = con
        .query_row(
            "SELECT rt.track_id FROM release_tracks rt JOIN tracks t ON t.id = rt.track_id WHERE rt.release_id = ?1 AND t.archived = 0
             ORDER BY rt.position, t.track_no, t.id LIMIT 1",
            [rid],
            |r| r.get(0),
        )
        .ok();
    if let Some(t) = first {
        for s in cover_sources(con, t) {
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

pub fn list_views(con: &Connection) -> rusqlite::Result<Vec<Json>> {
    let mut st = con.prepare("SELECT id, name FROM saved_views ORDER BY name")?;
    let rows = st.query_map([], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)? })))?;
    rows.collect()
}

/// Les morceaux d'une vue enregistrée (sa requête telle quelle).
pub fn view_track_ids(con: &Connection, vid: i64) -> Option<Vec<i64>> {
    let (w, params): (String, String) = con.query_row("SELECT where_sql, IFNULL(params, '[]') FROM saved_views WHERE id = ?1", [vid], |r| Ok((r.get(0)?, r.get(1)?))).ok()?;
    let params: Vec<Value> = serde_json::from_str::<Vec<Json>>(&params)
        .unwrap_or_default()
        .into_iter()
        .map(|v| match v {
            Json::Number(n) if n.is_i64() => Value::Integer(n.as_i64().unwrap_or(0)),
            Json::Number(n) => Value::Real(n.as_f64().unwrap_or(0.0)),
            Json::String(s) => Value::Text(s),
            Json::Bool(b) => Value::Integer(b as i64),
            _ => Value::Null,
        })
        .collect();
    let w = if w.trim().is_empty() { "1".to_string() } else { w };
    let sql = format!("SELECT t.id FROM tracks t WHERE t.archived = 0 AND ({w})");
    let ids = con.prepare(&sql).and_then(|mut st| st.query_map(params_from_iter(params.iter()), |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>());
    Some(ids.unwrap_or_default())
}

pub fn summary(con: &Connection) -> Json {
    let n: i64 = con
        .query_row("SELECT COUNT(*) FROM tracks WHERE archived = 0 AND missing = 0 AND parent_id IS NULL AND role IN ('mixdown', 'render')", [], |r| r.get(0))
        .unwrap_or(0);
    let total: i64 = con.query_row("SELECT COUNT(*) FROM tracks WHERE archived = 0 AND missing = 0", [], |r| r.get(0)).unwrap_or(0);
    let rel: i64 = con.query_row("SELECT COUNT(*) FROM releases", [], |r| r.get(0)).unwrap_or(0);
    json!({ "tracks": n, "all_tracks": total, "releases": rel })
}
