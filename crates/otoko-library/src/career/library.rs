//! La bibliothèque de la Carrière (comme l'écran Bibliothèque de Studio Vault) : la navigation à
//! gauche (vues, mes vues, sorties, dossiers, genres, nature, statut, tempo, tonalités, tags,
//! formats), les filtres en pastilles, la recherche, les colonnes au choix et le tri. Chaque
//! entrée est une condition SQL sur `tracks`, au même format que Studio Vault : une vue
//! enregistrée ici se relit là-bas (et dans l'app mobile).

use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use super::{camelot, nature_short, release_label, search, status_label};

/// Une entrée de la navigation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nav {
    pub section: String,
    pub label: String,
    pub count: i64,
    pub where_sql: String,
    pub params: Vec<Value>,
    pub color: String,
    /// Vue enregistrée (supprimable), et sa recherche.
    pub view: Option<i64>,
    pub search: String,
    /// Dossier surveillé (retirable) ou sous-dossier, et sa profondeur.
    pub folder: Option<(i64, String)>,
    pub depth: u8,
}

pub const ARCHIVE_PATTERNS: [&str; 15] = ["ARCHIVE", "ARCHIVES", "_archive", "_archives", "OLD", "_old", "old", "BACKUP", "backup", "_backup", "TRASH", "_trash", "tmp", "TMP", "tmp"];

/// Les noms de dossiers d'archives (réglage `archive_patterns` de Studio Vault).
pub fn archive_patterns(conn: &Connection) -> Vec<String> {
    let raw: Option<String> = conn.query_row("SELECT value FROM settings WHERE key = 'archive_patterns'", [], |r| r.get(0)).ok();
    match raw.and_then(|r| serde_json::from_str::<Vec<String>>(&r).ok()) {
        Some(v) => v.into_iter().filter(|p| !p.is_empty()).collect(),
        None => {
            let mut v: Vec<String> = ARCHIVE_PATTERNS.iter().map(|s| s.to_string()).collect();
            v.dedup();
            v
        }
    }
}

/// « AND (…) » qui écarte les morceaux archivés et ceux des dossiers d'archives.
pub fn exclude_archives(conn: &Connection) -> (String, Vec<Value>) {
    let mut c = vec!["COALESCE(archived, 0) = 0".to_string()];
    let mut p = Vec::new();
    for pat in archive_patterns(conn).iter().filter(|p| !p.contains('*')) {
        for sep in ['/', '\\'] {
            c.push("LOWER(path) NOT LIKE ?".into());
            p.push(Value::Text(format!("%{sep}{}{sep}%", pat.to_lowercase())));
        }
    }
    (format!(" AND ({})", c.join(" AND ")), p)
}

/// « AND (…) » qui ne garde que les archives.
pub fn only_archives(conn: &Connection) -> (String, Vec<Value>) {
    let mut c = vec!["archived = 1".to_string()];
    let mut p = Vec::new();
    for pat in archive_patterns(conn).iter().filter(|p| !p.contains('*')) {
        for sep in ['/', '\\'] {
            c.push("LOWER(path) LIKE ?".into());
            p.push(Value::Text(format!("%{sep}{}{sep}%", pat.to_lowercase())));
        }
    }
    (format!(" AND ({})", c.join(" OR ")), p)
}

/// Les nouveautés : 14 jours, sans les imports en bloc (40 fichiers ou plus le même jour).
pub fn recent_where(days: i64) -> String {
    let d = "COALESCE(file_date, added_at)";
    format!(
        "missing = 0 AND {d} > datetime('now', 'localtime', '-{days} days') AND substr({d}, 1, 10) NOT IN (SELECT substr({d}, 1, 10) FROM tracks GROUP BY 1 HAVING COUNT(*) >= 40)"
    )
}

pub const TO_PUBLISH_SQL: &str = "missing = 0 AND COALESCE(archived, 0) = 0 AND COALESCE(to_publish, 0) = 1 AND COALESCE(status, '') <> 'released' AND COALESCE(sc_url, '') = '' AND COALESCE(published_at, '') = ''";

pub const BPM_BUCKETS: [(&str, f64, f64); 7] = [
    ("Moins de 90 BPM", 0.0, 90.0),
    ("90 à 110 BPM", 90.0, 110.0),
    ("110 à 120 BPM", 110.0, 120.0),
    ("120 à 130 BPM", 120.0, 130.0),
    ("130 à 140 BPM", 130.0, 140.0),
    ("140 à 160 BPM", 140.0, 160.0),
    ("Plus de 160 BPM", 160.0, 999.0),
];

pub const STATUS_COLORS: [(&str, &str); 7] =
    [("idea", "#8f8f9a"), ("draft", "#5bc0eb"), ("wip", "#f5b942"), ("to_mix", "#ff7043"), ("to_master", "#d64545"), ("done", "#59c17a"), ("released", "#6d6bff")];

pub fn status_color(s: &str) -> &'static str {
    STATUS_COLORS.iter().find(|x| x.0 == s).map(|x| x.1).unwrap_or("#8f8f9a")
}

fn count(conn: &Connection, where_sql: &str, params: &[Value]) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM tracks WHERE {where_sql}"), params_from_iter(params.iter()), |r| r.get(0)).unwrap_or(0)
}

fn join(mut p: Vec<Value>, extra: &[Value]) -> Vec<Value> {
    p.extend(extra.iter().cloned());
    p
}

