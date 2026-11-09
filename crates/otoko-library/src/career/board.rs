//! Production (comme `composer_vault/ui/production_page.py`) : les morceaux en colonnes, de l'idée à
//! la publication, ou les commandes clients (prods) par étape. Chaque carte signale ce qui bloque :
//! pas analysé, crête trop haute, saturation, pas de genre, échéance, paiement attendu.

use rusqlite::{Connection, params};
use serde_json::Value as Json;

use super::nature::{self, ORDER_STATUSES};
use super::{STATUSES, nature_short};

/// Colonnes : (clé, titre, statuts regroupés, statut donné à une carte déposée).
pub const COLUMNS: [(&str, &str, &[&str], &str); 5] = [
    ("idea", "Idées", &["idea"], "idea"),
    ("wip", "En cours", &["draft", "wip"], "wip"),
    ("mix", "Mix et master", &["to_mix", "to_master"], "to_mix"),
    ("done", "Terminés", &["done"], "done"),
    ("released", "Publiés", &["released"], "released"),
];

pub const LIMIT: usize = 150;

/// Une carte.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Card {
    pub id: i64,
    pub title: String,
    pub sub: String,
    /// (texte, couleur : « danger », « warning », « muted », « accent »).
    pub chips: Vec<(String, &'static str)>,
    pub keep: bool,
    pub to_publish: bool,
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Column {
    pub key: String,
    pub title: String,
    pub count: usize,
    pub cards: Vec<Card>,
}

/// Jours entre aujourd'hui (heure locale) et une date « AAAA-MM-JJ ».
pub fn days_until(date: &str) -> Option<i64> {
    let d = chrono::NaiveDate::parse_from_str(date.get(..10)?, "%Y-%m-%d").ok()?;
    Some((d - chrono::Local::now().date_naive()).num_days())
}

/// (texte, couleur) pour une échéance proche ou dépassée.
pub fn deadline_alert(deadline: &str) -> Option<(String, &'static str)> {
    let days = days_until(deadline)?;
    Some(match days {
        d if d < 0 => (format!("en retard de {} j", -d), "danger"),
        0 => ("pour aujourd'hui".into(), "danger"),
        d if d <= 7 => (format!("dans {d} j"), "warning"),
        _ => (format!("le {}/{}", &deadline[8..10], &deadline[5..7]), "muted"),
    })
}

struct R {
    id: i64,
    title: String,
    path: String,
    album: String,
    genre: String,
    bpm: Option<f64>,
    mood: String,
    nature: String,
    meta: Json,
    analyzed: bool,
    true_peak: Option<f64>,
    clip: Option<f64>,
    pick: i64,
    status: String,
    to_publish: bool,
}

/// Ce qui bloque la sortie du morceau.
fn alerts(r: &R) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    if ["prod", "remix", "edit"].contains(&r.nature.as_str()) {
        let paid = r.meta["paid"].as_bool().unwrap_or(false) || r.meta["order_status"] == "paid";
        if !paid {
            if let Some(dl) = r.meta["deadline"].as_str().and_then(deadline_alert) {
                out.push(dl);
            }
        }
        if r.nature == "prod" {
            if r.meta["order_status"] == "delivered" && !r.meta["paid"].as_bool().unwrap_or(false) {
                out.push(("à encaisser".into(), "warning"));
            }
            return out;
        }
    }
    if !r.analyzed {
        out.push(("pas analysé".into(), "muted"));
    } else {
        if let Some(tp) = r.true_peak.filter(|t| *t > -1.0) {
            out.push((format!("crête {tp:+.1} dB"), "warning"));
        }
        if r.clip.unwrap_or(0.0) > 0.001 {
            out.push(("saturation".into(), "danger"));
        }
    }
    if r.genre.is_empty() {
        out.push(("sans genre".into(), "muted"));
    }
    out
}

