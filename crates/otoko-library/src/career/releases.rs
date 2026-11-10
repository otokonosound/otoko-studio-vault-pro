//! Les sorties : planning à rebours depuis la date de sortie (comme `services/planning.py`) et
//! rapprochement entre l'album écrit dans les fichiers et les fiches (comme `services/releases_sync.py`).

use std::path::Path;

use chrono::NaiveDate;
use rusqlite::{Connection, params};

use super::{nature, platforms_label, platforms_of, release_label, steps_for};

/// Étape → jours avant la sortie, pour une sortie mise en ligne par soi-même (SoundCloud, Bandcamp, YouTube).
pub const OFFSETS: [(&str, i64); 9] =
    [("master", 21), ("pochette", 14), ("infos", 10), ("controle", 7), ("textes", 3), ("soundcloud", 1), ("bandcamp", 1), ("youtube", 1), ("promo", 0)];
/// Les mêmes, quand la sortie passe aussi par un distributeur : il lui faut tout (master, pochette, infos) environ trois
/// semaines avant, le temps de livrer les plateformes et de proposer le morceau aux playlists.
pub const OFFSETS_STREAMING: [(&str, i64); 5] = [("master", 35), ("pochette", 30), ("infos", 28), ("controle", 25), ("distrib", 21)];

fn offset(key: &str, streaming: bool) -> i64 {
    let early = if streaming { OFFSETS_STREAMING.iter().find(|o| o.0 == key) } else { None };
    early.or_else(|| OFFSETS.iter().find(|o| o.0 == key)).map(|o| o.1).unwrap_or(0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub key: &'static str,
    pub label: &'static str,
    pub done: bool,
    pub due: NaiveDate,
    pub late: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    pub id: i64,
    pub title: String,
    pub kind: String,
    pub date: NaiveDate,
    pub days: i64,
    pub steps: Vec<Step>,
    pub remaining: usize,
}

pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// Les sorties datées, chaque étape avec sa date conseillée.
pub fn planning(conn: &Connection, today: NaiveDate) -> Vec<Planned> {
    let rows: Vec<(i64, String, String, String, String, String)> = conn
        .prepare("SELECT id, title, IFNULL(rtype, ''), IFNULL(release_date, ''), IFNULL(steps, '{}'), IFNULL(extra, '{}') FROM releases WHERE release_date <> '' ORDER BY release_date")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect())
        .unwrap_or_default();
    rows.into_iter()
        .filter_map(|(id, title, kind, date, steps, extra): (i64, String, String, String, String, String)| {
            let d = NaiveDate::parse_from_str(date.get(..10)?, "%Y-%m-%d").ok()?;
            let done = nature::load_meta(&steps);
            let platforms = platforms_of(&nature::load_meta(&extra));
            let streaming = platforms.iter().any(|p| p == "streaming");
            let steps: Vec<Step> = steps_for(&platforms)
                .into_iter()
                .map(|(key, label)| {
                    let due = d - chrono::Days::new(offset(key, streaming) as u64);
                    let ok = done[key].as_bool().unwrap_or(false);
                    Step { key, label, done: ok, due, late: !ok && due < today }
                })
                .collect();
            let remaining = steps.iter().filter(|s| !s.done).count();
            Some(Planned { id, title, kind, date: d, days: (d - today).num_days(), steps, remaining })
        })
        .collect()
}

/// Les libellés longs des étapes (comme dans la fiche de Studio Vault).
pub fn step_label(key: &str) -> &'static str {
    match key {
        "master" => "Master terminé",
        "pochette" => "Pochette prête",
        "infos" => "Titres, crédits et description",
        "controle" => "Contrôle avant publication",
        "distrib" => "Envoyé au distributeur",
        "soundcloud" => "Mis en ligne sur SoundCloud",
        "bandcamp" => "Mis en ligne sur Bandcamp",
        "youtube" => "Mis en ligne sur YouTube",
        "textes" => "Textes de publication écrits",
        "promo" => "Annoncé sur les réseaux",
        _ => "",
    }
}

/// Sorties à venir (ou du jour) dans les `horizon` jours avec des étapes en retard ou dues.
pub fn alerts(conn: &Connection, horizon: i64, today: NaiveDate) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    for rel in planning(conn, today) {
        if !(0..=horizon).contains(&rel.days) || rel.remaining == 0 {
            continue;
        }
        let late: Vec<String> = rel.steps.iter().filter(|s| s.late).map(|s| s.label.to_lowercase()).collect();
        let when = if rel.days == 0 { "aujourd'hui".to_string() } else { format!("dans {} jour(s)", rel.days) };
        let txt = if !late.is_empty() {
            format!("{} sort {when} : en retard sur {}{}", rel.title, late.iter().take(3).cloned().collect::<Vec<_>>().join(", "), if late.len() > 3 { "…" } else { "" })
        } else {
            let next = rel.steps.iter().find(|s| !s.done).expect("une étape reste");
            format!("{} sort {when} : prochaine étape « {} » avant le {}", rel.title, next.label.to_lowercase(), next.due.format("%d/%m"))
        };
        out.push((rel.id, txt));
    }
    out
}

pub fn planning_alerts(conn: &Connection) -> Vec<(i64, String)> {
    alerts(conn, 30, today())
}

