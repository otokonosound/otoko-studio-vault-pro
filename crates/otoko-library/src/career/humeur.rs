//! Ambiance déduite des mesures audio (comme `composer_vault/services/humeur.py`, sans IA) : à
//! partir du mode, du tempo, de l'énergie, de la brillance et de la dynamique, deux adjectifs en
//! français. Chacun connaît son féminin, pour que les textes s'accordent.

/// Adjectif masculin → féminin.
pub const ADJECTIFS: [(&str, &str); 30] = [
    ("sombre", "sombre"),
    ("mélancolique", "mélancolique"),
    ("nocturne", "nocturne"),
    ("hypnotique", "hypnotique"),
    ("tendu", "tendue"),
    ("brut", "brute"),
    ("intense", "intense"),
    ("puissant", "puissante"),
    ("euphorique", "euphorique"),
    ("solaire", "solaire"),
    ("lumineux", "lumineuse"),
    ("aérien", "aérienne"),
    ("contemplatif", "contemplative"),
    ("planant", "planante"),
    ("feutré", "feutrée"),
    ("intime", "intime"),
    ("fiévreux", "fiévreuse"),
    ("dansant", "dansante"),
    ("rêveur", "rêveuse"),
    ("mystérieux", "mystérieuse"),
    ("cinématique", "cinématique"),
    ("organique", "organique"),
    ("énergique", "énergique"),
    ("doux", "douce"),
    ("orageux", "orageuse"),
    ("glacé", "glacée"),
    ("chaleureux", "chaleureuse"),
    ("industriel", "industrielle"),
    ("spirituel", "spirituelle"),
    ("nostalgique", "nostalgique"),
];

pub fn feminin(adj: &str) -> &str {
    ADJECTIFS.iter().find(|a| a.0 == adj).map(|a| a.1).unwrap_or(adj)
}

/// Deux adjectifs (masculins) qui décrivent le morceau.
pub fn deduire(mode: Option<&str>, bpm: Option<f64>, energy: Option<f64>, brightness: Option<f64>, dynamics: Option<f64>) -> Vec<&'static str> {
    if energy.is_none() && bpm.is_none() && mode.is_none() {
        return Vec::new();
    }
    let e = energy.unwrap_or(0.5);
    let b = brightness.unwrap_or(0.5);
    let d = dynamics.unwrap_or(0.5);
    let t = bpm.filter(|x| *x != 0.0).unwrap_or(110.0);
    let mineur = mode == Some("mineur");
    // Ordre d'insertion gardé : à score égal, le premier ajouté passe devant (comme Python).
    let mut scores: Vec<(&'static str, f64)> = Vec::new();
    let mut add = |adj: &'static str, s: f64| match scores.iter_mut().find(|x| x.0 == adj) {
        Some(x) => x.1 += s,
        None => scores.push((adj, s)),
    };
    let when = |c: bool, v: f64| if c { v } else { 0.0 };
    if mineur {
        add("sombre", 1.2 - b);
        add("mélancolique", (1.0 - e) * 1.3 + when(t < 110.0, 0.3));
        add("mystérieux", 0.7 + (1.0 - b) * 0.4);
        add("nocturne", 0.6 + when((115.0..=140.0).contains(&t), 0.4));
        add("tendu", e * 0.9 + when(t > 125.0, 0.3));
    } else {
        add("lumineux", 0.6 + b);
        add("solaire", b * 0.8 + e * 0.5);
        add("euphorique", e * 1.1 + when(t > 120.0, 0.3));
        add("rêveur", (1.0 - e) * 1.1);
        add("chaleureux", (1.0 - b) * 0.6 + 0.4);
    }
    if e > 0.75 {
        add("intense", e);
        add("puissant", e * 0.8 + (1.0 - d) * 0.3);
        add("énergique", e * 0.7 + when(t > 125.0, 0.4));
    }
    if e < 0.45 {
        add("contemplatif", 1.0 - e);
        add("planant", (1.0 - e) * 0.8 + when(t < 100.0, 0.3));
        add("feutré", (1.0 - b) * 0.8);
        add("intime", (1.0 - e) * 0.6 + d * 0.3);
    }
    if (118.0..=135.0).contains(&t) && e > 0.55 {
        add("hypnotique", 0.8 + when(mineur, 0.3));
        add("dansant", 0.5 + when(!mineur, 0.3));
    }
    if t > 140.0 && e > 0.6 {
        add("fiévreux", 0.9);
    }
    if b < 0.35 {
        add("organique", 0.4);
    }
    if b > 0.7 && mineur {
        add("glacé", 0.6);
    }
    if d > 0.6 {
        add("cinématique", d);
    }
    if d < 0.25 && e > 0.7 {
        add("brut", 0.7);
        add("industriel", if mineur { 0.5 } else { 0.1 });
    }
    // Tri stable décroissant, comme `sorted(..., reverse=True)`.
    let mut ranked = scores;
    ranked.sort_by(|x, y| y.1.total_cmp(&x.1));
    ranked.into_iter().take(2).map(|x| x.0).collect()
}

/// « sombre, hypnotique » : le format gardé en base.
pub fn texte(adjs: &[&str]) -> String {
    adjs.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moods_like_studio_vault() {
        // Valeurs vérifiées avec humeur.py de Studio Vault.
        assert_eq!(deduire(Some("mineur"), Some(128.0), Some(0.8), Some(0.4), Some(0.3)), vec!["hypnotique", "tendu"]);
        assert_eq!(deduire(Some("majeur"), Some(124.0), Some(0.7), Some(0.6), Some(0.4)), vec!["lumineux", "euphorique"]);
        assert_eq!(deduire(Some("mineur"), Some(90.0), Some(0.3), Some(0.2), Some(0.7)), vec!["mélancolique", "mystérieux"]);
        assert_eq!(deduire(Some("majeur"), Some(150.0), Some(0.9), Some(0.8), Some(0.1)), vec!["lumineux", "euphorique"]);
        assert_eq!(deduire(None, Some(174.0), Some(0.65), None, Some(0.5)), vec!["lumineux", "euphorique"]);
        assert_eq!(deduire(Some("mineur"), Some(122.0), Some(0.6), Some(0.75), Some(0.2)), vec!["hypnotique", "nocturne"]);
        assert_eq!(deduire(None, None, None, None, None), Vec::<&str>::new());
        assert_eq!(feminin("lumineux"), "lumineuse");
    }
}
