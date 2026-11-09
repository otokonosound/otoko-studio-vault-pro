//! Nature d'un morceau : original, remix, edit, mashup, set, prod pour un artiste (comme
//! `composer_vault/services/nature.py`). Devinée d'après le nom du fichier ; informations propres à
//! chaque nature en JSON (`nature_meta`) ; liste minutée des sets ; repères sur les droits.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Value as Json, json};

pub const AUTHORIZATION: [(&str, &str); 4] = [("none", "Non demandée"), ("asked", "Demandée"), ("granted", "Accordée"), ("refused", "Refusée")];
pub const ORDER_STATUSES: [(&str, &str); 6] =
    [("brief", "Brief reçu"), ("demo", "Maquette envoyée"), ("feedback", "Retours"), ("validated", "Validé"), ("delivered", "Livré"), ("paid", "Payé")];
pub const AGREEMENTS: [(&str, &str); 5] = [
    ("", "Non défini"),
    ("assignment", "Cession des droits"),
    ("exclusive", "Licence exclusive"),
    ("lease", "Licence non exclusive (lease)"),
    ("split", "Partage des droits"),
];
pub const DELIVERABLES: [(&str, &str); 4] = [("master", "Master"), ("instrumental", "Instrumental"), ("acapella", "A cappella"), ("stems", "Stems")];

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("expression"))
}

/// Nature probable d'après un nom de fichier ou un titre (le plus spécifique d'abord).
pub fn detect(text: &str) -> &'static str {
    static OWN: OnceLock<Regex> = OnceLock::new();
    static PATS: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    static PAREN: OnceLock<Regex> = OnceLock::new();
    let t = text.replace('_', " ").to_lowercase();
    // Versions de ses propres morceaux : ce ne sont pas des edits d'un titre existant.
    let t = re(&OWN, r"\b(radio|single|extended|club|short|clean|explicit|album)\s+(edit|mix|version)\b").replace_all(&t, " ");
    let pats = PATS.get_or_init(|| {
        [
            ("set", r"\b(dj[\s_-]?set|live[\s_-]?set|mixtape|podcast|b2b)\b|\bset\b|\bmix\s*\d{1,3}\b|\bmix\s*(vol|#)"),
            // Python : « \bvs\.?\b(?!.*remix) » ; le regard en avant est vérifié à part.
            ("mashup", r"\bmash[\s_-]?up\b"),
            ("edit", r"\bbootleg\b|\b(re)?edit\b|\bflip\b|\brework\b"),
            ("remix", r"\bremix\b|\brmx\b|\bvip\b"),
            ("prod", r"\bprod\.\s|\bprod\s?by\b|\bprod\s+(pour|for)\b|\btype\s?beat\b"),
        ]
        .into_iter()
        .map(|(n, p)| (n, Regex::new(p).expect("expression")))
        .collect()
    });
    static VS: OnceLock<Regex> = OnceLock::new();
    for (n, r) in pats {
        if r.is_match(&t) {
            return n;
        }
        if *n == "mashup" {
            // « A vs B » sans « remix » après.
            if let Some(m) = re(&VS, r"\bvs\.?\b").find(&t) {
                if !t[m.end()..].contains("remix") {
                    return "mashup";
                }
            }
        }
    }
    if re(&PAREN, r"\(.*remix.*\)").is_match(&t) {
        return "remix";
    }
    "original"
}

/// « Artiste - Titre (X Remix) » → {orig_artist, orig_title}.
pub fn parse_remix_title(text: &str) -> Json {
    static EXT: OnceLock<Regex> = OnceLock::new();
    static NUM: OnceLock<Regex> = OnceLock::new();
    static PAR: OnceLock<Regex> = OnceLock::new();
    static TAIL: OnceLock<Regex> = OnceLock::new();
    let base = re(&EXT, r"(?i)\.[a-z0-9]{2,4}$").replace(text, "");
    let base = re(&NUM, r"^\s*\d{1,3}\s*[.\-_)]\s*").replace(&base, "");
    let title = re(&PAR, r"(?i)\s*[\(\[].*?(remix|edit|bootleg|rework|flip|vip|rmx).*?[\)\]]").replace_all(&base, "");
    let title = re(&TAIL, r"(?i)\s*-\s*(remix|edit|bootleg).*$").replace(&title, "").trim().to_string();
    match title.split_once(" - ") {
        Some((a, t)) => json!({ "orig_artist": a.trim(), "orig_title": t.trim() }),
        None if !title.is_empty() => json!({ "orig_title": title }),
        None => json!({}),
    }
}

pub fn load_meta(raw: &str) -> Json {
    match serde_json::from_str::<Json>(raw) {
        Ok(v @ Json::Object(_)) => v,
        _ => json!({}),
    }
}

