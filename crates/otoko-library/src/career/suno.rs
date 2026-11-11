//! Atelier Suno v6 (comme `models/prompts.py`, `data/suno_caps.py` et le composeur de Studio
//! Vault) : un prompt en six cases (genre, tempo, instruments, voix, production, ambiance), le champ
//! « Exclude », les paroles et leurs tags de section, les réglages (étrangeté, influence du style
//! et de l'audio), le vérificateur des pièges de Suno avec correction en un clic, les versions.
//! Les connaissances sur Suno viennent de `suno_capabilities.json` (le même fichier que Studio
//! Vault ; une copie posée à côté des données d'OTOKO le remplace, et le logiciel va lire sur GitHub s'il en existe
//! de plus récentes, pour suivre Suno sans nouvelle version). Les fichiers téléchargés de Suno sont repérés dans un
//! dossier surveillé ; chaque morceau garde le prompt qui l'a donné, d'où « ce qui marche ».

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime};

use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value as Json, json};

use super::scan;

pub const EMBEDDED_CAPS: &str = include_str!("suno_capabilities.json");
pub const SLOTS: [&str; 6] = ["genre", "tempo", "instruments", "vocals", "production", "mood"];

/// Le fichier posé à la main à côté des données : il gagne toujours.
pub const CAPS_FILE: &str = "suno_capabilities.json";
/// Le fichier rapporté de GitHub : il ne sert que s'il est plus récent que celui du logiciel.
pub const CAPS_FETCHED: &str = "suno_capabilities.maj.json";
/// Où le logiciel lit les connaissances à jour. Rien n'est envoyé : c'est la lecture d'un fichier public.
pub const CAPS_URL: &str = "https://raw.githubusercontent.com/otokonosound/otoko-studio-vault-pro/main/crates/otoko-library/src/career/suno_capabilities.json";

/// Les connaissances en service, et leur texte (pour l'ordre des familles de genres).
struct Caps {
    json: Json,
    text: String,
}

static CAPS: RwLock<Option<&'static Caps>> = RwLock::new(None);

fn embedded() -> Caps {
    Caps { json: serde_json::from_str(EMBEDDED_CAPS).unwrap_or(json!({})), text: EMBEDDED_CAPS.to_string() }
}

/// Met ces connaissances en service. Les anciennes restent en mémoire : des listes peuvent encore les lire.
fn put(c: Caps) -> &'static Caps {
    let kept: &'static Caps = Box::leak(Box::new(c));
    *CAPS.write().unwrap_or_else(|e| e.into_inner()) = Some(kept);
    kept
}

fn active() -> &'static Caps {
    if let Some(c) = *CAPS.read().unwrap_or_else(|e| e.into_inner()) {
        return c;
    }
    put(embedded())
}

fn version_of(j: &Json) -> &str {
    j["_meta"]["version"].as_str().unwrap_or("")
}

/// Un fichier de connaissances utilisable : les modèles et les six cases y sont.
fn usable(text: &str) -> Option<Caps> {
    if text.len() > 1_000_000 {
        return None;
    }
    let json: Json = serde_json::from_str(text).ok()?;
    let ok = json["models"].as_array().is_some_and(|m| !m.is_empty()) && json["style_field"]["slots"].as_array().is_some_and(|s| s.len() == SLOTS.len()) && !version_of(&json).is_empty();
    ok.then(|| Caps { json, text: text.to_string() })
}

fn read_caps(path: Option<&Path>) -> Option<Caps> {
    usable(&std::fs::read_to_string(path?).ok()?)
}

/// Les connaissances sur Suno : le fichier posé à la main s'il se lit ; sinon le plus récent entre celui du logiciel
/// et celui rapporté de GitHub (après une mise à jour du logiciel, un vieux fichier rapporté ne masque rien).
pub fn init_caps(manual: Option<&Path>, fetched: Option<&Path>) {
    let chosen = read_caps(manual).unwrap_or_else(|| {
        let ours = embedded();
        match read_caps(fetched) {
            Some(f) if version_of(&f.json) > version_of(&ours.json) => f,
            _ => ours,
        }
    });
    put(chosen);
}

pub fn caps() -> &'static Json {
    &active().json
}

