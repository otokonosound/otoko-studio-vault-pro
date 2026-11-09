//! Mes textes : les modèles et les phrases de l'artiste, pour que les publications sonnent comme lui. Tout reste
//! hors ligne. Un modèle est un texte complet avec des champs (`{titre}`, `{bpm}`…) ; une phrase (accroche, corps,
//! appel) passe avant celles du moteur. Une ligne dont un champ est inconnu disparaît : un modèle peut donc prévoir
//! « BPM : {bpm} » sans risque, et il garde la casse et la ponctuation telles qu'elles ont été écrites.

use std::collections::HashSet;

use rusqlite::{Connection, params};

use super::pyrand::PyRandom;
use super::textes::{self, Brand, Ctx, Generated, Track};

/// Les genres de texte : (clé, libellé).
pub const KINDS: [(&str, &str); 4] = [("modele", "Modèle"), ("accroche", "Accroche"), ("corps", "Corps"), ("appel", "Appel")];

/// Les champs qu'un texte peut citer : (champ, ce qu'il donne).
pub const FIELDS: [(&str, &str); 16] = [
    ("titre", "le titre"),
    ("artiste", "ton nom d'artiste"),
    ("genre", "le genre"),
    ("bpm", "le tempo"),
    ("tonalite", "la tonalité"),
    ("duree", "la durée"),
    ("ambiance", "l'ambiance"),
    ("sortie", "le nom de la sortie"),
    ("date", "la date de sortie"),
    ("lien", "le lien du morceau"),
    ("ville", "ta ville"),
    ("signature", "ta signature"),
    ("hashtags", "les hashtags"),
    ("accroche", "une de tes accroches"),
    ("corps", "une de tes phrases"),
    ("appel", "un de tes appels"),
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mine {
    pub id: i64,
    /// « modele », « accroche », « corps » ou « appel ».
    pub kind: String,
    /// Vide : toutes les plateformes.
    pub platform: String,
    pub name: String,
    pub body: String,
}

pub fn list(conn: &Connection) -> Vec<Mine> {
    conn.prepare("SELECT id, kind, platform, name, body FROM text_models ORDER BY CASE kind WHEN 'modele' THEN 0 WHEN 'accroche' THEN 1 WHEN 'corps' THEN 2 ELSE 3 END, id")
        .and_then(|mut s| s.query_map([], |r| Ok(Mine { id: r.get(0)?, kind: r.get(1)?, platform: r.get(2)?, name: r.get(3)?, body: r.get(4)? }))?.collect())
        .unwrap_or_default()
}

pub fn add(conn: &Connection, m: &Mine) -> rusqlite::Result<i64> {
    conn.execute("INSERT INTO text_models (kind, platform, name, body) VALUES (?1, ?2, ?3, ?4)", params![m.kind, m.platform, m.name, m.body])?;
    Ok(conn.last_insert_rowid())
}

pub fn save(conn: &Connection, m: &Mine) -> rusqlite::Result<()> {
    conn.execute("UPDATE text_models SET kind = ?2, platform = ?3, name = ?4, body = ?5 WHERE id = ?1", params![m.id, m.kind, m.platform, m.name, m.body])?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM text_models WHERE id = ?1", [id])?;
    Ok(())
}

/// Les modèles valables sur cette plateforme.
pub fn models(conn: &Connection, platform: &str) -> Vec<Mine> {
    list(conn).into_iter().filter(|m| m.kind == "modele" && (m.platform.is_empty() || m.platform == platform)).collect()
}

/// Les phrases de l'artiste pour cette plateforme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Phrases {
    pub accroches: Vec<(i64, String)>,
    pub corps: Vec<(i64, String)>,
    pub appels: Vec<(i64, String)>,
}

pub fn phrases(conn: &Connection, platform: &str) -> Phrases {
    let mut p = Phrases::default();
    for m in list(conn).into_iter().filter(|m| m.platform.is_empty() || m.platform == platform) {
        let item = (m.id, m.body.trim().to_string());
        if item.1.is_empty() {
            continue;
        }
        match m.kind.as_str() {
            "accroche" => p.accroches.push(item),
            "corps" => p.corps.push(item),
            "appel" => p.appels.push(item),
            _ => {}
        }
    }
    p
}

/// La valeur d'un champ de l'artiste : `{titre}` est le titre nu, sans guillemets, pour que le texte reste le sien.
pub fn field(ctx: &Ctx, key: &str) -> String {
    match key {
        "titre" => ctx.get("titre_brut").to_string(),
        "ambiance" => ctx.get("amb_m").to_string(),
        k => ctx.get(k).to_string(),
    }
}

/// Les champs cités dans un texte, dans l'ordre.
pub fn fields_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        match rest[i..].find('}') {
            Some(j) => {
                out.push(rest[i + 1..i + j].trim().to_string());
                rest = &rest[i + j + 1..];
            }
            None => break,
        }
    }
    out
}