/// Rattacher des morceaux à une fiche (existante, ou à créer).
#[derive(Clone, Debug, PartialEq)]
pub struct SyncAction {
    /// « link » ou « create ».
    pub kind: &'static str,
    pub release_id: Option<i64>,
    pub title: String,
    pub rtype: String,
    pub track_ids: Vec<i64>,
    pub copies: usize,
    pub elsewhere: usize,
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// « 14.Kreyòl Horizon.wav », « LAB 7 - 01 - Slow Burn » → nom sans numéro ni extension.
pub fn name_key(s: &str) -> String {
    let tail: String = s.chars().rev().take(6).collect();
    let s = if tail.contains('.') { Path::new(s).file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default() } else { s.to_string() };
    let re = regex::Regex::new(r"(?i)^(lab\s*\d+\s*-\s*)?\d+\s*[.\-_)]*\s*").unwrap();
    let s = re.replace(s.trim(), "");
    let words = regex::Regex::new(r"[^\w]+").unwrap();
    words.replace_all(&s, " ").trim().to_lowercase()
}

pub fn guess_type(n: usize, current: &str) -> String {
    if !current.is_empty() {
        return current.into();
    }
    match n {
        0..=2 => "single",
        3..=6 => "ep",
        _ => "album",
    }
    .into()
}

fn parent_key(p: &str) -> String {
    Path::new(p).parent().map(|d| d.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Ce qu'il faudrait rattacher (à montrer avant d'appliquer).
pub fn sync_plan(conn: &Connection) -> Vec<SyncAction> {
    let releases: Vec<(i64, String, String)> = conn
        .prepare("SELECT id, title, IFNULL(rtype, '') FROM releases")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
        .unwrap_or_default();
    struct T {
        id: i64,
        album: String,
        rtype: String,
        title: String,
        filename: String,
        path: String,
    }
    let rows: Vec<T> = conn
        .prepare(
            "SELECT id, album, IFNULL(release_type, ''), IFNULL(title, ''), IFNULL(filename, ''), path FROM tracks
             WHERE missing = 0 AND album IS NOT NULL AND TRIM(album) <> '' AND (role IS NULL OR role NOT IN ('stem', 'version', 'reference'))
               AND id NOT IN (SELECT track_id FROM release_tracks) ORDER BY album, track_no, title",
        )
        .and_then(|mut s| s.query_map([], |r| Ok(T { id: r.get(0)?, album: r.get(1)?, rtype: r.get(2)?, title: r.get(3)?, filename: r.get(4)?, path: r.get(5)? }))?.collect())
        .unwrap_or_default();
    // Groupes par album (casse et espaces ignorés), dans l'ordre d'apparition.
    let mut groups: Vec<(String, String, Vec<&T>, Vec<String>)> = Vec::new();
    for r in &rows {
        let key = norm(&r.album);
        let i = match groups.iter().position(|g| g.0 == key) {
            Some(i) => i,
            None => {
                groups.push((key, r.album.trim().to_string(), Vec::new(), Vec::new()));
                groups.len() - 1
            }
        };
        groups[i].2.push(r);
        if !r.rtype.is_empty() && !groups[i].3.contains(&r.rtype) {
            groups[i].3.push(r.rtype.clone());
        }
    }
    let mut actions = Vec::new();
    for (key, album, members, types) in groups {
        let typ = if types.len() == 1 { types[0].clone() } else { String::new() };
        let Some((rid, rtitle, rtype)) = releases.iter().find(|r| norm(&r.1) == key) else {
            actions.push(SyncAction {
                kind: "create",
                release_id: None,
                title: album,
                rtype: guess_type(members.len(), &typ),
                track_ids: members.iter().map(|r| r.id).collect(),
                copies: 0,
                elsewhere: 0,
            });
            continue;
        };
        let existing: Vec<(String, String, String)> = conn
            .prepare("SELECT IFNULL(t.title, ''), IFNULL(t.filename, ''), t.path FROM release_tracks rt JOIN tracks t ON t.id = rt.track_id WHERE rt.release_id = ?1")
            .and_then(|mut s| s.query_map([rid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
            .unwrap_or_default();
        let mut names: Vec<String> = existing.iter().flat_map(|e| [name_key(&e.1), name_key(&e.0)]).filter(|n| !n.is_empty()).collect();
        names.dedup();
        let dirs: Vec<String> = existing.iter().map(|e| parent_key(&e.2)).collect();
        let (mut ids, mut copies, mut elsewhere) = (Vec::new(), 0, 0);
        for r in members {
            if names.contains(&name_key(&r.filename)) || names.contains(&name_key(&r.title)) {
                copies += 1;
            } else if !dirs.is_empty() && !dirs.contains(&parent_key(&r.path)) {
                elsewhere += 1;
            } else {
                ids.push(r.id);
            }
        }
        if !ids.is_empty() {
            actions.push(SyncAction { kind: "link", release_id: Some(*rid), title: rtitle.clone(), rtype: rtype.clone(), track_ids: ids, copies, elsewhere });
        }
    }
    actions
}

pub fn describe(a: &SyncAction) -> String {
    let n = a.track_ids.len();
    if a.kind == "link" {
        format!("« {} » : rattacher {n} morceau(x) à la fiche existante", a.title)
    } else {
        format!("« {} » : créer une fiche ({}) avec {n} morceau(x)", a.title, release_label(&a.rtype).to_lowercase())
    }
}

/// Applique le rapprochement ; rend le nombre de morceaux rattachés.
pub fn sync_apply(conn: &Connection, actions: &[SyncAction]) -> rusqlite::Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    for a in actions {
        let rid = match a.release_id {
            Some(r) => r,
            None => {
                let extra = serde_json::json!({ "platforms": default_platforms(&tx) }).to_string();
                tx.execute("INSERT INTO releases (title, rtype, extra) VALUES (?1, ?2, ?3)", params![a.title, a.rtype, extra])?;
                tx.last_insert_rowid()
            }
        };
        let mut pos: i64 = tx.query_row("SELECT COALESCE(MAX(position), 0) FROM release_tracks WHERE release_id = ?1", [rid], |r| r.get(0))?;
        for tid in &a.track_ids {
            pos += 1;
            tx.execute("INSERT OR IGNORE INTO release_tracks (release_id, track_id, position) VALUES (?1, ?2, ?3)", params![rid, tid, pos])?;
            // Le type de sortie suit la fiche ; l'album écrit dans le fichier n'est pas touché.
            tx.execute("UPDATE tracks SET release_type = ?2 WHERE id = ?1", params![tid, a.rtype])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

// ---------------------------------------------------------------------- galerie et fiches
// (comme `ui/releases_page.py` et `ui/releases_dialog.py`)

/// « 2026-07-08 » → « 08/07/2026 » (vide si la date n'en est pas une).
pub fn fr_date(iso: &str) -> String {
    match iso.get(..10).map(|d| d.split('-').collect::<Vec<_>>()) {
        Some(p) if p.len() == 3 => format!("{}/{}/{}", p[2], p[1], p[0]),
        _ => String::new(),
    }
}

/// « 08/07/2026 » (ou « 8/7/2026 », « 08-07-2026 ») → « 2026-07-08 » ; vide si ce n'est pas une date.
pub fn iso_date(fr: &str) -> String {
    let p: Vec<&str> = fr.trim().split(['/', '-', '.', ' ']).filter(|s| !s.is_empty()).collect();
    if p.len() != 3 {
        return String::new();
    }
    let (d, m, y) = (p[0].parse::<u32>(), p[1].parse::<u32>(), p[2].parse::<i32>());
    match (d, m, y) {
        (Ok(d), Ok(m), Ok(y)) => {
            let y = if y < 100 { 2000 + y } else { y };
            NaiveDate::from_ymd_opt(y, m, d).map(|x| x.format("%Y-%m-%d").to_string()).unwrap_or_default()
        }
        _ => String::new(),
    }
}

const JOURS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
const MOIS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

/// « 2026-07-08 » → « mercredi 8 juillet 2026 ».
pub fn long_date(iso: &str) -> String {
    use chrono::Datelike;
    match iso.get(..10).and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) {
        Some(d) => format!("{} {}{} {} {}", JOURS[d.weekday().num_days_from_monday() as usize], d.day(), if d.day() == 1 { "er" } else { "" }, MOIS[d.month0() as usize], d.year()),
        None => String::new(),
    }
}

pub fn month_label(year: i32, month: u32) -> String {
    format!("{} {year}", MOIS[(month.clamp(1, 12) - 1) as usize])
}

/// Les 42 jours affichés pour un mois (semaines du lundi au dimanche) : (date, jour, dans le mois).
pub fn month_grid(year: i32, month: u32) -> Vec<(NaiveDate, u32, bool)> {
    use chrono::Datelike;
    let first = NaiveDate::from_ymd_opt(year, month.clamp(1, 12), 1).unwrap_or_else(today);
    let start = first - chrono::Days::new(first.weekday().num_days_from_monday() as u64);
    (0..42).map(|k| start + chrono::Days::new(k)).map(|d| (d, d.day(), d.month() == first.month())).collect()
}

/// Le mois d'à côté.
pub fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let k = year * 12 + month as i32 - 1 + delta;
    (k.div_euclid(12), k.rem_euclid(12) as u32 + 1)
}

/// CRC-32 (celui de zlib) : Studio Vault en tire la couleur d'une pochette générée.
fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// La couleur de la pochette générée d'une sortie sans image (la même que dans Studio Vault).
pub fn tile_color(title: &str) -> &'static str {
    const PALETTE: [&str; 8] = ["#534AB7", "#0F6E56", "#993C1D", "#185FA5", "#993556", "#3B6D11", "#854F0B", "#444441"];
    PALETTE[crc32(title.as_bytes()) as usize % PALETTE.len()]
}

/// Les initiales d'une pochette générée (deux mots au plus).
pub fn initials(title: &str) -> String {
    let s: String = title.split_whitespace().take(2).filter_map(|w| w.chars().next()).filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_uppercase()).collect();
    if s.is_empty() { "?".into() } else { s }
}

/// Une tuile de la galerie.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GalleryItem {
    pub id: i64,
    pub title: String,
    pub sub: String,
    pub done: usize,
    pub total: usize,
    pub cover_path: String,
}

pub fn gallery(conn: &Connection) -> Vec<GalleryItem> {
    conn.prepare(
        "SELECT r.id, r.title, IFNULL(r.rtype, ''), IFNULL(r.release_date, ''), IFNULL(r.steps, '{}'), IFNULL(r.cover_path, ''), COUNT(rt.track_id), IFNULL(r.extra, '{}')
         FROM releases r LEFT JOIN release_tracks rt ON rt.release_id = r.id GROUP BY r.id
         ORDER BY (IFNULL(r.release_date, '') = ''), r.release_date DESC, r.title",
    )
    .and_then(|mut s| {
        s.query_map([], |r| {
            let date: String = r.get(3)?;
            let (done, total) = super::steps_count(&r.get::<_, String>(4)?, &r.get::<_, String>(7)?);
            let n: i64 = r.get(6)?;
            let when = fr_date(&date);
            Ok(GalleryItem {
                id: r.get(0)?,
                title: r.get(1)?,
                sub: format!("{} · {n} titre(s){}", release_label(&r.get::<_, String>(2)?), if when.is_empty() { String::new() } else { format!(" · {when}") }),
                done,
                total,
                cover_path: r.get(5)?,
            })
        })?
        .collect()
    })
    .unwrap_or_default()
}

/// La fiche d'une sortie.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sheet {
    pub id: i64,
    pub title: String,
    pub rtype: String,
    /// « AAAA-MM-JJ », vide : date à fixer.
    pub date: String,
    pub sc_link: String,
    pub cover_path: String,
    pub credits: String,
    pub description: String,
    pub notes: String,
    /// Les étapes faites (clés de `ALL_STEPS`, parmi celles de ses plateformes).
    pub steps: Vec<String>,
    /// Les morceaux, dans l'ordre.
    pub tracks: Vec<i64>,
    /// Où elle sort (clés de `PLATFORMS`).
    pub platforms: Vec<String>,
    /// Pour le distributeur : le code-barres (UPC/EAN), le label, la langue des paroles, contenu explicite.
    pub upc: String,
    pub label: String,
    pub language: String,
    pub explicit: bool,
    /// Le reste du JSON `extra`, gardé tel quel.
    pub extra: serde_json::Value,
}

impl Sheet {
    pub fn streaming(&self) -> bool {
        self.platforms.iter().any(|p| p == "streaming")
    }