/// Toute la navigation, avec les nombres de morceaux.
pub fn nav(conn: &Connection) -> Vec<Nav> {
    let (ex, ep) = exclude_archives(conn);
    let mut out = Vec::new();
    let mut add = |section: &str, label: String, where_sql: String, params: Vec<Value>, color: &str| {
        let n = count(conn, &where_sql, &params);
        out.push(Nav { section: section.into(), label, count: n, where_sql, params, color: color.into(), ..Default::default() });
    };
    // ---- Vues
    add("VUES", "Tous les morceaux".into(), format!("missing = 0{ex}"), ep.clone(), "");
    add("VUES", "Nouveaux morceaux".into(), format!("{}{ex}", recent_where(14)), ep.clone(), "");
    add("VUES", "Déjà écoutés".into(), format!("missing = 0 AND last_played IS NOT NULL{ex}"), ep.clone(), "");
    add("VUES", "Sorties Suno".into(), format!("missing = 0 AND {}{ex}", search::SUNO_WHERE), ep.clone(), "#c67bff");
    add("VUES", "Gardés".into(), format!("missing = 0 AND pick = 1{ex}"), ep.clone(), "#59c17a");
    add("VUES", "Rejetés".into(), format!("missing = 0 AND pick = -1{ex}"), ep.clone(), "#d64545");
    add("VUES", "À analyser".into(), format!("missing = 0 AND analyzed_at IS NULL{ex}"), ep.clone(), "#5bc0eb");
    add("VUES", "Fichiers manquants".into(), format!("missing = 1{ex}"), ep.clone(), "");
    let (ix, ip) = only_archives(conn);
    add("VUES", "Archives".into(), format!("missing = 0{ix}"), ip, "#8f8f9a");
    // Les vues vides (rejetés, à analyser, manquants) ne s'affichent pas, comme dans Studio Vault.
    let hide_empty = ["Rejetés", "À analyser", "Fichiers manquants"];
    out.retain(|n| !(hide_empty.contains(&n.label.as_str()) && n.count == 0));

    // ---- Mes vues
    if let Ok(mut st) = conn.prepare("SELECT id, name, where_sql, IFNULL(params, '[]'), IFNULL(search, '') FROM saved_views ORDER BY name COLLATE NOCASE") {
        let views: Vec<(i64, String, String, String, String)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map(|r| r.flatten().collect()).unwrap_or_default();
        for (id, name, w, p, s) in views {
            let params = json_params(&p);
            let n = count(conn, &w, &params);
            out.push(Nav { section: "MES VUES".into(), label: name, count: n, where_sql: w, params, color: "#f5b942".into(), view: Some(id), search: s, ..Default::default() });
        }
    }

    // ---- Sorties
    if let Ok(mut st) = conn.prepare(
        "SELECT r.id, r.title, COUNT(rt.track_id) FROM releases r LEFT JOIN release_tracks rt ON rt.release_id = r.id
         GROUP BY r.id ORDER BY (r.release_date = ''), r.release_date DESC, r.title",
    ) {
        let rels: Vec<(i64, String, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map(|r| r.flatten().collect()).unwrap_or_default();
        for (id, title, n) in rels {
            out.push(Nav {
                section: "SORTIES".into(),
                label: title,
                count: n,
                where_sql: "missing = 0 AND id IN (SELECT track_id FROM release_tracks WHERE release_id = ?)".into(),
                params: vec![Value::Integer(id)],
                color: "#6d6bff".into(),
                ..Default::default()
            });
        }
    }

    // ---- Prompts Suno (les 10 plus utilisés)
    if let Ok(mut st) = conn.prepare("SELECT id, IFNULL(NULLIF(name, ''), 'Sans nom') FROM prompts ORDER BY use_count DESC, modified_at DESC LIMIT 10") {
        let prompts: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|r| r.flatten().collect()).unwrap_or_default();
        for (id, name) in prompts {
            let params = vec![Value::Integer(id)];
            let n = count(conn, "missing = 0 AND prompt_id = ?", &params);
            out.push(Nav { section: "PROMPTS SUNO".into(), label: name, count: n, where_sql: "missing = 0 AND prompt_id = ?".into(), params, color: "#c67bff".into(), ..Default::default() });
        }
    }

    // ---- Dossiers : les dossiers surveillés et leurs sous-dossiers directs
    let paths: Vec<String> = conn.prepare("SELECT path FROM tracks WHERE missing = 0").and_then(|mut s| s.query_map([], |r| r.get(0))?.collect()).unwrap_or_default();
    for (id, root, _) in super::scan::watch_folders(conn) {
        let r = root.trim_end_matches(['\\', '/']).replace('/', "\\");
        let rl = format!("{}\\", r.to_lowercase());
        let inside: Vec<&String> = paths.iter().filter(|p| p.replace('/', "\\").to_lowercase().starts_with(&rl)).collect();
        let name = std::path::Path::new(&r).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| r.clone());
        out.push(Nav {
            section: "DOSSIERS".into(),
            label: name,
            count: inside.len() as i64,
            where_sql: "missing = 0 AND path LIKE ?".into(),
            params: vec![Value::Text(format!("{r}\\%"))],
            folder: Some((id, r.clone())),
            ..Default::default()
        });
        let mut subs: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for p in inside {
            let rest = &p.replace('/', "\\")[rl.len()..];
            if let Some((sub, _)) = rest.split_once('\\') {
                *subs.entry(sub.to_string()).or_default() += 1;
            }
        }
        for (sub, n) in subs {
            out.push(Nav {
                section: "DOSSIERS".into(),
                label: sub.clone(),
                count: n,
                where_sql: "missing = 0 AND path LIKE ?".into(),
                params: vec![Value::Text(format!("{r}\\{sub}\\%"))],
                folder: Some((0, format!("{r}\\{sub}"))),
                depth: 1,
                ..Default::default()
            });
        }
    }

    grouped(
        conn,
        &mut out,
        "GENRES",
        "SELECT genre, COUNT(*) c FROM tracks WHERE missing = 0 AND genre IS NOT NULL AND genre <> '' GROUP BY genre ORDER BY c DESC LIMIT 20",
        &|g| g.to_string(),
        "missing = 0 AND genre = ?",
    );
    let natures: Vec<(String, i64)> = conn
        .prepare("SELECT COALESCE(nature, 'original'), COUNT(*) FROM tracks WHERE missing = 0 GROUP BY 1")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .unwrap_or_default();
    if natures.iter().any(|n| n.0 != "original") {
        for (code, label, _) in super::NATURES {
            if let Some((_, n)) = natures.iter().find(|x| x.0 == code) {
                out.push(Nav {
                    section: "NATURE".into(),
                    label: label.into(),
                    count: *n,
                    where_sql: "missing = 0 AND COALESCE(nature, 'original') = ?".into(),
                    params: vec![Value::Text(code.into())],
                    ..Default::default()
                });
            }
        }
    }
    for (code, label) in super::STATUSES {
        let params = vec![Value::Text(code.into())];
        let n = count(conn, "missing = 0 AND status = ?", &params);
        if n == 0 && !["draft", "wip", "done"].contains(&code) {
            continue;
        }
        out.push(Nav { section: "STATUT".into(), label: label.into(), count: n, where_sql: "missing = 0 AND status = ?".into(), params, color: status_color(code).into(), ..Default::default() });
    }
    let n = count(conn, TO_PUBLISH_SQL, &[]);
    out.push(Nav { section: "STATUT".into(), label: "À publier".into(), count: n, where_sql: TO_PUBLISH_SQL.into(), ..Default::default() });
    for (label, lo, hi) in BPM_BUCKETS {
        let params = vec![Value::Real(lo), Value::Real(hi)];
        let n = count(conn, "missing = 0 AND bpm >= ? AND bpm < ?", &params);
        if n > 0 {
            out.push(Nav { section: "TEMPO".into(), label: label.into(), count: n, where_sql: "missing = 0 AND bpm >= ? AND bpm < ?".into(), params, ..Default::default() });
        }
    }
    grouped(
        conn,
        &mut out,
        "TONALITÉS",
        "SELECT music_key, COUNT(*) c FROM tracks WHERE missing = 0 AND music_key IS NOT NULL AND music_key <> '' GROUP BY music_key ORDER BY c DESC LIMIT 15",
        &|k| {
            let c = camelot::camelot(k);
            if c.is_empty() { k.to_string() } else { format!("{k}  ({c})") }
        },
        "missing = 0 AND music_key = ?",
    );
    grouped(
        conn,
        &mut out,
        "TAGS",
        "SELECT t.name, COUNT(tt.track_id) c FROM tags t JOIN track_tags tt ON tt.tag_id = t.id JOIN tracks tr ON tr.id = tt.track_id AND tr.missing = 0 GROUP BY t.name ORDER BY c DESC LIMIT 30",
        &|t| t.to_string(),
        "missing = 0 AND id IN (SELECT track_id FROM track_tags JOIN tags ON tags.id = track_tags.tag_id WHERE tags.name = ?)",
    );
    grouped(
        conn,
        &mut out,
        "FORMATS",
        "SELECT IFNULL(file_format, ''), COUNT(*) c FROM tracks WHERE missing = 0 GROUP BY file_format ORDER BY c DESC",
        &|f| if f.is_empty() { "?".into() } else { f.to_uppercase() },
        "missing = 0 AND file_format = ?",
    );
    out
}