/// Prod commandée marquée « ne pas publier sur mes réseaux ».
/// Les morceaux encore « originaux » dont le nom dit autre chose : (morceau, chemin, nature).
/// `ids` vide : toute la bibliothèque.
pub fn suggestions(conn: &rusqlite::Connection, ids: &[i64]) -> Vec<(i64, String, &'static str)> {
    let rows: Vec<(i64, String, String)> = conn
        .prepare("SELECT id, path, IFNULL(title, '') FROM tracks WHERE missing = 0 AND COALESCE(NULLIF(nature, ''), 'original') = 'original' ORDER BY id")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
        .unwrap_or_default();
    rows.into_iter()
        .filter(|r| ids.is_empty() || ids.contains(&r.0))
        .filter_map(|(id, path, title)| {
            let stem = std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let nat = detect(&format!("{stem} {title}"));
            (nat != "original").then_some((id, path, nat))
        })
        .collect()
}

/// Applique les natures proposées (et, pour un remix ou un edit, l'artiste d'origine lu dans le nom).
pub fn apply_suggestions(conn: &rusqlite::Connection, found: &[(i64, String, &str)]) -> rusqlite::Result<usize> {
    for (id, path, nat) in found {
        let raw: String = conn.query_row("SELECT IFNULL(nature_meta, '{}') FROM tracks WHERE id = ?1", [id], |r| r.get(0))?;
        let mut meta = load_meta(&raw);
        if ["remix", "edit"].contains(nat) {
            let name = std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if let (Json::Object(m), Json::Object(found)) = (&mut meta, parse_remix_title(&name)) {
                for (k, v) in found {
                    m.entry(k).or_insert(v);
                }
            }
        }
        conn.execute("UPDATE tracks SET nature = ?2, nature_meta = ?3 WHERE id = ?1", rusqlite::params![id, nat, meta.to_string()])?;
    }
    Ok(found.len())
}

pub fn is_private(nature: &str, meta: &Json) -> bool {
    nature == "prod" && meta["private"].as_bool().unwrap_or(false)
}

/// Repères sur les droits : (niveau « info » ou « attention », message). Pas un avis juridique.
pub fn rights_notes(nature: &str, meta: &Json) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    match nature {
        "remix" | "edit" => {
            let official = meta["official"].as_bool().unwrap_or(false) && meta["authorization"] == "granted";
            if official {
                out.push(("info", "Remix officiel : pense à faire déclarer ta part de remixeur (accord avec l'ayant droit, puis déclaration SACEM).".into()));
            } else {
                out.push((
                    "attention",
                    "Remix ou edit non autorisé : SoundCloud peut le retirer si l'ayant droit le signale. Il n'est ni monétisable, ni déclarable à la SACEM sans accord écrit.".into(),
                ));
            }
        }
        "mashup" => out.push((
            "attention",
            "Un mashup réutilise des enregistrements existants : sans l'accord de chaque ayant droit, il peut être retiré et ne peut pas être déclaré ni monétisé.".into(),
        )),
        "set" => out.push(("info", "Set ou mix : SoundCloud peut bloquer un mix contenant des titres protégés. Créditer chaque titre dans la liste minutée est indispensable.".into())),
        "prod" => match meta["agreement"].as_str().filter(|a| !a.is_empty()) {
            None => out.push(("attention", "Fixe par écrit la répartition des droits (cession, licence, partage) avant de livrer.".into())),
            Some(a) => {
                let label = AGREEMENTS.iter().find(|x| x.0 == a).map(|x| x.1).unwrap_or(a);
                let split = meta["split"].as_str().map(str::to_string).or_else(|| meta["split"].as_f64().map(|f| f.to_string())).filter(|s| !s.is_empty());
                out.push(("info", format!("Accord : {label}{}", split.map(|s| format!(" — ta part : {s} %")).unwrap_or_default())));
            }
        },
        _ => {}
    }
    out
}

// ---------------------------------------------------------------------- sets