    pub fn on(&self, platform: &str) -> bool {
        self.platforms.iter().any(|p| p == platform)
    }
}

/// Les plateformes proposées pour une nouvelle sortie (réglage partagé `release_platforms`, sinon SoundCloud).
pub fn default_platforms(conn: &Connection) -> Vec<String> {
    let raw: String = conn.query_row("SELECT value FROM settings WHERE key = 'release_platforms'", [], |r| r.get(0)).unwrap_or_default();
    let list: Vec<String> = super::PLATFORMS.iter().map(|p| p.0).filter(|k| raw.split(',').any(|x| x.trim() == *k)).map(String::from).collect();
    if list.is_empty() { vec!["soundcloud".into()] } else { list }
}

pub fn set_default_platforms(conn: &Connection, platforms: &[String]) -> rusqlite::Result<()> {
    conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('release_platforms', ?1)", [platforms.join(",")])?;
    Ok(())
}

pub fn load(conn: &Connection, id: i64) -> Option<Sheet> {
    let mut s = conn
        .query_row(
            "SELECT id, title, IFNULL(rtype, 'single'), IFNULL(release_date, ''), IFNULL(sc_link, ''), IFNULL(cover_path, ''), IFNULL(credits, ''),
                    IFNULL(description, ''), IFNULL(notes, ''), IFNULL(steps, '{}'), IFNULL(extra, '{}') FROM releases WHERE id = ?1",
            [id],
            |r| {
                let steps = nature::load_meta(&r.get::<_, String>(9)?);
                let extra = nature::load_meta(&r.get::<_, String>(10)?);
                let platforms = platforms_of(&extra);
                let text = |k: &str| extra[k].as_str().unwrap_or("").to_string();
                Ok(Sheet {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    rtype: r.get(2)?,
                    date: r.get(3)?,
                    sc_link: r.get(4)?,
                    cover_path: r.get(5)?,
                    credits: r.get(6)?,
                    description: r.get(7)?,
                    notes: r.get(8)?,
                    steps: steps_for(&platforms).iter().filter(|(k, _)| steps[*k].as_bool().unwrap_or(false)).map(|(k, _)| k.to_string()).collect(),
                    tracks: Vec::new(),
                    upc: text("upc"),
                    label: text("label"),
                    language: text("language"),
                    explicit: extra["explicit"].as_bool().unwrap_or(false),
                    platforms,
                    extra,
                })
            },
        )
        .ok()?;
    s.tracks = conn
        .prepare("SELECT track_id FROM release_tracks WHERE release_id = ?1 ORDER BY position")
        .and_then(|mut st| st.query_map([id], |r| r.get(0))?.collect())
        .unwrap_or_default();
    Some(s)
}

/// Enregistre la fiche (et l'ordre de ses morceaux).
pub fn save(conn: &Connection, s: &Sheet) -> rusqlite::Result<()> {
    // Toutes les étapes connues sont écrites : une plateforme retirée puis remise retrouve ce qui était coché.
    let steps: serde_json::Map<String, serde_json::Value> =
        super::ALL_STEPS.iter().map(|(k, _)| (k.to_string(), serde_json::Value::Bool(s.steps.iter().any(|x| x == k)))).collect();
    let mut extra = if s.extra.is_object() { s.extra.clone() } else { serde_json::json!({}) };
    extra["platforms"] = serde_json::json!(s.platforms);
    extra["upc"] = serde_json::json!(s.upc.trim());
    extra["label"] = serde_json::json!(s.label.trim());
    extra["language"] = serde_json::json!(s.language.trim());
    extra["explicit"] = serde_json::json!(s.explicit);
    let title = if s.title.trim().is_empty() { "Sans titre" } else { s.title.trim() };
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE releases SET title = ?2, rtype = ?3, release_date = ?4, cover_path = ?5, credits = ?6, description = ?7, sc_link = ?8, notes = ?9, steps = ?10,
                extra = ?11
         WHERE id = ?1",
        params![
            s.id,
            title,
            s.rtype,
            s.date,
            s.cover_path,
            s.credits.trim(),
            s.description.trim(),
            s.sc_link.trim(),
            s.notes.trim(),
            serde_json::Value::Object(steps).to_string(),
            extra.to_string()
        ],
    )?;
    tx.execute("DELETE FROM release_tracks WHERE release_id = ?1", [s.id])?;
    for (pos, tid) in s.tracks.iter().enumerate() {
        tx.execute("INSERT OR IGNORE INTO release_tracks (release_id, track_id, position) VALUES (?1, ?2, ?3)", params![s.id, tid, pos as i64 + 1])?;
    }
    tx.commit()
}

/// Une nouvelle sortie (type deviné d'après le nombre de morceaux), avec ces morceaux.
pub fn create(conn: &Connection, title: &str, track_ids: &[i64]) -> rusqlite::Result<i64> {
    let extra = serde_json::json!({ "platforms": default_platforms(conn) }).to_string();
    conn.execute("INSERT INTO releases (title, rtype, extra) VALUES (?1, ?2, ?3)", params![title.trim(), guess_type(track_ids.len(), ""), extra])?;
    let id = conn.last_insert_rowid();
    add_tracks(conn, id, track_ids)?;
    Ok(id)
}

/// Ajoute des morceaux à la fin d'une sortie (ceux qui y sont déjà restent à leur place).
pub fn add_tracks(conn: &Connection, rid: i64, ids: &[i64]) -> rusqlite::Result<usize> {
    let mut pos: i64 = conn.query_row("SELECT COALESCE(MAX(position), 0) FROM release_tracks WHERE release_id = ?1", [rid], |r| r.get(0))?;
    let mut n = 0;
    for id in ids {
        pos += 1;
        n += conn.execute("INSERT OR IGNORE INTO release_tracks (release_id, track_id, position) VALUES (?1, ?2, ?3)", params![rid, id, pos])?;
    }
    Ok(n)
}