/// Une section dont chaque valeur (genre, tonalité…) est une entrée.
fn grouped(conn: &Connection, out: &mut Vec<Nav>, section: &str, sql: &str, label: &dyn Fn(&str) -> String, where_sql: &str) {
    let rows: Vec<(String, i64)> = conn.prepare(sql).and_then(|mut s| s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?.collect()).unwrap_or_default();
    for (v, n) in rows {
        out.push(Nav { section: section.into(), label: label(&v), count: n, where_sql: where_sql.into(), params: vec![Value::Text(v)], ..Default::default() });
    }
}

pub fn json_params(raw: &str) -> Vec<Value> {
    serde_json::from_str::<Vec<serde_json::Value>>(raw)
        .unwrap_or_default()
        .into_iter()
        .map(|v| match v {
            serde_json::Value::Number(n) if n.is_i64() => Value::Integer(n.as_i64().unwrap_or(0)),
            serde_json::Value::Number(n) => Value::Real(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::String(s) => Value::Text(s),
            serde_json::Value::Bool(b) => Value::Integer(b as i64),
            _ => Value::Null,
        })
        .collect()
}

pub fn params_json(p: &[Value]) -> String {
    let v: Vec<serde_json::Value> = p
        .iter()
        .map(|x| match x {
            Value::Integer(i) => serde_json::json!(i),
            Value::Real(f) => serde_json::json!(f),
            Value::Text(s) => serde_json::json!(s),
            _ => serde_json::Value::Null,
        })
        .collect();
    serde_json::Value::Array(v).to_string()
}

/// Les filtres en pastilles au-dessus du tableau.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chips {
    /// « single », « ep », « album » (vide : toutes).
    pub release: String,
    pub nature: String,
    pub keep: bool,
    pub unanalyzed: bool,
    pub status: String,
    pub genre: String,
}

/// La condition complète : entrée de navigation, archives écartées (sauf la vue Archives),
/// pastilles, recherche.
pub fn full_where(conn: &Connection, base: &str, base_params: &[Value], include_archives: bool, chips: &Chips, text: &str) -> (String, Vec<Value>) {
    let mut w = format!("({base})");
    let mut p: Vec<Value> = base_params.to_vec();
    if !include_archives && !base.contains("archived = 1") {
        let (ex, ep) = exclude_archives(conn);
        w.push_str(&ex);
        p = join(p, &ep);
    }
    let mut parts: Vec<&str> = Vec::new();
    if !chips.release.is_empty() {
        parts.push("release_type = ?");
        p.push(Value::Text(chips.release.clone()));
    }
    if !chips.status.is_empty() {
        parts.push("status = ?");
        p.push(Value::Text(chips.status.clone()));
    }
    if !chips.nature.is_empty() {
        parts.push("COALESCE(nature, 'original') = ?");
        p.push(Value::Text(chips.nature.clone()));
    }
    if chips.keep {
        parts.push("pick = 1");
    }
    if chips.unanalyzed {
        parts.push("analyzed_at IS NULL");
    }
    if !chips.genre.is_empty() {
        parts.push("LOWER(genre) = LOWER(?)");
        p.push(Value::Text(chips.genre.clone()));
    }
    if !parts.is_empty() {
        w.push_str(" AND ");
        w.push_str(&parts.join(" AND "));
    }
    if !text.trim().is_empty() {
        let (sw, sp) = search::parse_query(text);
        w.push_str(&format!(" AND ({sw})"));
        p = join(p, &sp);
    }
    (w, p)
}