pub fn fmt_time(sec: f64, force_hours: bool) -> String {
    let s = sec.max(0.0).round() as i64;
    let (h, m, s) = (s / 3600, s % 3600 / 60, s % 60);
    if h > 0 || force_hours { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// Une ligne de la liste minutée d'un set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub time: f64,
    pub artist: String,
    pub title: String,
    pub bpm: Option<f64>,
    pub key: String,
}

/// Format reconnu par SoundCloud (les minutages deviennent cliquables).
pub fn tracklist_text(entries: &[Entry], total: Option<f64>) -> String {
    let long = total.unwrap_or(0.0) >= 3600.0 || entries.iter().any(|e| e.time >= 3600.0);
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
    sorted
        .iter()
        .map(|e| {
            let who = e.artist.trim();
            let title = if e.title.trim().is_empty() { "Titre inconnu" } else { e.title.trim() };
            format!("{} {}{title}", fmt_time(e.time, long), if who.is_empty() { String::new() } else { format!("{who} – ") })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn cue_sheet(entries: &[Entry], audio_filename: &str, performer: &str, title: &str) -> String {
    let esc = |s: &str| s.replace('"', "'");
    let mut out = vec![format!("PERFORMER \"{}\"", esc(performer)), format!("TITLE \"{}\"", esc(title)), format!("FILE \"{}\" WAVE", esc(audio_filename))];
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
    for (n, e) in sorted.iter().enumerate() {
        let (m, s) = ((e.time / 60.0).floor(), e.time % 60.0);
        let frames = (((s - s.floor()) * 75.0).round() as i64).min(74);
        out.push(format!("  TRACK {:02} AUDIO", n + 1));
        out.push(format!("    TITLE \"{}\"", esc(if e.title.is_empty() { "Titre inconnu" } else { &e.title })));
        out.push(format!("    PERFORMER \"{}\"", esc(if e.artist.is_empty() { performer } else { &e.artist })));
        out.push(format!("    INDEX 01 {:02}:{:02}:{frames:02}", m as i64, s.floor() as i64));
    }
    out.join("\n") + "\n"
}

/// Contrôle des enchaînements : écart de tempo (%) et tonalités compatibles (roue de Camelot).
pub fn transitions(entries: &[Entry]) -> Vec<(usize, Option<f64>, Option<bool>)> {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
    sorted
        .windows(2)
        .enumerate()
        .map(|(i, w)| {
            let bpm = match (w[0].bpm, w[1].bpm) {
                (Some(a), Some(b)) if a > 0.0 => Some(((b - a) / a * 1000.0).round() / 10.0),
                _ => None,
            };
            let (ca, cb) = (super::camelot::camelot(&w[0].key), super::camelot::camelot(&w[1].key));
            let ok = (!ca.is_empty() && !cb.is_empty()).then(|| super::camelot::compatibles(ca).iter().any(|c| c == cb));
            (i, bpm, ok)
        })
        .collect()
}

/// Ligne de crédits pré-remplie selon la nature.
pub fn credits_for(nature: &str, meta: &Json, artist: &str) -> String {
    let orig = || [meta["orig_artist"].as_str(), meta["orig_title"].as_str()].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" – ");
    match nature {
        "remix" => {
            let o = orig();
            format!("Remix par {artist}{}.", if o.is_empty() { String::new() } else { format!(" — titre original : {o}") })
        }
        "edit" => {
            let o = orig();
            format!("Edit par {artist}{}.", if o.is_empty() { String::new() } else { format!(" — d'après {o}") })
        }
        "mashup" => {
            let srcs: Vec<String> = meta["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| [s["artist"].as_str(), s["title"].as_str()].into_iter().flatten().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" – "))
                .filter(|s| !s.is_empty())
                .collect();
            format!("Mashup par {artist}{}.", if srcs.is_empty() { String::new() } else { format!(" — à partir de : {}", srcs.join(" / ")) })
        }
        "set" => format!("Mix par {artist}. Tous les titres appartiennent à leurs ayants droit respectifs."),
        "prod" => {
            let client = meta["client_name"].as_str().unwrap_or("");
            format!("Produit par {artist}{}.", if client.is_empty() { String::new() } else { format!(" pour {client}") })
        }
        _ => format!("Composé et produit par {artist}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natures_like_studio_vault() {
        for (t, n) in [
            ("Otoko - Nuit (Tribal House Remix).wav", "remix"),
            ("Track RMX", "remix"),
            ("Mon titre (Radio Edit)", "original"),
            ("Sunset DJ Set 2025", "set"),
            ("A vs B", "mashup"),
            ("A vs B remix", "remix"),
            ("Bootleg de nuit", "edit"),
            ("Prod by Otoko", "prod"),
            ("Concrete Jungle", "original"),
        ] {
            assert_eq!(detect(t), n, "{t}");
        }
        assert_eq!(parse_remix_title("03. Daft Punk - One More Time (Otoko Remix).wav"), json!({ "orig_artist": "Daft Punk", "orig_title": "One More Time" }));
        let e = vec![
            Entry { time: 0.0, artist: "A".into(), title: "Un".into(), bpm: Some(124.0), key: "Am".into() },
            Entry { time: 245.4, artist: String::new(), title: "Deux".into(), bpm: Some(126.0), key: "Em".into() },
        ];
        assert_eq!(tracklist_text(&e, None), "00:00 A – Un\n04:05 Deux");
        assert_eq!(transitions(&e), vec![(0, Some(1.6), Some(true))]);
        assert!(cue_sheet(&e, "set.wav", "Otoko", "Set").contains("INDEX 01 04:05:30"));
    }
}