fn card(r: &R) -> Card {
    let mut bits = Vec::new();
    if r.nature == "prod" {
        if let Some(c) = r.meta["client_name"].as_str().filter(|c| !c.is_empty()) {
            bits.push(c.to_string());
        }
        if let Some(f) = r.meta["fee"].as_f64().or_else(|| r.meta["fee"].as_str().and_then(|s| s.parse().ok())) {
            bits.push(format!("{f:.0} €"));
        }
    } else if r.nature != "original" {
        bits.push(nature_short(&r.nature));
    }
    if let Some(b) = r.bpm {
        bits.push(format!("{} BPM", b.round() as i64));
    }
    if let Some(m) = r.mood.split(',').next().map(str::trim).filter(|m| !m.is_empty()) {
        bits.push(m.to_string());
    }
    if !r.album.is_empty() {
        bits.push(r.album.clone());
    }
    let mut chips: Vec<(String, &'static str)> = alerts(r).into_iter().take(2).collect();
    if r.to_publish && r.status != "released" {
        chips.insert(0, ("à publier".into(), "accent"));
    }
    chips.truncate(3);
    Card { id: r.id, title: r.title.clone(), sub: bits.join(" · "), chips, keep: r.pick == 1, to_publish: r.to_publish, path: r.path.clone() }
}

/// Le tableau : « mine » (mes morceaux) ou « clients » (les prods, par étape de commande).
pub fn board(conn: &Connection, mode: &str, filter: &str) -> Vec<Column> {
    let base = "SELECT id, COALESCE(NULLIF(title, ''), filename), path, IFNULL(album, ''), IFNULL(genre, ''), bpm, IFNULL(mood_auto, ''),
                       IFNULL(NULLIF(nature, ''), 'original'), IFNULL(nature_meta, '{}'), analyzed_at IS NOT NULL, true_peak, clip_ratio, IFNULL(pick, 0),
                       IFNULL(status, 'idea'), IFNULL(to_publish, 0)
                FROM tracks WHERE missing = 0 AND COALESCE(archived, 0) = 0 AND (role IS NULL OR role NOT IN ('stem', 'version', 'reference'))";
    let sql = if mode == "clients" { format!("{base} AND nature = 'prod' ORDER BY modified_at DESC") } else { format!("{base} AND COALESCE(nature, 'original') <> 'prod' ORDER BY modified_at DESC") };
    let rows: Vec<R> = conn
        .prepare(&sql)
        .and_then(|mut s| {
            s.query_map([], |x| {
                let title: String = x.get(1)?;
                let title = match title.rsplit_once('.') {
                    Some((stem, ext)) if super::scan::AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()) => stem.to_string(),
                    _ => title,
                };
                Ok(R {
                    id: x.get(0)?,
                    title,
                    path: x.get(2)?,
                    album: x.get(3)?,
                    genre: x.get(4)?,
                    bpm: x.get(5)?,
                    mood: x.get(6)?,
                    nature: x.get(7)?,
                    meta: nature::load_meta(&x.get::<_, String>(8)?),
                    analyzed: x.get(9)?,
                    true_peak: x.get(10)?,
                    clip: x.get(11)?,
                    pick: x.get(12)?,
                    status: x.get(13)?,
                    to_publish: x.get::<_, i64>(14)? != 0,
                })
            })?
            .collect()
        })
        .unwrap_or_default();
    let filter = filter.trim().to_lowercase();
    let cols: Vec<(String, String)> =
        if mode == "clients" { ORDER_STATUSES.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect() } else { COLUMNS.iter().map(|c| (c.0.to_string(), c.1.to_string())).collect() };
    let mut out: Vec<Column> = cols.into_iter().map(|(key, title)| Column { key, title, ..Default::default() }).collect();
    let mut buckets: Vec<Vec<&R>> = vec![Vec::new(); out.len()];
    for r in &rows {
        if !filter.is_empty() {
            let hay = format!("{} {} {}", r.title, r.album, r.meta["client_name"].as_str().unwrap_or("")).to_lowercase();
            if !hay.contains(&filter) {
                continue;
            }
        }
        let key = if mode == "clients" {
            if r.meta["paid"].as_bool().unwrap_or(false) { "paid".to_string() } else { r.meta["order_status"].as_str().filter(|s| !s.is_empty()).unwrap_or("brief").to_string() }
        } else {
            COLUMNS.iter().find(|c| c.2.contains(&r.status.as_str())).map(|c| c.0).unwrap_or("idea").to_string()
        };
        if let Some(i) = out.iter().position(|c| c.key == key) {
            buckets[i].push(r);
        }
    }
    for (i, b) in buckets.iter_mut().enumerate() {
        if mode == "clients" {
            b.sort_by_key(|r| r.meta["deadline"].as_str().unwrap_or("9999").to_string());
        }
        out[i].count = b.len();
        out[i].cards = b.iter().take(LIMIT).map(|r| card(r)).collect();
    }
    out
}

