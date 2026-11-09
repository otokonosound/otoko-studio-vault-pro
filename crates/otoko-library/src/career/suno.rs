//! Atelier Suno v6 (comme `models/prompts.py`, `data/suno_caps.py` et le composeur de Studio
//! Vault) : un prompt en six cases (genre, tempo, instruments, voix, production, ambiance), le champ
//! « Exclude », les paroles et leurs tags de section, les réglages (étrangeté, influence du style
//! et de l'audio), le vérificateur des pièges de Suno avec correction en un clic, les versions.
//! Les connaissances sur Suno viennent de `suno_capabilities.json` (le même fichier que Studio
//! Vault ; une copie posée à côté des données d'OTOKO le remplace, pour suivre Suno sans mise à jour).

use std::sync::OnceLock;

use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value as Json, json};

pub const EMBEDDED_CAPS: &str = include_str!("suno_capabilities.json");
pub const SLOTS: [&str; 6] = ["genre", "tempo", "instruments", "vocals", "production", "mood"];

static CAPS: OnceLock<Json> = OnceLock::new();

/// Les connaissances sur Suno : `override_path` s'il existe et se lit, sinon celles d'OTOKO.
pub fn init_caps(override_path: Option<&std::path::Path>) {
    let from_file = override_path.and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str::<Json>(&s).ok());
    let _ = CAPS.set(from_file.unwrap_or_else(|| serde_json::from_str(EMBEDDED_CAPS).unwrap_or(json!({}))));
}

pub fn caps() -> &'static Json {
    CAPS.get_or_init(|| serde_json::from_str(EMBEDDED_CAPS).unwrap_or(json!({})))
}

fn strs(v: &Json) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect()
}

pub fn caps_list(key: &str) -> Vec<String> {
    strs(&caps()[key])
}

