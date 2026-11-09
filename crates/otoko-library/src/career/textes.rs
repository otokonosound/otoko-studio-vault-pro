//! Moteur de textes de publication (comme `services/textes.py` et `services/publishing.py` de
//! Studio Vault) — 100 % local, 100 % français. On rassemble ce qu'on sait du morceau, on choisit
//! des fragments dans les réserves (`textes_fr`) en ne gardant que ceux dont les informations sont
//! connues, selon le morceau, la plateforme, le ton et le numéro de variante ; les fragments
//! utilisés récemment sont évités, les mots interdits ne passent jamais. Même tirage que Python :
//! à données égales, le même texte que Studio Vault.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use rusqlite::{Connection, params};
use serde_json::Value as Json;

use super::humeur;
use super::mes_textes::{self, Phrases};
use super::nature::{self, Entry};
use super::pyrand::{PyRandom, crc32};
use super::textes_fr::{self as t, Frag};

pub const PLATEFORMES: [(&str, &str); 3] = [("soundcloud", "SoundCloud — description et tags"), ("instagram", "Instagram — publication"), ("story", "Instagram — story ou reel")];

/// L'univers de marque : (clé, libellé, exemple).
pub const BRAND_FIELDS: [(&str, &str, &str); 8] = [
    ("artist_name", "Nom d'artiste", "Ton nom d'artiste"),
    ("sc_url", "Lien SoundCloud", "https://soundcloud.com/…"),
    ("ig_handle", "Compte Instagram", "@…"),
    ("city", "Ville", "Ta ville"),
    ("signature", "Signature", "Phrase de fin, facultative"),
    ("emoji_prefs", "Émojis favoris", "🌑 🎧"),
    ("base_hashtags", "Hashtags perso", "#tonnom #…"),
    ("banned_words", "Mots interdits", "séparés par des virgules"),
];

/// Le profil d'artiste (réglages `pub_*`, comme Studio Vault).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Brand {
    pub artist_name: String,
    pub sc_url: String,
    pub ig_handle: String,
    pub default_tone: String,
    pub emoji_prefs: String,
    pub signature: String,
    pub base_hashtags: String,
    pub city: String,
    pub banned_words: String,
}

impl Brand {
    pub fn get(&self, key: &str) -> &str {
        match key {
            "artist_name" => &self.artist_name,
            "sc_url" => &self.sc_url,
            "ig_handle" => &self.ig_handle,
            "default_tone" => &self.default_tone,
            "emoji_prefs" => &self.emoji_prefs,
            "signature" => &self.signature,
            "base_hashtags" => &self.base_hashtags,
            "city" => &self.city,
            "banned_words" => &self.banned_words,
            _ => "",
        }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let v = value.to_string();
        match key {
            "artist_name" => self.artist_name = v,
            "sc_url" => self.sc_url = v,
            "ig_handle" => self.ig_handle = v,
            "default_tone" => self.default_tone = v,
            "emoji_prefs" => self.emoji_prefs = v,
            "signature" => self.signature = v,
            "base_hashtags" => self.base_hashtags = v,
            "city" => self.city = v,
            "banned_words" => self.banned_words = v,
            _ => {}
        }
    }
}

const BRAND_KEYS: [&str; 9] = ["artist_name", "sc_url", "ig_handle", "default_tone", "emoji_prefs", "signature", "base_hashtags", "city", "banned_words"];

fn setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get::<_, Option<String>>(0)).ok().flatten()
}

pub fn brand_load(conn: &Connection) -> Brand {
    let mut b = Brand { default_tone: "mysterieux".into(), ..Default::default() };
    for k in BRAND_KEYS {
        if let Some(v) = setting(conn, &format!("pub_{k}")) {
            b.set(k, &v);
        }
    }
    if b.artist_name.is_empty() {
        b.artist_name = setting(conn, "artist_alias").unwrap_or_default();
    }
    b
}

pub fn brand_save(conn: &Connection, b: &Brand) -> rusqlite::Result<()> {
    for k in BRAND_KEYS {
        conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![format!("pub_{k}"), b.get(k)])?;
    }
    Ok(())
}

pub fn brand_is_empty(conn: &Connection) -> bool {
    let b = brand_load(conn);
    b.artist_name.is_empty() && b.sc_url.is_empty() && b.city.is_empty()
}