/// La date des connaissances en service (« 2026-10-22 »).
pub fn caps_version() -> String {
    version_of(caps()).to_string()
}

/// Des connaissances lues sur GitHub : mises en service et gardées dans `keep` si elles sont plus récentes. Rend leur
/// date, ou rien s'il n'y a rien de neuf.
pub fn install_caps(text: &str, keep: &Path) -> Result<Option<String>, String> {
    let new = usable(text).ok_or("le fichier des connaissances Suno est illisible")?;
    let version = version_of(&new.json).to_string();
    if version.as_str() <= version_of(caps()) {
        return Ok(None);
    }
    std::fs::write(keep, text).map_err(|e| format!("{} : {e}", keep.display()))?;
    put(new);
    Ok(Some(version))
}

/// « 2026-10-22 » en « 22/10/2026 ».
pub fn date_fr(iso: &str) -> String {
    let p: Vec<&str> = iso.split('-').collect();
    if p.len() == 3 { format!("{}/{}/{}", p[2], p[1], p[0]) } else { iso.to_string() }
}

fn strs(v: &Json) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect()
}

pub fn caps_list(key: &str) -> Vec<String> {
    strs(&caps()[key])
}

/// Les familles de genres, dans l'ordre du fichier (serde_json trie les clés : l'ordre est relu
/// dans le texte du fichier en service).
pub fn genre_seeds() -> Vec<(String, Vec<String>)> {
    let c = active();
    let seeds = &c.json["genre_seeds"];
    let Some(map) = seeds.as_object() else { return Vec::new() };
    let mut order: Vec<String> = Vec::new();
    if let Some(i) = c.text.find("\"genre_seeds\"") {
        let re = Regex::new(r#""([^"]+)"\s*:\s*\["#).unwrap();
        let body = &c.text[i + 13..];
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

// ---------------------------------------------------------------------- réglages gardés dans la base

fn setting(conn: &Connection, key: &str) -> String {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).unwrap_or_default()
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
    Ok(())
}

// ---------------------------------------------------------------------- revenir à une version

/// Le prompt tel qu'il était à cette version (rien n'est enregistré : c'est à l'artiste de le garder).
pub fn version(conn: &Connection, id: i64, no: i64) -> Option<SunoPrompt> {
    let now = get(conn, id)?;
    let (snapshot, style): (String, String) =
        conn.query_row("SELECT IFNULL(snapshot, ''), IFNULL(prompt_final, '') FROM prompt_versions WHERE prompt_id = ?1 AND version_no = ?2", params![id, no], |r| Ok((r.get(0)?, r.get(1)?))).optional().ok().flatten()?;
    let j: Json = serde_json::from_str(&snapshot).unwrap_or(Json::Null);
    let text = |k: &str, or: &str| j[k].as_str().unwrap_or(or).to_string();
    let mut p = SunoPrompt { id, parent_id: now.parent_id, use_count: now.use_count, last_used: now.last_used.clone(), name: text("name", &now.name), lyrics: text("lyrics", ""), notes: text("notes", ""), ..Default::default() };
    if j["extras"].is_object() {
        p.apply_json(&j["extras"]);
    }
    // Une version sans ses cases (ancien format) : le texte final fait office de genre.
    if SLOTS.iter().all(|k| p.slot(k).is_empty()) {
        p.genre = text("style_final", &style);
    }
    Some(p)
}

// ---------------------------------------------------------------------- modèles de départ

/// Les modèles de l'artiste (une liste, dans les réglages de la base).
pub const TEMPLATES_KEY: &str = "suno_templates";

#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub id: String,
    pub name: String,
    /// Un modèle de l'artiste (il peut le retirer), pas un modèle fourni.
    pub mine: bool,
    pub prompt: SunoPrompt,
}

fn template_from(j: &Json, mine: bool) -> Option<Template> {
    let name = j["name"].as_str().filter(|n| !n.trim().is_empty())?.trim().to_string();
    let mut prompt = SunoPrompt { name: name.clone(), ..Default::default() };
    for part in ["style_slots", "sliders", "extras"] {
        if j[part].is_object() {
            prompt.apply_json(&j[part]);
        }
    }
    prompt.lyrics = j["lyrics"].as_str().unwrap_or("").to_string();
    prompt.notes = j["notes"].as_str().unwrap_or("").to_string();
    let id = if mine { format!("mine:{name}") } else { j["id"].as_str().unwrap_or(&name).to_string() };
    Some(Template { id, name, mine, prompt })
}

fn my_templates(conn: &Connection) -> Vec<Json> {
    serde_json::from_str(&setting(conn, TEMPLATES_KEY)).unwrap_or_default()
}

/// Les modèles de départ : ceux de l'artiste d'abord, puis ceux fournis (un par grand genre).
pub fn templates(conn: Option<&Connection>) -> Vec<Template> {
    let mine = conn.map(my_templates).unwrap_or_default();
    mine.iter().filter_map(|j| template_from(j, true)).chain(caps()["workflow_templates"].as_array().into_iter().flatten().filter_map(|j| template_from(j, false))).collect()
}

/// Garde ce prompt comme modèle de départ (un modèle du même nom est remplacé).
pub fn save_template(conn: &Connection, p: &SunoPrompt) -> rusqlite::Result<()> {
    let name = p.name.trim();
    let mut list: Vec<Json> = my_templates(conn).into_iter().filter(|j| !j["name"].as_str().is_some_and(|n| n.trim().eq_ignore_ascii_case(name))).collect();
    list.insert(0, json!({"name": name, "extras": p.slots_json(), "lyrics": p.lyrics, "notes": p.notes}));
    set_setting(conn, TEMPLATES_KEY, &Json::Array(list).to_string())
}

/// Retire un modèle de l'artiste (« mine:Nom »). Les modèles fournis ne se retirent pas.
pub fn delete_template(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    let Some(name) = id.strip_prefix("mine:") else { return Ok(()) };
    let list: Vec<Json> = my_templates(conn).into_iter().filter(|j| j["name"].as_str().map(str::trim) != Some(name)).collect();
    set_setting(conn, TEMPLATES_KEY, &Json::Array(list).to_string())
}

// ---------------------------------------------------------------------- ce qui marche

/// Ce qu'un prompt, ou un mot du Style, a donné : morceaux gardés, rejetés, pas encore jugés.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Score {
    /// Le prompt (0 pour un mot).
    pub id: i64,
    pub label: String,
    /// La case du mot (« genre », « mood »…), vide pour un prompt.
    pub slot: String,
    pub kept: i64,
    pub rejected: i64,
    pub waiting: i64,
}