/// Les familles de genres, dans l'ordre du fichier (serde_json trie les clés : l'ordre est relu
/// dans le texte du fichier embarqué).
pub fn genre_seeds() -> Vec<(String, Vec<String>)> {
    let seeds = &caps()["genre_seeds"];
    let Some(map) = seeds.as_object() else { return Vec::new() };
    let mut order: Vec<String> = Vec::new();
    if let Some(i) = EMBEDDED_CAPS.find("\"genre_seeds\"") {
        let re = Regex::new(r#""([^"]+)"\s*:\s*\["#).unwrap();
        let body = &EMBEDDED_CAPS[i + 13..];
        let end = body.find('}').unwrap_or(body.len());
        order = re.captures_iter(&body[..end]).map(|c| c[1].to_string()).filter(|k| map.contains_key(k)).collect();
    }
    for k in map.keys() {
        if !order.contains(k) {
            order.push(k.clone());
        }
    }
    order.into_iter().map(|k| { let v = strs(&seeds[&k]); (k, v) }).collect()
}

/// Les modèles proposés (les retirés seulement si le prompt en utilise un) : (id, libellé, note).
pub fn models(current: &str) -> Vec<(String, String, String)> {
    caps()["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| !m["retired"].as_bool().unwrap_or(false) || m["id"] == current)
        .map(|m| (m["id"].as_str().unwrap_or("").into(), m["label"].as_str().unwrap_or("").into(), m["notes"].as_str().unwrap_or("").into()))
        .collect()
}

/// Les six cases : (clé, libellé, exemple, conseil).
pub fn slots() -> Vec<(String, String, String, String)> {
    caps()["style_field"]["slots"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| (s["key"].as_str().unwrap_or("").into(), s["label"].as_str().unwrap_or("").into(), s["placeholder"].as_str().unwrap_or("").into(), s["hint"].as_str().unwrap_or("").into()))
        .collect()
}

fn limit(path: &[&str], default: i64) -> i64 {
    let mut v = caps();
    for p in path {
        v = &v[*p];
    }
    v.as_i64().unwrap_or(default)
}

pub fn style_max() -> usize {
    limit(&["style_field", "max_chars"], 200) as usize
}
pub fn descriptors_target() -> (usize, usize) {
    (limit(&["style_field", "descriptor_target_min"], 4) as usize, limit(&["style_field", "descriptor_target_max"], 7) as usize)
}
pub fn lyrics_max() -> usize {
    limit(&["lyrics_limits", "max_chars"], 3000) as usize
}

// ---------------------------------------------------------------------- le prompt

#[derive(Clone, Debug, PartialEq)]
pub struct SunoPrompt {
    pub id: i64,
    pub name: String,
    pub model: String,
    pub custom_model: String,
    pub voice: String,
    pub duration_target_s: i64,
    pub language: String,
    pub genre: String,
    pub tempo: String,
    pub instruments: String,
    pub vocals: String,
    pub production: String,
    pub mood: String,
    pub exclude_field: String,
    pub hook_instrument: String,
    pub lyrics: String,
    pub notes: String,
    pub weirdness: i64,
    pub style_influence: i64,
    /// Aucune si on n'envoie pas d'audio de référence.
    pub audio_influence: Option<i64>,
    pub reference_audio_path: String,
    pub section_edit: String,
    pub parent_id: Option<i64>,
    pub use_count: i64,
    pub last_used: String,
}

impl Default for SunoPrompt {
    fn default() -> Self {
        SunoPrompt {
            id: 0,
            name: String::new(),
            model: "v6".into(),
            custom_model: String::new(),
            voice: String::new(),
            duration_target_s: 240,
            language: "en".into(),
            genre: String::new(),
            tempo: String::new(),
            instruments: String::new(),
            vocals: String::new(),
            production: String::new(),
            mood: String::new(),
            exclude_field: String::new(),
            hook_instrument: String::new(),
            lyrics: String::new(),
            notes: String::new(),
            weirdness: 40,
            style_influence: 70,
            audio_influence: None,
            reference_audio_path: String::new(),
            section_edit: String::new(),
            parent_id: None,
            use_count: 0,
            last_used: String::new(),
        }
    }
}

impl SunoPrompt {
    pub fn slot(&self, k: &str) -> &str {
        match k {
            "genre" => &self.genre,
            "tempo" => &self.tempo,
            "instruments" => &self.instruments,
            "vocals" => &self.vocals,
            "production" => &self.production,
            "mood" => &self.mood,
            "exclude_field" => &self.exclude_field,
            "hook_instrument" => &self.hook_instrument,
            "lyrics" => &self.lyrics,
            "notes" => &self.notes,
            "name" => &self.name,
            "custom_model" => &self.custom_model,
            "voice" => &self.voice,
            "section_edit" => &self.section_edit,
            "model" => &self.model,
            _ => "",
        }
    }

    pub fn set(&mut self, k: &str, v: &str) {
        let v = v.to_string();
        match k {
            "genre" => self.genre = v,
            "tempo" => self.tempo = v,
            "instruments" => self.instruments = v,
            "vocals" => self.vocals = v,
            "production" => self.production = v,
            "mood" => self.mood = v,
            "exclude_field" => self.exclude_field = v,
            "hook_instrument" => self.hook_instrument = v,
            "lyrics" => self.lyrics = v,
            "notes" => self.notes = v,
            "name" => self.name = v,
            "custom_model" => self.custom_model = v,
            "voice" => self.voice = v,
            "section_edit" => self.section_edit = v,
            "model" => self.model = v,
            _ => {}
        }
    }

    /// Les réglages et les cases, comme Studio Vault les range dans `custom_extra`.
    fn slots_json(&self) -> Json {
        json!({
            "model": self.model, "custom_model": self.custom_model, "voice": self.voice,
            "duration_target_s": self.duration_target_s, "language": self.language,
            "genre": self.genre, "tempo": self.tempo, "instruments": self.instruments, "vocals": self.vocals,
            "production": self.production, "mood": self.mood,
            "exclude_field": self.exclude_field, "hook_instrument": self.hook_instrument,
            "weirdness": self.weirdness, "style_influence": self.style_influence, "audio_influence": self.audio_influence,
            "reference_audio_path": self.reference_audio_path, "section_edit": self.section_edit,
        })
    }

    fn apply_json(&mut self, j: &Json) {
        let s = |k: &str| j[k].as_str().map(str::to_string);
        for k in ["model", "custom_model", "voice", "language", "genre", "tempo", "instruments", "vocals", "production", "mood", "exclude_field", "hook_instrument", "section_edit"] {
            if let Some(v) = s(k) {
                if k == "language" {
                    self.language = v;
                } else {
                    self.set(k, &v);
                }
            }
        }
        if let Some(v) = s("reference_audio_path") {
            self.reference_audio_path = v;
        }
        if let Some(v) = j["duration_target_s"].as_i64() {
            self.duration_target_s = v;
        }
        if let Some(v) = j["weirdness"].as_i64() {
            self.weirdness = v;
        }
        if let Some(v) = j["style_influence"].as_i64() {
            self.style_influence = v;
        }
        if j.get("audio_influence").is_some() {
            self.audio_influence = j["audio_influence"].as_i64();
        }
    }
}

/// Le champ Style : les six cases dans l'ordre, séparées par des virgules.
pub fn assemble_style_field(p: &SunoPrompt) -> String {
    SLOTS.iter().map(|k| p.slot(k).trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
}

pub fn count_descriptors(style: &str) -> usize {
    style.split(',').filter(|p| !p.trim().is_empty()).count()
}

/// Une liste JSON (« ["a","b"] ») ou un texte, en « a, b ».
fn plain_list(s: &str) -> String {
    match serde_json::from_str::<Json>(s) {
        Ok(Json::Array(a)) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "),
        _ => s.to_string(),
    }
}

fn from_row(r: &rusqlite::Row) -> rusqlite::Result<SunoPrompt> {
    let mut p = SunoPrompt {
        id: r.get(0)?,
        name: r.get(1)?,
        lyrics: r.get(2)?,
        notes: r.get(3)?,
        parent_id: r.get(4)?,
        use_count: r.get(6)?,
        last_used: r.get(7)?,
        ..Default::default()
    };
    // Les anciens prompts rangeaient les ingrédients dans des colonnes : repris si `custom_extra` ne les a pas.
    let cols: [(&str, String); 7] = [
        ("genre", plain_list(&r.get::<_, String>(8)?)),
        ("mood", plain_list(&r.get::<_, String>(9)?)),
        ("instruments", plain_list(&r.get::<_, String>(10)?)),
        ("vocals", r.get(11)?),
        ("production", r.get(12)?),
        ("exclude_field", plain_list(&r.get::<_, String>(13)?)),
        ("tempo", r.get::<_, Option<i64>>(14)?.map(|b| format!("{b} BPM")).unwrap_or_default()),
    ];
    for (k, v) in cols {
        p.set(k, &v);
    }
    let extra: String = r.get(5)?;
    if let Ok(j) = serde_json::from_str::<Json>(&extra) {
        if j.is_object() {
            p.apply_json(&j);
        }
    }
    // Un prompt sans case remplie : le texte final fait office de genre.
    if SLOTS.iter().all(|k| p.slot(k).is_empty()) {
        let final_text: String = r.get(15)?;
        p.genre = final_text;
    }
    Ok(p)
}

const SELECT: &str = "SELECT id, IFNULL(name, ''), IFNULL(lyrics, ''), IFNULL(notes, ''), parent_id, IFNULL(custom_extra, ''), IFNULL(use_count, 0), IFNULL(last_used_at, ''),
    IFNULL(genre_tags, ''), IFNULL(mood_tags, ''), IFNULL(instrument_tags, ''), IFNULL(vocals, ''), IFNULL(production, ''), IFNULL(anti_tags, ''), bpm_target, IFNULL(prompt_final, '')
    FROM prompts";

/// Les prompts, du dernier utilisé au plus ancien (`search` : dans le nom ou le texte final).
pub fn list(conn: &Connection, search: &str) -> Vec<SunoPrompt> {
    let like = format!("%{}%", search.trim());
    conn.prepare(&format!("{SELECT} WHERE (?1 = '%%' OR name LIKE ?1 OR prompt_final LIKE ?1) ORDER BY (last_used_at IS NULL), last_used_at DESC, modified_at DESC"))
        .and_then(|mut s| s.query_map([like], from_row)?.collect())
        .unwrap_or_default()
}

pub fn get(conn: &Connection, id: i64) -> Option<SunoPrompt> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id], from_row).optional().ok().flatten()
}