// ---------------------------------------------------------------------- le morceau

/// Ce que le moteur sait d'un morceau (`load_track_full`) et de sa sortie (`release_of`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub filename: String,
    pub artist: String,
    pub genre: String,
    pub tags: Vec<String>,
    pub mood_auto: String,
    pub bpm: Option<f64>,
    pub key: String,
    pub duration: Option<f64>,
    pub album: String,
    pub release_type: String,
    pub nature: String,
    pub meta: Json,
    pub set_entries: Vec<Entry>,
    /// (titre, type) de la sortie qui contient le morceau.
    pub release: Option<(String, String)>,
}

pub fn load_track(conn: &Connection, id: i64) -> Option<Track> {
    let mut t = conn
        .query_row(
            "SELECT id, path, IFNULL(title, ''), IFNULL(filename, ''), IFNULL(artist, ''), IFNULL(genre, ''), IFNULL(mood_auto, ''), bpm, IFNULL(music_key, ''),
                    duration_s, IFNULL(album, ''), IFNULL(release_type, ''), IFNULL(NULLIF(nature, ''), 'original'), IFNULL(nature_meta, '{}')
             FROM tracks WHERE id = ?1",
            [id],
            |r| {
                Ok(Track {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    title: r.get(2)?,
                    filename: r.get(3)?,
                    artist: r.get(4)?,
                    genre: r.get(5)?,
                    mood_auto: r.get(6)?,
                    bpm: r.get(7)?,
                    key: r.get(8)?,
                    duration: r.get(9)?,
                    album: r.get(10)?,
                    release_type: r.get(11)?,
                    nature: r.get(12)?,
                    meta: nature::load_meta(&r.get::<_, String>(13)?),
                    ..Default::default()
                })
            },
        )
        .ok()?;
    t.tags = conn
        .prepare("SELECT g.name FROM track_tags tt JOIN tags g ON g.id = tt.tag_id WHERE tt.track_id = ?1")
        .and_then(|mut s| s.query_map([id], |r| r.get(0))?.collect())
        .unwrap_or_default();
    if t.nature == "set" {
        t.set_entries = conn
            .prepare("SELECT time_s, IFNULL(artist, ''), IFNULL(title, '') FROM set_entries WHERE set_id = ?1 ORDER BY time_s")
            .and_then(|mut s| s.query_map([id], |r| Ok(Entry { time: r.get::<_, Option<f64>>(0)?.unwrap_or(0.0), artist: r.get(1)?, title: r.get(2)?, ..Default::default() }))?.collect())
            .unwrap_or_default();
    }
    t.release = conn
        .query_row(
            "SELECT r.title, IFNULL(r.rtype, '') FROM releases r JOIN release_tracks rt ON rt.release_id = r.id WHERE rt.track_id = ?1 ORDER BY r.id DESC LIMIT 1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    Some(t)
}

/// Titre lisible : tag titre, sinon nom de fichier sans numéro ni artistes.
pub fn nettoyer_titre(title: &str, filename: &str) -> String {
    super::projects::clean_title(title, filename)
}

pub fn tonalite_fr(cle: &str) -> String {
    let cle = cle.trim();
    if cle.is_empty() {
        return String::new();
    }
    let (mineur, racine) = if let Some(r) = cle.strip_suffix("min") {
        (true, r.to_string())
    } else if let Some(r) = cle.strip_suffix('m') {
        (true, r.to_string())
    } else {
        (false, cle.replace("maj", ""))
    };
    match t::NOTES_FR.iter().find(|n| n.0 == racine) {
        Some((_, nom)) => format!("{nom} {}", if mineur { "mineur" } else { "majeur" }),
        None => String::new(),
    }
}

pub fn duree_fr(sec: Option<f64>) -> String {
    let Some(sec) = sec.filter(|s| *s != 0.0) else { return String::new() };
    let s = sec.round_ties_even() as i64;
    let (m, s) = (s.div_euclid(60), s.rem_euclid(60));
    if m == 0 {
        format!("{s} s")
    } else if s != 0 {
        format!("{m} min {s:02}")
    } else {
        format!("{m} min")
    }
}

/// Noms d'artistes invités, à partir du champ artiste (X, feat., &, avec…).
pub fn collaborateurs(brut: &str, artiste: &str) -> Vec<String> {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    if brut.is_empty() {
        return Vec::new();
    }
    let re = SPLIT.get_or_init(|| Regex::new(r"\s+(?:x|X|feat\.?|ft\.?|featuring|&|et|avec|vs\.?)\s+|,\s*").unwrap());
    let moi = artiste.trim().to_lowercase();
    let alias: Vec<String> = if moi.is_empty() { Vec::new() } else { vec![moi.clone(), moi.replace("otoko no sound", "otoko")] };
    let mut out: Vec<String> = Vec::new();
    for p in re.split(brut) {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        let pl = p.to_lowercase();
        if alias.contains(&pl) || (!moi.is_empty() && (moi.contains(&pl) || moi.starts_with(&pl))) {
            continue;
        }
        if !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}

fn liste_fr(noms: &[String]) -> String {
    match noms.len() {
        0 => String::new(),
        1 => noms[0].clone(),
        n => format!("{} et {}", noms[..n - 1].join(", "), noms[n - 1]),
    }
}

pub fn famille_genre(genre: &str, tags: &[String]) -> &'static str {
    let texte = std::iter::once(genre.to_string()).chain(tags.iter().cloned()).collect::<Vec<_>>().join(" ").to_lowercase();
    for (fam, mots, _) in t::FAMILLES {
        if mots.iter().any(|m| texte.contains(m)) {
            return fam;
        }
    }
    "electronique"
}

/// Toutes les informations exploitables, déjà mises en forme (les champs des fragments).
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    pub nature: String,
    pub meta: Json,
    pub set_entries: Vec<Entry>,
    pub ambiances: Vec<String>,
    pub famille: &'static str,
    fields: Vec<(&'static str, String)>,
}

impl Ctx {
    pub fn get(&self, k: &str) -> &str {
        self.fields.iter().find(|f| f.0 == k).map(|f| f.1.as_str()).unwrap_or("")
    }

    /// `str.format(**ctx)` : chaque `{champ}` remplacé, en un seul passage.
    fn format(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 32);
        let mut rest = text;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            match rest[i..].find('}') {
                Some(j) => {
                    out.push_str(self.get(&rest[i + 1..i + j]));
                    rest = &rest[i + j + 1..];
                }
                None => {
                    out.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }
}

pub fn contexte(tr: &Track, brand: &Brand) -> Ctx {
    let tags = &tr.tags;
    let artiste = if brand.artist_name.trim().is_empty() { "l'artiste".to_string() } else { brand.artist_name.trim().to_string() };
    let genre_brut = tr.genre.trim();
    let fam = famille_genre(genre_brut, tags);
    let genre = if genre_brut.is_empty() { t::FAMILLES.iter().find(|f| f.0 == fam).map(|f| f.2.to_string()).unwrap_or_default() } else { genre_brut.to_lowercase() };
    // Ambiance : celle choisie par l'utilisateur (tags) en priorité, puis l'automatique.
    let mut amb: Vec<String> = tags.iter().map(|t| t.to_lowercase()).filter(|t| humeur::ADJECTIFS.iter().any(|a| a.0 == t)).collect();
    for a in tr.mood_auto.split(',').map(str::trim).filter(|a| !a.is_empty()) {
        if !amb.iter().any(|x| x == a) {
            amb.push(a.to_string());
        }
    }
    let bpm = tr.bpm.map(|b| (b.round_ties_even() as i64).to_string()).unwrap_or_default();
    let (rel_title, rel_type) = tr.release.clone().unwrap_or_default();
    let rtype = if rel_type.is_empty() { tr.release_type.clone() } else { rel_type };
    let nom = if rel_title.is_empty() { tr.album.clone() } else { rel_title };
    let (sortie, de_sortie) = match t::TYPES_SORTIE.iter().find(|x| x.0 == rtype) {
        Some((_, le, du)) if !nom.is_empty() => (format!("{le} {nom}"), format!("{du} {nom}")),
        _ if !nom.is_empty() => (format!("« {nom} »"), format!("de « {nom} »")),
        _ => (String::new(), String::new()),
    };
    let collab = collaborateurs(&tr.artist, &artiste);
    let titre = nettoyer_titre(&tr.title, &tr.filename);
    let m = &tr.meta;
    let orig = match m["orig_title"].as_str().filter(|s| !s.is_empty()) {
        Some(o) => format!("« {o} »{}", m["orig_artist"].as_str().filter(|s| !s.is_empty()).map(|a| format!(" de {a}")).unwrap_or_default()),
        None => String::new(),
    };
    let fem = |i: usize| amb.get(i).map(|a| humeur::feminin(a).to_string()).unwrap_or_default();
    let fields = vec![
        ("nature", tr.nature.clone()),
        ("orig", orig),
        ("client", m["client_name"].as_str().unwrap_or("").to_string()),
        ("nb_titres", if tr.set_entries.is_empty() { String::new() } else { tr.set_entries.len().to_string() }),
        ("titre", format!("« {titre} »")),
        ("titre_brut", titre),
        ("artiste", artiste.clone()),
        ("genre", genre),
        ("famille", fam.to_string()),
        ("amb_m", amb.first().cloned().unwrap_or_default()),
        ("amb_f", fem(0)),
        ("amb2_f", fem(1)),
        ("bpm", bpm),
        ("tonalite", tonalite_fr(&tr.key)),
        ("duree", duree_fr(tr.duration)),
        ("sortie", sortie),
        ("de_sortie", de_sortie),
        ("collab", liste_fr(&collab)),
        ("avec_collab", if collab.is_empty() { String::new() } else { format!("avec {}", liste_fr(&collab)) }),
        ("ville", brand.city.trim().to_string()),
    ];
    Ctx { nature: tr.nature.clone(), meta: tr.meta.clone(), set_entries: tr.set_entries.clone(), ambiances: amb, famille: fam, fields }
}

// ---------------------------------------------------------------------- choix

fn graine(tr: &Track, plateforme: &str, ton: &str, variante: u32) -> u32 {
    let path = if tr.path.is_empty() { tr.title.clone() } else { tr.path.clone() };
    let id = if tr.id == 0 { String::new() } else { tr.id.to_string() };
    crc32(format!("{id}|{path}|{plateforme}|{ton}|{variante}").as_bytes())
}

/// La graine des champs d'un modèle (tirage des phrases, ordre des hashtags).
pub fn graine_modele(tr: &Track, plateforme: &str, variante: u32) -> u32 {
    graine(tr, plateforme, "modele", variante)
}

fn interdits(brand: &Brand) -> Vec<String> {
    let mut v: Vec<String> = t::MOTS_TOUJOURS_INTERDITS.iter().map(|s| s.to_string()).collect();
    v.extend(brand.banned_words.split([',', '\n']).map(|m| m.trim().to_lowercase()).filter(|m| !m.is_empty()));
    v
}

fn ok(texte: &str, interdits: &[String]) -> bool {
    let low = format!(" {} ", texte.to_lowercase());
    !interdits.iter().any(|m| low.contains(m.as_str()))
}

fn choisir(rng: &mut PyRandom, reserve: &[Frag], ctx: &Ctx, recents: &HashSet<String>, interdits: &[String]) -> Option<(&'static str, String)> {
    let possibles: Vec<(&'static str, String)> = reserve
        .iter()
        .filter(|(_, _, req)| req.iter().all(|r| !ctx.get(r).is_empty()))
        .map(|(k, texte, _)| (*k, ctx.format(texte)))
        .filter(|(_, rempli)| ok(rempli, interdits))
        .collect();
    if possibles.is_empty() {
        return None;
    }
    let frais: Vec<&(&'static str, String)> = possibles.iter().filter(|p| !recents.contains(p.0)).collect();
    let pool: Vec<&(&'static str, String)> = if frais.is_empty() { possibles.iter().collect() } else { frais };
    let k = rng.randrange(pool.len());
    Some(pool[k].clone())
}

fn capitalize_first(l: &str) -> String {
    let mut c = l.chars();
    match c.next() {
        Some(f) if f.is_lowercase() => f.to_uppercase().collect::<String>() + c.as_str(),
        Some(f) => f.to_string() + c.as_str(),
        None => String::new(),
    }
}

/// `str.capitalize()` de Python : la première lettre en majuscule, le reste en minuscules.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

fn finir(texte: &str) -> String {
    static SP: OnceLock<Regex> = OnceLock::new();
    static PUNCT: OnceLock<Regex> = OnceLock::new();
    static NL: OnceLock<Regex> = OnceLock::new();
    let t1 = SP.get_or_init(|| Regex::new(r"[ \t]+").unwrap()).replace_all(texte, " ");
    let t2 = PUNCT.get_or_init(|| Regex::new(r" +([.,])").unwrap()).replace_all(&t1, "$1");
    let t3 = NL.get_or_init(|| Regex::new(r"\n{3,}").unwrap()).replace_all(&t2, "\n\n");
    t3.split('\n').map(|l| capitalize_first(l.trim())).collect::<Vec<_>>().join("\n").trim().to_string()
}

/// Coupe à `limite` caractères, à la fin d'une phrase si possible.
fn couper(texte: &str, limite: usize) -> String {
    let chars: Vec<char> = texte.chars().collect();
    if chars.len() <= limite {
        return texte.to_string();
    }
    let cut: String = chars[..limite].iter().collect();
    let cutc: Vec<char> = cut.chars().collect();
    let rfind = |pat: &str| -> Option<usize> {
        let p: Vec<char> = pat.chars().collect();
        (0..=cutc.len().saturating_sub(p.len())).rev().find(|&i| cutc[i..i + p.len()] == p[..])
    };
    let fin = [". ", ".\n", "! ", "? "].iter().filter_map(|p| rfind(p)).max();
    if let Some(f) = fin.filter(|f| *f as f64 > limite as f64 * 0.6) {
        return cutc[..=f].iter().collect();
    }
    match cut.rsplit_once(' ') {
        Some((a, _)) => format!("{a}…"),
        None => format!("{cut}…"),
    }
}

// ---------------------------------------------------------------------- hashtags et tags

/// Hashtags : ceux de l'artiste, puis nature et ambiance, genre ciblé, puis larges.
pub fn hashtags(ctx: &Ctx, brand: &Brand, limite: usize, rng: Option<&mut PyRandom>) -> Vec<String> {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    let re = SPLIT.get_or_init(|| Regex::new(r"[\s,]+").unwrap());
    let perso: Vec<String> = re.split(&brand.base_hashtags).filter(|h| !h.trim_matches('#').is_empty()).map(|h| if h.starts_with('#') { h.to_string() } else { format!("#{h}") }).collect();
    let mut ambiance: Vec<String> = t::HASHTAGS_NATURE.iter().find(|n| n.0 == ctx.nature).map(|n| n.1.iter().map(|s| s.to_string()).collect()).unwrap_or_default();
    ambiance.extend(ctx.ambiances.iter().take(2).filter_map(|a| t::HASHTAGS_AMBIANCE.iter().find(|x| x.0 == a).map(|x| x.1.to_string())));
    let fam_list = t::HASHTAGS_FAMILLE.iter().find(|f| f.0 == ctx.famille).or_else(|| t::HASHTAGS_FAMILLE.iter().find(|f| f.0 == "electronique")).map(|f| f.1).unwrap_or(&[]);
    let mut famille: Vec<String> = fam_list.iter().map(|s| s.to_string()).collect();
    let mut larges: Vec<String> = t::HASHTAGS_LARGES.iter().map(|s| s.to_string()).collect();
    if let Some(r) = rng {
        r.shuffle(&mut famille);
        r.shuffle(&mut larges);
    }
    let mut out: Vec<String> = Vec::new();
    for h in perso.into_iter().chain(ambiance).chain(famille).chain(larges) {
        let hl = h.to_lowercase();
        if t::HASHTAGS_BANNIS.contains(&hl.as_str()) || out.iter().any(|x| x.to_lowercase() == hl) {
            continue;
        }
        out.push(h);
        if out.len() >= limite {
            break;
        }
    }
    out
}

pub fn tags_soundcloud(ctx: &Ctx) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    if !ctx.get("genre").is_empty() {
        tags.push(ctx.get("genre").to_string());
    }
    tags.extend(t::TAGS_SOUNDCLOUD.iter().find(|f| f.0 == ctx.famille).map(|f| f.1.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap_or_default());
    tags.extend(t::TAGS_SC_NATURE.iter().find(|f| f.0 == ctx.nature).map(|f| f.1.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap_or_default());
    tags.extend(ctx.ambiances.iter().take(2).cloned());
    if !ctx.get("ville").is_empty() {
        tags.push(ctx.get("ville").to_string());
    }
    if !ctx.get("artiste").is_empty() && ctx.get("artiste") != "l'artiste" {
        tags.push(ctx.get("artiste").to_string());
    }
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        if !t.is_empty() && !out.iter().any(|x| x.to_lowercase() == t.to_lowercase()) {
            out.push(t);
        }
    }
    out.truncate(12);
    out
}

/// Le champ « Tags » de SoundCloud : les tags à plusieurs mots entre guillemets.
pub fn format_tags_soundcloud(tags: &[String]) -> String {
    tags.iter().map(|t| if t.contains(' ') { format!("\"{t}\"") } else { t.clone() }).collect::<Vec<_>>().join(" ")
}

pub fn genre_soundcloud(famille: &str) -> &'static str {
    t::GENRE_SOUNDCLOUD.iter().find(|g| g.0 == famille).map(|g| g.1).unwrap_or("Electronic")
}

// ---------------------------------------------------------------------- génération

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Generated {
    /// Ce qu'on copie tel quel.
    pub texte: String,
    pub corps: String,
    pub hashtags: Vec<String>,
    pub tags_sc: Vec<String>,
    pub genre_sc: String,
    /// Les fragments utilisés (pour ne pas les répéter).
    pub cles: Vec<String>,
}

fn reserve<'a>(map: &'a [(&str, &'a [Frag])], k: &str) -> &'a [Frag] {
    map.iter().find(|x| x.0 == k).map(|x| x.1).unwrap_or(&[])
}

pub fn generer(tr: &Track, plateforme: &str, ton: &str, brand: &Brand, variante: u32, recents: &HashSet<String>) -> Generated {
    generer_avec(tr, plateforme, ton, brand, variante, recents, &Phrases::default())
}

/// Comme `generer`, les phrases de l'artiste passant avant celles des réserves. Sans phrases, le tirage reste celui de
/// Studio Vault.
pub fn generer_avec(tr: &Track, plateforme: &str, ton: &str, brand: &Brand, variante: u32, recents: &HashSet<String>, mes: &Phrases) -> Generated {
    let ton = if t::ACCROCHES.iter().any(|a| a.0 == ton) { ton } else { "mysterieux" };
    let ctx = contexte(tr, brand);
    let mut rng = PyRandom::new(graine(tr, plateforme, ton, variante) as u64);
    let interdits = interdits(brand);
    let mut cles: Vec<String> = Vec::new();
    let mut prendre = |rng: &mut PyRandom, res: &[Frag], perso: &[(i64, String)]| -> String {
        if let Some((k, texte)) = mes_textes::pick(rng, perso, &ctx, recents) {
            cles.push(k);
            return texte;
        }
        match choisir(rng, res, &ctx, recents, &interdits) {
            Some((k, texte)) => {
                cles.push(k.to_string());
                texte
            }
            None => String::new(),
        }
    };
    if plateforme == "story" {
        let a = prendre(&mut rng, t::ACCROCHES_STORY, &mes.accroches);
        let b = prendre(&mut rng, t::APPELS_STORY, &mes.appels);
        let texte = couper(&finir(&format!("{a} {b}")), t::LIMITE_STORY);
        let tags = hashtags(&ctx, brand, 4, Some(&mut rng));
        return Generated { corps: texte.clone(), texte, hashtags: tags, cles, ..Default::default() };
    }
    let mut accroche = String::new();
    if mes.accroches.is_empty() && t::ACCROCHES_NATURE.iter().any(|a| a.0 == ctx.nature) && rng.random() < 0.8 {
        accroche = prendre(&mut rng, reserve(t::ACCROCHES_NATURE, &ctx.nature), &[]);
    }
    if accroche.is_empty() {
        accroche = prendre(&mut rng, reserve(t::ACCROCHES, ton), &mes.accroches);
    }
    let mut corps = prendre(&mut rng, reserve(t::CORPS, ton), &mes.corps);
    // Une seconde phrase de corps, d'un ton voisin, pour varier la longueur.
    if mes.corps.is_empty() && rng.random() < 0.6 {
        let voisin = match ton {
            "mysterieux" => "intime",
            "intime" => "emotionnel",
            "emotionnel" => "intime",
            "technique" => "audacieux",
            "audacieux" => "technique",
            _ => "sobre",
        };
        let extra = prendre(&mut rng, reserve(t::CORPS, voisin), &[]);
        if !extra.is_empty() && extra != corps {
            corps = format!("{corps} {extra}");
        }
    }
    if plateforme == "soundcloud" {
        let mut details = Vec::new();
        if !ctx.get("genre").is_empty() {
            details.push(capitalize(ctx.get("genre")));
        }
        if !ctx.get("bpm").is_empty() && ctx.nature != "set" {
            details.push(format!("{} BPM", ctx.get("bpm")));
        }
        if !ctx.get("tonalite").is_empty() && ctx.nature != "set" {
            details.push(capitalize(ctx.get("tonalite")));
        }
        if !ctx.get("duree").is_empty() {
            details.push(ctx.get("duree").to_string());
        }
        let credit = if ctx.nature == "original" {
            prendre(&mut rng, if ctx.get("collab").is_empty() { t::CREDITS } else { t::CREDITS_COLLAB }, &[])
        } else {
            nature::credits_for(&ctx.nature, &ctx.meta, ctx.get("artiste"))
        };
        let appel = prendre(&mut rng, t::APPELS_SOUNDCLOUD, &mes.appels);
        let mut blocs = vec![accroche, corps];
        if !details.is_empty() {
            blocs.push(details.join(" · "));
        }
        if !ctx.get("sortie").is_empty() {
            blocs.push(format!("Extrait {}.", ctx.get("de_sortie")));
        }
        if ctx.nature == "set" && !ctx.set_entries.is_empty() {
            blocs.push(format!("Liste des titres :\n{}", nature::tracklist_text(&ctx.set_entries, tr.duration)));
        }
        blocs.push(credit);
        blocs.push(appel);
        if !brand.signature.is_empty() {
            blocs.push(brand.signature.clone());
        }
        let texte = couper(&finir(&blocs.into_iter().filter(|b| !b.is_empty()).collect::<Vec<_>>().join("\n\n")), t::LIMITE_SOUNDCLOUD);
        return Generated { corps: texte.clone(), texte, tags_sc: tags_soundcloud(&ctx), genre_sc: genre_soundcloud(ctx.famille).into(), cles, ..Default::default() };
    }
    // Instagram (publication).
    let appel = prendre(&mut rng, t::APPELS_INSTAGRAM, &mes.appels);
    let signature = if brand.signature.is_empty() { ctx.get("artiste").to_string() } else { brand.signature.clone() };
    let emoji = brand.emoji_prefs.trim();
    let tags = hashtags(&ctx, brand, 15, Some(&mut rng));
    let corps_txt = finir(&format!("{accroche}\n\n{corps}{}\n\n— {signature}\n\n{appel}", if emoji.is_empty() { String::new() } else { format!(" {emoji}") }));
    let texte = couper(&format!("{corps_txt}\n\n{}", tags.join(" ")), t::LIMITE_INSTAGRAM);
    Generated { texte, corps: corps_txt, hashtags: tags, cles, ..Default::default() }
}

/// Ce qui manque au morceau pour des textes plus précis.
pub fn manque(tr: &Track, brand: &Brand) -> Vec<&'static str> {
    let ctx = contexte(tr, brand);
    let mut m = Vec::new();
    if ctx.get("genre").is_empty() || tr.genre.is_empty() {
        m.push("genre");
    }
    if ctx.ambiances.is_empty() {
        m.push("ambiance");
    }
    if ctx.get("bpm").is_empty() {
        m.push("tempo");
    }
    m
}

// ---------------------------------------------------------------------- historique

/// Les fragments utilisés dans les `n` derniers textes enregistrés.
pub fn cles_recentes(conn: &Connection, n: usize) -> HashSet<String> {
    conn.prepare("SELECT IFNULL(used_keys, '') FROM post_drafts ORDER BY id DESC LIMIT ?1")
        .and_then(|mut s| s.query_map([n as i64], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>())
        .unwrap_or_default()
        .iter()
        .flat_map(|k| k.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>())
        .collect()
}

pub fn save_text(conn: &Connection, track: i64, platform: &str, tone: &str, body: &str, hashtags: &str, used: &[String], posted: bool) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO post_drafts (track_id, platform, tone, lang, body, hashtags, used_keys, posted_at, created_at)
         VALUES (?1, ?2, ?3, 'fr', ?4, ?5, ?6, CASE WHEN ?7 THEN datetime('now') END, datetime('now'))",
        params![track, platform, tone, body, hashtags, used.join(","), posted],
    )?;
    Ok(conn.last_insert_rowid())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HistoryRow {
    pub id: i64,
    pub track: Option<i64>,
    pub created: String,
    pub title: String,
    pub platform: String,
    pub posted: bool,
    pub body: String,
    pub hashtags: String,
}

pub fn history(conn: &Connection, track: Option<i64>, limit: usize) -> Vec<HistoryRow> {
    conn.prepare(
        "SELECT d.id, d.track_id, IFNULL(d.created_at, ''), IFNULL(t.title, ''), IFNULL(t.filename, ''), IFNULL(d.platform, ''), d.posted_at IS NOT NULL,
                IFNULL(d.body, ''), IFNULL(d.hashtags, '')
         FROM post_drafts d LEFT JOIN tracks t ON t.id = d.track_id WHERE (?1 IS NULL OR d.track_id = ?1) ORDER BY d.id DESC LIMIT ?2",
    )
    .and_then(|mut s| {
        s.query_map(params![track, limit as i64], |r| {
            let (t, f): (String, String) = (r.get(3)?, r.get(4)?);
            Ok(HistoryRow {
                id: r.get(0)?,
                track: r.get(1)?,
                created: r.get(2)?,
                title: if t.is_empty() && f.is_empty() { "—".into() } else { nettoyer_titre(&t, &f) },
                platform: r.get(5)?,
                posted: r.get(6)?,
                body: r.get(7)?,
                hashtags: r.get(8)?,
            })
        })?
        .collect()
    })
    .unwrap_or_default()
}

pub fn mark_posted(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE post_drafts SET posted_at = datetime('now') WHERE id = ?1", [id])?;
    Ok(())
}

pub fn delete_text(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM post_drafts WHERE id = ?1", [id])?;
    Ok(())
}

/// Les morceaux proposés à l'écriture (pas les prods « à ne pas publier ») : (id, libellé).
pub fn publishable(conn: &Connection) -> Vec<(i64, String)> {
    conn.prepare(
        "SELECT id, IFNULL(title, ''), IFNULL(filename, ''), IFNULL(album, ''), IFNULL(NULLIF(nature, ''), 'original'), IFNULL(nature_meta, '{}')
         FROM tracks WHERE missing = 0 AND COALESCE(archived, 0) = 0
         ORDER BY (album IS NULL OR album = ''), album COLLATE NOCASE, track_no, title COLLATE NOCASE",
    )
    .and_then(|mut s| s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?)))?.collect::<rusqlite::Result<Vec<_>>>())
    .unwrap_or_default()
    .into_iter()
    .filter(|r| !nature::is_private(&r.4, &nature::load_meta(&r.5)))
    .map(|r| (r.0, if r.3.is_empty() { nettoyer_titre(&r.1, &r.2) } else { format!("{}   —  {}", nettoyer_titre(&r.1, &r.2), r.3) }))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(tonalite_fr("F#m"), "fa dièse mineur");
        assert_eq!(tonalite_fr("Eb"), "mi bémol majeur");
        assert_eq!(tonalite_fr("H"), "");
        assert_eq!(duree_fr(Some(252.4)), "4 min 12");
        assert_eq!(duree_fr(Some(240.0)), "4 min");
        assert_eq!(duree_fr(Some(42.0)), "42 s");
        assert_eq!(collaborateurs("Otoko No Sound x Quiet Place feat. D.Cave", "Otoko No Sound"), vec!["Quiet Place", "D.Cave"]);
        assert_eq!(liste_fr(&["A".into(), "B".into(), "C".into()]), "A, B et C");
        assert_eq!(famille_genre("Melodic Techno", &[]), "techno_melodique");
        assert_eq!(famille_genre("", &["afro".into()]), "afro_house");
        assert_eq!(finir("  bonjour  le monde .\n\n\n\nsuite"), "Bonjour le monde.\n\nSuite");
        assert_eq!(couper("Une phrase. Une autre qui dépasse", 20), "Une phrase. Une…");
        assert_eq!(couper("Une phrase assez longue. Une autre", 30), "Une phrase assez longue.");
        assert_eq!(format_tags_soundcloud(&["deep house".into(), "groove".into()]), "\"deep house\" groove");
        assert_eq!(capitalize("techno MÉLODIQUE"), "Techno mélodique");
    }
}