/// Les colonnes du tableau : (clé, libellé, largeur en pixels, tri SQL).
pub const COLUMNS: [(&str, &str, u32, &str); 23] = [
    ("title", "Titre", 320, "title COLLATE NOCASE"),
    ("wave", "Onde", 132, "energy"),
    ("track_no", "N°", 50, "track_no"),
    ("artist", "Artiste", 150, "artist COLLATE NOCASE"),
    ("album", "Album", 160, "album COLLATE NOCASE"),
    ("genre", "Genre", 120, "genre COLLATE NOCASE"),
    ("mood_auto", "Ambiance", 200, "mood_auto"),
    ("nature", "Nature", 84, "nature"),
    ("release_type", "Sortie", 90, "release_type"),
    ("bpm", "BPM", 56, "bpm"),
    ("music_key", "Tonalité", 72, "music_key"),
    ("camelot", "Clé", 56, "music_key"),
    ("duration_s", "Durée", 62, "duration_s"),
    ("status", "Statut", 112, "status"),
    ("pick", "", 40, "pick"),
    ("role", "Type", 120, "role"),
    ("version_label", "Version", 90, "version_label"),
    ("file_format", "Format", 64, "file_format"),
    ("lufs", "LUFS", 62, "lufs"),
    ("file_date", "Créé le", 100, "COALESCE(file_date, added_at)"),
    ("published_at", "Publié le", 100, "published_at"),
    ("added_at", "Ajouté le", 100, "added_at"),
    ("path", "Emplacement", 380, "path"),
];

/// Colonnes montrées tant que l'utilisateur n'a pas choisi (celles du QCM du 4 novembre 2026).
pub const DEFAULT_COLUMNS: [&str; 12] = ["title", "wave", "artist", "status", "bpm", "camelot", "duration_s", "lufs", "genre", "mood_auto", "release_type", "pick"];

/// Le tri par défaut : regroupé par album, puis par numéro de piste, le plus récent d'abord.
pub const DEFAULT_ORDER: &str = "(album IS NOT NULL AND album <> '') DESC, album COLLATE NOCASE ASC, track_no IS NULL, track_no ASC, added_at DESC";

pub fn order_sql(column: &str, ascending: bool) -> String {
    match COLUMNS.iter().find(|c| c.0 == column) {
        Some(c) if c.0 == "album" && ascending => DEFAULT_ORDER.into(),
        Some(c) => format!("{0} IS NULL, {0} {1}", c.3, if ascending { "ASC" } else { "DESC" }),
        None => DEFAULT_ORDER.into(),
    }
}

/// Les types de fichiers (rôles) de Studio Vault.
pub const ROLE_LABELS: [(&str, &str); 8] = [
    ("mixdown", "Morceau principal"),
    ("version", "Version"),
    ("stem", "Piste séparée (stem)"),
    ("suno_generation", "Sortie Suno"),
    ("demo", "Démo"),
    ("reference", "Référence"),
    ("render", "Rendu"),
    ("draft", "Brouillon"),
];

pub fn role_label(r: &str) -> String {
    ROLE_LABELS.iter().find(|x| x.0 == r).map(|x| x.1.to_string()).unwrap_or_else(|| r.to_string())
}

/// Une ligne du tableau.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub track_no: Option<i64>,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub mood: String,
    pub nature: String,
    pub release_type: String,
    pub bpm: Option<f64>,
    pub key: String,
    pub duration: Option<f64>,
    pub status: String,
    pub pick: i64,
    pub role: String,
    pub version_label: String,
    pub format: String,
    pub lufs: Option<f64>,
    pub file_date: String,
    pub published_at: String,
    pub added_at: String,
    pub missing: bool,
    pub archived: bool,
    pub parent_id: Option<i64>,
    /// Versions et stems rattachés.
    pub children: i64,
    pub analyzed: bool,
}

pub fn duration_text(sec: Option<f64>) -> String {
    let Some(s) = sec else { return String::new() };
    let s = s as i64;
    let (h, m, s) = (s / 3600, s % 3600 / 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

fn fr_day(v: &str) -> String {
    let d: String = v.chars().take(10).collect();
    if d.len() == 10 { format!("{}/{}/{}", &d[8..10], &d[5..7], &d[0..4]) } else { d }
}

impl Row {
    /// Le texte d'une colonne, comme Studio Vault l'affiche.
    pub fn cell(&self, key: &str) -> String {
        match key {
            "title" => self.title.clone(),
            "track_no" => self.track_no.filter(|n| *n > 0).map(|n| if n < 100 { format!("{n:02}") } else { n.to_string() }).unwrap_or_default(),
            "artist" => self.artist.clone(),
            "album" => self.album.clone(),
            "genre" => self.genre.clone(),
            "mood_auto" => self.mood.clone(),
            "nature" => nature_short(&self.nature),
            "release_type" => {
                if self.release_type.is_empty() {
                    String::new()
                } else {
                    release_label(&self.release_type)
                }
            }
            "bpm" => self.bpm.map(|b| format!("{b:.0}")).unwrap_or_default(),
            "music_key" => self.key.clone(),
            "camelot" => camelot::camelot(&self.key).to_string(),
            "duration_s" => duration_text(self.duration),
            "status" => status_label(&self.status).to_string(),
            "pick" => match self.pick {
                1 => "✓".into(),
                -1 => "✗".into(),
                _ => String::new(),
            },
            "role" => role_label(&self.role),
            "version_label" => self.version_label.clone(),
            "file_format" => self.format.to_uppercase(),
            "lufs" => self.lufs.map(|l| format!("{l:+.1}")).unwrap_or_default(),
            "file_date" => fr_day(if self.file_date.is_empty() { &self.added_at } else { &self.file_date }),
            "published_at" => fr_day(&self.published_at),
            "added_at" => fr_day(&self.added_at),
            "path" => std::path::Path::new(&self.path).parent().map(|p| p.display().to_string()).unwrap_or_default(),
            _ => String::new(),
        }
    }
}

const ROW_COLS: &str = "id, path, COALESCE(NULLIF(title, ''), filename), track_no, IFNULL(artist, ''), IFNULL(album, ''), IFNULL(genre, ''), IFNULL(mood_auto, ''),
    IFNULL(NULLIF(nature, ''), 'original'), IFNULL(release_type, ''), bpm, IFNULL(music_key, ''), duration_s, IFNULL(status, 'idea'), IFNULL(pick, 0),
    IFNULL(role, 'mixdown'), IFNULL(version_label, ''), IFNULL(file_format, ''), lufs, IFNULL(file_date, ''), IFNULL(published_at, ''), IFNULL(added_at, ''),
    IFNULL(missing, 0), IFNULL(archived, 0), parent_id, (SELECT COUNT(*) FROM tracks c WHERE c.parent_id = tracks.id), analyzed_at IS NOT NULL";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Row> {
    let title: String = r.get(2)?;
    Ok(Row {
        id: r.get(0)?,
        path: r.get(1)?,
        title: match title.rsplit_once('.') {
            Some((stem, ext)) if super::scan::AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()) => stem.to_string(),
            _ => title,
        },
        track_no: r.get(3)?,
        artist: r.get(4)?,
        album: r.get(5)?,
        genre: r.get(6)?,
        mood: r.get(7)?,
        nature: r.get(8)?,
        release_type: r.get(9)?,
        bpm: r.get(10)?,
        key: r.get(11)?,
        duration: r.get(12)?,
        status: r.get(13)?,
        pick: r.get(14)?,
        role: r.get(15)?,
        version_label: r.get(16)?,
        format: r.get(17)?,
        lufs: r.get(18)?,
        file_date: r.get(19)?,
        published_at: r.get(20)?,
        added_at: r.get(21)?,
        missing: r.get::<_, i64>(22)? != 0,
        archived: r.get::<_, i64>(23)? != 0,
        parent_id: r.get(24)?,
        children: r.get(25)?,
        analyzed: r.get(26)?,
    })
}

