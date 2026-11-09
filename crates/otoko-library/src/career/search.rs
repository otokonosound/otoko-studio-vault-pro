//! Recherche de la bibliothèque (comme `composer_vault/ui/search.py`) : mots libres et filtres
//! `clé:valeur` combinables, traduits en condition SQL sur la table `tracks`. Les vues
//! enregistrées gardent cette condition telle quelle (même format que Studio Vault).
//!
//! `bpm:120-128` `key:Am` `camelot:8A` `genre:techno` `ambiance:sombre` `status:terminé`
//! `release:ep` `pick:gardé` `tag:club` `format:wav` `suno:oui` `analyse:non` `nature:remix`
//! `version:master` `role:stem` `track:3`

use rusqlite::types::Value;

use super::camelot;

/// Découpe comme `shlex.split` : espaces, guillemets simples ou doubles.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() || any {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        // Guillemet non fermé : comme Studio Vault, on retombe sur un simple découpage.
        return text.split_whitespace().map(str::to_string).collect();
    }
    if !cur.is_empty() || any {
        out.push(cur);
    }
    out
}

fn status_of(v: &str) -> String {
    match v {
        "idée" | "idea" => "idea",
        "draft" | "brouillon" => "draft",
        "wip" | "cours" | "en cours" => "wip",
        "mix" | "à mixer" | "amixer" => "to_mix",
        "master" | "à masteriser" => "to_master",
        "done" | "terminé" | "fini" => "done",
        "released" | "publié" => "released",
        other => other,
    }
    .to_string()
}

fn release_of(v: &str) -> String {
    match v {
        "lp" => "album",
        "compil" => "compilation",
        "autre" => "other",
        other => other,
    }
    .to_string()
}

fn num(s: &str) -> Option<f64> {
    s.trim().parse().ok()
}