/// Supprime la fiche ; ses morceaux restent dans la bibliothèque.
pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM release_tracks WHERE release_id = ?1", [id])?;
    conn.execute("DELETE FROM releases WHERE id = ?1", [id])?;
    Ok(())
}

/// Album = titre de la sortie, type de sortie et numéro de piste selon l'ordre, écrits dans la bibliothèque.
pub fn apply_to_tracks(conn: &Connection, s: &Sheet) -> rusqlite::Result<usize> {
    for (n, tid) in s.tracks.iter().enumerate() {
        conn.execute(
            "UPDATE tracks SET album = ?2, release_type = ?3, track_no = ?4, modified_at = datetime('now') WHERE id = ?1",
            params![tid, s.title.trim(), s.rtype, n as i64 + 1],
        )?;
    }
    Ok(s.tracks.len())
}

/// Un morceau tel que la fiche le montre.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackLine {
    pub id: i64,
    pub title: String,
    pub sub: String,
    pub path: String,
    pub private: bool,
}

pub fn track_lines(conn: &Connection, ids: &[i64]) -> Vec<TrackLine> {
    ids.iter()
        .filter_map(|id| {
            conn.query_row(
                "SELECT COALESCE(NULLIF(title, ''), filename), path, duration_s, bpm, IFNULL(music_key, ''), IFNULL(NULLIF(nature, ''), 'original'), IFNULL(nature_meta, '{}'), missing
                 FROM tracks WHERE id = ?1",
                [id],
                |r| {
                    let (nat, meta): (String, String) = (r.get(5)?, r.get(6)?);
                    let mut bits = Vec::new();
                    if let Some(d) = r.get::<_, Option<f64>>(2)? {
                        bits.push(nature::fmt_time(d, false));
                    }
                    if let Some(b) = r.get::<_, Option<f64>>(3)? {
                        bits.push(format!("{} BPM", b.round() as i64));
                    }
                    let key: String = r.get(4)?;
                    if !key.is_empty() {
                        bits.push(key);
                    }
                    if nat != "original" {
                        bits.push(super::nature_short(&nat));
                    }
                    if r.get::<_, i64>(7)? != 0 {
                        bits.push("fichier introuvable".into());
                    }
                    let title: String = r.get(0)?;
                    Ok(TrackLine {
                        id: *id,
                        title: match title.rsplit_once('.') {
                            Some((stem, ext)) if super::scan::AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()) => stem.to_string(),
                            _ => title,
                        },
                        sub: bits.join(" · "),
                        path: r.get(1)?,
                        private: nature::is_private(&nat, &nature::load_meta(&meta)),
                    })
                },
            )
            .ok()
        })
        .collect()
}

/// Le nom d'artiste (univers de marque, sinon le réglage `artist_alias`).
pub fn artist_name(conn: &Connection) -> String {
    let get = |k: &str| conn.query_row("SELECT value FROM settings WHERE key = ?1", [k], |r| r.get::<_, String>(0)).unwrap_or_default();
    let a = get("pub_artist_name");
    if a.trim().is_empty() { get("artist_alias") } else { a }
}

/// Crédits pré-remplis selon la nature des morceaux (une ligne par nature, le titre devant si
/// la sortie a plusieurs morceaux).
pub fn prefill_credits(conn: &Connection, ids: &[i64], artist: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for id in ids {
        let Ok((title, nat, meta)) = conn.query_row(
            "SELECT COALESCE(NULLIF(title, ''), filename), IFNULL(NULLIF(nature, ''), 'original'), IFNULL(nature_meta, '{}') FROM tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        ) else {
            continue;
        };
        let mut line = nature::credits_for(&nat, &nature::load_meta(&meta), artist);
        if nat != "original" && ids.len() > 1 {
            line = format!("{title} : {line}");
        }
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        lines.push(nature::credits_for("original", &serde_json::json!({}), artist));
    }
    lines.join("\n")
}

/// La fiche en texte (à envoyer, à imprimer).
pub fn export_text(conn: &Connection, s: &Sheet) -> String {
    let mut lines = vec![s.title.clone(), format!("Type : {}", release_label(&s.rtype))];
    if !s.date.is_empty() {
        lines.push(format!("Date de sortie : {}", fr_date(&s.date)));
    }
    lines.push(format!("Plateformes : {}", platforms_label(&s.platforms)));
    if !s.sc_link.trim().is_empty() {
        lines.push(format!("SoundCloud : {}", s.sc_link.trim()));
    }
    if !s.cover_path.is_empty() {
        lines.push(format!("Pochette : {}", s.cover_path));
    }
    for (name, v) in [("UPC / EAN", s.upc.trim()), ("Label", s.label.trim()), ("Langue", s.language.trim())] {
        if !v.is_empty() {
            lines.push(format!("{name} : {v}"));
        }
    }
    if s.explicit {
        lines.push("Contenu explicite : oui".into());
    }
    lines.push(String::new());
    lines.push("Morceaux :".into());
    for (n, id) in s.tracks.iter().enumerate() {
        let Ok((title, path, dur, bpm, key, lufs, isrc)) = conn.query_row(
            "SELECT COALESCE(NULLIF(title, ''), filename), path, duration_s, bpm, IFNULL(music_key, ''), lufs, IFNULL(isrc, '') FROM tracks WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<f64>>(5)?,
                    r.get::<_, String>(6)?,
                ))
            },
        ) else {
            continue;
        };
        let mut bits = Vec::new();
        if let Some(d) = dur {
            let s = d.round() as i64;
            bits.push(format!("{}:{:02}", s / 60, s % 60));
        }
        if let Some(b) = bpm {
            bits.push(format!("{} BPM", b.round() as i64));
        }
        if !key.is_empty() {
            bits.push(key);
        }
        if let Some(l) = lufs {
            bits.push(format!("{l:.1} LUFS"));
        }
        if !isrc.is_empty() {
            bits.push(format!("ISRC {isrc}"));
        }
        lines.push(format!("  {:02}. {title}  ({})", n + 1, bits.join(", ")));
        lines.push(format!("      {path}"));
    }
    if !s.credits.trim().is_empty() {
        lines.extend([String::new(), "Crédits :".into(), s.credits.trim().to_string()]);
    }
    if !s.description.trim().is_empty() {
        lines.extend([String::new(), "Description :".into(), s.description.trim().to_string()]);
    }
    lines.join("\n") + "\n"
}

/// Un ISRC écrit proprement (« FR-ABC-26-00001 » → « FRABC2600001 »), ou rien s'il n'en est pas un : deux lettres
/// (le pays), trois caractères, deux chiffres (l'année), cinq chiffres.
pub fn isrc_clean(raw: &str) -> Option<String> {
    let s: String = raw.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase();
    let b = s.as_bytes();
    let ok = b.len() == 12 && b[..2].iter().all(u8::is_ascii_alphabetic) && b[2..5].iter().all(u8::is_ascii_alphanumeric) && b[5..].iter().all(u8::is_ascii_digit);
    ok.then_some(s)
}

/// Écrit l'ISRC d'un morceau (vide l'efface). Rend ce qui est gardé, ou pourquoi c'est refusé.
pub fn set_isrc(conn: &Connection, track: i64, raw: &str) -> Result<String, String> {
    let v = if raw.trim().is_empty() { String::new() } else { isrc_clean(raw).ok_or("ISRC invalide : 12 caractères, comme FR-ABC-26-00001.")? };
    conn.execute("UPDATE tracks SET isrc = ?2, modified_at = datetime('now') WHERE id = ?1", params![track, v]).map_err(|e| e.to_string())?;
    Ok(v)
}