/// Remplace chaque champ d'une ligne ; `None` si l'un d'eux est inconnu (la ligne disparaît).
fn fill_line(line: &str, value: &dyn Fn(&str) -> String) -> Option<String> {
    let mut out = String::with_capacity(line.len() + 16);
    let mut rest = line;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        match rest[i..].find('}') {
            Some(j) => {
                let v = value(rest[i + 1..i + j].trim());
                if v.is_empty() {
                    return None;
                }
                out.push_str(&v);
                rest = &rest[i + j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    Some(out)
}

/// Une phrase de l'artiste remplie, s'il en a une dont tous les champs sont connus. Celles utilisées récemment
/// attendent leur tour, comme dans le moteur.
pub fn pick(rng: &mut PyRandom, list: &[(i64, String)], ctx: &Ctx, recents: &HashSet<String>) -> Option<(String, String)> {
    let possible: Vec<(String, String)> = list.iter().filter_map(|(id, t)| fill_line(t, &|k| field(ctx, k)).map(|text| (format!("perso{id}"), text))).collect();
    if possible.is_empty() {
        return None;
    }
    let fresh: Vec<&(String, String)> = possible.iter().filter(|p| !recents.contains(&p.0)).collect();
    let pool: Vec<&(String, String)> = if fresh.is_empty() { possible.iter().collect() } else { fresh };
    let k = rng.randrange(pool.len());
    Some(pool[k].clone())
}

/// Ce qui ne vient ni du morceau ni de l'univers de marque.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extra {
    pub link: String,
    /// La date de sortie, écrite comme elle doit apparaître.
    pub date: String,
}

/// Le lien du morceau (sinon celui de sa sortie) et la date de sa sortie, en toutes lettres.
pub fn extra_of(conn: &Connection, track: i64) -> Extra {
    let own: String = conn.query_row("SELECT IFNULL(sc_url, '') FROM tracks WHERE id = ?1", [track], |r| r.get(0)).unwrap_or_default();
    let (link, date): (String, String) = conn
        .query_row(
            "SELECT IFNULL(r.sc_link, ''), IFNULL(r.release_date, '') FROM releases r JOIN release_tracks rt ON rt.release_id = r.id WHERE rt.track_id = ?1 ORDER BY r.id DESC LIMIT 1",
            [track],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap_or_default();
    Extra { link: if own.trim().is_empty() { link } else { own }, date: super::releases::long_date(&date) }
}

/// Écrit le texte d'un modèle pour ce morceau.
pub fn fill(model: &str, tr: &Track, platform: &str, tone: &str, brand: &Brand, extra: &Extra, variant: u32, recents: &HashSet<String>, mine: &Phrases) -> Generated {
    let ctx = textes::contexte(tr, brand);
    // Le moteur fournit les phrases qui manquent à l'artiste, et le genre et les tags de SoundCloud.
    let base = textes::generer_avec(tr, platform, tone, brand, variant, recents, mine);
    let mut rng = PyRandom::new(textes::graine_modele(tr, platform, variant) as u64);
    let tags = textes::hashtags(&ctx, brand, if platform == "story" { 4 } else { 15 }, Some(&mut rng));
    let mut used = Vec::new();
    let mut lines = Vec::new();
    let mut drawn: Vec<(String, String)> = Vec::new();
    for key in fields_in(model) {
        let list = match key.as_str() {
            "accroche" => &mine.accroches,
            "corps" => &mine.corps,
            "appel" => &mine.appels,
            _ => continue,
        };
        if drawn.iter().any(|d| d.0 == key) {
            continue;
        }
        if let Some((k, text)) = pick(&mut rng, list, &ctx, recents) {
            used.push(k);
            drawn.push((key, text));
        }
    }
    let value = |k: &str| -> String {
        if let Some(d) = drawn.iter().find(|d| d.0 == k) {
            return d.1.clone();
        }
        match k {
            "lien" => extra.link.trim().to_string(),
            "date" => extra.date.clone(),
            "signature" => brand.signature.trim().to_string(),
            "hashtags" => tags.join(" "),
            "accroche" | "corps" | "appel" => String::new(),
            k => field(&ctx, k),
        }
    };
    for line in model.lines() {
        if let Some(l) = fill_line(line, &value) {
            lines.push(l);
        }
    }
    let mut text = lines.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    let text = text.trim().to_string();
    Generated { corps: text.clone(), texte: text, hashtags: tags, tags_sc: base.tags_sc, genre_sc: base.genre_sc, cles: used }
}

/// Fait d'un texte écrit pour un morceau un modèle réutilisable : ce qui appartient au morceau devient un champ.
pub fn to_model(text: &str, tr: &Track, brand: &Brand, link: &str) -> String {
    let ctx = textes::contexte(tr, brand);
    let mut pairs: Vec<(String, &str)> = vec![
        (link.trim().to_string(), "{lien}"),
        (ctx.get("titre_brut").to_string(), "{titre}"),
        (ctx.get("artiste").to_string(), "{artiste}"),
        (tr.genre.trim().to_string(), "{genre}"),
        (ctx.get("tonalite").to_string(), "{tonalite}"),
        (ctx.get("duree").to_string(), "{duree}"),
        (ctx.get("ville").to_string(), "{ville}"),
    ];
    let bpm = ctx.get("bpm");
    if !bpm.is_empty() {
        pairs.push((format!("{bpm} BPM"), "{bpm} BPM"));
    }
    // Du plus long au plus court : « Otoko No Sound » passe avant « Otoko ».
    pairs.sort_by_key(|p| std::cmp::Reverse(p.0.chars().count()));
    let mut out = text.to_string();
    for (from, to) in pairs {
        if from.chars().count() >= 2 && from != "l'artiste" {
            out = out.replace(&from, to);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track {
            id: 7,
            path: "a.wav".into(),
            title: "I'm Alive".into(),
            filename: "I'm Alive.wav".into(),
            genre: "UK Garage".into(),
            bpm: Some(138.0),
            key: "Am".into(),
            duration: Some(252.0),
            nature: "original".into(),
            meta: serde_json::json!({}),
            ..Default::default()
        }
    }

    fn brand() -> Brand {
        Brand { artist_name: "Otoko No Sound".into(), city: "Marseille".into(), signature: "— O.".into(), ..Default::default() }
    }

    #[test]
    fn a_model_keeps_my_words_and_drops_unknown_lines() {
        let m = "nouveau son : {titre}\n{bpm} bpm, {tonalite}\nambiance : {ambiance}\n\n{lien}\nsorti le {date}\n{signature}";
        let g = fill(m, &track(), "instagram", "sobre", &brand(), &Extra { link: "https://x.y/z".into(), date: "4 décembre".into() }, 0, &HashSet::new(), &Phrases::default());
        assert_eq!(g.texte, "nouveau son : I'm Alive\n138 bpm, la mineur\n\nhttps://x.y/z\nsorti le 4 décembre\n— O.");
    }

    #[test]
    fn my_phrases_come_first() {
        let mine = Phrases { accroches: vec![(1, "{titre} sort de l'atelier.".into())], corps: vec![(2, "Écrit à {ville}.".into()), (3, "Un {inconnu}.".into())], appels: vec![(4, "Dis-moi ce que t'en penses.".into())] };
        let g = textes::generer_avec(&track(), "soundcloud", "sobre", &brand(), 0, &HashSet::new(), &mine);
        assert!(g.texte.starts_with("I'm Alive sort de l'atelier."), "{}", g.texte);
        assert!(g.texte.contains("Écrit à Marseille."), "{}", g.texte);
        assert!(g.texte.contains("Dis-moi ce que t'en penses."), "{}", g.texte);
        assert!(g.cles.iter().any(|k| k == "perso1"));
        // Une phrase déjà utilisée attend son tour.
        let recents: HashSet<String> = ["perso1".to_string()].into_iter().collect();
        let mine2 = Phrases { accroches: vec![(1, "Une.".into()), (5, "Deux.".into())], ..Default::default() };
        let g = textes::generer_avec(&track(), "instagram", "sobre", &brand(), 0, &recents, &mine2);
        assert!(g.texte.starts_with("Deux."), "{}", g.texte);
    }

    #[test]
    fn a_model_draws_my_phrases() {
        let mine = Phrases { accroches: vec![(1, "{titre}, enfin.".into())], ..Default::default() };
        let g = fill("{accroche}\n{corps}\nfin", &track(), "story", "sobre", &brand(), &Extra::default(), 0, &HashSet::new(), &mine);
        assert_eq!(g.texte, "I'm Alive, enfin.\nfin");
        assert_eq!(g.cles, vec!["perso1"]);
    }

    #[test]
    fn a_written_text_becomes_a_model() {
        let t = "I'm Alive par Otoko No Sound, 138 BPM, UK Garage, depuis Marseille. https://s.c/a";
        assert_eq!(to_model(t, &track(), &brand(), "https://s.c/a"), "{titre} par {artiste}, {bpm} BPM, {genre}, depuis {ville}. {lien}");
    }

    #[test]
    fn fields_are_listed_in_order() {
        assert_eq!(fields_in("a {titre} b {bpm} {"), vec!["titre", "bpm"]);
    }

    #[test]
    fn the_store_round_trips() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        let id = add(&conn, &Mine { kind: "modele".into(), platform: "soundcloud".into(), name: "Sobre".into(), body: "{titre}".into(), ..Default::default() }).unwrap();
        add(&conn, &Mine { kind: "appel".into(), body: "Écoute.".into(), ..Default::default() }).unwrap();
        assert_eq!(models(&conn, "soundcloud").len(), 1);
        assert_eq!(models(&conn, "instagram").len(), 0);
        assert_eq!(phrases(&conn, "instagram").appels.len(), 1);
        save(&conn, &Mine { id, kind: "modele".into(), platform: String::new(), name: "Sobre".into(), body: "x".into() }).unwrap();
        assert_eq!(models(&conn, "instagram")[0].body, "x");
        delete(&conn, id).unwrap();
        assert_eq!(list(&conn).len(), 1);
    }
}