/// (condition SQL, paramètres). Toujours au moins `missing = 0`.
pub fn parse_query(text: &str) -> (String, Vec<Value>) {
    let text = text.trim();
    let mut wh: Vec<String> = vec!["missing = 0".into()];
    let mut p: Vec<Value> = Vec::new();
    let t = |s: &str| Value::Text(s.to_string());
    for tok in tokens(text) {
        if let Some((key, val)) = tok.split_once(':') {
            let key = key.to_lowercase();
            let val = val.trim();
            let low = val.to_lowercase();
            let handled = match key.as_str() {
                "bpm" => {
                    let range = val.split_once(['-', '–']).and_then(|(a, b)| Some((num(a)?, num(b)?)));
                    if let Some((a, b)) = range {
                        wh.push("bpm BETWEEN ? AND ?".into());
                        p.extend([Value::Real(a), Value::Real(b)]);
                    } else if let Some(v) = num(val) {
                        wh.push("ABS(bpm - ?) < 1".into());
                        p.push(Value::Real(v));
                    }
                    true
                }
                "key" => {
                    wh.push("LOWER(music_key) = LOWER(?)".into());
                    p.push(t(val));
                    true
                }
                "status" => {
                    wh.push("status = ?".into());
                    p.push(Value::Text(status_of(&low)));
                    true
                }
                "release" => {
                    wh.push("LOWER(release_type) = ?".into());
                    p.push(Value::Text(release_of(&low)));
                    true
                }
                "track" | "trackno" | "track_no" => {
                    if let Ok(n) = val.parse::<i64>() {
                        wh.push("track_no = ?".into());
                        p.push(Value::Integer(n));
                    }
                    true
                }
                "format" => {
                    wh.push("LOWER(file_format) = LOWER(?)".into());
                    p.push(t(val));
                    true
                }
                "role" => {
                    wh.push("role = ?".into());
                    p.push(t(if ["suno", "suno_ai", "suno-ai"].contains(&low.as_str()) { "suno_generation" } else { val }));
                    true
                }
                "suno" => {
                    if ["true", "1", "yes", "oui", "o"].contains(&low.as_str()) {
                        wh.push(SUNO_WHERE.into());
                    }
                    true
                }
                "nature" => {
                    let nat = match low.as_str() {
                        "originaux" => "original",
                        "remixes" => "remix",
                        "bootleg" => "edit",
                        "mashups" => "mashup",
                        "sets" | "mix" => "set",
                        "prods" => "prod",
                        o => o,
                    };
                    if super::NATURES.iter().any(|n| n.0 == nat) {
                        wh.push("COALESCE(nature, 'original') = ?".into());
                        p.push(t(nat));
                    }
                    true
                }
                "genre" => {
                    wh.push("LOWER(genre) LIKE LOWER(?)".into());
                    p.push(Value::Text(format!("%{val}%")));
                    true
                }
                "ambiance" | "mood" => {
                    wh.push("(LOWER(mood_auto) LIKE LOWER(?) OR id IN (SELECT track_id FROM track_tags JOIN tags ON tags.id = track_tags.tag_id WHERE LOWER(tags.name) = LOWER(?)))".into());
                    p.extend([Value::Text(format!("%{val}%")), t(val)]);
                    true
                }
                "pick" | "selection" | "sélection" => {
                    wh.push(
                        if low.starts_with("gard") || ["1", "oui", "keeper"].contains(&low.as_str()) {
                            "pick = 1"
                        } else if low.starts_with("rej") || ["-1", "non"].contains(&low.as_str()) {
                            "pick = -1"
                        } else {
                            "(pick = 0 OR pick IS NULL)"
                        }
                        .into(),
                    );
                    true
                }
                "camelot" => {
                    let ks = camelot::keys_for(val);
                    if !ks.is_empty() {
                        wh.push(format!("music_key IN ({})", vec!["?"; ks.len()].join(",")));
                        p.extend(ks.iter().map(|k| t(k)));
                    }
                    true
                }
                "analyse" | "analyzed" => {
                    wh.push(if ["non", "no", "0", "false"].contains(&low.as_str()) { "analyzed_at IS NULL" } else { "analyzed_at IS NOT NULL" }.into());
                    true
                }
                "version" => {
                    wh.push("LOWER(version_label) LIKE LOWER(?)".into());
                    p.push(Value::Text(format!("%{val}%")));
                    true
                }
                "tag" => {
                    wh.push("id IN (SELECT track_id FROM track_tags JOIN tags ON tags.id = track_tags.tag_id WHERE LOWER(tags.name) = LOWER(?))".into());
                    p.push(t(val));
                    true
                }
                _ => false,
            };
            if handled {
                continue;
            }
        }
        // Mot libre.
        let like = format!("%{tok}%");
        wh.push("(title LIKE ? OR artist LIKE ? OR album LIKE ? OR genre LIKE ? OR filename LIKE ? OR path LIKE ?)".into());
        p.extend(std::iter::repeat_n(Value::Text(like), 6));
    }
    (wh.join(" AND "), p)
}

/// Les sorties Suno : rôle « suno_generation » ou tag « suno ».
pub const SUNO_WHERE: &str =
    "(role = 'suno_generation' OR id IN (SELECT track_id FROM track_tags JOIN tags ON tags.id = track_tags.tag_id WHERE lower(tags.name) = 'suno'))";

/// Ce que la recherche sait faire (F1).
pub const HELP: &str = "Mots libres (titre, artiste, album, genre, nom de fichier) et filtres combinables : \
bpm:120-128 · key:Am · camelot:8A · genre:techno · ambiance:sombre · status:terminé · release:ep · pick:gardé · \
tag:club · format:wav · suno:oui · analyse:non · nature:remix (original, remix, edit, mashup, set, prod) · \
version:master. Guillemets pour plusieurs mots : genre:\"tribal house\".";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_like_studio_vault() {
        assert_eq!(tokens(r#"genre:"tribal house" bpm:120-128 nuit"#), vec!["genre:tribal house", "bpm:120-128", "nuit"]);
        let (w, p) = parse_query("bpm:120-128 status:terminé camelot:8A pick:gardé nuit");
        assert_eq!(w, "missing = 0 AND bpm BETWEEN ? AND ? AND status = ? AND music_key IN (?) AND pick = 1 AND (title LIKE ? OR artist LIKE ? OR album LIKE ? OR genre LIKE ? OR filename LIKE ? OR path LIKE ?)");
        assert_eq!(p[0], Value::Real(120.0));
        assert_eq!(p[2], Value::Text("done".into()));
        assert_eq!(p[3], Value::Text("Am".into()));
        assert_eq!(parse_query("").0, "missing = 0");
        assert_eq!(parse_query("http://x").1.len(), 6, "une clé inconnue reste un mot libre");
    }
}