impl Score {
    pub fn judged(&self) -> i64 {
        self.kept + self.rejected
    }
    /// La part de morceaux gardés parmi ceux qui ont été jugés (0 à 100).
    pub fn rate(&self) -> i64 {
        if self.judged() == 0 { 0 } else { (self.kept * 100 + self.judged() / 2) / self.judged() }
    }
    fn count(&mut self, pick: i64) {
        match pick {
            1 => self.kept += 1,
            -1 => self.rejected += 1,
            _ => self.waiting += 1,
        }
    }
}

/// Les plus sûrs d'abord : le taux de gardés, puis le nombre de morceaux jugés. Ce qui n'a pas été jugé va à la fin.
fn best_first(list: &mut [Score]) {
    list.sort_by(|a, b| (b.judged() > 0).cmp(&(a.judged() > 0)).then(b.rate().cmp(&a.rate())).then(b.judged().cmp(&a.judged())).then(a.label.to_lowercase().cmp(&b.label.to_lowercase())));
}

/// Les mots du Style d'un prompt : (case, mot en minuscules), sans doublon.
fn descriptors(p: &SunoPrompt) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for k in SLOTS {
        for w in p.slot(k).split(',').map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()) {
            if !out.iter().any(|o| o.1 == w) {
                out.push((k.to_string(), w));
            }
        }
    }
    out
}