fn save_version(conn: &Connection, id: i64, p: &SunoPrompt, style: &str) -> rusqlite::Result<()> {
    let n: i64 = conn.query_row("SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_id = ?1", [id], |r| r.get(0))?;
    let snapshot = json!({"name": p.name, "style_final": style, "lyrics": p.lyrics, "notes": p.notes, "extras": p.slots_json()});
    conn.execute("INSERT INTO prompt_versions (prompt_id, version_no, snapshot, prompt_final) VALUES (?1, ?2, ?3, ?4)", params![id, n, snapshot.to_string(), style])?;
    Ok(())
}

/// Enregistre (nouveau si `id` vaut 0), garde une version ; rend l'identifiant.
pub fn save(conn: &Connection, p: &SunoPrompt) -> rusqlite::Result<i64> {
    let style = assemble_style_field(p);
    let name = if p.name.trim().is_empty() { "Sans nom" } else { p.name.trim() };
    let id = if p.id == 0 {
        conn.execute(
            "INSERT INTO prompts (name, prompt_final, lyrics, notes, parent_id, custom_extra, created_at, modified_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'), datetime('now'))",
            params![name, style, p.lyrics, p.notes, p.parent_id, p.slots_json().to_string()],
        )?;
        conn.last_insert_rowid()
    } else {
        conn.execute(
            "UPDATE prompts SET name = ?2, prompt_final = ?3, lyrics = ?4, notes = ?5, custom_extra = ?6, modified_at = datetime('now') WHERE id = ?1",
            params![p.id, name, style, p.lyrics, p.notes, p.slots_json().to_string()],
        )?;
        p.id
    };
    save_version(conn, id, p, &style)?;
    Ok(id)
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM prompt_versions WHERE prompt_id = ?1", [id])?;
    conn.execute("DELETE FROM prompts WHERE id = ?1", [id])?;
    Ok(())
}

