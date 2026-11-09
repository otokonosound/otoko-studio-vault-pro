//! Le pilotage d'une sortie : le compte à rebours, ce qui reste, ce qui bloque. Tout est lu dans la base : un point
//! n'apparaît que si les données le montrent, et une étape que les données prouvent est proposée à cocher.

use chrono::NaiveDate;
use rusqlite::Connection;

use super::releases::{self, Sheet};
use super::{STEPS, nature};

/// Un point à traiter. `level` : 0 prêt à cocher, 1 à voir, 2 bloquant. `action` : ce que fait le bouton
/// (« cover », « credits », « step:infos », « track:12 »…), vide s'il n'y a rien à lancer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Point {
    pub level: i32,
    pub text: String,
    pub action: String,
    pub label: &'static str,
}

fn point(level: i32, text: impl Into<String>, action: impl Into<String>, label: &'static str) -> Point {
    Point { level, text: text.into(), action: action.into(), label }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pilot {
    /// « J-28 », « Aujourd'hui », « Sortie », « Date à fixer ».
    pub countdown: String,
    /// 0 calme, 1 proche, 2 en retard.
    pub tone: i32,
    pub date: String,
    pub done: usize,
    pub total: usize,
    pub next: String,
    pub points: Vec<Point>,
}

/// Ce que le pilotage doit savoir d'un morceau de la sortie.
struct Row {
    id: i64,
    title: String,
    missing: bool,
    analyzed: bool,
    genre: bool,
    artist: bool,
    private: bool,
    /// La crête du fichier qui partira : le master s'il existe, sinon le morceau.
    peak: Option<f64>,
    master: bool,
}

fn rows(conn: &Connection, ids: &[i64]) -> Vec<Row> {
    ids.iter()
        .filter_map(|id| {
            let row = conn.query_row(
                "SELECT COALESCE(NULLIF(title, ''), filename), missing, analyzed_at IS NOT NULL, IFNULL(genre, '') <> '', IFNULL(artist, '') <> '',
                        IFNULL(NULLIF(nature, ''), 'original'), IFNULL(nature_meta, '{}'), true_peak, IFNULL(version_label, '') = 'master'
                 FROM tracks WHERE id = ?1",
                [id],
                |r| {
                    Ok(Row {
                        id: *id,
                        title: r.get(0)?,
                        missing: r.get::<_, i64>(1)? != 0,
                        analyzed: r.get(2)?,
                        genre: r.get(3)?,
                        artist: r.get(4)?,
                        private: nature::is_private(&r.get::<_, String>(5)?, &nature::load_meta(&r.get::<_, String>(6)?)),
                        peak: r.get(7)?,
                        master: r.get(8)?,
                    })
                },
            );
            let mut row = row.ok()?;
            // Le master rangé avec le morceau, s'il y en a un : c'est lui qui sera envoyé.
            let child: Option<Option<f64>> = conn
                .query_row("SELECT true_peak FROM tracks WHERE parent_id = ?1 AND version_label = 'master' AND missing = 0 ORDER BY id DESC LIMIT 1", [id], |r| r.get(0))
                .ok();
            if let Some(peak) = child {
                row.master = true;
                row.peak = peak.or(row.peak);
            }
            Some(row)
        })
        .collect()
}

fn clean(t: &str) -> String {
    super::projects::clean_title(t, t)
}

/// Une liste de morceaux concernés, nommés s'ils sont peu nombreux.
fn who(list: &[&Row], one: &str, many: &str) -> String {
    match list {
        [a] => format!("« {} » {one}", clean(&a.title)),
        [a, b] => format!("« {} » et « {} » {many}", clean(&a.title), clean(&b.title)),
        _ => format!("{} morceaux {many}", list.len()),
    }
}

/// Le pilotage d'une sortie. `cover` : le niveau de la pochette (0 bon, 1 à voir, 2 problème) et son explication, lus
/// par l'application qui sait décoder les images.
pub fn pilot(conn: &Connection, s: &Sheet, today: NaiveDate, cover: (i32, &str)) -> Pilot {
    let planned = releases::planning(conn, today).into_iter().find(|p| p.id == s.id);
    let checked = |k: &str| s.steps.iter().any(|x| x == k);
    let done = STEPS.iter().filter(|(k, _)| checked(k)).count();
    let (countdown, tone) = match planned.as_ref().map(|p| p.days) {
        None => ("Date à fixer".to_string(), 0),
        Some(0) => ("Aujourd'hui".to_string(), 1),
        Some(d) if d > 0 => (format!("J-{d}"), if d <= 7 { 1 } else { 0 }),
        Some(d) => (format!("J+{}", -d), 0),
    };
    let late = planned.as_ref().is_some_and(|p| p.steps.iter().any(|x| x.late));
    let tone = if late { 2 } else { tone };
    let out = planned.as_ref().is_some_and(|p| p.days < 0);
    // La prochaine étape : celle dont l'échéance arrive le plus tôt.
    let next = match planned.as_ref() {
        Some(p) => match p.steps.iter().filter(|x| !x.done).min_by_key(|x| x.due) {
            Some(x) => format!("{}, avant le {}", releases::step_label(x.key), x.due.format("%d/%m")),
            None => "Tout est fait.".to_string(),
        },
        None => match STEPS.iter().find(|(k, _)| !checked(k)) {
            Some((k, _)) => releases::step_label(k).to_string(),
            None => "Tout est fait.".to_string(),
        },
    };
    let tracks = rows(conn, &s.tracks);
    let mut points = Vec::new();
    if let Some(p) = planned.as_ref().filter(|_| !out) {
        for x in p.steps.iter().filter(|x| x.late) {
            points.push(point(2, format!("En retard : {} (prévu le {}).", releases::step_label(x.key).to_lowercase(), x.due.format("%d/%m")), format!("step:{}", x.key), "Fait"));
        }
    }
    if s.date.is_empty() {
        points.push(point(1, "Pas de date : le compte à rebours ne peut pas démarrer.", "date", "Fixer la date"));
    }
    if tracks.is_empty() {
        points.push(point(2, "Aucun morceau dans la sortie.", "tracks", "Ajouter"));
    }
    let missing: Vec<&Row> = tracks.iter().filter(|t| t.missing).collect();
    if let [first, ..] = missing[..] {
        points.push(point(2, who(&missing, "est introuvable sur le disque.", "sont introuvables sur le disque."), format!("track:{}", first.id), "Voir"));
    }
    for t in tracks.iter().filter(|t| t.private) {
        points.push(point(2, format!("« {} » est une prod marquée « ne pas publier ».", clean(&t.title)), format!("track:{}", t.id), "Voir"));
    }
    let hot: Vec<&Row> = tracks.iter().filter(|t| t.peak.is_some_and(|p| p > 0.0)).collect();
    if !hot.is_empty() {
        points.push(point(2, who(&hot, "dépasse 0 dB : il saturera à la conversion.", "dépassent 0 dB : ils satureront à la conversion."), "master", "Masteriser"));
    }
    if !checked("pochette") && (cover.0 > 0 || s.cover_path.is_empty()) {
        points.push(point(if s.cover_path.is_empty() || cover.0 == 2 { 2 } else { 1 }, cover.1, "cover", "Choisir"));
    }
    let idle: Vec<&Row> = tracks.iter().filter(|t| !t.analyzed && !t.missing).collect();
    if !idle.is_empty() {
        points.push(point(1, who(&idle, "n'est pas analysé : le contrôle ne peut pas conclure.", "ne sont pas analysés : le contrôle ne peut pas conclure."), "analyze", "Analyser"));
    }
    let bare: Vec<&Row> = tracks.iter().filter(|t| !t.genre || !t.artist).collect();
    if !bare.is_empty() {
        points.push(point(1, who(&bare, "n'a pas de genre ou d'artiste.", "n'ont pas de genre ou d'artiste."), "genre", "Compléter"));
    }
    if s.credits.trim().is_empty() {
        points.push(point(1, "Crédits vides.", "credits", "Pré-remplir"));
    }
    if s.description.trim().is_empty() {
        points.push(point(1, "Pas de description de la sortie.", "texts", "Écrire"));
    }
    if s.sc_link.trim().is_empty() && (checked("soundcloud") || planned.as_ref().is_some_and(|p| p.days <= 1)) {
        points.push(point(1, "Pas de lien SoundCloud sur la fiche.", "", ""));
    }
    if super::liens::is_soundcloud(&s.sc_link) {
        let bare = s.tracks.iter().filter(|id| conn.query_row("SELECT 1 FROM tracks WHERE id = ?1 AND IFNULL(sc_url, '') = ''", [id], |_| Ok(())).is_ok()).count();
        if bare > 0 {
            points.push(point(1, format!("{bare} titre{} sans lien.", if bare > 1 { "s" } else { "" }), "links", "Relier"));
        }
    }
    // Ce que les données prouvent déjà : à cocher d'un clic.
    let proven = |k: &str, text: &str, points: &mut Vec<Point>| {
        if !checked(k) {
            points.push(point(0, text, format!("step:{k}"), "Cocher"));
        }
    };
    if !tracks.is_empty() && tracks.iter().all(|t| t.master) {
        proven("master", if tracks.len() > 1 { "Tous les masters sont là." } else { "Le master est là." }, &mut points);
    }
    if !s.cover_path.is_empty() && cover.0 < 2 {
        proven("pochette", "La pochette convient.", &mut points);
    }
    if !tracks.is_empty() && bare.is_empty() && !s.credits.trim().is_empty() && !s.description.trim().is_empty() {
        proven("infos", "Titres, crédits et description sont remplis.", &mut points);
    }
    let qc_ok = !tracks.is_empty() && idle.is_empty() && tracks.iter().all(|t| !releases::qc(conn, t.id).iter().any(|x| x.1 == "problème"));
    if qc_ok {
        proven("controle", "Le contrôle ne trouve aucun problème.", &mut points);
    }
    if !s.sc_link.trim().is_empty() {
        proven("soundcloud", "Le lien SoundCloud est sur la fiche.", &mut points);
    }
    let written = !s.tracks.is_empty() && s.tracks.iter().all(|id| conn.query_row("SELECT 1 FROM post_drafts WHERE track_id = ?1 LIMIT 1", [id], |_| Ok(())).is_ok());
    if written {
        proven("textes", "Les textes sont écrits.", &mut points);
    }
    points.sort_by_key(|p| match p.level {
        2 => 0,
        1 => 1,
        _ => 2,
    });
    Pilot { countdown, tone, date: releases::long_date(&s.date), done, total: STEPS.len(), next, points }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, title, artist, genre, missing, analyzed_at, true_peak, lufs, file_format, samplerate, clip_ratio) VALUES
                (1, 'a.wav', 'a.wav', 'Nuit', 'Otoko', 'House', 0, '2026-01-01', 0.4, -9.0, 'wav', 44100, 0),
                (2, 'b.wav', 'b.wav', 'Jour', '', '', 0, NULL, NULL, NULL, 'wav', 44100, NULL);
             INSERT INTO releases (id, title, rtype, release_date, steps) VALUES (1, 'Nuit', 'single', '2026-12-04', '{\"master\": true}');
             INSERT INTO release_tracks (release_id, track_id, position) VALUES (1, 1, 1), (1, 2, 2);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn counts_down_and_names_what_blocks() {
        let conn = base();
        let s = releases::load(&conn, 1).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 6).unwrap();
        let p = pilot(&conn, &s, today, (1, "Aucune pochette."));
        assert_eq!((p.countdown.as_str(), p.tone, p.done, p.total), ("J-28", 0, 1, 7));
        assert_eq!(p.next, "Pochette prête, avant le 20/11");
        let has = |t: &str| p.points.iter().find(|x| x.text.contains(t)).cloned();
        assert_eq!(has("dépasse 0 dB").map(|x| x.level), Some(2));
        assert_eq!(has("Aucune pochette").map(|x| (x.level, x.action)), Some((2, "cover".to_string())));
        assert_eq!(has("pas analysé").map(|x| x.action), Some("analyze".to_string()));
        assert_eq!(has("Crédits vides").map(|x| x.label), Some("Pré-remplir"));
        // Les plus graves d'abord.
        assert!(p.points.windows(2).all(|w| w[0].level >= w[1].level || w[1].level == 0), "{:?}", p.points);
    }

    #[test]
    fn late_steps_and_proven_steps() {
        let conn = base();
        conn.execute_batch(
            "UPDATE tracks SET true_peak = -1.2, genre = 'House', artist = 'Otoko', analyzed_at = '2026-01-01', lufs = -9.0, clip_ratio = 0;
             UPDATE releases SET credits = 'Otoko', description = 'Une nuit.', sc_link = 'https://soundcloud.com/x';",
        )
        .unwrap();
        let s = releases::load(&conn, 1).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 28).unwrap();
        let p = pilot(&conn, &s, today, (0, "3000×3000, carrée : parfait."));
        assert_eq!((p.countdown.as_str(), p.tone), ("J-6", 2));
        assert!(p.points.iter().any(|x| x.level == 2 && x.action == "step:pochette"), "{:?}", p.points);
        let proven: Vec<&str> = p.points.iter().filter(|x| x.level == 0).map(|x| x.action.as_str()).collect();
        assert!(proven.contains(&"step:infos") && proven.contains(&"step:controle") && proven.contains(&"step:soundcloud"), "{proven:?}");
        assert!(!proven.contains(&"step:master"), "déjà cochée");
    }

    #[test]
    fn a_release_without_date_says_so() {
        let conn = base();
        conn.execute("UPDATE releases SET release_date = ''", []).unwrap();
        let s = releases::load(&conn, 1).unwrap();
        let p = pilot(&conn, &s, NaiveDate::from_ymd_opt(2026, 11, 6).unwrap(), (0, ""));
        assert_eq!(p.countdown, "Date à fixer");
        assert_eq!(p.next, "Pochette prête");
        assert!(p.points.iter().any(|x| x.action == "date"));
    }
}