/// Les morceaux d'une condition, triés (20 000 au plus, comme Studio Vault).
pub fn rows(conn: &Connection, where_sql: &str, params: &[Value], order: &str) -> rusqlite::Result<Vec<Row>> {
    let mut st = conn.prepare(&format!("SELECT {ROW_COLS} FROM tracks WHERE {where_sql} ORDER BY {order} LIMIT 20000"))?;
    st.query_map(params_from_iter(params.iter()), row)?.collect()
}

pub fn row_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<Row>> {
    rows(conn, "id = ?", &[Value::Integer(id)], "id").map(|mut v| v.pop())
}

/// Les versions et stems rattachés à un morceau.
pub fn children(conn: &Connection, id: i64) -> rusqlite::Result<Vec<Row>> {
    rows(conn, "parent_id = ?", &[Value::Integer(id)], "added_at")
}

/// Une condition de vue enregistrée reçue d'ailleurs (l'autre PC) : elle est exécutée telle quelle à chaque affichage, donc
/// elle doit être une seule expression qui ne fait que lire les morceaux, avec autant de paramètres que prévu. Les mots
/// qui servent à faire tourner SQLite sans fin ou à remplir la mémoire sont refusés.
pub fn view_condition_ok(conn: &Connection, where_sql: &str, params: &str) -> bool {
    const REFUSED: [&str; 9] = ["recursive", "randomblob", "zeroblob", "printf", "format", "load_extension", "pragma", "attach", "detach"];
    let w = where_sql.trim();
    if w.is_empty() || w.contains(';') || w.contains("--") || w.contains("/*") {
        return false;
    }
    let lower = w.to_lowercase();
    if lower.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|word| REFUSED.contains(&word)) {
        return false;
    }
    let Ok(st) = conn.prepare(&format!("SELECT COUNT(*) FROM tracks WHERE ({w})")) else { return false };
    st.readonly() && st.parameter_count() == json_params(params).len()
}

/// Enregistre la vue affichée (condition, paramètres, recherche) sous un nom.
pub fn save_view(conn: &Connection, name: &str, where_sql: &str, params: &[Value], search: &str) -> rusqlite::Result<i64> {
    conn.execute("INSERT INTO saved_views (name, where_sql, params, search) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![name, where_sql, params_json(params), search])?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_view(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM saved_views WHERE id = ?1", [id])?;
    Ok(())
}

/// Archiver ou sortir des archives (les fichiers ne bougent pas).
pub fn set_archived(conn: &Connection, ids: &[i64], archived: bool) -> rusqlite::Result<usize> {
    let mut n = 0;
    for id in ids {
        n += conn.execute("UPDATE tracks SET archived = ?2, modified_at = datetime('now') WHERE id = ?1", rusqlite::params![id, archived as i64])?;
    }
    Ok(n)
}

pub fn set_pick(conn: &Connection, ids: &[i64], pick: i64) -> rusqlite::Result<()> {
    for id in ids {
        conn.execute("UPDATE tracks SET pick = ?2 WHERE id = ?1", rusqlite::params![id, pick.clamp(-1, 1)])?;
    }
    Ok(())
}

/// Les morceaux qui s'enchaînent avec `id` : tonalité identique, voisine ou relative sur la roue de
/// Camelot, tempo à 6 % près (demi et double tempo compris). Rend l'entrée de navigation.
/// Une vue ouverte depuis ailleurs (l'accueil : « À terminer », « Fichiers manquants »…).
pub fn custom_nav(conn: &Connection, label: &str, where_sql: &str) -> Nav {
    let mut n = Nav { section: "VUES".into(), label: label.into(), where_sql: where_sql.into(), color: "#ff8a3d".into(), ..Default::default() };
    n.count = count(conn, &n.where_sql, &n.params);
    n
}

pub fn compatible_nav(conn: &Connection, id: i64) -> Option<Nav> {
    let (title, bpm, key): (String, Option<f64>, Option<String>) =
        conn.query_row("SELECT COALESCE(NULLIF(title, ''), filename), bpm, music_key FROM tracks WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok()?;
    let mut w = vec!["missing = 0".to_string(), "id <> ?".to_string()];
    let mut p = vec![Value::Integer(id)];
    let keys = camelot::compatible_keys(key.as_deref().unwrap_or(""));
    if !keys.is_empty() {
        w.push(format!("music_key IN ({})", vec!["?"; keys.len()].join(",")));
        p.extend(keys.iter().map(|k| Value::Text(k.to_string())));
    }
    if let Some(b) = bpm.filter(|b| *b > 0.0) {
        let mut tempo = Vec::new();
        for target in [b, b / 2.0, b * 2.0] {
            tempo.push("(bpm BETWEEN ? AND ?)");
            p.push(Value::Real(target * 0.94));
            p.push(Value::Real(target * 1.06));
        }
        w.push(format!("({})", tempo.join(" OR ")));
    }
    if keys.is_empty() && bpm.is_none() {
        return None;
    }
    let title = match title.rsplit_once('.') {
        Some((stem, ext)) if super::scan::AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()) => stem.to_string(),
        _ => title,
    };
    let mut n = Nav { section: "VUES".into(), label: format!("Compatibles avec « {title} »"), where_sql: w.join(" AND "), params: p, color: "#35d9c3".into(), ..Default::default() };
    n.count = count(conn, &n.where_sql, &n.params);
    Some(n)
}