/// Une variation : une copie rattachée à son modèle.
pub fn fork(conn: &Connection, id: i64, name: &str) -> rusqlite::Result<i64> {
    let Some(mut p) = get(conn, id) else { return Ok(0) };
    p.name = if name.trim().is_empty() { format!("{} (copie)", p.name) } else { name.trim().to_string() };
    p.parent_id = Some(id);
    p.id = 0;
    save(conn, &p)
}

pub fn mark_used(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE prompts SET use_count = IFNULL(use_count, 0) + 1, last_used_at = datetime('now') WHERE id = ?1", [id])?;
    Ok(())
}

/// Les versions : (numéro, date, champ Style).
pub fn versions(conn: &Connection, id: i64) -> Vec<(i64, String, String)> {
    conn.prepare("SELECT version_no, IFNULL(created_at, ''), IFNULL(prompt_final, '') FROM prompt_versions WHERE prompt_id = ?1 ORDER BY version_no DESC")
        .and_then(|mut s| s.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
        .unwrap_or_default()
}

/// Les morceaux tirés de ce prompt : (total, gardés, rejetés, neutres).
pub fn stats(conn: &Connection, id: i64) -> (i64, i64, i64, i64) {
    let rows: Vec<(i64, i64)> = conn
        .prepare("SELECT IFNULL(pick, 0), COUNT(*) FROM tracks WHERE prompt_id = ?1 GROUP BY pick")
        .and_then(|mut s| s.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .unwrap_or_default();
    let mut out = (0, 0, 0, 0);
    for (pick, c) in rows {
        out.0 += c;
        match pick {
            1 => out.1 += c,
            -1 => out.2 += c,
            _ => out.3 += c,
        }
    }
    out
}

/// Tout le prompt à coller (Style, paroles, à exclure, réglages).
pub fn copy_all(p: &SunoPrompt) -> String {
    let mut b = format!(
        "=== STYLE ===\n{}\n\n=== PAROLES ===\n{}\n\n=== À EXCLURE ===\n{}\n\n=== RÉGLAGES ===\nModèle               : {}\nModèle perso         : {}\nVoix                 : {}\nDurée                : {} s\nÉtrangeté            : {}\nInfluence du style   : {}\n",
        assemble_style_field(p),
        p.lyrics,
        if p.exclude_field.is_empty() { "(rien)" } else { &p.exclude_field },
        p.model,
        if p.custom_model.is_empty() { "(aucun)" } else { &p.custom_model },
        if p.voice.is_empty() { "(aucune)" } else { &p.voice },
        p.duration_target_s,
        p.weirdness,
        p.style_influence
    );
    if let Some(a) = p.audio_influence {
        b.push_str(&format!("Influence de l'audio : {a}\n"));
    }
    if !p.section_edit.is_empty() {
        b.push_str(&format!("\n=== MODIFIER UNE SECTION ===\n{}\n", p.section_edit));
    }
    b
}

/// Les sections des paroles (« [Chorus] »…).
pub fn sections(lyrics: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^\s*\[([^\]]+)\]").unwrap()).captures_iter(lyrics).map(|c| c[1].to_string()).collect()
}

pub const VERSION_ANCHOR: &str = "[Version Anchor] Same melody, same vocals, same paroles as previous génération.\n[This Version] Acoustic reimagining. Remove drums and synths.\nJust fingerpicked guitar, upright bass, single female vocal.\nSlower tempo, more intimate.\n\n";

// ---------------------------------------------------------------------- vérificateur

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub id: String,
    pub message: String,
    pub fixable: bool,
}