/// Une carte déposée dans une colonne.
pub fn drop_card(conn: &Connection, mode: &str, id: i64, column: &str) -> rusqlite::Result<()> {
    if mode == "clients" {
        return set_order(conn, id, column);
    }
    let target = COLUMNS.iter().find(|c| c.0 == column).map(|c| c.3).unwrap_or("idea");
    set_status(conn, id, target)?;
    // Déposer une carte est un choix : les statuts automatiques n'y reviendront pas.
    super::library::lock(conn, id, "status", true)
}

pub fn set_status(conn: &Connection, id: i64, status: &str) -> rusqlite::Result<()> {
    if STATUSES.iter().any(|s| s.0 == status) {
        conn.execute("UPDATE tracks SET status = ?2, modified_at = datetime('now') WHERE id = ?1", params![id, status])?;
    }
    Ok(())
}

/// Étape d'une commande client (gardée dans `nature_meta`).
pub fn set_order(conn: &Connection, id: i64, order: &str) -> rusqlite::Result<()> {
    let raw: String = conn.query_row("SELECT IFNULL(nature_meta, '{}') FROM tracks WHERE id = ?1", [id], |r| r.get(0))?;
    let mut meta = nature::load_meta(&raw);
    meta["order_status"] = Json::String(order.into());
    meta["paid"] = Json::Bool(order == "paid");
    conn.execute("UPDATE tracks SET nature_meta = ?2, modified_at = datetime('now') WHERE id = ?1", params![id, meta.to_string()])?;
    Ok(())
}

/// « À publier » : un choix, indépendant du statut (terminé ne veut pas dire à publier).
pub fn set_to_publish(conn: &Connection, ids: &[i64], on: bool) -> rusqlite::Result<()> {
    for id in ids {
        conn.execute("UPDATE tracks SET to_publish = ?2, modified_at = datetime('now') WHERE id = ?1", params![id, on as i64])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_and_deadlines() {
        let today = chrono::Local::now().date_naive();
        let fmt = |d: chrono::NaiveDate| d.format("%Y-%m-%d").to_string();
        assert_eq!(deadline_alert(&fmt(today)).map(|x| x.1), Some("danger"));
        assert_eq!(deadline_alert(&fmt(today + chrono::Days::new(3))).map(|x| x.0), Some("dans 3 j".into()));
        assert_eq!(deadline_alert(&fmt(today - chrono::Days::new(2))).map(|x| x.0), Some("en retard de 2 j".into()));
        let r = R {
            id: 1,
            title: "Nuit".into(),
            path: String::new(),
            album: String::new(),
            genre: String::new(),
            bpm: Some(124.0),
            mood: "sombre, hypnotique".into(),
            nature: "original".into(),
            meta: serde_json::json!({}),
            analyzed: true,
            true_peak: Some(0.3),
            clip: Some(0.01),
            pick: 1,
            status: "done".into(),
            to_publish: true,
        };
        let c = card(&r);
        assert_eq!(c.sub, "124 BPM · sombre");
        assert_eq!(c.chips.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["à publier", "crête +0.3 dB", "saturation"]);
    }
}
