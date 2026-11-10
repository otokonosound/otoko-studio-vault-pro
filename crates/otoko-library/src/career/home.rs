//! L'accueil de la Carrière et les statistiques en graphiques (comme `composer_vault/ui/dashboard.py`
//! et `stats_dialog.py`) : chiffres clés, morceaux à finir, bandeau des sorties, production des
//! douze derniers mois, nouveautés, choses à faire, échéances et paiements.

use rusqlite::Connection;

use super::library::{TO_PUBLISH_SQL, recent_where};
use super::{STATUSES, board, nature, release_label, status_label, steps_count};

const MOIS: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];

fn scalar(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get::<_, Option<i64>>(0)).ok().flatten().unwrap_or(0)
}

fn real(conn: &Connection, sql: &str) -> f64 {
    conn.query_row(sql, [], |r| r.get::<_, Option<f64>>(0)).ok().flatten().unwrap_or(0.0)
}

fn pairs(conn: &Connection, sql: &str) -> Vec<(String, i64)> {
    conn.prepare(sql).and_then(|mut s| s.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get(1)?)))?.collect()).unwrap_or_default()
}

/// La semaine en chiffres, pour l'accueil d'OTOKO : morceaux ajoutés, morceaux terminés ou
/// publiés, sorties prévues dans les trente jours, morceaux encore à finir.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Week {
    pub added: i64,
    pub finished: i64,
    pub soon: i64,
    pub unfinished: i64,
}

pub fn week(conn: &Connection) -> Week {
    let recent = recent_where(7);
    Week {
        added: scalar(conn, &format!("SELECT COUNT(*) FROM tracks WHERE {recent}")),
        finished: scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND status IN ('done', 'released') AND modified_at > datetime('now', 'localtime', '-7 days')"),
        soon: scalar(conn, "SELECT COUNT(*) FROM releases WHERE release_date <> '' AND release_date >= date('now', 'localtime') AND release_date <= date('now', 'localtime', '+30 days')"),
        unfinished: scalar(conn, &format!("SELECT COUNT(*) FROM tracks WHERE {UNFINISHED}")),
    }
}

/// Une sortie du bandeau : pochette, compte à rebours, avancement.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tile {
    pub id: i64,
    pub title: String,
    /// Le type (« single »…) et son libellé.
    pub rtype: String,
    pub kind: String,
    pub when: String,
    pub soon: bool,
    pub done: usize,
    pub total: usize,
    pub cover: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Home {
    /// Le nom d'artiste (réglage `artist_alias`), pour « Bonjour … ».
    pub alias: String,
    pub total: i64,
    pub unfinished: i64,
    pub released: i64,
    pub recent: i64,
    /// (morceau, titre, statut).
    pub priority: Vec<(i64, String, String)>,
    /// Morceaux sortis par mois, les douze derniers.
    pub months: Vec<(String, i64)>,
    pub news: Vec<(i64, String)>,
    /// (texte, action) : « backup », « to_publish », « analyze », « complete », « nature », « sync »,
    /// « brand », « missing ».
    pub todo: Vec<(String, String)>,
    pub releases: Vec<Tile>,
    /// (sortie, texte) : étapes en retard ou dues des sorties à venir.
    pub release_alerts: Vec<(i64, String)>,
    pub deadlines: Vec<String>,
    pub unpaid: Vec<String>,
    pub unpaid_total: f64,
}

/// Les idées jamais triées (ni gardées ni rejetées), pour le tri des idées.
pub const UNTRIAGED: &str = "missing = 0 AND COALESCE(archived, 0) = 0 AND IFNULL(status, 'idea') = 'idea' AND IFNULL(pick, 0) = 0";

/// À partir de combien d'idées non triées l'accueil propose le tri.
pub const TRIAGE_MIN: i64 = 5;

pub const UNFINISHED: &str = "missing = 0 AND COALESCE(archived, 0) = 0 AND status IN ('idea','draft','wip','to_mix','to_master')";

fn clean(t: String) -> String {
    match t.rsplit_once('.') {
        Some((stem, ext)) if super::scan::AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()) => stem.to_string(),
        _ => t,
    }
}

fn setting(conn: &Connection, key: &str) -> String {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get::<_, String>(0)).unwrap_or_default()
}

/// L'univers de marque est vide (ni nom, ni lien, ni ville).
pub fn brand_is_empty(conn: &Connection) -> bool {
    let name = setting(conn, "pub_artist_name");
    let name = if name.is_empty() { setting(conn, "artist_alias") } else { name };
    name.is_empty() && setting(conn, "pub_sc_url").is_empty() && setting(conn, "pub_city").is_empty()
}