const SECTION_WORDS: [&str; 17] =
    ["intro", "verse", "pre-chorus", "chorus", "post-chorus", "bridge", "outro", "hook", "drop", "breakdown", "build up", "interlude", "instrumental", "break", "couplet", "refrain", "pont"];

fn ci(pat: &str) -> Option<Regex> {
    Regex::new(&format!("(?i){pat}")).ok()
}

fn bad_tag(line: &str) -> Option<String> {
    static PAREN: OnceLock<Regex> = OnceLock::new();
    static COLON: OnceLock<Regex> = OnceLock::new();
    let s = line.trim();
    let paren = PAREN.get_or_init(|| Regex::new(r"^\((.+?)\)$").unwrap());
    let colon = COLON.get_or_init(|| Regex::new(r"^([A-Za-zéèÉ\- ]+\d?)\s*:$").unwrap());
    let inner = paren.captures(s).or_else(|| colon.captures(s)).map(|c| c[1].to_string())?;
    let low = inner.to_lowercase();
    SECTION_WORDS.iter().any(|w| low.contains(w)).then_some(inner)
}

/// Les pièges détectés dans un prompt.
pub fn verifier(p: &SunoPrompt) -> Vec<Issue> {
    let mut issues = Vec::new();
    let style = assemble_style_field(p);
    let c = caps();
    for (i, entry) in c["silent_fail_patterns"].as_array().into_iter().flatten().enumerate() {
        if let Some(re) = entry["pattern"].as_str().and_then(ci) {
            if re.is_match(&style) {
                issues.push(Issue { id: format!("pattern:{i}"), message: entry["message"].as_str().unwrap_or("").into(), fixable: entry.get("fix").is_some_and(|f| !f.is_null()) });
            }
        }
    }
    let low = format!("{} {}", style.to_lowercase(), p.lyrics.to_lowercase());
    for (j, ctr) in c["contradictions"].as_array().into_iter().flatten().enumerate() {
        let any = |k: &str| strs(&ctr[k]).iter().any(|a| low.contains(a.as_str()));
        if any("a") && any("b") {
            issues.push(Issue { id: format!("contra:{j}"), message: ctr["message"].as_str().unwrap_or("").into(), fixable: false });
        }
    }
    let n = count_descriptors(&style);
    let (tmin, tmax) = descriptors_target();
    if !style.is_empty() && n < tmin {
        issues.push(Issue { id: "maigre".into(), message: format!("Style trop maigre ({n} descripteurs) : Suno comblera les vides avec des choix génériques."), fixable: false });
    }
    if n > tmax {
        issues.push(Issue { id: "surcharge".into(), message: format!("Style surchargé ({n} descripteurs) : les consignes vont se contredire. Garde les {tmax} plus importantes."), fixable: true });
    }
    let cap = style_max();
    if style.chars().count() > cap {
        issues.push(Issue { id: "trop_long".into(), message: format!("Le Style dépasse {cap} caractères : la fin risque d'être ignorée."), fixable: true });
    }
    let cap_l = lyrics_max();
    if p.lyrics.chars().count() > cap_l {
        issues.push(Issue { id: "paroles_longues".into(), message: format!("Paroles au-delà de {cap_l} caractères : Suno perd le fil sur la fin."), fixable: false });
    }
    if p.lyrics.matches('[').count() != p.lyrics.matches(']').count() {
        issues.push(Issue { id: "crochets".into(), message: "Crochets déséquilibrés dans les paroles : un tag de section est mal fermé.".into(), fixable: false });
    }
    if p.lyrics.lines().any(|l| bad_tag(l).is_some()) {
        issues.push(Issue { id: "tags_paroles".into(), message: "Des sections sont écrites « (Chorus) » ou « Chorus: » : Suno attend « [Chorus] ».".into(), fixable: true });
    }
    issues
}