/// Un code-barres de sortie (UPC à 12 chiffres, EAN à 13), espaces retirés.
pub fn upc_clean(raw: &str) -> Option<String> {
    let s: String = raw.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    (matches!(s.len(), 12 | 13) && s.bytes().all(|b| b.is_ascii_digit())).then_some(s)
}

/// Ce que `delivery` a préparé.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Delivery {
    pub dir: std::path::PathBuf,
    /// Les fichiers audio copiés.
    pub copied: usize,
    /// Ce qui n'a pas pu être copié (titre ou pochette).
    pub missing: Vec<String>,
}

/// Le fichier à envoyer pour ce morceau : son master le plus récent s'il en a un (et qu'il est là), sinon lui-même.
fn delivered_file(conn: &Connection, id: i64) -> Option<String> {
    conn.query_row(
        "SELECT path FROM tracks WHERE parent_id = ?1 AND version_label = 'master' AND IFNULL(missing, 0) = 0 ORDER BY id DESC LIMIT 1",
        [id],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .filter(|p| Path::new(p).is_file())
    .or_else(|| conn.query_row("SELECT path FROM tracks WHERE id = ?1", [id], |r| r.get::<_, String>(0)).ok())
}

fn file_name(s: &str) -> String {
    let t: String = s.trim().chars().map(|ch| if r#"\/:*?"<>|"#.contains(ch) || ch.is_control() { '_' } else { ch }).collect();
    let t = t.trim_end_matches(['.', ' ']).to_string();
    if t.is_empty() { "sans titre".into() } else { t }
}

/// Un champ de tableur : entre guillemets s'il contient un séparateur, un guillemet ou un retour.
fn csv_field(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// Prépare le dossier à envoyer au distributeur (ou à garder) : les morceaux numérotés dans l'ordre de la sortie (le
/// master quand il existe), la pochette, un tableau des infos (Excel, LibreOffice) et la fiche en texte. Un dossier déjà
/// là n'est pas écrasé : le nouveau prend un numéro.
pub fn delivery(conn: &Connection, s: &Sheet, dir: &Path) -> Result<Delivery, String> {
    let mut target = dir.to_path_buf();
    let mut n = 2;
    while target.exists() {
        target = std::path::PathBuf::from(format!("{} {n}", dir.display()));
        n += 1;
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("{} : {e}", target.display()))?;
    let artist = artist_name(conn);
    let mut out = Delivery { dir: target.clone(), ..Default::default() };
    let mut rows = vec!["Piste;Titre;Artiste;Genre;Durée;ISRC;Langue;Explicite;Fichier".to_string()];
    for (i, id) in s.tracks.iter().enumerate() {
        let Ok((title, track_artist, genre, dur, isrc)) = conn.query_row(
            "SELECT COALESCE(NULLIF(title, ''), filename), IFNULL(artist, ''), IFNULL(genre, ''), duration_s, IFNULL(isrc, '') FROM tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<f64>>(3)?, r.get::<_, String>(4)?)),
        ) else {
            continue;
        };
        let title = match title.rsplit_once('.') {
            Some((stem, ext)) if super::scan::AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()) => stem.to_string(),
            _ => title,
        };
        let src = delivered_file(conn, *id).unwrap_or_default();
        let ext = Path::new(&src).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "wav".into());
        let name = format!("{:02} - {}.{ext}", i + 1, file_name(&title));
        if !src.is_empty() && Path::new(&src).is_file() && std::fs::copy(&src, target.join(&name)).is_ok() {
            out.copied += 1;
        } else {
            out.missing.push(title.clone());
        }
        let duration = dur.map(|d| d.round() as i64).map(|d| format!("{}:{:02}", d / 60, d % 60)).unwrap_or_default();
        let who = if track_artist.trim().is_empty() { artist.clone() } else { track_artist };
        let explicit = if s.explicit { "oui" } else { "non" };
        let cells = [format!("{}", i + 1), title, who, genre, duration, isrc, s.language.trim().to_string(), explicit.to_string(), name];
        rows.push(cells.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(";"));
    }
    if !s.cover_path.trim().is_empty() {
        let cover = Path::new(s.cover_path.trim());
        let ext = cover.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "jpg".into());
        if !(cover.is_file() && std::fs::copy(cover, target.join(format!("Pochette.{ext}"))).is_ok()) {
            out.missing.push("la pochette".into());
        }
    }
    // Un BOM pour qu'Excel lise les accents, des fins de ligne Windows.
    let csv = format!("\u{feff}{}\r\n", rows.join("\r\n"));
    std::fs::write(target.join("Infos.csv"), csv).map_err(|e| e.to_string())?;
    std::fs::write(target.join("Fiche.txt"), export_text(conn, s).replace('\n', "\r\n")).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Le texte d'un champ d'agenda (virgules, points-virgules et retours échappés).
fn ics_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

/// Les rappels à mettre dans un agenda (Google Agenda, Outlook, Calendrier d'Apple ou du téléphone) : chaque sortie
/// à venir, chaque étape qui reste avec sa date conseillée, et les échéances des commandes de clients. Un rappel le jour
/// même à 9 h. Les identifiants sont stables : réimporter le fichier met à jour au lieu de doubler.
pub fn agenda_ics(conn: &Connection, today: NaiveDate) -> String {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut out = vec!["BEGIN:VCALENDAR".to_string(), "VERSION:2.0".into(), "PRODID:-//OTOKO//Studio Vault Pro//FR".into(), "CALSCALE:GREGORIAN".into(), "X-WR-CALNAME:OTOKO".into()];
    let mut event = |uid: String, day: NaiveDate, summary: String, desc: String| {
        let next = day + chrono::Days::new(1);
        out.extend([
            "BEGIN:VEVENT".to_string(),
            format!("UID:{uid}@otoko"),
            format!("DTSTAMP:{stamp}"),
            format!("DTSTART;VALUE=DATE:{}", day.format("%Y%m%d")),
            format!("DTEND;VALUE=DATE:{}", next.format("%Y%m%d")),
            format!("SUMMARY:{}", ics_text(&summary)),
            format!("DESCRIPTION:{}", ics_text(&desc)),
            "TRANSP:TRANSPARENT".into(),
            "BEGIN:VALARM".into(),
            "ACTION:DISPLAY".into(),
            format!("DESCRIPTION:{}", ics_text(&summary)),
            "TRIGGER:PT9H".into(),
            "END:VALARM".into(),
            "END:VEVENT".into(),
        ]);
    };
    for rel in planning(conn, today).into_iter().filter(|r| r.days >= 0) {
        event(format!("sortie-{}", rel.id), rel.date, format!("Sortie : {}", rel.title), format!("{} sort aujourd'hui.", rel.title));
        for st in rel.steps.iter().filter(|s| !s.done) {
            // Une étape en retard est rappelée aujourd'hui plutôt qu'à une date passée.
            let day = st.due.max(today);
            event(format!("sortie-{}-{}", rel.id, st.key), day, format!("{} : {}", rel.title, step_label(st.key).to_lowercase()), format!("Pour la sortie du {}.", rel.date.format("%d/%m/%Y")));
        }
    }
    let orders: Vec<(i64, String, String)> = conn
        .prepare("SELECT id, COALESCE(NULLIF(title, ''), filename), IFNULL(nature_meta, '{}') FROM tracks WHERE nature = 'prod' AND IFNULL(archived, 0) = 0")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
        .unwrap_or_default();
    for (id, title, meta) in orders {
        let m = nature::load_meta(&meta);
        let open = !["delivered", "paid"].contains(&m["order_status"].as_str().unwrap_or(""));
        let Some(day) = m["deadline"].as_str().and_then(|d| NaiveDate::parse_from_str(d.get(..10)?, "%Y-%m-%d").ok()) else { continue };
        if open && day >= today {
            let who = m["commissioned_by"].as_str().filter(|c| !c.is_empty()).map(|c| format!(" pour {c}")).unwrap_or_default();
            event(format!("commande-{id}"), day, format!("À livrer : {title}{who}"), "Échéance de la commande (fiche projet).".into());
        }
    }
    out.push("END:VCALENDAR".into());
    // Les lignes d'un agenda finissent par CRLF, et se replient au-delà de 75 octets.
    out.iter().map(|l| fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut len = 0;
    for c in line.chars() {
        if len + c.len_utf8() > 74 {
            out.push_str("\r\n ");
            len = 1;
        }
        out.push(c);
        len += c.len_utf8();
    }
    out
}

pub const QC_NAMES: [&str; 6] = ["Format", "Niveau", "Crêtes", "Saturation", "Silences", "Infos"];

/// Contrôle avant publication sur SoundCloud (comme `ui/qc_dialog.py`) : voir `qc_on`.
pub fn qc(conn: &Connection, id: i64) -> Vec<(&'static str, &'static str, String)> {
    qc_on(conn, id, &["soundcloud".to_string()])
}

/// Contrôle avant publication sur ces plateformes : (contrôle, niveau « ok », « attention », « problème » ou « — »,
/// explication). Le streaming (par un distributeur) et YouTube ramènent le volume vers -14 LUFS ; SoundCloud et Bandcamp
/// le laissent tel quel ; un distributeur refuse un fichier compressé.
pub fn qc_on(conn: &Connection, id: i64, platforms: &[String]) -> Vec<(&'static str, &'static str, String)> {
    let streaming = platforms.iter().any(|p| p == "streaming");
    let leveled = streaming || platforms.iter().any(|p| p == "youtube");
    let names = platforms_label(platforms);
    let row = conn.query_row(
        "SELECT LOWER(IFNULL(file_format, '')), samplerate, analyzed_at IS NOT NULL, lufs, true_peak, clip_ratio, silence_start, silence_end,
                IFNULL(title, ''), IFNULL(artist, ''), IFNULL(genre, ''), IFNULL(NULLIF(nature, ''), 'original'), IFNULL(nature_meta, '{}'),
                (SELECT COUNT(*) FROM set_entries WHERE set_id = tracks.id)
         FROM tracks WHERE id = ?1",
        [id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<f64>>(5)?,
                r.get::<_, Option<f64>>(6)?,
                r.get::<_, Option<f64>>(7)?,
                [r.get::<_, String>(8)?, r.get::<_, String>(9)?, r.get::<_, String>(10)?],
                r.get::<_, String>(11)?,
                r.get::<_, String>(12)?,
                r.get::<_, i64>(13)?,
            ))
        },
    );
    let Ok((fmt, sr, analyzed, lufs, tp, clip, ss, se, infos, nat, meta, set_count)) = row else { return Vec::new() };
    let (ok, warn, bad, na) = ("ok", "attention", "problème", "—");
    let mut out: Vec<(&'static str, &'static str, String)> = Vec::new();
    let sr = sr.unwrap_or(0);
    if ["wav", "flac", "aiff", "aif"].contains(&fmt.as_str()) {
        if sr > 48000 {
            let who = if streaming { "vérifie ce que ton distributeur accepte (souvent 44,1 kHz minimum, parfois 96 kHz au plus)" } else { "44,1 ou 48 kHz suffisent et l'envoi sera plus rapide" };
            out.push(("Format", warn, format!("{} en {} kHz : {who}.", fmt.to_uppercase(), sr as f64 / 1000.0)));
        } else if streaming {
            out.push(("Format", ok, format!("{} sans perte : ce que demandent les distributeurs.", fmt.to_uppercase())));
        } else {
            out.push(("Format", ok, format!("{} sans perte : idéal pour {names}, qui fera sa propre conversion.", fmt.to_uppercase())));
        }
    } else if ["mp3", "m4a", "aac", "ogg", "opus"].contains(&fmt.as_str()) {
        if streaming {
            out.push(("Format", bad, format!("{} est compressé : les distributeurs le refusent. Envoie le WAV ou le FLAC.", fmt.to_uppercase())));
        } else {
            out.push(("Format", warn, format!("{} est déjà compressé : {names} va le recompresser. Envoie plutôt un WAV ou un FLAC.", fmt.to_uppercase())));
        }
    } else {
        out.push(("Format", warn, format!("Format « {} » : préfère un WAV ou un FLAC.", if fmt.is_empty() { "?" } else { &fmt })));
    }
    if !analyzed {
        for name in ["Niveau", "Crêtes", "Saturation", "Silences"] {
            out.push((name, na, "Pas encore analysé : lance l'analyse pour ce contrôle.".into()));
        }
    } else {
        out.push(match lufs {
            None => ("Niveau", na, "Niveau sonore non mesuré.".into()),
            // Spotify, Apple Music, Deezer, Tidal et YouTube ramènent tout vers -14 LUFS environ.
            Some(l) if leveled && l > -7.0 => ("Niveau", warn, format!("{l:.1} LUFS : très fort. Les plateformes de streaming vont le baisser d'environ {:.0} dB : la compression en plus n'apporte rien, et elle s'entend.", l + 14.0)),
            Some(l) if leveled && l > -14.0 => ("Niveau", ok, format!("{l:.1} LUFS : les plateformes de streaming le baisseront d'environ {:.0} dB, comme la plupart des morceaux.", l + 14.0)),
            Some(l) if leveled && l < -16.0 => ("Niveau", warn, format!("{l:.1} LUFS : plus bas que -14. Spotify ne le remonte que s'il a de la marge, Apple Music et YouTube jamais : il paraîtra plus faible.")),
            Some(l) if leveled => ("Niveau", ok, format!("{l:.1} LUFS : pile dans la cible du streaming (-14 LUFS), il ne sera presque pas touché.")),
            Some(l) if l > -6.0 => ("Niveau", warn, format!("{l:.1} LUFS : très fort. {names} ne baisse pas le volume, mais un master aussi compressé fatigue l'oreille et sature plus facilement.")),
            Some(l) if l < -16.0 => ("Niveau", warn, format!("{l:.1} LUFS : nettement plus bas que la plupart des morceaux sur {names}, qui ne remonte pas le volume.")),
            Some(l) => ("Niveau", ok, format!("{l:.1} LUFS : dans la zone habituelle sur {names} (repère : -14 à -8).")),
        });
        out.push(match tp {
            None => ("Crêtes", na, "Crête non mesurée.".into()),
            Some(t) if t > 0.0 => ("Crêtes", bad, format!("Crête à {t:+.1} dB : au-dessus de 0. La conversion des plateformes va produire de la distorsion. Vise -1 dB maximum.")),
            Some(t) if t > -1.0 => ("Crêtes", warn, format!("Crête à {t:+.1} dB : un peu haute. Après conversion, de petites saturations sont possibles. Vise -1 dB maximum.")),
            Some(t) => ("Crêtes", ok, format!("Crête à {t:+.1} dB : marge suffisante.")),
        });
        let cr = clip.unwrap_or(0.0);
        out.push(if cr > 0.001 {
            ("Saturation", bad, format!("{:.2} % d'échantillons à 0 dB : saturation audible probable.", cr * 100.0))
        } else if cr > 0.00005 {
            ("Saturation", warn, format!("Quelques échantillons touchent 0 dB ({:.3} %).", cr * 100.0))
        } else {
            ("Saturation", ok, "Aucune saturation détectée.".into())
        });
        let mut msgs = Vec::new();
        if let Some(s) = ss.filter(|s| *s > 1.0) {
            msgs.push(format!("{s:.1} s de silence au début"));
        }
        if let Some(s) = se.filter(|s| *s > 3.0) {
            msgs.push(format!("{s:.1} s de silence à la fin"));
        }
        out.push(if msgs.is_empty() { ("Silences", ok, "Début et fin propres.".into()) } else { ("Silences", warn, format!("{} : pense à couper.", msgs.join(", "))) });
    }
    let missing: Vec<&str> = infos.iter().zip(["titre", "artiste", "genre"]).filter(|(v, _)| v.trim().is_empty()).map(|(_, l)| l).collect();
    out.push(if missing.is_empty() {
        ("Infos", ok, "Titre, artiste et genre renseignés.".into())
    } else {
        ("Infos", warn, format!("Manque : {}. {} lit ces informations à l'envoi.", missing.join(", "), if streaming { "Ton distributeur" } else { names.as_str() }))
    });
    if nat != "original" {
        let meta = nature::load_meta(&meta);
        for (level, msg) in nature::rights_notes(&nat, &meta) {
            out.push(("Droits", if level == "attention" { warn } else { ok }, msg));
        }
        if nat == "set" && set_count == 0 {
            out.push(("Droits", warn, "Aucune liste de titres : remplis-la dans la fiche projet.".into()));
        }
        if nature::is_private(&nat, &meta) {
            out.push(("Droits", bad, "Prod marquée « ne pas publier sur mes réseaux ».".into()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, title, duration_s, bpm, music_key, nature, nature_meta, missing, file_format, samplerate, analyzed_at, lufs, true_peak, clip_ratio)
             VALUES (1, 'D:\\a.wav', 'a.wav', 'Nuit', 245, 124, 'Am', 'original', '{}', 0, 'wav', 44100, '2026-01-01', -9.2, 0.3, 0),
                    (2, 'D:\\b.wav', 'b.wav', 'Jour (Otoko Remix)', 200, 122, '', 'remix', '{\"orig_artist\": \"Daft\", \"orig_title\": \"Jour\"}', 0, 'mp3', 44100, NULL, NULL, NULL, NULL);
             INSERT INTO settings (key, value) VALUES ('artist_alias', 'Otoko');",
        )
        .unwrap();
        let id = create(&conn, " Nuit blanche ", &[1, 2]).unwrap();
        let mut s = load(&conn, id).unwrap();
        assert_eq!((s.title.as_str(), s.rtype.as_str(), s.tracks.clone()), ("Nuit blanche", "single", vec![1, 2]));
        s.tracks.reverse();
        s.steps = vec!["master".into(), "pochette".into()];
        s.date = iso_date("8/7/26");
        assert_eq!(s.date, "2026-07-08");
        save(&conn, &s).unwrap();
        let back = load(&conn, id).unwrap();
        assert_eq!(back.tracks, vec![2, 1]);
        assert_eq!(back.steps, vec!["master", "pochette"]);
        let g = gallery(&conn);
        assert_eq!((g[0].sub.as_str(), g[0].done), ("Single · 2 titre(s) · 08/07/2026", 2));
        assert_eq!(apply_to_tracks(&conn, &back).unwrap(), 2);
        let no: i64 = conn.query_row("SELECT track_no FROM tracks WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(no, 2);
        let credits = prefill_credits(&conn, &back.tracks, &artist_name(&conn));
        assert_eq!(credits, "Jour (Otoko Remix) : Remix par Otoko — titre original : Daft – Jour.\nComposé et produit par Otoko.");
        let text = export_text(&conn, &back);
        assert!(text.contains("  02. Nuit  (4:05, 124 BPM, Am, -9.2 LUFS)"), "{text}");
        let q = qc(&conn, 1);
        // Crête à +0,3 dB : au-dessus de 0, un problème ; ni artiste ni genre.
        assert_eq!(q.iter().map(|x| x.1).collect::<Vec<_>>(), vec!["ok", "ok", "problème", "ok", "ok", "attention"]);
        let q2 = qc(&conn, 2);
        assert_eq!(q2[0].1, "attention");
        assert!(q2.iter().any(|x| x.0 == "Droits"));
        delete(&conn, id).unwrap();
        assert!(load(&conn, id).is_none());
        assert_eq!(fr_date("2026-07-08"), "08/07/2026");
        assert_eq!(iso_date("31/02/2026"), "");
    }

    #[test]
    fn a_release_follows_its_platforms() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, title, artist, genre, file_format, samplerate, analyzed_at, lufs, true_peak, clip_ratio, nature, nature_meta)
             VALUES (1, 'D:\\a.mp3', 'a.mp3', 'Nuit', 'Otoko', 'House', 'mp3', 44100, '2026-01-01', -8.0, -0.5, 0, 'original', '{}'),
                    (2, 'D:\\b.wav', 'b.wav', 'Jour', 'Otoko', 'House', 'wav', 44100, '2026-01-01', -14.2, -1.5, 0, 'original', '{}'),
                    (3, 'D:\\c.wav', 'c.wav', 'Commande', '', '', 'wav', 44100, NULL, NULL, NULL, NULL, 'prod', '{\"deadline\": \"2026-11-20\", \"commissioned_by\": \"Léa\"}');",
        )
        .unwrap();
        // Une sortie d'avant (sans choix) : SoundCloud, comme avant.
        conn.execute("INSERT INTO releases (id, title, release_date) VALUES (1, 'Ancienne', '2026-12-31')", []).unwrap();
        assert_eq!(load(&conn, 1).unwrap().platforms, vec!["soundcloud"]);
        // Une nouvelle sortie prend les plateformes par défaut.
        super::set_default_platforms(&conn, &["streaming".into(), "bandcamp".into()]).unwrap();
        let id = create(&conn, "Nuit", &[1, 2]).unwrap();
        let mut s = load(&conn, id).unwrap();
        assert_eq!(s.platforms, vec!["streaming", "bandcamp"]);
        let keys: Vec<&str> = super::super::steps_for(&s.platforms).iter().map(|x| x.0).collect();
        assert_eq!(keys, vec!["master", "pochette", "infos", "controle", "distrib", "bandcamp", "textes", "promo"]);
        // Avec un distributeur, tout est prêt plus tôt : le master 5 semaines avant, l'envoi 3 semaines avant.
        s.date = "2026-12-31".into();
        s.upc = " 0123 4567 8901 2 ".into();
        s.explicit = true;
        s.steps = vec!["master".into(), "soundcloud".into()];
        save(&conn, &s).unwrap();
        let back = load(&conn, id).unwrap();
        assert_eq!((back.upc.as_str(), back.explicit, back.steps.clone()), ("0123 4567 8901 2", true, vec!["master".to_string()]));
        assert_eq!(upc_clean(&back.upc).as_deref(), Some("0123456789012"));
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let p = planning(&conn, today).into_iter().find(|p| p.id == id).unwrap();
        let due = |k: &str| p.steps.iter().find(|x| x.key == k).unwrap().due.format("%d/%m").to_string();
        assert_eq!((due("master"), due("distrib"), due("bandcamp"), p.steps.len()), ("26/11".into(), "10/12".into(), "30/12".into(), 8));
        // Le contrôle suit les plateformes : un MP3 est refusé par un distributeur, un master fort sera baissé.
        let q = qc_on(&conn, 1, &back.platforms);
        assert_eq!(q[0].1, "problème");
        assert!(q[1].2.contains("environ 6 dB"), "{}", q[1].2);
        assert_eq!(qc(&conn, 1)[0].1, "attention", "SoundCloud seul : un MP3 passe, mais sera recompressé");
        assert_eq!(qc_on(&conn, 2, &back.platforms)[1].1, "ok");
        // ISRC
        assert_eq!(isrc_clean("fr-abc-26-00001").as_deref(), Some("FRABC2600001"));
        assert!(isrc_clean("FR-ABC-26-0001").is_none() && isrc_clean("12ABC2600001").is_none());
        assert_eq!(set_isrc(&conn, 2, "FR-ABC-26-00001").unwrap(), "FRABC2600001");
        assert!(set_isrc(&conn, 2, "n'importe quoi").is_err());
        assert!(export_text(&conn, &back).contains("ISRC FRABC2600001"));
        // L'agenda : la sortie, ses étapes restantes, la commande à livrer.
        let ics = agenda_ics(&conn, today);
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n") && ics.ends_with("END:VCALENDAR\r\n"));
        assert!(ics.contains(&format!("UID:sortie-{id}@otoko")) && ics.contains("DTSTART;VALUE=DATE:20261231"));
        assert!(ics.contains(&format!("UID:sortie-{id}-distrib@otoko")) && !ics.contains(&format!("UID:sortie-{id}-master@otoko")));
        assert!(ics.contains("SUMMARY:À livrer : Commande pour Léa"));
        assert!(ics.split("\r\n").all(|l| l.len() <= 75), "lignes repliées");
    }

    #[test]
    fn the_delivery_folder_holds_masters_cover_and_infos() {
        let dir = std::env::temp_dir().join(format!("otoko-envoi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = |n: &str, body: &str| {
            let p = dir.join(n);
            std::fs::write(&p, body).unwrap();
            p.to_string_lossy().replace('\'', "''")
        };
        let (a, a_master, b, cover) = (file("a.wav", "brouillon"), file("a master.wav", "master"), file("b.flac", "b"), file("cover.PNG", "png"));
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO settings (key, value) VALUES ('artist_alias', 'Otoko');
             INSERT INTO tracks (id, path, filename, title, artist, genre, duration_s, isrc) VALUES
                (1, '{a}', 'a.wav', 'Nuit; blanche', '', 'House', 185.4, 'FRABC2600001'),
                (2, '{b}', 'b.flac', 'b.flac', 'Léa', '', NULL, ''),
                (3, 'D:\\perdu.wav', 'perdu.wav', 'Perdu', '', '', NULL, '');
             INSERT INTO tracks (id, path, filename, title, parent_id, version_label) VALUES (4, '{a_master}', 'a master.wav', 'Nuit (master)', 1, 'master');"
        ))
        .unwrap();
        let s = Sheet { title: "Nuit".into(), tracks: vec![1, 2, 3], cover_path: cover, language: "Français".into(), ..Default::default() };
        let d = delivery(&conn, &s, &dir.join("Nuit (envoi)")).unwrap();
        assert_eq!((d.copied, d.missing.clone()), (2, vec!["Perdu".to_string()]));
        // Le master part à la place du brouillon, dans l'ordre de la sortie ; le titre « b.flac » perd son extension.
        assert_eq!(std::fs::read_to_string(d.dir.join("01 - Nuit; blanche.wav")).unwrap(), "master");
        assert!(d.dir.join("02 - b.flac").is_file() && d.dir.join("Pochette.png").is_file() && d.dir.join("Fiche.txt").is_file());
        let csv = std::fs::read_to_string(d.dir.join("Infos.csv")).unwrap();
        assert!(csv.starts_with("\u{feff}Piste;Titre;Artiste"), "{csv}");
        assert!(csv.contains("1;\"Nuit; blanche\";Otoko;House;3:05;FRABC2600001;Français;non;\"01 - Nuit; blanche.wav\"\r\n"), "{csv}");
        assert!(csv.contains("2;b;Léa;"), "{csv}");
        // Un deuxième envoi n'écrase pas le premier.
        let again = delivery(&conn, &s, &dir.join("Nuit (envoi)")).unwrap();
        assert!(again.dir.ends_with("Nuit (envoi) 2"), "{}", again.dir.display());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn calendar_and_colors() {
        assert_eq!(long_date("2026-07-08"), "mercredi 8 juillet 2026");
        assert_eq!(long_date("2026-11-01"), "dimanche 1er novembre 2026");
        let g = month_grid(2026, 11);
        assert_eq!(g.len(), 42);
        // Le 1er novembre 2026 est un dimanche : la grille commence le lundi 26 octobre.
        assert_eq!((g[0].1, g[0].2, g[6].1, g[6].2), (26, false, 1, true));
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(month_label(2026, 11), "novembre 2026");
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(initials("Nuit blanche à Paris"), "NB");
        assert_eq!(initials("…"), "?");
    }

    #[test]
    fn names_and_types() {
        assert_eq!(name_key("14.Kreyòl Horizon.wav"), "kreyòl horizon");
        assert_eq!(name_key("LAB 7 - 01 - Slow Burn"), "slow burn");
        assert_eq!(guess_type(2, ""), "single");
        assert_eq!(guess_type(5, ""), "ep");
        assert_eq!(guess_type(9, ""), "album");
        assert_eq!(guess_type(9, "mixtape"), "mixtape");
    }

    #[test]
    fn sync_link_and_create() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO releases (id, title, rtype) VALUES (1, 'Jubilee', 'ep');
             INSERT INTO tracks (id, path, filename, title, album, missing) VALUES
               (1, 'D:\\Jubilee\\01 Slow Burn.wav', '01 Slow Burn.wav', 'Slow Burn', 'Jubilee', 0),
               (2, 'D:\\Jubilee\\02 Rise.wav', '02 Rise.wav', 'Rise', ' jubilee ', 0),
               (3, 'D:\\Jubilee\\Slow Burn (copie).wav', 'Slow Burn.wav', 'Slow Burn', 'Jubilee', 0),
               (4, 'D:\\Demos\\Ocean.wav', 'Ocean.wav', 'Ocean', 'Jubilee', 0),
               (5, 'D:\\Nuit\\a.wav', 'a.wav', 'A', 'Nuit', 0),
               (6, 'D:\\Nuit\\b.wav', 'b.wav', 'B', 'Nuit', 0),
               (7, 'D:\\Nuit\\c.wav', 'c.wav', 'C', 'Nuit', 0);
             INSERT INTO release_tracks (release_id, track_id, position) VALUES (1, 1, 1);",
        )
        .unwrap();
        let plan = sync_plan(&conn);
        assert_eq!(plan.len(), 2);
        let link = plan.iter().find(|a| a.kind == "link").unwrap();
        // La copie (même nom) et la démo rangée ailleurs restent à l'écart.
        assert_eq!((link.track_ids.clone(), link.copies, link.elsewhere), (vec![2], 1, 1));
        let create = plan.iter().find(|a| a.kind == "create").unwrap();
        assert_eq!((create.title.as_str(), create.rtype.as_str(), create.track_ids.len()), ("Nuit", "ep", 3));
        assert_eq!(describe(create), "« Nuit » : créer une fiche (ep) avec 3 morceau(x)");
        assert_eq!(sync_apply(&mut conn, &plan).unwrap(), 4);
        let pos: i64 = conn.query_row("SELECT position FROM release_tracks WHERE track_id = 2", [], |r| r.get(0)).unwrap();
        assert_eq!(pos, 2);
        assert!(sync_plan(&conn).is_empty() || sync_plan(&conn).iter().all(|a| a.track_ids.is_empty()));
    }

    #[test]
    fn plan_and_alerts() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        let t = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        conn.execute("INSERT INTO releases (title, rtype, release_date, steps) VALUES ('Nuit', 'single', '2026-11-13', '{\"master\": true}')", []).unwrap();
        let p = planning(&conn, t);
        assert_eq!(p[0].days, 10);
        assert_eq!(p[0].remaining, 6);
        // Pochette due le 30/10 (J-14) : en retard ; infos dues le 03/11 : pas encore.
        assert!(p[0].steps.iter().find(|s| s.key == "pochette").unwrap().late);
        assert!(!p[0].steps.iter().find(|s| s.key == "infos").unwrap().late);
        let a = alerts(&conn, 30, t);
        assert_eq!(a[0].1, "Nuit sort dans 10 jour(s) : en retard sur pochette");
        conn.execute("UPDATE releases SET steps = '{\"master\": true, \"pochette\": true}'", []).unwrap();
        assert_eq!(alerts(&conn, 30, t)[0].1, "Nuit sort dans 10 jour(s) : prochaine étape « infos » avant le 03/11");
    }
}