/// Ce qui marche : les prompts, puis les mots du Style, d'après les morceaux gardés et rejetés qu'ils ont donnés.
/// Un morceau compte pour les mots de la version du prompt qui l'a donné, quand elle est connue.
pub fn what_works(conn: &Connection) -> (Vec<Score>, Vec<Score>) {
    let rows: Vec<(i64, Option<i64>, i64)> = conn
        .prepare("SELECT prompt_id, prompt_version_no, IFNULL(pick, 0) FROM tracks WHERE prompt_id IS NOT NULL AND IFNULL(missing, 0) = 0")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
        .unwrap_or_default();
    let mut prompts: BTreeMap<i64, Score> = BTreeMap::new();
    let mut words: BTreeMap<String, Score> = BTreeMap::new();
    let mut seen: HashMap<(i64, Option<i64>), Option<(String, Vec<(String, String)>)>> = HashMap::new();
    for (pid, vno, pick) in rows {
        let known = seen.entry((pid, vno)).or_insert_with(|| {
            let now = get(conn, pid)?;
            let then = vno.and_then(|n| version(conn, pid, n));
            Some((now.name.clone(), descriptors(then.as_ref().unwrap_or(&now))))
        });
        let Some((name, list)) = known else { continue };
        prompts.entry(pid).or_insert_with(|| Score { id: pid, label: name.clone(), ..Default::default() }).count(pick);
        for (slot, w) in list.iter() {
            words.entry(w.clone()).or_insert_with(|| Score { label: w.clone(), slot: slot.clone(), ..Default::default() }).count(pick);
        }
    }
    let (mut p, mut w): (Vec<Score>, Vec<Score>) = (prompts.into_values().collect(), words.into_values().collect());
    best_first(&mut p);
    best_first(&mut w);
    (p, w)
}

/// Le prompt qui a donné un morceau : (le prompt s'il est encore dans la bibliothèque, son nom, le texte gardé avec
/// le morceau). Rien si le morceau ne vient pas d'un prompt.
pub fn prompt_of(conn: &Connection, track: i64) -> Option<(Option<SunoPrompt>, String, String)> {
    let (pid, text): (Option<i64>, String) = conn.query_row("SELECT prompt_id, IFNULL(suno_prompt, '') FROM tracks WHERE id = ?1", [track], |r| Ok((r.get(0)?, r.get(1)?))).optional().ok().flatten()?;
    let prompt = pid.and_then(|id| get(conn, id));
    if prompt.is_none() && text.trim().is_empty() {
        return None;
    }
    let name = prompt.as_ref().map(|p| p.name.clone()).unwrap_or_default();
    let text = if text.trim().is_empty() { prompt.as_ref().map(assemble_style_field).unwrap_or_default() } else { text };
    Some((prompt, name, text))
}

/// Une variation à partir d'un morceau : le prompt qui l'a donné (ou, s'il n'existe plus, le texte gardé avec le
/// morceau), pas encore enregistré et rattaché à son modèle.
pub fn variation_of(conn: &Connection, track: i64) -> Option<SunoPrompt> {
    let (prompt, _, text) = prompt_of(conn, track)?;
    Some(match prompt {
        Some(mut p) => {
            p.parent_id = Some(p.id);
            p.id = 0;
            p.name = format!("{} (variation)", p.name);
            p.use_count = 0;
            p.last_used = String::new();
            p
        }
        None => {
            let title: String = conn.query_row("SELECT IFNULL(title, '') FROM tracks WHERE id = ?1", [track], |r| r.get(0)).unwrap_or_default();
            SunoPrompt { name: if title.is_empty() { "Variation".into() } else { format!("{title} (variation)") }, genre: text, ..Default::default() }
        }
    })
}

// ---------------------------------------------------------------------- les téléchargements de Suno

/// Le dossier où le navigateur télécharge (vide : rien n'est surveillé).
pub const WATCH_KEY: &str = "suno_watch_dir";
/// Le moment où la surveillance a commencé (secondes Unix) : ce qui était déjà là n'est pas proposé.
pub const WATCH_SINCE_KEY: &str = "suno_watch_since";
/// Les fichiers écartés par l'artiste (liste de chemins).
pub const WATCH_IGNORED_KEY: &str = "suno_watch_ignored";
/// Le dernier prompt copié : c'est à lui qu'un téléchargement est relié.
pub const LAST_COPIED_KEY: &str = "suno_last_copied";
/// Le dossier où les fichiers de Suno sont rangés.
pub const IMPORT_DIR_KEY: &str = "suno_import_folder";

/// Un téléchargement est fini quand le fichier n'a plus bougé depuis ce temps.
const SETTLED: Duration = Duration::from_secs(10);