fn bpm_word(bpm: i64) -> &'static str {
    match bpm {
        b if b < 90 => "slow",
        b if b < 115 => "mid-tempo",
        b if b < 135 => "upbeat",
        _ => "fast",
    }
}

fn clean_commas(v: &str) -> String {
    v.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
}

/// Applique la correction d'un piège ; rend le prompt corrigé.
pub fn corriger(p: &SunoPrompt, issue: &str) -> SunoPrompt {
    let mut d = p.clone();
    let c = caps();
    if let Some(i) = issue.strip_prefix("pattern:").and_then(|i| i.parse::<usize>().ok()) {
        let Some(entry) = c["silent_fail_patterns"].as_array().and_then(|a| a.get(i)) else { return d };
        let fix = &entry["fix"];
        for k in SLOTS {
            let mut v = d.slot(k).to_string();
            if v.is_empty() {
                continue;
            }
            match fix["type"].as_str().unwrap_or("") {
                "remove_phrase" => {
                    if let Some(re) = fix["regex"].as_str().and_then(ci) {
                        v = re.replace_all(&v, "").into_owned();
                    }
                }
                "replace" => {
                    if let Some(map) = fix["map"].as_object() {
                        for (src, dst) in map {
                            if let Some(re) = ci(&format!(r"\b{}\b", regex::escape(src))) {
                                v = re.replace_all(&v, regex::NoExpand(dst.as_str().unwrap_or(""))).into_owned();
                            }
                        }
                    }
                }
                "bpm_words" => {
                    static BPM: OnceLock<Regex> = OnceLock::new();
                    let re = BPM.get_or_init(|| Regex::new(r"(?i)\b(\d{2,3})\s*BPM\b").unwrap());
                    v = re.replace_all(&v, |cap: &regex::Captures| bpm_word(cap[1].parse().unwrap_or(120)).to_string()).into_owned();
                }
                "move_to_exclude" => {
                    // « no drums » va dans le champ Exclude (Python : `(?=,|$)`, sans regard en avant ici).
                    static NO: OnceLock<Regex> = OnceLock::new();
                    let re = NO.get_or_init(|| Regex::new(r"(?i)\b(?:no|without)\s+([\w\- ]+?)(,|$)").unwrap());
                    let found: Vec<String> = re.captures_iter(&v).map(|cap| cap[1].trim().to_string()).filter(|f| !f.is_empty()).collect();
                    if !found.is_empty() {
                        let mut excl: Vec<String> = d.exclude_field.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                        for f in found {
                            if !excl.contains(&f) {
                                excl.push(f);
                            }
                        }
                        d.exclude_field = excl.join(", ");
                        v = re.replace_all(&v, "$2").into_owned();
                    }
                }
                _ => {}
            }
            d.set(k, &clean_commas(&v));
        }
    } else if issue == "surcharge" || issue == "trop_long" {
        let (_, tmax) = descriptors_target();
        let cap = style_max();
        let mut budget = tmax as i64;
        for k in SLOTS {
            let parts: Vec<String> = d.slot(k).split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
            let allowed = if k == "genre" { budget.max(1) } else { budget }.max(0) as usize;
            let mut keep: Vec<String> = parts.into_iter().take(allowed).collect();
            if k != "genre" && keep.len() > 2 {
                keep.truncate(2);
            }
            budget -= keep.len() as i64;
            d.set(k, &keep.join(", "));
        }
        while assemble_style_field(&d).chars().count() > cap {
            let mut cut = false;
            for k in SLOTS.iter().rev() {
                let mut parts: Vec<String> = d.slot(k).split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                if !parts.is_empty() {
                    parts.pop();
                    d.set(k, &parts.join(", "));
                    cut = true;
                    break;
                }
            }
            if !cut {
                break;
            }
        }
    } else if issue == "tags_paroles" {
        d.lyrics = d.lyrics.lines().map(|l| match bad_tag(l) { Some(inner) => format!("[{}]", inner.trim()), None => l.to_string() }).collect::<Vec<_>>().join("\n");
    }
    d
}