/// La date du jour (heure locale), « 03/11/2026 ».
pub fn today_text() -> String {
    chrono::Local::now().format("%d/%m/%Y").to_string()
}

/// Le dossier Google Drive du PC (« Mon Drive »), s'il existe : là où proposer une seconde copie.
pub fn suggested_drive_dir() -> Option<std::path::PathBuf> {
    for letter in 'D'..='Z' {
        for name in ["Mon Drive", "My Drive"] {
            let p = std::path::PathBuf::from(format!("{letter}:\\{name}"));
            if p.is_dir() {
                return Some(p.join("OTOKO - sauvegardes"));
            }
        }
    }
    None
}

pub fn home(conn: &Connection) -> Home {
    let mut h = Home { alias: setting(conn, "artist_alias"), total: scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0"), ..Default::default() };
    if h.total == 0 {
        return h;
    }
    let recent = recent_where(14);
    h.unfinished = scalar(conn, &format!("SELECT COUNT(*) FROM tracks WHERE {UNFINISHED}"));
    h.released = scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND status = 'released'");
    h.recent = scalar(conn, &format!("SELECT COUNT(*) FROM tracks WHERE {recent}"));
    h.priority = conn
        .prepare(&format!(
            "SELECT id, COALESCE(NULLIF(title, ''), filename), status FROM tracks WHERE {UNFINISHED}
             ORDER BY CASE status WHEN 'to_master' THEN 0 WHEN 'to_mix' THEN 1 WHEN 'wip' THEN 2 WHEN 'draft' THEN 3 ELSE 4 END, modified_at DESC LIMIT 8"
        ))
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, clean(r.get(1)?), status_label(&r.get::<_, String>(2)?).to_string())))?.collect())
        .unwrap_or_default();
    // Production : date de publication du morceau, sinon celle de sa sortie déjà parue.
    let today = chrono::Local::now().date_naive();
    let mut keys = Vec::new();
    let (mut y, mut m) = (chrono::Datelike::year(&today), chrono::Datelike::month(&today) as i32);
    for _ in 0..12 {
        keys.push(format!("{y:04}-{m:02}"));
        m -= 1;
        if m == 0 {
            y -= 1;
            m = 12;
        }
    }
    keys.reverse();
    let published: Vec<String> = conn
        .prepare(
            "SELECT substr(COALESCE(NULLIF(t.published_at, ''), (SELECT MIN(r.release_date) FROM release_tracks rt JOIN releases r ON r.id = rt.release_id
                 WHERE rt.track_id = t.id AND r.release_date <> '' AND r.release_date <= date('now', 'localtime'))), 1, 7) FROM tracks t WHERE t.missing = 0",
        )
        .and_then(|mut s| Ok(s.query_map([], |r| r.get::<_, Option<String>>(0))?.flatten().flatten().collect()))
        .unwrap_or_default();
    h.months = keys
        .iter()
        .map(|k| {
            let n = published.iter().filter(|p| p.as_str() == k).count() as i64;
            (MOIS[k[5..7].parse::<usize>().unwrap_or(1) - 1].to_string(), n)
        })
        .collect();
    h.news = conn
        .prepare(&format!("SELECT id, COALESCE(NULLIF(title, ''), filename) FROM tracks WHERE {recent} ORDER BY COALESCE(file_date, added_at) DESC LIMIT 6"))
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, clean(r.get(1)?))))?.collect())
        .unwrap_or_default();
    // À faire.
    let mut todo = Vec::new();
    if setting(conn, "backup_extra_dir").is_empty() && suggested_drive_dir().is_some() {
        todo.push(("Ta Carrière n'est sauvegardée que sur ce PC — copier aussi sur Google Drive".to_string(), "backup".to_string()));
    }
    let unpub = scalar(conn, &format!("SELECT COUNT(*) FROM tracks WHERE {TO_PUBLISH_SQL}"));
    if unpub > 0 {
        todo.push((format!("{unpub} morceau(x) à publier — les voir"), "to_publish".into()));
    }
    let n_an = scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND analyzed_at IS NULL");
    if n_an > 0 {
        todo.push((format!("{n_an} morceau(x) jamais analysé(s) — lancer l'analyse"), "analyze".into()));
    }
    let n_genre = scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND (genre IS NULL OR genre = '')");
    if n_genre > 0 {
        let n_art = scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND (artist IS NULL OR artist = '')");
        todo.push((format!("{n_genre} morceau(x) sans genre{} — compléter", if n_art > 0 { format!(" et {n_art} sans artiste") } else { String::new() }), "complete".into()));
    }
    let n_nat = conn
        .prepare("SELECT IFNULL(filename, ''), IFNULL(title, '') FROM tracks WHERE missing = 0 AND COALESCE(nature, 'original') = 'original'")
        .and_then(|mut s| Ok(s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.flatten().filter(|(f, t)| nature::detect(&format!("{f} {t}")) != "original").count()))
        .unwrap_or(0);
    if n_nat > 0 {
        todo.push((format!("{n_nat} remix, edit ou mashup repéré(s) — les classer"), "nature".into()));
    }
    let n_sync: usize = super::releases::sync_plan(conn).iter().map(|a| a.track_ids.len()).sum();
    if n_sync > 0 {
        todo.push((format!("{n_sync} morceau(x) pas rattaché(s) à leur sortie — les ranger"), "sync".into()));
    }
    if brand_is_empty(conn) {
        todo.push(("Remplir ton univers de marque (pour les textes)".into(), "brand".into()));
    }
    let n_miss = scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 1");
    if n_miss > 0 {
        todo.push((format!("{n_miss} fichier(s) introuvable(s) — les voir"), "missing".into()));
    }
    h.todo = todo;
    h.release_alerts = super::releases::planning_alerts(conn).into_iter().take(2).collect();
    // Bandeau des sorties : à venir d'abord, puis sans date, puis passées.
    h.releases = conn
        .prepare(
            "SELECT id, title, IFNULL(rtype, ''), IFNULL(release_date, ''), IFNULL(steps, '{}'), IFNULL(cover_path, ''), IFNULL(extra, '{}') FROM releases ORDER BY
             CASE WHEN release_date >= date('now', 'localtime') THEN 0 WHEN release_date = '' THEN 1 ELSE 2 END,
             CASE WHEN release_date >= date('now', 'localtime') THEN release_date END ASC, release_date DESC LIMIT 5",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                let date: String = r.get(3)?;
                let (done, total) = steps_count(&r.get::<_, String>(4)?, &r.get::<_, String>(6)?);
                let (when, soon) = match board::days_until(&date) {
                    Some(d) if d > 0 => (format!("J-{d}"), true),
                    Some(0) => ("Aujourd'hui".into(), true),
                    Some(_) => (format!("Sortie le {}/{}/{}", &date[8..10], &date[5..7], &date[0..4]), false),
                    None => ("Date à fixer".into(), false),
                };
                let rtype: String = r.get(2)?;
                Ok(Tile { id: r.get(0)?, title: r.get(1)?, kind: release_label(&rtype), rtype, when, soon, done, total, cover: r.get(5)? })
            })?
            .collect()
        })
        .unwrap_or_default();
    // Échéances (remix, prods) et paiements en attente.
    let metas: Vec<(String, String, String)> = conn
        .prepare("SELECT COALESCE(NULLIF(title, ''), filename), nature, IFNULL(nature_meta, '{}') FROM tracks WHERE missing = 0 AND nature IN ('prod', 'remix', 'edit')")
        .and_then(|mut s| s.query_map([], |r| Ok((clean(r.get(0)?), r.get(1)?, r.get(2)?)))?.collect())
        .unwrap_or_default();
    let mut due = Vec::new();
    for (title, nat, raw) in &metas {
        let m = nature::load_meta(raw);
        let done = m["paid"].as_bool().unwrap_or(false) || ["delivered", "paid"].contains(&m["order_status"].as_str().unwrap_or(""));
        if let Some(dl) = m["deadline"].as_str().filter(|d| !d.is_empty() && !done) {
            let who = m["client_name"].as_str().or(m["commissioned_by"].as_str()).unwrap_or("");
            let mut lab = format!("{title} — {}", super::nature_short(nat));
            if !who.is_empty() {
                lab.push_str(&format!(" pour {who}"));
            }
            if let Some((a, _)) = board::deadline_alert(dl) {
                lab.push_str(&format!(" — {a}"));
            }
            due.push((dl.to_string(), lab));
        }
        if nat == "prod" && !m["paid"].as_bool().unwrap_or(false) && ["validated", "delivered"].contains(&m["order_status"].as_str().unwrap_or("")) {
            let fee = m["fee"].as_f64().or_else(|| m["fee"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
            h.unpaid_total += fee;
            let status = nature::ORDER_STATUSES.iter().find(|s| m["order_status"] == s.0).map(|s| s.1).unwrap_or("");
            h.unpaid.push(format!(
                "{} — {title}{} ({status})",
                m["client_name"].as_str().filter(|c| !c.is_empty()).unwrap_or("Client"),
                if fee > 0.0 { format!(" — {fee:.0} €") } else { String::new() }
            ));
        }
    }
    due.sort();
    h.deadlines = due.into_iter().take(8).map(|d| d.1).collect();
    h
}

/// Une action proposée par l'accueil.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Action {
    pub title: String,
    pub why: String,
    /// Le texte du bouton.
    pub label: String,
    /// Ce que fait le bouton (voir `accueil::act`).
    pub action: String,
    /// 0 calme, 1 à surveiller, 2 en retard.
    pub tone: i32,
    pub score: i32,
}