/// Nombre de morceaux par code de Camelot (« 8A » → 12).
pub fn camelot_counts(conn: &Connection) -> std::collections::HashMap<String, i64> {
    let mut out = std::collections::HashMap::new();
    let rows: Vec<(String, i64)> = conn
        .prepare("SELECT music_key, COUNT(*) FROM tracks WHERE missing = 0 AND COALESCE(archived, 0) = 0 AND IFNULL(music_key, '') <> '' GROUP BY music_key")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .unwrap_or_default();
    for (k, n) in rows {
        let c = camelot::camelot(&k);
        if !c.is_empty() {
            *out.entry(c.to_string()).or_default() += n;
        }
    }
    out
}

/// Les repères d'un morceau : (secondes, nom).
pub fn markers(conn: &Connection, id: i64) -> Vec<(f64, String)> {
    conn.prepare("SELECT time_s, label FROM markers WHERE track_id = ?1 ORDER BY time_s")
        .and_then(|mut s| s.query_map([id], |x| Ok((x.get(0)?, x.get(1)?)))?.collect())
        .unwrap_or_default()
}

pub fn add_marker(conn: &Connection, id: i64, time: f64, label: &str) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO markers (track_id, time_s, label) VALUES (?1, ?2, ?3)", rusqlite::params![id, time, label])?;
    Ok(())
}

pub fn remove_marker(conn: &Connection, id: i64, time: f64, label: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM markers WHERE id = (SELECT id FROM markers WHERE track_id = ?1 AND label = ?2 AND ABS(time_s - ?3) < 0.05 LIMIT 1)",
        rusqlite::params![id, label, time],
    )?;
    Ok(())
}

/// Un morceau écouté (compteur et date, pour « Déjà écoutés »).
pub fn played(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE tracks SET play_count = IFNULL(play_count, 0) + 1, last_played = datetime('now', 'localtime') WHERE id = ?1", [id])?;
    Ok(())
}

/// Les champs qu'on modifie dans le panneau de détails.
pub const EDITABLE: [&str; 21] = [
    "title",
    "artist",
    "album",
    "genre",
    "year",
    "track_no",
    "bpm",
    "music_key",
    "status",
    "nature",
    "role",
    "version_label",
    "notes",
    "mood_auto",
    "sc_url",
    "published_at",
    "release_type",
    "to_publish",
    "daw_project",
    "cover_path",
    "parent_id",
];

/// Modifie un champ d'un morceau (texte vide : effacé ; les nombres sont vérifiés).
pub fn update(conn: &Connection, id: i64, key: &str, value: &str) -> Result<(), String> {
    if !EDITABLE.contains(&key) {
        return Err(format!("champ inconnu : {key}"));
    }
    let v = value.trim();
    let val = match key {
        "year" | "track_no" | "to_publish" | "parent_id" => {
            if v.is_empty() {
                Value::Null
            } else {
                Value::Integer(v.parse().map_err(|_| format!("« {v} » n'est pas un nombre entier"))?)
            }
        }
        "bpm" => {
            if v.is_empty() {
                Value::Null
            } else {
                Value::Real(v.replace(',', ".").parse().map_err(|_| format!("« {v} » n'est pas un tempo"))?)
            }
        }
        "status" if !super::STATUSES.iter().any(|s| s.0 == v) => return Err(format!("statut inconnu : {v}")),
        "nature" if !super::NATURES.iter().any(|s| s.0 == v) => return Err(format!("nature inconnue : {v}")),
        _ => Value::Text(v.to_string()),
    };
    if key == "parent_id" && val == Value::Integer(id) {
        return Err("un morceau ne peut pas être sa propre version".into());
    }
    conn.execute(&format!("UPDATE tracks SET {key} = ?1, modified_at = datetime('now') WHERE id = ?2"), rusqlite::params![val, id]).map_err(|e| e.to_string())?;
    if LOCKABLE.contains(&key) {
        lock(conn, id, key, val != Value::Null && val != Value::Text(String::new())).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Ce qu'une correction à la main protège : l'analyse et les statuts automatiques n'y touchent plus.
pub const LOCKABLE: [&str; 4] = ["bpm", "music_key", "mood_auto", "status"];

/// Verrouille un champ d'un morceau, ou le rend à l'analyse.
pub fn lock(conn: &Connection, id: i64, field: &str, on: bool) -> rusqlite::Result<()> {
    if on {
        conn.execute("INSERT OR IGNORE INTO field_locks (track_id, field) VALUES (?1, ?2)", rusqlite::params![id, field])?;
    } else {
        conn.execute("DELETE FROM field_locks WHERE track_id = ?1 AND field = ?2", rusqlite::params![id, field])?;
    }
    Ok(())
}

/// Le champ a-t-il été corrigé à la main ?
pub fn locked(conn: &Connection, id: i64, field: &str) -> bool {
    conn.query_row("SELECT 1 FROM field_locks WHERE track_id = ?1 AND field = ?2", rusqlite::params![id, field], |_| Ok(())).is_ok()
}

/// Tempo doublé ou divisé par deux (l'erreur la plus courante de l'analyse), gardé comme une correction.
pub fn scale_bpm(conn: &Connection, ids: &[i64], factor: f64) -> rusqlite::Result<usize> {
    let mut n = 0;
    for id in ids {
        let done = conn.execute("UPDATE tracks SET bpm = ROUND(bpm * ?2, 1), modified_at = datetime('now') WHERE id = ?1 AND bpm IS NOT NULL", rusqlite::params![id, factor])?;
        if done > 0 {
            lock(conn, *id, "bpm", true)?;
        }
        n += done;
    }
    Ok(n)
}

/// La mini forme d'onde des morceaux qui en ont une.
pub fn waves(conn: &Connection) -> std::collections::HashMap<i64, Vec<u8>> {
    conn.prepare("SELECT track_id, peaks FROM track_waves")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .unwrap_or_default()
}

pub fn save_wave(conn: &Connection, id: i64, peaks: &[u8]) -> rusqlite::Result<()> {
    conn.execute("INSERT OR REPLACE INTO track_waves (track_id, peaks) VALUES (?1, ?2)", rusqlite::params![id, peaks])?;
    Ok(())
}

/// Les morceaux dont la mini forme d'onde reste à calculer, les plus récents d'abord.
pub fn without_wave(conn: &Connection) -> Vec<(i64, String)> {
    conn.prepare("SELECT id, path FROM tracks WHERE missing = 0 AND id NOT IN (SELECT track_id FROM track_waves) ORDER BY added_at DESC, id DESC")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .unwrap_or_default()
}

/// Les sorties d'un morceau : (id, titre, type et date).
pub fn releases_of(conn: &Connection, id: i64) -> Vec<(i64, String, String)> {
    conn.prepare("SELECT r.id, r.title, IFNULL(r.rtype, ''), IFNULL(r.release_date, '') FROM releases r JOIN release_tracks rt ON rt.release_id = r.id WHERE rt.track_id = ?1 ORDER BY r.release_date DESC")
        .and_then(|mut s| {
            s.query_map([id], |r| {
                let (kind, date): (String, String) = (r.get(2)?, r.get(3)?);
                let when = super::releases::fr_date(&date);
                Ok((r.get(0)?, r.get(1)?, if when.is_empty() { release_label(&kind) } else { format!("{} · {when}", release_label(&kind)) }))
            })?
            .collect()
        })
        .unwrap_or_default()
}

/// Les tonalités proposées pour une correction, majeures puis mineures.
pub const KEYS: [&str; 24] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B", "Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "Bbm", "Bm"];

/// Les tags d'un morceau.
pub fn tags_of(conn: &Connection, id: i64) -> Vec<String> {
    conn.prepare("SELECT t.name FROM tags t JOIN track_tags tt ON tt.tag_id = t.id WHERE tt.track_id = ?1 ORDER BY t.name COLLATE NOCASE")
        .and_then(|mut s| s.query_map([id], |r| r.get(0))?.collect())
        .unwrap_or_default()
}

/// Remplace les tags d'un morceau (« club, nuit »).
pub fn set_tags(conn: &Connection, id: i64, list: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM track_tags WHERE track_id = ?1", [id])?;
    for t in list.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [t])?;
        conn.execute("INSERT OR IGNORE INTO track_tags (track_id, tag_id) SELECT ?1, id FROM tags WHERE name = ?2", rusqlite::params![id, t])?;
    }
    Ok(())
}