/// Corrige tout ce qui peut l'être (trois passes, comme Studio Vault).
pub fn fix_all(p: &SunoPrompt) -> SunoPrompt {
    let mut d = p.clone();
    for _ in 0..3 {
        let fixable: Vec<Issue> = verifier(&d).into_iter().filter(|i| i.fixable).collect();
        if fixable.is_empty() {
            break;
        }
        for i in fixable {
            d = corriger(&d, &i.id);
        }
    }
    d
}

// ---------------------------------------------------------------------- import de fichiers Suno

/// Un groupe de génération (les variations A/B d'un même prompt, importées ensemble).
pub fn generation_group() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    format!("gen_{}_{:06x}", chrono::Local::now().format("%Y%m%d_%H%M%S"), (nanos ^ std::process::id()) & 0xFF_FFFF)
}

/// Le nom de fichier d'un import (« Titre - v2.wav »), sans caractère interdit.
pub fn import_name(title: &str, version: &str, ext: &str) -> String {
    let base = if version.trim().is_empty() { title.trim().to_string() } else { format!("{} - {}", title.trim(), version.trim()) };
    let safe: String = base.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    format!("{}{ext}", safe.trim())
}

/// Après l'indexation d'un fichier importé : génération Suno, prompt, version, tags (« suno » toujours).
pub fn tag_import(conn: &Connection, track: i64, prompt_text: &str, version: &str, prompt_id: Option<i64>, group: Option<&str>, tags: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tracks SET role = 'suno_generation', suno_prompt = NULLIF(?2, ''), version_label = COALESCE(NULLIF(?3, ''), version_label), prompt_id = ?4, generation_group = ?5 WHERE id = ?1",
        params![track, prompt_text, version, prompt_id, group],
    )?;
    let mut list: Vec<String> = tags.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    if !list.iter().any(|t| t.eq_ignore_ascii_case("suno")) {
        list.push("suno".into());
    }
    for name in list {
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [&name])?;
        let tid: i64 = conn.query_row("SELECT id FROM tags WHERE name = ?1", [&name], |r| r.get(0))?;
        conn.execute("INSERT OR IGNORE INTO track_tags (track_id, tag_id) VALUES (?1, ?2)", params![track, tid])?;
    }
    if let Some(pid) = prompt_id {
        let _ = mark_used(conn, pid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt() -> SunoPrompt {
        SunoPrompt {
            name: "Techno sombre".into(),
            genre: "dark techno, industrial, sounds like Amelie Lens".into(),
            tempo: "128 BPM, driving".into(),
            instruments: "analog bass, distorted 909, metallic percs, no drums".into(),
            vocals: "female whisper".into(),
            production: "gritty, sidechain compression".into(),
            mood: "sad".into(),
            lyrics: "(Chorus)\nWe burn\nVerse 1:\nLow".into(),
            ..Default::default()
        }
    }

    #[test]
    fn caps_and_assembly() {
        assert!(models("v6").iter().any(|m| m.0 == "v6") && !models("v6").iter().any(|m| m.0 == "v5"));
        assert!(models("v5").iter().any(|m| m.0 == "v5"), "un ancien prompt garde son modèle");
        assert_eq!(slots().len(), 6);
        assert_eq!(genre_seeds().first().map(|g| g.0.as_str()), Some("electronic"), "ordre du fichier");
        assert!(!caps_list("structural_tags").is_empty());
        let p = prompt();
        assert!(assemble_style_field(&p).starts_with("dark techno, industrial, sounds like Amelie Lens, 128 BPM"));
        assert_eq!(sections("[Intro]\nla\n [Chorus: whispered]\n"), vec!["Intro", "Chorus: whispered"]);
    }

    #[test]
    fn verifier_and_fixes() {
        let p = prompt();
        let ids: Vec<String> = verifier(&p).iter().map(|i| i.id.clone()).collect();
        for want in ["pattern:0", "pattern:1", "pattern:2", "pattern:3", "pattern:4", "surcharge", "tags_paroles"] {
            assert!(ids.iter().any(|i| i == want), "{want} manque : {ids:?}");
        }
        let f = corriger(&p, "pattern:3");
        assert_eq!((f.instruments.as_str(), f.exclude_field.as_str()), ("analog bass, distorted 909, metallic percs", "drums"));
        assert_eq!(corriger(&p, "pattern:2").tempo, "upbeat, driving");
        assert_eq!(corriger(&p, "pattern:1").production, "gritty, pumping");
        assert_eq!(corriger(&p, "pattern:4").mood, "rainy evening, quiet heartbreak");
        assert_eq!(corriger(&p, "pattern:0").genre, "dark techno, industrial");
        assert_eq!(corriger(&p, "tags_paroles").lyrics, "[Chorus]\nWe burn\n[Verse 1]\nLow");
        let all = fix_all(&p);
        assert!(verifier(&all).iter().all(|i| !i.fixable), "{:?}", verifier(&all));
        assert!(count_descriptors(&assemble_style_field(&all)) <= 7);
    }

    #[test]
    fn stored_like_studio_vault() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        let mut p = prompt();
        p.audio_influence = Some(55);
        let id = save(&conn, &p).unwrap();
        let back = get(&conn, id).unwrap();
        assert_eq!((back.genre.as_str(), back.audio_influence, back.weirdness), (p.genre.as_str(), Some(55), 40));
        let extra: String = conn.query_row("SELECT custom_extra FROM prompts WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert!(extra.contains("\"style_influence\":70"));
        let f = fork(&conn, id, "").unwrap();
        assert_eq!(get(&conn, f).map(|x| (x.name, x.parent_id)), Some(("Techno sombre (copie)".into(), Some(id))));
        mark_used(&conn, f).unwrap();
        assert_eq!(list(&conn, "")[0].id, f, "le dernier utilisé d'abord");
        assert_eq!(list(&conn, "copie").len(), 1);
        assert_eq!(versions(&conn, id).len(), 1);
        delete(&conn, f).unwrap();
        assert_eq!(list(&conn, "").len(), 1);
        assert_eq!(import_name("Nuit: v2?", "v3", ".wav"), "Nuit_ v2_ - v3.wav");
    }
}