fn action(score: i32, tone: i32, title: String, why: impl Into<String>, label: &str, what: impl Into<String>) -> Action {
    Action { title, why: why.into(), label: label.into(), action: what.into(), tone, score }
}

fn plural(n: i64) -> &'static str {
    if n > 1 { "x" } else { "" }
}

/// « Aujourd'hui » : ce qu'il y a de plus utile à faire maintenant, le plus urgent d'abord. Les chiffres, eux, restent
/// dans Statistiques. Une sortie qui approche passe avant tout ; les idées sont un stock, pas une tâche.
pub fn today_actions(conn: &Connection, h: &Home) -> Vec<Action> {
    let today = chrono::Local::now().date_naive();
    let mut out = Vec::new();
    for r in super::releases::planning(conn, today).into_iter().filter(|r| r.days >= 0 && r.remaining > 0) {
        let Some(s) = r.steps.iter().filter(|s| !s.done).min_by_key(|s| s.due) else { continue };
        let late = s.late;
        let score = if late {
            100
        } else if r.days <= 7 {
            90
        } else if r.days <= 30 {
            70
        } else {
            40
        };
        let when = if late { format!("en retard depuis le {}", s.due.format("%d/%m")) } else { format!("avant le {}", s.due.format("%d/%m")) };
        out.push(action(score, if late { 2 } else { i32::from(r.days <= 7) }, format!("{} : {}", r.title, super::releases::step_label(s.key).to_lowercase()), format!("{when} · sortie dans {} jour{}", r.days, if r.days > 1 { "s" } else { "" }), "Ouvrir la sortie", format!("release:{}", r.id)));
    }
    let arrived = super::arrivee::todo(conn).len() as i64;
    if arrived > 0 {
        out.push(action(80, 1, format!("{arrived} export{} à ranger", if arrived > 1 { "s" } else { "" }), "Plusieurs morceaux portent ce nom : dis lequel c'est.", "Ranger", "arrivals"));
    }
    if let Some(d) = h.deadlines.first() {
        out.push(action(75, 1, d.clone(), "Commande en cours", "Voir", "production-clients"));
    }
    if let Some((n, _)) = super::copies::last(conn).filter(|l| l.0 > 0) {
        out.push(action(65, 1, format!("{n} morceau{} en une seule copie", plural(n)), "Un disque qui lâche et ils sont perdus.", "Les voir", "copies"));
    }
    let missing = scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 1");
    if missing > 0 {
        out.push(action(60, 1, format!("{missing} fichier{} introuvable{}", if missing > 1 { "s" } else { "" }, if missing > 1 { "s" } else { "" }), "Déplacés, renommés, ou disque débranché.", "Les voir", "missing"));
    }
    let to_master = scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND COALESCE(archived, 0) = 0 AND status = 'to_master'");
    if to_master > 0 {
        out.push(action(55, 0, format!("{to_master} morceau{} à masteriser", plural(to_master)), "Ils attendent leur master.", "Voir", "to_master"));
    }
    if !h.unpaid.is_empty() {
        out.push(action(50, 0, format!("{:.0} € à encaisser", h.unpaid_total), h.unpaid[0].clone(), "Voir", "production-clients"));
    }
    let no_genre = scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND COALESCE(archived, 0) = 0 AND (genre IS NULL OR genre = '')");
    if no_genre > 0 {
        out.push(action(45, 0, format!("{no_genre} morceau{} sans genre", plural(no_genre)), "Des lots sont prêts : quelques clics suffisent.", "Compléter", "batches"));
    }
    let links = super::liens::to_link(conn).len() as i64;
    if links > 0 {
        out.push(action(40, 0, format!("{links} sortie{} à relier à leurs titres", if links > 1 { "s" } else { "" }), "Le lien de la sortie retrouve celui de chaque titre.", "Relier", "links"));
    }
    let unpub = scalar(conn, &format!("SELECT COUNT(*) FROM tracks WHERE {TO_PUBLISH_SQL}"));
    if unpub > 0 {
        out.push(action(35, 0, format!("{unpub} morceau{} à publier", plural(unpub)), "Marqués « à publier », pas encore en ligne.", "Voir", "to_publish"));
    }
    if super::copies::roots(conn).is_empty() && suggested_drive_dir().is_some() {
        out.push(action(32, 0, "Indique où sont tes copies".to_string(), "L'application te dira ce qui n'existe qu'à un seul endroit.", "Réglages", "copies-setup"));
    }
    let idle = scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND analyzed_at IS NULL");
    if idle > 0 {
        out.push(action(30, 0, format!("{idle} morceau{} pas analysé{}", plural(idle), if idle > 1 { "s" } else { "" }), "Tempo, tonalité, sonie, crêtes.", "Analyser", "analyze"));
    }
    let untriaged = scalar(conn, &format!("SELECT COUNT(*) FROM tracks WHERE {UNTRIAGED}"));
    if untriaged >= TRIAGE_MIN {
        out.push(action(28, 0, format!("{untriaged} idées jamais triées"), "Le meilleur passage de chacune, puis P garde, X rejette, N passe.", "Trier", "triage"));
    }
    if brand_is_empty(conn) {
        out.push(action(25, 0, "Remplis ton univers de marque".to_string(), "Nom, ville, signature : il sert aux textes.", "Remplir", "brand"));
    }
    if setting(conn, "backup_extra_dir").is_empty() && suggested_drive_dir().is_some() {
        out.push(action(20, 0, "Ta base n'est sauvegardée que sur ce PC".to_string(), "Une copie sur Google Drive la protège.", "Copier", "backup"));
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.score));
    out
}