/// Les morceaux dont le titre, le nom de fichier ou l'album contient `q` (palette de commandes) :
/// (id, titre, album).
pub fn quick_find(conn: &Connection, q: &str, limit: usize) -> Vec<(i64, String, String)> {
    let like = format!("%{}%", q.trim());
    conn.prepare(
        "SELECT id, COALESCE(NULLIF(title, ''), filename), IFNULL(album, '') FROM tracks
         WHERE missing = 0 AND (title LIKE ?1 OR filename LIKE ?1 OR album LIKE ?1) ORDER BY COALESCE(NULLIF(title, ''), filename) COLLATE NOCASE LIMIT ?2",
    )
    .and_then(|mut s| s.query_map(rusqlite::params![like, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
    .unwrap_or_default()
}

/// Les genres proposés (« Genre et ambiance ») : les familles du moteur de textes, puis ceux de la bibliothèque.
pub fn genre_choices(conn: &Connection) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in super::textes_fr::FAMILLES {
        if !out.iter().any(|g| g == f.2) {
            out.push(f.2.to_string());
        }
    }
    let known: Vec<String> = conn
        .prepare("SELECT DISTINCT genre FROM tracks WHERE genre IS NOT NULL AND genre <> '' ORDER BY genre")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .unwrap_or_default();
    for g in known {
        if !out.contains(&g) {
            out.push(g);
        }
    }
    out
}

/// Les ambiances (adjectifs de `humeur`) communes à tous ces morceaux (les 200 premiers).
pub fn common_moods(conn: &Connection, ids: &[i64]) -> Vec<&'static str> {
    let mut common: Option<Vec<&'static str>> = None;
    for id in ids.iter().take(200) {
        let names: Vec<String> = tags_of(conn, *id).into_iter().map(|t| t.to_lowercase()).collect();
        let mine: Vec<&'static str> = super::humeur::ADJECTIFS.iter().map(|a| a.0).filter(|a| names.iter().any(|n| n == a)).collect();
        common = Some(match common {
            None => mine,
            Some(c) => c.into_iter().filter(|a| mine.contains(a)).collect(),
        });
    }
    common.unwrap_or_default()
}

/// « Genre et ambiance » en lot : le genre (s'il est donné) et les ambiances, qui remplacent les
/// anciennes ou s'y ajoutent. Comme `genre_dialog.py`.
pub fn apply_genre_mood(conn: &Connection, ids: &[i64], genre: Option<&str>, moods: &[&str], replace: bool) -> rusqlite::Result<usize> {
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        if let Some(g) = genre.filter(|g| !g.trim().is_empty()) {
            tx.execute("UPDATE tracks SET genre = ?1, modified_at = datetime('now') WHERE id = ?2", rusqlite::params![g.trim(), id])?;
        }
        if replace {
            for (adj, _) in super::humeur::ADJECTIFS {
                tx.execute("DELETE FROM track_tags WHERE track_id = ?1 AND tag_id IN (SELECT id FROM tags WHERE lower(name) = ?2)", rusqlite::params![id, adj])?;
            }
        }
        for m in moods {
            tx.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [m])?;
            tx.execute("INSERT OR IGNORE INTO track_tags (track_id, tag_id) SELECT ?1, id FROM tags WHERE name = ?2", rusqlite::params![id, m])?;
        }
    }
    tx.commit()?;
    Ok(ids.len())
}

/// Le texte d'un champ (pour le panneau de détails).
pub fn field(conn: &Connection, id: i64, key: &str) -> String {
    if !EDITABLE.contains(&key) && !["path", "filename", "file_size", "samplerate", "channels", "bitrate_kbps", "lufs", "energy", "brightness", "dynamics", "mode", "true_peak", "clip_ratio", "silence_start", "silence_end", "analyzed_at", "play_count", "last_played", "added_at", "file_date"].contains(&key) {
        return String::new();
    }
    conn.query_row(&format!("SELECT {key} FROM tracks WHERE id = ?1"), [id], |r| {
        Ok(match r.get::<_, Value>(0)? {
            Value::Null => String::new(),
            Value::Integer(i) => i.to_string(),
            Value::Real(f) => {
                if f.fract() == 0.0 {
                    format!("{f:.0}")
                } else {
                    format!("{f:.2}").trim_end_matches('0').to_string()
                }
            }
            Value::Text(s) => s,
            Value::Blob(_) => String::new(),
        })
    })
    .unwrap_or_default()
}

