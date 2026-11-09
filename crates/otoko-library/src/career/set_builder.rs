//! Construire un set (comme `services/set_builder.py`) : un ordre de passage qui enchaîne bien en
//! tempo (à 6 % près, demi et double tempo compris), en tonalité (roue de Camelot) et en énergie ;
//! glouton (toujours le meilleur suivant) puis amélioré par échanges (2-opt). Mêmes coûts, même
//! ordre de parcours et mêmes égalités que Studio Vault : à données égales, le même set.

use rusqlite::Connection;

use super::camelot::{camelot, compatibles};

pub const MODES: [(&str, &str); 3] = [("montee", "Montée en énergie"), ("plateau", "Garder le niveau"), ("libre", "Tempo et tonalité seulement")];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetTrack {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub path: String,
    pub bpm: Option<f64>,
    pub key: String,
    pub energy: Option<f64>,
    pub duration: Option<f64>,
}

/// Les morceaux (hors sets) dont l'identifiant est donné, dans cet ordre.
pub fn tracks(conn: &Connection, ids: &[i64]) -> Vec<SetTrack> {
    ids.iter()
        .filter_map(|id| {
            conn.query_row(
                "SELECT id, IFNULL(title, ''), IFNULL(filename, ''), IFNULL(artist, ''), path, bpm, IFNULL(music_key, ''), energy, duration_s
                 FROM tracks WHERE id = ?1 AND COALESCE(nature, 'original') <> 'set' AND IFNULL(path, '') <> ''",
                [id],
                |r| {
                    let (t, f): (String, String) = (r.get(1)?, r.get(2)?);
                    Ok(SetTrack {
                        id: r.get(0)?,
                        title: if t.is_empty() { std::path::Path::new(&f).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(f) } else { t },
                        artist: r.get(3)?,
                        path: r.get(4)?,
                        bpm: r.get(5)?,
                        key: r.get(6)?,
                        energy: r.get(7)?,
                        duration: r.get(8)?,
                    })
                },
            )
            .ok()
        })
        .collect()
}

/// Écart de tempo en % (le demi-tempo et le double tempo comptent comme proches).
pub fn bpm_gap(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    let (a, b) = (a?, b?);
    if a <= 0.0 || b <= 0.0 {
        return None;
    }
    [1.0, 2.0, 0.5].iter().map(|r| (b * r - a).abs() / a * 100.0).reduce(f64::min)
}

/// « same », « ok » (voisin ou relatif), « boost » (+2, montée d'énergie), « clash » ou « » (inconnu).
pub fn key_relation(ka: &str, kb: &str) -> &'static str {
    let (ca, cb) = (camelot(ka), camelot(kb));
    if ca.is_empty() || cb.is_empty() {
        return "";
    }
    if ca == cb {
        return "same";
    }
    if compatibles(ca).iter().any(|c| c == cb) {
        return "ok";
    }
    let num = |c: &str| c[..c.len() - 1].parse::<i32>().unwrap_or(0);
    if ca.as_bytes()[ca.len() - 1] == cb.as_bytes()[cb.len() - 1] && (num(cb) - num(ca)).rem_euclid(12) == 2 {
        return "boost";
    }
    "clash"
}

fn energy(t: &SetTrack) -> f64 {
    t.energy.unwrap_or(0.5)
}

/// Coût d'enchaîner a puis b : plus il est bas, mieux ça s'enchaîne.
pub fn cost(a: &SetTrack, b: &SetTrack, mode: &str) -> f64 {
    let mut c = match bpm_gap(a.bpm, b.bpm) {
        None => 4.0,
        Some(g) => g.min(25.0) * 0.6,
    };
    c += match key_relation(&a.key, &b.key) {
        "same" => 0.0,
        "ok" => 0.5,
        "boost" => 2.0,
        "clash" => 6.0,
        _ => 2.5,
    };
    let de = energy(b) - energy(a);
    match mode {
        "montee" => c += 8.0 * (-de).max(0.0) + 1.5 * (de - 0.15).max(0.0),
        "plateau" => c += 6.0 * de.abs(),
        _ => {}
    }
    c
}

pub fn total_cost(order: &[SetTrack], mode: &str) -> f64 {
    order.windows(2).map(|w| cost(&w[0], &w[1], mode)).sum()
}

/// Le premier élément minimal (comme `min()` de Python : à égalité, le premier).
fn argmin<T>(items: &[T], key: impl Fn(&T) -> f64) -> usize {
    let mut best = 0;
    for i in 1..items.len() {
        if key(&items[i]) < key(&items[best]) {
            best = i;
        }
    }
    best
}

/// L'ordre proposé ; `start` : le morceau par lequel commencer ; `target_min` : durée visée (0 : tous).
pub fn build(pool: &[SetTrack], mode: &str, start: Option<i64>, target_min: f64) -> Vec<SetTrack> {
    if pool.len() <= 1 {
        return pool.to_vec();
    }
    let fixed = start.is_some_and(|s| pool.iter().any(|t| t.id == s));
    let first = if fixed {
        pool.iter().position(|t| Some(t.id) == start).unwrap_or(0)
    } else if mode == "montee" {
        // (énergie, tempo) le plus bas ; les morceaux sans tempo passent après.
        let mut best = 0;
        for i in 1..pool.len() {
            let k = |t: &SetTrack| (energy(t), t.bpm.unwrap_or(999.0));
            if k(&pool[i]) < k(&pool[best]) {
                best = i;
            }
        }
        best
    } else {
        argmin(pool, |t| t.bpm.unwrap_or(999.0))
    };
    let mut order = vec![pool[first].clone()];
    let mut left: Vec<SetTrack> = pool.iter().enumerate().filter(|(i, _)| *i != first).map(|(_, t)| t.clone()).collect();
    while !left.is_empty() {
        let last = order.last().unwrap().clone();
        let k = argmin(&left, |t| cost(&last, t, mode));
        order.push(left.remove(k));
    }
    let mut order = two_opt(order, mode, fixed);
    if target_min > 0.0 {
        let mut kept = Vec::new();
        let mut total = 0.0;
        for t in order {
            if !kept.is_empty() && total >= target_min * 60.0 {
                break;
            }
            total += t.duration.unwrap_or(0.0);
            kept.push(t);
        }
        order = kept;
    }
    order
}

