//! Roue de Camelot : deux morceaux s'enchaînent bien si leurs codes sont identiques, voisins
//! sur la roue (±1, même lettre) ou relatifs (même chiffre, A ↔ B).

/// Tonalité (notation de l'analyse) → code Camelot.
const CAMELOT: [(&str, &str); 34] = [
    ("Abm", "1A"), ("G#m", "1A"), ("B", "1B"),
    ("Ebm", "2A"), ("D#m", "2A"), ("F#", "2B"), ("Gb", "2B"),
    ("Bbm", "3A"), ("A#m", "3A"), ("Db", "3B"), ("C#", "3B"),
    ("Fm", "4A"), ("Ab", "4B"), ("G#", "4B"),
    ("Cm", "5A"), ("Eb", "5B"), ("D#", "5B"),
    ("Gm", "6A"), ("Bb", "6B"), ("A#", "6B"),
    ("Dm", "7A"), ("F", "7B"),
    ("Am", "8A"), ("C", "8B"),
    ("Em", "9A"), ("G", "9B"),
    ("Bm", "10A"), ("D", "10B"),
    ("F#m", "11A"), ("Gbm", "11A"), ("A", "11B"),
    ("C#m", "12A"), ("Dbm", "12A"), ("E", "12B"),
];

pub fn camelot(key: &str) -> &'static str {
    let k = key.trim();
    CAMELOT.iter().find(|c| c.0 == k).map(|c| c.1).unwrap_or("")
}

/// Rang sur la roue (1A = 0, 1B = 1, 2A = 2…), pour trier par tonalité.
pub fn rank(code: &str) -> Option<i64> {
    let c = code.trim().to_uppercase();
    let (n, l) = c.split_at(c.len().checked_sub(1)?);
    let n: i64 = n.parse().ok()?;
    if !(1..=12).contains(&n) {
        return None;
    }
    match l {
        "A" => Some((n - 1) * 2),
        "B" => Some((n - 1) * 2 + 1),
        _ => None,
    }
}

pub fn keys_for(code: &str) -> Vec<&'static str> {
    let c = code.trim().to_uppercase();
    let mut v: Vec<&str> = CAMELOT.iter().filter(|x| x.1 == c).map(|x| x.0).collect();
    v.dedup();
    v
}

/// Codes compatibles : lui-même, ses deux voisins, son relatif.
pub fn compatibles(code: &str) -> Vec<String> {
    let c = code.trim().to_uppercase();
    let Some(r) = rank(&c) else { return Vec::new() };
    let n = r / 2 + 1;
    let (l, o) = if r % 2 == 0 { ("A", "B") } else { ("B", "A") };
    let prev = if n == 1 { 12 } else { n - 1 };
    let next = if n == 12 { 1 } else { n + 1 };
    vec![format!("{n}{l}"), format!("{prev}{l}"), format!("{next}{l}"), format!("{n}{o}")]
}

/// Toutes les tonalités compatibles avec `key` (pour une requête).
pub fn compatible_keys(key: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for c in compatibles(camelot(key)) {
        for k in keys_for(&c) {
            if !out.contains(&k) {
                out.push(k);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_turns() {
        assert_eq!(camelot("Am"), "8A");
        assert_eq!(rank("1A"), Some(0));
        assert_eq!(rank("12B"), Some(23));
        assert_eq!(compatibles("1A"), vec!["1A", "12A", "2A", "1B"]);
        let k = compatible_keys("Am");
        assert!(k.contains(&"Am") && k.contains(&"Em") && k.contains(&"Dm") && k.contains(&"C"));
    }
}