/// Ce que l'analyse audio mesure (comme `composer_vault/audio/analyzer.py`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub lufs: Option<f64>,
    pub duration: Option<f64>,
    pub samplerate: Option<i64>,
    /// 0 à 1.
    pub energy: Option<f64>,
    pub brightness: Option<f64>,
    pub dynamics: Option<f64>,
    /// « majeur » ou « mineur ».
    pub mode: Option<String>,
    /// dBTP.
    pub true_peak: Option<f64>,
    pub clip_ratio: Option<f64>,
    pub silence_start: Option<f64>,
    pub silence_end: Option<f64>,
    /// « durée:empreinte » (variations d'énergie sur 64 tranches), pour les doublons convertis.
    pub audio_fp: Option<String>,
}

/// Écrit l'analyse et l'ambiance qui en découle.
pub fn save_analysis(conn: &Connection, id: i64, a: &Analysis) -> rusqlite::Result<()> {
    let mood = super::humeur::texte(&super::humeur::deduire(a.mode.as_deref(), a.bpm, a.energy, a.brightness, a.dynamics));
    conn.execute(
        "UPDATE tracks SET
             bpm = CASE WHEN EXISTS (SELECT 1 FROM field_locks WHERE track_id = ?16 AND field = 'bpm') THEN bpm ELSE ?1 END,
             music_key = CASE WHEN EXISTS (SELECT 1 FROM field_locks WHERE track_id = ?16 AND field = 'music_key') THEN music_key ELSE ?2 END,
             lufs = ?3, duration_s = COALESCE(?4, duration_s), samplerate = COALESCE(?5, samplerate),
             energy = ?6, brightness = ?7, dynamics = ?8, mode = ?9,
             mood_auto = CASE WHEN EXISTS (SELECT 1 FROM field_locks WHERE track_id = ?16 AND field = 'mood_auto') THEN mood_auto ELSE ?10 END,
             true_peak = ?11, clip_ratio = ?12, silence_start = ?13,
             silence_end = ?14, audio_fp = ?15, analyzed_at = datetime('now'), modified_at = datetime('now') WHERE id = ?16",
        rusqlite::params![
            a.bpm,
            a.key,
            a.lufs,
            a.duration,
            a.samplerate,
            a.energy,
            a.brightness,
            a.dynamics,
            a.mode,
            if mood.is_empty() { None } else { Some(mood) },
            a.true_peak,
            a.clip_ratio,
            a.silence_start,
            a.silence_end,
            a.audio_fp,
            id
        ],
    )?;
    Ok(())
}

/// Les morceaux jamais analysés (hors ceux qui ont déjà échoué), les plus récents d'abord.
pub fn unanalyzed(conn: &Connection) -> Vec<(i64, String)> {
    let failed: Vec<i64> = conn
        .query_row("SELECT value FROM settings WHERE key = 'analyze_failed'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    let (ex, ep) = exclude_archives(conn);
    rows(conn, &format!("missing = 0 AND analyzed_at IS NULL{ex}"), &ep, "added_at DESC")
        .unwrap_or_default()
        .into_iter()
        .filter(|r| !failed.contains(&r.id))
        .map(|r| (r.id, r.path))
        .collect()
}

/// Note les morceaux illisibles : l'analyse en fond ne les relance pas.
pub fn mark_failed(conn: &Connection, ids: &[i64]) {
    if ids.is_empty() {
        return;
    }
    let mut failed: Vec<i64> = conn
        .query_row("SELECT value FROM settings WHERE key = 'analyze_failed'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    failed.extend_from_slice(ids);
    failed.sort_unstable();
    failed.dedup();
    let _ = conn.execute(
        "INSERT INTO settings (key, value) VALUES ('analyze_failed', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [serde_json::to_string(&failed).unwrap_or_default()],
    );
}

/// Un réglage de la Carrière (« 1 » ou « 0 ») avec sa valeur par défaut (celle de Studio Vault).
pub fn flag(conn: &Connection, key: &str, default: bool) -> bool {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get::<_, String>(0)).map(|v| v == "1").unwrap_or(default)
}

pub fn set_flag(conn: &Connection, key: &str, on: bool) {
    let _ = conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, if on { "1" } else { "0" }],
    );
}

/// Nombre de bits différents entre deux empreintes audio (999 si incomparables).
pub fn fingerprint_distance(a: &str, b: &str) -> u32 {
    let parse = |s: &str| -> Option<(i64, u64)> {
        let (d, h) = s.split_once(':')?;
        Some((d.parse().ok()?, u64::from_str_radix(h, 16).ok()?))
    };
    match (parse(a), parse(b)) {
        (Some((da, ha)), Some((db, hb))) if (da - db).abs() <= 1 => (ha ^ hb).count_ones(),
        _ => 999,
    }
}

/// Les genres déjà employés (pastille « Genre »).
pub fn genres(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT DISTINCT genre FROM tracks WHERE missing = 0 AND genre IS NOT NULL AND genre <> '' ORDER BY genre COLLATE NOCASE")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_and_orders() {
        let r = Row { title: "Nuit".into(), bpm: Some(124.4), key: "Am".into(), duration: Some(245.0), lufs: Some(-8.04), pick: 1, file_date: "2026-11-03 10:00:00".into(), track_no: Some(3), status: "done".into(), ..Default::default() };
        assert_eq!((r.cell("bpm"), r.cell("camelot"), r.cell("duration_s"), r.cell("lufs")), ("124".into(), "8A".into(), "4:05".into(), "-8.0".into()));
        assert_eq!((r.cell("pick"), r.cell("file_date"), r.cell("track_no"), r.cell("status")), ("✓".into(), "03/11/2026".into(), "03".into(), "Terminé".into()));
        assert_eq!(order_sql("bpm", false), "bpm IS NULL, bpm DESC");
        assert_eq!(order_sql("album", true), DEFAULT_ORDER);
        let p = vec![Value::Integer(3), Value::Real(1.5), Value::Text("a".into())];
        assert_eq!(json_params(&params_json(&p)), p);
    }
}