fn two_opt(order: Vec<SetTrack>, mode: &str, fixed_first: bool) -> Vec<SetTrack> {
    if order.len() < 4 || order.len() > 250 {
        return order;
    }
    let mut best = order;
    let mut best_c = total_cost(&best, mode);
    let lo = if fixed_first || mode == "montee" { 1 } else { 0 };
    for _ in 0..3 {
        let mut improved = false;
        for i in lo..best.len() - 2 {
            for j in i + 1..best.len() {
                let mut cand = best.clone();
                cand[i..=j].reverse();
                let c = total_cost(&cand, mode);
                if c + 1e-9 < best_c {
                    best = cand;
                    best_c = c;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
    best
}

/// Pour chaque enchaînement : (écart de tempo, relation des tonalités, sans souci).
pub fn transitions(order: &[SetTrack]) -> Vec<(Option<f64>, &'static str, bool)> {
    order
        .windows(2)
        .map(|w| {
            let gap = bpm_gap(w[0].bpm, w[1].bpm);
            let rel = key_relation(&w[0].key, &w[1].key);
            (gap, rel, gap.is_none_or(|g| g <= 6.0) && ["same", "ok", "boost", ""].contains(&rel))
        })
        .collect()
}

pub fn describe(gap: Option<f64>, rel: &str) -> String {
    let mut bits = Vec::new();
    if let Some(g) = gap {
        bits.push(if g >= 0.5 { format!("tempo {g:.0} %") } else { "même tempo".into() });
    }
    bits.push(
        match rel {
            "same" => "même tonalité",
            "ok" => "tonalités voisines",
            "boost" => "+2 (montée)",
            "clash" => "tonalités qui se heurtent",
            _ => "tonalité inconnue",
        }
        .into(),
    );
    bits.join(" · ")
}

/// Début de chaque morceau dans le mix (fondu de `overlap` secondes entre deux titres).
pub fn timeline(order: &[SetTrack], overlap: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut t = 0.0;
    for tr in order {
        out.push(t);
        t += (tr.duration.unwrap_or(240.0) - overlap).max(30.0);
    }
    out
}

pub fn total_duration(order: &[SetTrack], overlap: f64) -> f64 {
    match order.last() {
        None => 0.0,
        Some(last) => timeline(order, overlap).last().copied().unwrap_or(0.0) + last.duration.unwrap_or(240.0),
    }
}

/// Playlist M3U8 (Rekordbox, VirtualDJ, Serato, Traktor l'importent).
pub fn to_m3u(order: &[SetTrack]) -> String {
    let mut lines = vec!["#EXTM3U".to_string()];
    for t in order {
        let dur = t.duration.map(|d| d.round() as i64).unwrap_or(-1);
        let who = t.artist.trim();
        lines.push(format!("#EXTINF:{dur},{}{}", if who.is_empty() { String::new() } else { format!("{who} - ") }, t.title));
        lines.push(t.path.clone());
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: i64, bpm: f64, key: &str, e: f64) -> SetTrack {
        SetTrack { id, title: format!("T{id}"), path: format!("D:\\{id}.wav"), bpm: Some(bpm), key: key.into(), energy: Some(e), duration: Some(300.0), ..Default::default() }
    }

    #[test]
    fn gaps_relations_and_order() {
        assert!((bpm_gap(Some(124.0), Some(62.0)).unwrap()).abs() < 1e-9, "demi-tempo");
        assert_eq!(bpm_gap(Some(0.0), Some(120.0)), None);
        assert_eq!(key_relation("Am", "Am"), "same");
        assert_eq!(key_relation("Am", "Em"), "ok");
        assert_eq!(key_relation("Am", "Bm"), "boost");
        assert_eq!(key_relation("Am", "F#"), "clash");
        assert_eq!(key_relation("", "Am"), "");
        let pool = vec![t(1, 128.0, "Am", 0.9), t(2, 120.0, "Am", 0.2), t(3, 124.0, "Em", 0.5), t(4, 126.0, "Bm", 0.7), t(5, 140.0, "F#", 0.95)];
        let o = build(&pool, "montee", None, 0.0);
        assert_eq!(o[0].id, 2, "départ calme");
        assert_eq!(o.len(), 5);
        let fixed = build(&pool, "libre", Some(5), 0.0);
        assert_eq!(fixed[0].id, 5);
        let short = build(&pool, "montee", None, 10.0);
        assert_eq!(short.len(), 2, "10 min visées : deux morceaux de 5 min");
        assert_eq!(timeline(&o, 30.0), vec![0.0, 270.0, 540.0, 810.0, 1080.0]);
        assert_eq!(total_duration(&o, 30.0), 1380.0);
        assert_eq!(describe(Some(0.2), "ok"), "même tempo · tonalités voisines");
        assert!(to_m3u(&o[..1]).starts_with("#EXTM3U\n#EXTINF:300,T2\nD:\\2.wav\n"));
    }
}