/// Les statistiques en graphiques.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Charts {
    pub total: i64,
    pub hours: f64,
    pub keep: i64,
    pub analyzed: i64,
    pub avg_bpm: Option<f64>,
    pub added_by_month: Vec<(String, i64)>,
    pub release_types: Vec<(String, i64)>,
    pub formats: Vec<(String, i64)>,
    pub tempo: Vec<(String, i64)>,
    pub keys: Vec<(String, i64)>,
    pub genres: Vec<(String, i64)>,
    pub moods: Vec<(String, i64)>,
    pub by_status: Vec<(String, i64)>,
    pub picks: Vec<(String, i64)>,
    pub most_played: Vec<(String, i64)>,
    /// Morceaux passés « terminé » ou « publié », par mois (douze derniers), et sorties datées par mois.
    pub done_by_month: Vec<(String, i64)>,
    pub releases_by_month: Vec<(String, i64)>,
}

/// « 2026-11 » → « nov. 26 ».
fn month_label(m: &str) -> String {
    if m.len() == 7 { format!("{} {}", MOIS[m[5..7].parse::<usize>().unwrap_or(1).clamp(1, 12) - 1], &m[2..4]) } else { m.to_string() }
}

pub fn charts(conn: &Connection) -> Charts {
    let mut c = Charts {
        total: scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0"),
        hours: real(conn, "SELECT SUM(duration_s) FROM tracks WHERE missing = 0") / 3600.0,
        keep: scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND pick = 1"),
        analyzed: scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND analyzed_at IS NOT NULL"),
        avg_bpm: conn.query_row("SELECT AVG(bpm) FROM tracks WHERE missing = 0 AND bpm IS NOT NULL", [], |r| r.get(0)).ok().flatten(),
        ..Default::default()
    };
    let mut months = pairs(conn, "SELECT substr(COALESCE(file_date, added_at), 1, 7) m, COUNT(*) c FROM tracks WHERE missing = 0 GROUP BY m ORDER BY m DESC LIMIT 12");
    months.reverse();
    c.added_by_month = months
        .into_iter()
        .map(|(m, n)| (if m.len() == 7 { format!("{} {}", MOIS[m[5..7].parse::<usize>().unwrap_or(1) - 1], &m[2..4]) } else { m }, n))
        .collect();
    let mut done = pairs(conn, "SELECT substr(modified_at, 1, 7) m, COUNT(*) c FROM tracks WHERE missing = 0 AND status IN ('done', 'released') AND modified_at IS NOT NULL GROUP BY m ORDER BY m DESC LIMIT 12");
    done.reverse();
    c.done_by_month = done.into_iter().map(|(m, n)| (month_label(&m), n)).collect();
    let mut rel = pairs(conn, "SELECT substr(release_date, 1, 7) m, COUNT(*) c FROM releases WHERE release_date <> '' GROUP BY m ORDER BY m DESC LIMIT 12");
    rel.reverse();
    c.releases_by_month = rel.into_iter().map(|(m, n)| (month_label(&m), n)).collect();
    c.release_types = pairs(conn, "SELECT COALESCE(NULLIF(release_type, ''), 'aucun') t, COUNT(*) c FROM tracks WHERE missing = 0 GROUP BY t ORDER BY c DESC")
        .into_iter()
        .map(|(t, n)| (if super::RELEASE_TYPES.iter().any(|r| r.0 == t) { release_label(&t) } else { "Aucun".to_string() }, n))
        .collect();
    c.formats = pairs(conn, "SELECT UPPER(file_format) f, COUNT(*) c FROM tracks WHERE missing = 0 GROUP BY f ORDER BY c DESC")
        .into_iter()
        .map(|(f, n)| (if f.is_empty() { "?".into() } else { f }, n))
        .collect();
    c.tempo = [(0, 90, "<90"), (90, 110, "90-110"), (110, 120, "110-120"), (120, 130, "120-130"), (130, 140, "130-140"), (140, 160, "140-160"), (160, 999, ">160")]
        .iter()
        .map(|(lo, hi, l)| (l.to_string(), scalar(conn, &format!("SELECT COUNT(*) FROM tracks WHERE missing = 0 AND bpm >= {lo} AND bpm < {hi}"))))
        .collect();
    c.keys = pairs(conn, "SELECT music_key k, COUNT(*) c FROM tracks WHERE missing = 0 AND music_key IS NOT NULL GROUP BY k ORDER BY c DESC LIMIT 10");
    c.genres = pairs(conn, "SELECT genre g, COUNT(*) c FROM tracks WHERE missing = 0 AND genre IS NOT NULL AND genre <> '' GROUP BY g ORDER BY c DESC LIMIT 8");
    let mut moods: Vec<(String, i64)> = Vec::new();
    for (m, _) in pairs(conn, "SELECT mood_auto, 1 FROM tracks WHERE missing = 0 AND mood_auto IS NOT NULL") {
        for a in m.split(',').map(str::trim).filter(|a| !a.is_empty()) {
            match moods.iter_mut().find(|x| x.0 == a) {
                Some(x) => x.1 += 1,
                None => moods.push((a.to_string(), 1)),
            }
        }
    }
    moods.sort_by(|a, b| b.1.cmp(&a.1));
    moods.truncate(8);
    c.moods = moods;
    c.by_status = STATUSES.iter().map(|(k, l)| (l.to_string(), scalar(conn, &format!("SELECT COUNT(*) FROM tracks WHERE missing = 0 AND status = '{k}'")))).collect();
    c.picks = vec![
        ("Gardés".into(), c.keep),
        ("Rejetés".into(), scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND pick = -1")),
        ("Neutres".into(), scalar(conn, "SELECT COUNT(*) FROM tracks WHERE missing = 0 AND (pick = 0 OR pick IS NULL)")),
    ];
    c.most_played = pairs(conn, "SELECT COALESCE(NULLIF(title, ''), filename), play_count FROM tracks WHERE missing = 0 AND play_count > 0 ORDER BY play_count DESC LIMIT 8")
        .into_iter()
        .map(|(t, n)| (clean(t).chars().take(24).collect(), n))
        .collect();
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_offers_triage_once_enough_ideas_wait() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        let add = |n: i64, status: &str, pick: i64| {
            conn.execute("INSERT INTO tracks (path, filename, status, pick, analyzed_at) VALUES (?1, ?1, ?2, ?3, 'x')", rusqlite::params![format!("{status}{n}.wav"), status, pick]).unwrap();
        };
        for n in 0..4 {
            add(n, "idea", 0);
        }
        // Gardées, rejetées ou en cours : déjà triées.
        add(10, "idea", 1);
        add(11, "idea", -1);
        add(12, "wip", 0);
        let triage = |c: &Connection| today_actions(c, &Home::default()).into_iter().find(|a| a.action == "triage");
        assert!(triage(&conn).is_none());
        add(4, "idea", 0);
        assert_eq!(triage(&conn).unwrap().title, "5 idées jamais triées");
    }
}