fn unix(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Surveille ce dossier à partir de maintenant (`None` : arrête).
pub fn set_watch(conn: &Connection, dir: Option<&Path>, now: SystemTime) -> rusqlite::Result<()> {
    set_setting(conn, WATCH_KEY, &dir.map(|d| d.to_string_lossy().into_owned()).unwrap_or_default())?;
    set_setting(conn, WATCH_SINCE_KEY, &unix(now).to_string())?;
    set_setting(conn, WATCH_IGNORED_KEY, "[]")
}

pub fn watch_dir(conn: &Connection) -> Option<PathBuf> {
    Some(setting(conn, WATCH_KEY)).filter(|d| !d.trim().is_empty()).map(PathBuf::from)
}

fn ignored(conn: &Connection) -> Vec<String> {
    serde_json::from_str(&setting(conn, WATCH_IGNORED_KEY)).unwrap_or_default()
}

/// Ces fichiers ne seront plus proposés (ils restent où ils sont).
pub fn ignore_downloads(conn: &Connection, files: &[PathBuf]) -> rusqlite::Result<()> {
    let mut list = ignored(conn);
    for f in files {
        let p = scan::normalize(f).to_string_lossy().into_owned();
        if !list.contains(&p) {
            list.push(p);
        }
    }
    // Les plus anciens s'oublient : s'ils sont toujours là, ils datent d'avant ce qui compte.
    let extra = list.len().saturating_sub(400);
    set_setting(conn, WATCH_IGNORED_KEY, &json!(list[extra..]).to_string())
}

/// Les fichiers audio arrivés dans le dossier surveillé depuis qu'il l'est, finis de télécharger, pas encore dans la
/// bibliothèque et pas écartés. Seul le dossier lui-même est regardé, pas ses sous-dossiers.
pub fn pending_downloads(conn: &Connection, now: SystemTime) -> Vec<PathBuf> {
    let Some(dir) = watch_dir(conn) else { return Vec::new() };
    let since = setting(conn, WATCH_SINCE_KEY).parse::<u64>().unwrap_or(0);
    let skip = ignored(conn);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if !scan::is_audio(&path) {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let Ok(changed) = meta.modified() else { continue };
        // Un navigateur peut garder la date du fichier d'origine : la création dit quand il est arrivé ici.
        let arrived = meta.created().ok().map_or(changed, |c| c.max(changed));
        if unix(arrived) < since || now.duration_since(changed).map_or(true, |d| d < SETTLED) {
            continue;
        }
        let p = scan::normalize(&path);
        let text = p.to_string_lossy().into_owned();
        if skip.contains(&text) || conn.query_row("SELECT 1 FROM tracks WHERE path = ?1 COLLATE NOCASE", [&text], |_| Ok(())).is_ok() {
            continue;
        }
        out.push(p);
    }
    out.sort();
    out
}

/// Le titre d'un téléchargement : le navigateur ajoute « (1) » au deuxième fichier du même nom.
pub fn clean_title(name: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\s*\(\d+\)\s*$").unwrap());
    let t = re.replace(name.trim(), "").trim().to_string();
    if t.is_empty() { name.trim().to_string() } else { t }
}

/// Une version pour chaque titre (« v1 », « v2 »…) : la suite de ce qui est déjà rangé dans `dest` sous ce titre, et
/// deux fichiers du même titre ne reçoivent pas la même.
pub fn next_versions(dest: &Path, titles: &[String]) -> Vec<String> {
    let there: Vec<String> = std::fs::read_dir(dest).map(|d| d.flatten().filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().to_lowercase())).collect()).unwrap_or_default();
    let mut last: HashMap<String, u32> = HashMap::new();
    titles
        .iter()
        .map(|title| {
            let base = import_name(title, "", "").to_lowercase();
            let n = last.entry(base.clone()).or_insert_with(|| {
                let prefix = format!("{base} - v");
                there.iter().filter_map(|s| s.strip_prefix(&prefix)).filter_map(|rest| rest.split(|c: char| !c.is_ascii_digit()).next().and_then(|d| d.parse::<u32>().ok())).max().unwrap_or(0)
            });
            *n += 1;
            format!("v{n}")
        })
        .collect()
}

pub fn last_copied(conn: &Connection) -> Option<SunoPrompt> {
    setting(conn, LAST_COPIED_KEY).parse::<i64>().ok().and_then(|id| get(conn, id))
}

pub fn set_last_copied(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    set_setting(conn, LAST_COPIED_KEY, &id.to_string())
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

/// Après l'indexation d'un fichier importé : génération Suno, prompt (et sa version du moment), version, tags
/// (« suno » toujours).
pub fn tag_import(conn: &Connection, track: i64, prompt_text: &str, version: &str, prompt_id: Option<i64>, group: Option<&str>, tags: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tracks SET role = 'suno_generation', suno_prompt = NULLIF(?2, ''), version_label = COALESCE(NULLIF(?3, ''), version_label), prompt_id = ?4, generation_group = ?5,
            prompt_version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_id = ?4) WHERE id = ?1",
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

    fn base() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn
    }

    fn track(conn: &Connection, path: &str, pick: i64) -> i64 {
        conn.execute("INSERT INTO tracks (path, filename, title, pick) VALUES (?1, ?1, ?1, ?2)", params![path, pick]).unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn every_template_passes_the_checker() {
        let list = templates(None);
        assert!(list.len() >= 12, "un modèle par grand genre : {}", list.len());
        for t in &list {
            assert!(!t.mine && !t.name.is_empty());
            let issues = verifier(&t.prompt);
            assert!(issues.is_empty(), "« {} » : {issues:?}", t.name);
        }
        let mut ids: Vec<&str> = list.iter().map(|t| t.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), list.len(), "pas deux modèles du même identifiant");
    }

    #[test]
    fn my_templates_come_first_and_can_go() {
        let conn = base();
        let mut p = prompt();
        p.name = "Ma techno".into();
        p.weirdness = 62;
        save_template(&conn, &p).unwrap();
        p.mood = "foggy harbour".into();
        save_template(&conn, &p).unwrap();
        let list = templates(Some(&conn));
        assert_eq!(list.iter().filter(|t| t.mine).count(), 1, "le même nom remplace");
        let mine = &list[0];
        assert_eq!((mine.id.as_str(), mine.prompt.id, mine.prompt.weirdness, mine.prompt.mood.as_str(), mine.prompt.lyrics.as_str()), ("mine:Ma techno", 0, 62, "foggy harbour", p.lyrics.as_str()));
        delete_template(&conn, "dark_techno").unwrap();
        delete_template(&conn, "mine:Ma techno").unwrap();
        assert_eq!(templates(Some(&conn)).len(), templates(None).len(), "le mien est parti, les modèles fournis restent");
    }

    #[test]
    fn a_version_comes_back_as_it_was() {
        let conn = base();
        let mut p = SunoPrompt { name: "House".into(), genre: "deep house".into(), mood: "late night".into(), lyrics: "[Intro]".into(), ..Default::default() };
        p.id = save(&conn, &p).unwrap();
        p.genre = "tech house".into();
        p.weirdness = 80;
        p.lyrics = "[Drop]".into();
        save(&conn, &p).unwrap();
        assert_eq!(versions(&conn, p.id).iter().map(|v| v.0).collect::<Vec<_>>(), vec![2, 1]);
        let first = version(&conn, p.id, 1).unwrap();
        assert_eq!((first.id, first.genre.as_str(), first.weirdness, first.lyrics.as_str()), (p.id, "deep house", 40, "[Intro]"));
        assert_eq!(get(&conn, p.id).unwrap().genre, "tech house", "revoir une version ne change rien tant qu'on n'enregistre pas");
        assert!(version(&conn, p.id, 9).is_none());
        // Une version à l'ancien format : le texte final sert de genre.
        conn.execute("INSERT INTO prompt_versions (prompt_id, version_no, snapshot, prompt_final) VALUES (?1, 3, 'pas du json', 'old style text')", [p.id]).unwrap();
        assert_eq!(version(&conn, p.id, 3).unwrap().genre, "old style text");
    }

    #[test]
    fn what_works_counts_kept_and_rejected() {
        let conn = base();
        let mut a = SunoPrompt { name: "A".into(), genre: "deep house".into(), mood: "Late Night".into(), ..Default::default() };
        a.id = save(&conn, &a).unwrap();
        let b = save(&conn, &SunoPrompt { name: "B".into(), genre: "trap".into(), mood: "late night".into(), ..Default::default() }).unwrap();
        let unused = save(&conn, &SunoPrompt { name: "C".into(), genre: "jazz".into(), ..Default::default() }).unwrap();
        for (i, (prompt, pick)) in [(a.id, 1), (a.id, 1), (a.id, -1), (a.id, 0), (b, -1), (b, -1)].into_iter().enumerate() {
            let t = track(&conn, &format!("t{i}.mp3"), pick);
            tag_import(&conn, t, "", "v1", Some(prompt), None, "").unwrap();
        }
        // Le prompt A change après coup : ses morceaux comptent pour les mots qu'il avait quand ils ont été faits.
        a.genre = "tech house".into();
        save(&conn, &a).unwrap();
        let (prompts, words) = what_works(&conn);
        assert_eq!(prompts.iter().map(|s| (s.label.as_str(), s.kept, s.rejected, s.waiting, s.rate())).collect::<Vec<_>>(), vec![("A", 2, 1, 1, 67), ("B", 0, 2, 0, 0)]);
        assert!(!prompts.iter().any(|s| s.id == unused), "un prompt sans morceau n'apparaît pas");
        let word = |w: &str| words.iter().find(|s| s.label == w).map(|s| (s.slot.as_str(), s.kept, s.rejected, s.waiting));
        assert_eq!(word("deep house"), Some(("genre", 2, 1, 1)));
        assert_eq!(word("late night"), Some(("mood", 2, 3, 1)), "le même mot, de deux prompts, sans tenir compte des majuscules");
        assert_eq!(word("tech house"), None);
        assert_eq!(words[0].label, "deep house", "le meilleur taux d'abord");
        assert_eq!(stats(&conn, a.id), (4, 2, 1, 1));
    }

    #[test]
    fn a_track_leads_back_to_its_prompt() {
        let conn = base();
        let id = save(&conn, &SunoPrompt { name: "House".into(), genre: "deep house".into(), ..Default::default() }).unwrap();
        mark_used(&conn, id).unwrap();
        let (linked, texte, rien) = (track(&conn, "a.mp3", 0), track(&conn, "b.mp3", 0), track(&conn, "c.mp3", 0));
        tag_import(&conn, linked, "", "v1", Some(id), None, "").unwrap();
        tag_import(&conn, texte, "uk garage, late night", "v1", None, None, "").unwrap();
        let (p, name, text) = prompt_of(&conn, linked).unwrap();
        assert_eq!((p.map(|p| p.id), name.as_str(), text.as_str()), (Some(id), "House", "deep house"));
        let v = variation_of(&conn, linked).unwrap();
        assert_eq!((v.id, v.parent_id, v.name.as_str(), v.genre.as_str(), v.use_count), (0, Some(id), "House (variation)", "deep house", 0));
        let v = variation_of(&conn, texte).unwrap();
        assert_eq!((v.id, v.parent_id, v.genre.as_str()), (0, None, "uk garage, late night"));
        assert!(prompt_of(&conn, rien).is_none() && variation_of(&conn, rien).is_none());
        // Le prompt est supprimé : le texte gardé avec le morceau reste.
        tag_import(&conn, linked, "deep house", "v1", Some(id), None, "").unwrap();
        delete(&conn, id).unwrap();
        assert_eq!(prompt_of(&conn, linked).map(|(p, _, t)| (p.is_none(), t)), Some((true, "deep house".into())));
    }

    #[test]
    fn downloads_are_found_once_they_have_landed() {
        let conn = base();
        let dir = std::env::temp_dir().join(format!("otoko-suno-dl-{}-{}", std::process::id(), generation_group()));
        std::fs::create_dir_all(dir.join("sous-dossier")).unwrap();
        let now = SystemTime::now();
        assert!(pending_downloads(&conn, now).is_empty(), "rien n'est surveillé");
        std::fs::write(dir.join("Avant.mp3"), b"x").unwrap();
        // La surveillance commence après ce fichier : il n'est pas proposé.
        set_watch(&conn, Some(&dir), now + Duration::from_secs(2)).unwrap();
        let later = now + Duration::from_secs(600);
        assert!(pending_downloads(&conn, later).is_empty(), "ce qui était déjà là reste tranquille");
        set_watch(&conn, Some(&dir), now - Duration::from_secs(60)).unwrap();
        for f in ["Night Drive.mp3", "Night Drive (1).mp3", "notes.txt", "sous-dossier/Autre.mp3", "En cours.mp3.crdownload"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert!(pending_downloads(&conn, now).is_empty(), "un fichier qui vient d'être écrit attend");
        let names = |list: &[PathBuf]| list.iter().map(|p| p.to_string_lossy().replace('\\', "/").rsplit('/').next().unwrap_or("").to_string()).collect::<Vec<_>>();
        let found = pending_downloads(&conn, later);
        assert_eq!(names(&found), vec!["Avant.mp3", "Night Drive (1).mp3", "Night Drive.mp3"]);
        ignore_downloads(&conn, &found[..1]).unwrap();
        track(&conn, &found[1].to_string_lossy(), 0);
        assert_eq!(names(&pending_downloads(&conn, later)), vec!["Night Drive.mp3"], "ni l'écarté, ni celui déjà dans la bibliothèque");
        set_watch(&conn, None, now).unwrap();
        assert!(watch_dir(&conn).is_none() && pending_downloads(&conn, later).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn downloads_get_a_title_and_the_next_version() {
        assert_eq!(clean_title("Night Drive (1)"), "Night Drive");
        assert_eq!(clean_title(" Night Drive "), "Night Drive");
        assert_eq!(clean_title("(1)"), "(1)", "un titre ne devient pas vide");
        let dir = std::env::temp_dir().join(format!("otoko-suno-v-{}-{}", std::process::id(), generation_group()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["Night Drive - v1.mp3", "night drive - v3 (2).wav", "Night Drive - vocal.mp3", "Autre - v7.mp3"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let t = |s: &str| s.to_string();
        assert_eq!(next_versions(&dir, &[t("Night Drive"), t("Night Drive"), t("Neuf"), t("Neuf: ?")]), vec!["v4", "v5", "v1", "v1"]);
        assert_eq!(next_versions(&dir.join("absent"), &[t("X")]), vec!["v1"]);
        let _ = std::fs::remove_dir_all(&dir);
        let conn = base();
        assert!(last_copied(&conn).is_none());
        let id = save(&conn, &prompt()).unwrap();
        set_last_copied(&conn, id).unwrap();
        assert_eq!(last_copied(&conn).map(|p| p.id), Some(id));
    }

    #[test]
    fn newer_knowledge_is_taken_and_older_is_left() {
        let dir = std::env::temp_dir().join(format!("otoko-suno-caps-{}-{}", std::process::id(), generation_group()));
        std::fs::create_dir_all(&dir).unwrap();
        let keep = dir.join(CAPS_FETCHED);
        let ours = caps_version();
        assert_eq!(install_caps(EMBEDDED_CAPS, &keep), Ok(None), "la même date : rien de neuf");
        assert!(install_caps("{}", &keep).is_err() && install_caps("pas du json", &keep).is_err() && !keep.exists());
        let older = EMBEDDED_CAPS.replacen(&ours, "2020-01-01", 1);
        assert_eq!(install_caps(&older, &keep), Ok(None));
        // Les mêmes connaissances, datées plus tard : les autres essais, qui tournent en même temps, ne voient rien changer.
        let newer = EMBEDDED_CAPS.replacen(&ours, "2999-01-01", 1);
        assert_eq!(install_caps(&newer, &keep), Ok(Some("2999-01-01".into())));
        assert_eq!((caps_version().as_str(), date_fr(&caps_version()).as_str()), ("2999-01-01", "01/01/2999"));
        assert_eq!(genre_seeds().first().map(|g| g.0.as_str()), Some("electronic"));
        // Au lancement suivant : le fichier rapporté sert s'il est plus récent ; le fichier posé à la main gagne toujours.
        init_caps(None, Some(&keep));
        assert_eq!(caps_version(), "2999-01-01");
        std::fs::write(&keep, &older).unwrap();
        init_caps(None, Some(&keep));
        assert_eq!(caps_version(), ours, "un vieux fichier rapporté ne masque pas celui du logiciel");
        init_caps(Some(&keep), None);
        assert_eq!(caps_version(), "2020-01-01");
        init_caps(Some(&dir.join("absent.json")), None);
        assert_eq!(caps_version(), ours);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
