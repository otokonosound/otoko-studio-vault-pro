//! Carrière : les morceaux de l'artiste (pas les sons de la bibliothèque), leurs versions, sorties,
//! projets, clients, textes de publication, prompts Suno… Base propre à OTOKO, vide pour un nouvel
//! utilisateur, au schéma de Studio Vault (voir `schema`), dans laquelle une base Studio Vault
//! s'importe à l'identique (voir `import`).

pub mod arrivee;
pub mod backup;
pub mod board;
pub mod camelot;
pub mod copies;
pub mod discography;
pub mod distant;
pub mod files;
pub mod folders;
pub mod home;
pub mod humeur;
pub mod import;
pub mod journal;
pub mod liens;
pub mod library;
pub mod mastering;
pub mod mastering_params;
pub mod mes_textes;
pub mod nature;
pub mod pilotage;
pub mod prefs;
pub mod projects;
pub mod pyrand;
pub mod releases;
pub mod scan;
pub mod schema;
pub mod search;
pub mod set_builder;
pub mod projet;
pub mod statuts;
pub mod suno;
pub mod textes;
pub mod textes_fr;
#[cfg(test)]
mod tests;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value as Json;

pub use import::ImportReport;
pub use schema::{
    ALL_STEPS, BOARD, MAIN_ROLES, NATURES, PLATFORMS, RELEASE_TYPES, STATUSES, nature_short, platforms_label, platforms_of, release_color, release_label, status_label,
    steps_count, steps_for,
};

pub struct Career {
    db: Connection,
}

/// Un morceau tel que la Carrière le montre.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Song {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub path: String,
    pub filename: String,
    pub genre: String,
    pub year: Option<i64>,
    pub secs: Option<f64>,
    pub bpm: Option<f64>,
    pub key: String,
    pub lufs: Option<f64>,
    /// Ancienne note (Studio Vault l'a remplacée par `pick`).
    pub rating: Option<i64>,
    /// Statut de production (voir `STATUSES`).
    pub status: String,
    /// « original », « remix », « edit », « mashup », « set », « prod ».
    pub nature: String,
    pub notes: String,
    pub added: String,
    /// Date de publication et lien (SoundCloud…), vides tant qu'il n'est pas publié.
    pub published: String,
    pub link: String,
    /// 1 gardé, -1 rejeté, 0 neutre.
    pub pick: i64,
    pub role: String,
    pub parent_id: Option<i64>,
    pub version_label: String,
    pub mood: String,
    pub energy: Option<f64>,
    pub format: String,
    pub daw_project: String,
    pub archived: bool,
    pub missing: bool,
    pub cover_path: String,
    pub play_count: i64,
}

const SONG_COLS: &str = "id, COALESCE(NULLIF(title, ''), filename), IFNULL(artist, ''), IFNULL(album, ''), path, filename, IFNULL(genre, ''),
    year, duration_s, bpm, IFNULL(music_key, ''), lufs, rating, IFNULL(status, 'idea'), IFNULL(NULLIF(nature, ''), 'original'),
    IFNULL(notes, ''), IFNULL(added_at, ''), IFNULL(published_at, ''), IFNULL(sc_url, ''), IFNULL(pick, 0), IFNULL(role, 'mixdown'),
    parent_id, IFNULL(version_label, ''), IFNULL(mood_auto, ''), energy, IFNULL(file_format, ''), IFNULL(daw_project, ''),
    IFNULL(archived, 0), IFNULL(missing, 0), IFNULL(cover_path, ''), IFNULL(play_count, 0)";

fn strip_ext(title: String) -> String {
    match title.rsplit_once('.') {
        Some((stem, ext)) if crate::AUDIO_EXT.contains(&ext.to_lowercase().as_str()) => stem.to_string(),
        _ => title,
    }
}

fn song_row(r: &rusqlite::Row) -> rusqlite::Result<Song> {
    Ok(Song {
        id: r.get(0)?,
        title: strip_ext(r.get(1)?),
        artist: r.get(2)?,
        album: r.get(3)?,
        path: r.get(4)?,
        filename: r.get(5)?,
        genre: r.get(6)?,
        year: r.get(7)?,
        secs: r.get(8)?,
        bpm: r.get(9)?,
        key: r.get(10)?,
        lufs: r.get(11)?,
        rating: r.get(12)?,
        status: r.get(13)?,
        nature: r.get(14)?,
        notes: r.get(15)?,
        added: r.get(16)?,
        published: r.get(17)?,
        link: r.get(18)?,
        pick: r.get(19)?,
        role: r.get(20)?,
        parent_id: r.get(21)?,
        version_label: r.get(22)?,
        mood: r.get(23)?,
        energy: r.get(24)?,
        format: r.get(25)?,
        daw_project: r.get(26)?,
        archived: r.get::<_, i64>(27)? != 0,
        missing: r.get::<_, i64>(28)? != 0,
        cover_path: r.get(29)?,
        play_count: r.get(30)?,
    })
}

/// Un texte prêt à publier (SoundCloud, Instagram…), gardé avec son morceau.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub id: i64,
    pub platform: String,
    pub tone: String,
    pub text: String,
    pub hashtags: String,
    pub created: String,
    pub posted: String,
}

/// Un prompt pour Suno : ses ingrédients, et le texte final qu'on colle dans le champ « Style ».
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prompt {
    pub id: i64,
    pub name: String,
    pub style: String,
    pub mood: String,
    pub instruments: String,
    pub vocals: String,
    pub production: String,
    pub era: String,
    pub references: String,
    pub avoid: String,
    pub bpm: Option<i64>,
    pub key: String,
    pub lyrics: String,
    pub final_text: String,
    pub extra: String,
    pub notes: String,
    pub uses: i64,
    pub modified: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    pub id: i64,
    pub title: String,
    /// « single », « ep », « album »… (voir `RELEASE_TYPES`).
    pub kind: String,
    pub date: String,
    pub link: String,
    pub description: String,
    pub credits: String,
    pub notes: String,
    pub cover_path: String,
    /// Étapes faites (clés de `STEPS`).
    pub steps: Vec<String>,
    pub steps_done: usize,
    pub steps_total: usize,
    pub songs: Vec<String>,
    pub song_ids: Vec<i64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overview {
    pub songs: u64,
    pub ideas: u64,
    pub in_progress: u64,
    pub finished: u64,
    pub released: u64,
    pub releases: u64,
    pub clients: u64,
    pub next_release: Option<(String, String)>,
    pub hours: f64,
}

/// Les chiffres d'une carrière.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// (année, morceaux, dont sortis), de la plus ancienne à la plus récente.
    pub by_year: Vec<(String, u64, u64)>,
    pub by_nature: Vec<(String, u64)>,
    pub genres: Vec<(String, u64)>,
    pub keys: Vec<(String, u64)>,
    pub kinds: Vec<(String, u64)>,
    /// (mois « AAAA-MM », morceaux publiés), les douze derniers mois.
    pub by_month: Vec<(String, u64)>,
    pub lufs_released: Option<f64>,
    pub bpm_median: Option<f64>,
    pub rating_avg: Option<f64>,
    pub published: u64,
}

/// Une liste « a, b » en tableau JSON, comme Studio Vault les garde.
fn json_list(s: &str) -> String {
    let v: Vec<Json> = s.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).map(|x| Json::String(x.into())).collect();
    if v.is_empty() { String::new() } else { Json::Array(v).to_string() }
}

/// Un tableau JSON (ou un texte) en liste « a, b ».
fn plain_list(s: &str) -> String {
    match serde_json::from_str::<Json>(s) {
        Ok(Json::Array(a)) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "),
        _ => s.to_string(),
    }
}

impl Career {
    pub fn open(path: &Path) -> rusqlite::Result<Career> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut db = Connection::open(path)?;
        // Des travaux écrivent en fond (formes d'onde, arrivées) : une transaction réserve l'écriture dès son début et attend
        // son tour. Commencée en lecture, elle échouerait d'un coup (« database is locked ») si l'un d'eux écrivait entre-temps.
        db.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);
        db.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let exists = |t: &str| db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [t], |_| Ok(())).is_ok();
        let old = exists("morceaux");
        if old {
            // Première version de la Carrière d'OTOKO (tables en français) : mise de côté, puis recopiée.
            for t in ["clients", "prompts"] {
                if exists(t) {
                    db.execute_batch(&format!("ALTER TABLE {t} RENAME TO ancien_{t};"))?;
                }
            }
            for t in ["sorties", "sortie_morceaux", "ancien_clients"] {
                if !exists(t) {
                    let cols = match t {
                        "sorties" => "id, titre, type, date, pochette, description, lien, remarques, etapes",
                        "sortie_morceaux" => "sortie, morceau, position",
                        _ => "id, nom, contact, remarques",
                    };
                    db.execute_batch(&format!("CREATE TABLE {t} ({cols});"))?;
                }
            }
        }
        db.execute_batch(schema::SCHEMA)?;
        // Les colonnes venues après coup : une base existante les reçoit à l'ouverture.
        for (table, col, def) in [("releases", "extra", "TEXT DEFAULT '{}'"), ("tracks", "isrc", "TEXT DEFAULT ''")] {
            if db.prepare(&format!("SELECT {col} FROM {table} LIMIT 0")).is_err() {
                db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {def}"))?;
            }
        }
        if old {
            migrate_first_version(&db)?;
        }
        db.execute_batch(schema::JOURNAL)?;
        Ok(Career { db })
    }

    /// La connexion, pour les modules qui lisent ou écrivent d'autres tables (bibliothèque, sorties…).
    pub fn conn(&self) -> &Connection {
        &self.db
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.db
    }

    /// Importe une base Studio Vault (sa copie, voir `vault::snapshot`). Rien n'est réécrit : ce
    /// qui a déjà été importé est laissé tel quel, ce qui manque est ajouté.
    pub fn import_vault(&mut self, copy: &Path) -> Result<ImportReport, String> {
        import::import(&mut self.db, copy)
    }

    /// Les morceaux principaux (ni versions, ni stems, ni archivés), filtrés par texte (titre,
    /// artiste, genre, tonalité) et par statut (vide : tous).
    pub fn songs(&self, query: &str, status: &str, limit: usize) -> rusqlite::Result<Vec<Song>> {
        let like = format!("%{}%", query.trim());
        let mut st = self.db.prepare_cached(&format!(
            "SELECT {SONG_COLS} FROM tracks
             WHERE IFNULL(archived, 0) = 0 AND parent_id IS NULL AND IFNULL(role, 'mixdown') IN ('mixdown', 'render')
               AND (title LIKE ?1 OR filename LIKE ?1 OR IFNULL(genre, '') LIKE ?1 OR IFNULL(music_key, '') LIKE ?1 OR IFNULL(artist, '') LIKE ?1)
               AND (?2 = '' OR status = ?2)
             ORDER BY COALESCE(added_at, '') DESC, id DESC LIMIT ?3"
        ))?;
        let rows = st.query_map(params![like, status, limit as i64], song_row)?;
        rows.collect()
    }

    pub fn song(&self, id: i64) -> rusqlite::Result<Option<Song>> {
        self.db.query_row(&format!("SELECT {SONG_COLS} FROM tracks WHERE id = ?1"), [id], song_row).optional()
    }

    pub fn releases(&self) -> rusqlite::Result<Vec<Release>> {
        let mut st = self.db.prepare(
            "SELECT id, title, IFNULL(rtype, ''), IFNULL(release_date, ''), IFNULL(sc_link, ''), IFNULL(description, ''), IFNULL(steps, ''),
                    IFNULL(credits, ''), IFNULL(notes, ''), IFNULL(cover_path, ''), IFNULL(extra, '{}')
             FROM releases ORDER BY COALESCE(release_date, '') DESC, id DESC",
        )?;
        let mut list: Vec<Release> = st
            .query_map([], |r| {
                let raw: String = r.get(6)?;
                let list = steps_for(&platforms_of(&nature::load_meta(&r.get::<_, String>(10)?)));
                let done: Vec<String> = match serde_json::from_str::<Json>(&raw) {
                    Ok(Json::Object(m)) => list.iter().filter(|(k, _)| m.get(*k).and_then(|v| v.as_bool()).unwrap_or(false)).map(|(k, _)| k.to_string()).collect(),
                    _ => Vec::new(),
                };
                Ok(Release {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    kind: r.get(2)?,
                    date: r.get(3)?,
                    link: r.get(4)?,
                    description: r.get(5)?,
                    steps_done: done.len(),
                    steps: done,
                    steps_total: list.len(),
                    credits: r.get(7)?,
                    notes: r.get(8)?,
                    cover_path: r.get(9)?,
                    songs: Vec::new(),
                    song_ids: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut songs = self.db.prepare(
            "SELECT t.id, COALESCE(NULLIF(t.title, ''), t.filename) FROM release_tracks rt JOIN tracks t ON t.id = rt.track_id
             WHERE rt.release_id = ?1 AND IFNULL(t.archived, 0) = 0 ORDER BY rt.position, t.track_no, t.id",
        )?;
        for r in list.iter_mut() {
            let rows: Vec<(i64, String)> = songs.query_map([r.id], |x| Ok((x.get(0)?, x.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            r.song_ids = rows.iter().map(|x| x.0).collect();
            r.songs = rows.into_iter().map(|x| strip_ext(x.1)).collect();
        }
        Ok(list)
    }

    pub fn overview(&self, today: &str) -> rusqlite::Result<Overview> {
        let count = |sql: &str| -> rusqlite::Result<u64> { self.db.query_row(sql, [], |r| r.get(0)) };
        let main = "IFNULL(archived, 0) = 0 AND parent_id IS NULL AND IFNULL(role, 'mixdown') IN ('mixdown', 'render')";
        let next_release = self
            .db
            .query_row("SELECT title, release_date FROM releases WHERE release_date >= ?1 ORDER BY release_date LIMIT 1", [today], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(Overview {
            songs: count(&format!("SELECT COUNT(*) FROM tracks WHERE {main}"))?,
            ideas: count(&format!("SELECT COUNT(*) FROM tracks WHERE {main} AND status IN ('idea', 'draft')"))?,
            in_progress: count(&format!("SELECT COUNT(*) FROM tracks WHERE {main} AND status IN ('wip', 'to_mix', 'to_master')"))?,
            finished: count(&format!("SELECT COUNT(*) FROM tracks WHERE {main} AND status = 'done'"))?,
            released: count(&format!("SELECT COUNT(*) FROM tracks WHERE {main} AND status = 'released'"))?,
            releases: count("SELECT COUNT(*) FROM releases")?,
            clients: count("SELECT COUNT(*) FROM clients")?,
            next_release,
            hours: self.db.query_row(&format!("SELECT IFNULL(SUM(duration_s), 0) / 3600.0 FROM tracks WHERE {main}"), [], |r| r.get(0))?,
        })
    }

    pub fn set_status(&self, id: i64, status: &str) -> rusqlite::Result<()> {
        self.db.execute("UPDATE tracks SET status = ?2, modified_at = datetime('now') WHERE id = ?1", params![id, status])?;
        Ok(())
    }

    /// Garder (1), rejeter (-1) ou neutre (0).
    pub fn set_pick(&self, id: i64, pick: i64) -> rusqlite::Result<()> {
        self.db.execute("UPDATE tracks SET pick = ?2 WHERE id = ?1", params![id, pick.clamp(-1, 1)])?;
        Ok(())
    }

    /// La sortie qui contient le morceau (la plus récente) : titre, type, date.
    pub fn release_of(&self, song: i64) -> rusqlite::Result<Option<(String, String, String)>> {
        self.db
            .query_row(
                "SELECT r.title, IFNULL(r.rtype, ''), IFNULL(r.release_date, '') FROM release_tracks rt JOIN releases r ON r.id = rt.release_id
                 WHERE rt.track_id = ?1 ORDER BY COALESCE(r.release_date, '') DESC LIMIT 1",
                [song],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
    }

    /// Les textes gardés pour un morceau, du plus récent au plus ancien (corps et mots-dièse réunis).
    pub fn drafts(&self, song: i64) -> rusqlite::Result<Vec<Draft>> {
        let mut st = self.db.prepare(
            "SELECT id, platform, IFNULL(tone, ''), body, IFNULL(hashtags, ''), IFNULL(created_at, ''), IFNULL(posted_at, '')
             FROM post_drafts WHERE track_id = ?1 ORDER BY id DESC",
        )?;
        st.query_map([song], |r| {
            let body: String = r.get(3)?;
            let tags: String = r.get(4)?;
            Ok(Draft {
                id: r.get(0)?,
                platform: r.get(1)?,
                tone: r.get(2)?,
                text: if tags.trim().is_empty() { body } else { format!("{}\n\n{}", body.trim_end(), tags.trim()) },
                hashtags: tags,
                created: r.get(5)?,
                posted: r.get(6)?,
            })
        })?
        .collect()
    }

    /// Garde un texte (un seul par plateforme et par jour pour un même ton : le dernier remplace).
    pub fn save_draft(&self, song: i64, platform: &str, text: &str, now: &str) -> rusqlite::Result<()> {
        self.save_draft_tone(song, platform, "neutre", text, now)
    }

    pub fn save_draft_tone(&self, song: i64, platform: &str, tone: &str, text: &str, now: &str) -> rusqlite::Result<()> {
        self.db.execute(
            "DELETE FROM post_drafts WHERE track_id = ?1 AND platform = ?2 AND tone = ?3 AND substr(created_at, 1, 10) = ?4 AND posted_at IS NULL",
            params![song, platform, tone, now],
        )?;
        self.db.execute("INSERT INTO post_drafts (track_id, platform, tone, lang, body, created_at) VALUES (?1, ?2, ?3, 'fr', ?4, ?5)", params![song, platform, tone, text, now])?;
        Ok(())
    }

    /// Le morceau est en ligne : publié, daté, avec son lien.
    pub fn mark_published(&self, song: i64, date: &str, link: &str) -> rusqlite::Result<()> {
        self.db.execute(
            "UPDATE tracks SET status = 'released', published_at = ?2, sc_url = ?3, modified_at = datetime('now') WHERE id = ?1",
            params![song, date, link],
        )?;
        Ok(())
    }

    pub fn prompts(&self) -> rusqlite::Result<Vec<Prompt>> {
        let mut st = self.db.prepare(
            "SELECT id, name, IFNULL(genre_tags, ''), IFNULL(mood_tags, ''), IFNULL(instrument_tags, ''), IFNULL(vocals, ''), IFNULL(production, ''),
                    IFNULL(era, ''), IFNULL(references_text, ''), IFNULL(anti_tags, ''), bpm_target, IFNULL(music_key, ''), IFNULL(lyrics, ''),
                    IFNULL(prompt_final, ''), IFNULL(custom_extra, ''), IFNULL(notes, ''), IFNULL(use_count, 0), IFNULL(modified_at, IFNULL(created_at, ''))
             FROM prompts ORDER BY COALESCE(modified_at, created_at, '') DESC, id DESC",
        )?;
        st.query_map([], |r| {
            let extra: String = r.get(14)?;
            let mut style = plain_list(&r.get::<_, String>(2)?);
            // Studio Vault garde souvent le style dans `custom_extra.genre`, ou tout dans le texte final.
            if style.is_empty() {
                style = serde_json::from_str::<Json>(&extra).ok().and_then(|j| j.get("genre").and_then(|g| g.as_str().map(str::to_string))).unwrap_or_default();
            }
            let final_text: String = r.get(13)?;
            if style.is_empty() {
                style = final_text.clone();
            }
            Ok(Prompt {
                id: r.get(0)?,
                name: r.get(1)?,
                style,
                mood: plain_list(&r.get::<_, String>(3)?),
                instruments: plain_list(&r.get::<_, String>(4)?),
                vocals: r.get(5)?,
                production: r.get(6)?,
                era: r.get(7)?,
                references: r.get(8)?,
                avoid: plain_list(&r.get::<_, String>(9)?),
                bpm: r.get(10)?,
                key: r.get(11)?,
                lyrics: r.get(12)?,
                final_text,
                extra,
                notes: r.get(15)?,
                uses: r.get(16)?,
                modified: r.get(17)?,
            })
        })?
        .collect()
    }

    /// Enregistre un prompt : nouveau si `p.id` vaut 0, sinon mis à jour (et une version gardée).
    /// Rend son identifiant.
    pub fn save_prompt(&self, p: &Prompt, now: &str) -> rusqlite::Result<i64> {
        let vals = params![
            p.name,
            json_list(&p.style),
            json_list(&p.mood),
            json_list(&p.instruments),
            p.vocals,
            p.production,
            p.era,
            p.references,
            json_list(&p.avoid),
            p.bpm,
            p.key,
            p.lyrics,
            p.final_text,
            p.extra,
            p.notes,
            now
        ];
        let id = if p.id == 0 {
            self.db.execute(
                "INSERT INTO prompts (name, genre_tags, mood_tags, instrument_tags, vocals, production, era, references_text, anti_tags, bpm_target,
                     music_key, lyrics, prompt_final, custom_extra, notes, created_at, modified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
                vals,
            )?;
            self.db.last_insert_rowid()
        } else {
            self.db.execute(
                "UPDATE prompts SET name = ?1, genre_tags = ?2, mood_tags = ?3, instrument_tags = ?4, vocals = ?5, production = ?6, era = ?7,
                     references_text = ?8, anti_tags = ?9, bpm_target = ?10, music_key = ?11, lyrics = ?12, prompt_final = ?13, custom_extra = ?14,
                     notes = ?15, modified_at = ?16 WHERE id = ?17",
                params![
                    p.name,
                    json_list(&p.style),
                    json_list(&p.mood),
                    json_list(&p.instruments),
                    p.vocals,
                    p.production,
                    p.era,
                    p.references,
                    json_list(&p.avoid),
                    p.bpm,
                    p.key,
                    p.lyrics,
                    p.final_text,
                    p.extra,
                    p.notes,
                    now,
                    p.id
                ],
            )?;
            p.id
        };
        // Chaque enregistrement est une version (comme dans Studio Vault).
        let n: i64 = self.db.query_row("SELECT IFNULL(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_id = ?1", [id], |r| r.get(0))?;
        let snapshot = serde_json::json!({
            "name": p.name, "style_final": p.final_text, "lyrics": p.lyrics, "notes": p.notes,
            "genre": p.style, "mood": p.mood, "instruments": p.instruments, "vocals": p.vocals,
            "production": p.production, "era": p.era, "avoid": p.avoid, "bpm": p.bpm, "key": p.key,
        });
        self.db.execute(
            "INSERT INTO prompt_versions (prompt_id, version_no, snapshot, prompt_final, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, n, snapshot.to_string(), p.final_text, now],
        )?;
        Ok(id)
    }

    /// Le prompt a servi (copié pour Suno).
    pub fn use_prompt(&self, id: i64) -> rusqlite::Result<()> {
        self.db.execute("UPDATE prompts SET use_count = IFNULL(use_count, 0) + 1, last_used_at = datetime('now') WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn stats(&self) -> rusqlite::Result<Stats> {
        let pairs = |sql: &str| -> rusqlite::Result<Vec<(String, u64)>> {
            let mut st = self.db.prepare(sql)?;
            st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))?.collect()
        };
        let main = "IFNULL(archived, 0) = 0 AND parent_id IS NULL AND IFNULL(role, 'mixdown') IN ('mixdown', 'render')";
        let mut st = self.db.prepare(&format!(
            "SELECT y, COUNT(*), SUM(status = 'released') FROM (
                 SELECT COALESCE(CAST(year AS TEXT), substr(NULLIF(published_at, ''), 1, 4), substr(NULLIF(file_date, ''), 1, 4), substr(NULLIF(added_at, ''), 1, 4)) AS y, status
                 FROM tracks WHERE {main})
             WHERE y IS NOT NULL AND length(y) = 4 GROUP BY y ORDER BY y"
        ))?;
        let by_year = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        let bpms: Vec<f64> = {
            let mut s = self.db.prepare(&format!("SELECT bpm FROM tracks WHERE bpm > 0 AND {main} ORDER BY bpm"))?;
            s.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        Ok(Stats {
            by_year,
            by_nature: pairs(&format!("SELECT IFNULL(NULLIF(nature, ''), 'original'), COUNT(*) FROM tracks WHERE {main} GROUP BY 1 ORDER BY 2 DESC"))?,
            genres: pairs(&format!("SELECT genre, COUNT(*) FROM tracks WHERE IFNULL(genre, '') <> '' AND {main} GROUP BY lower(genre) ORDER BY 2 DESC LIMIT 6"))?,
            keys: pairs(&format!("SELECT music_key, COUNT(*) FROM tracks WHERE IFNULL(music_key, '') <> '' AND {main} GROUP BY music_key ORDER BY 2 DESC LIMIT 6"))?,
            kinds: pairs("SELECT IFNULL(NULLIF(rtype, ''), 'single'), COUNT(*) FROM releases GROUP BY 1 ORDER BY 2 DESC")?,
            by_month: pairs(&format!(
                "SELECT substr(published_at, 1, 7) AS m, COUNT(*) FROM tracks
                 WHERE {main} AND IFNULL(published_at, '') <> '' AND published_at >= date('now', '-11 months', 'start of month') GROUP BY m ORDER BY m"
            ))?,
            lufs_released: self.db.query_row(&format!("SELECT AVG(lufs) FROM tracks WHERE status = 'released' AND lufs IS NOT NULL AND {main}"), [], |r| r.get(0))?,
            bpm_median: bpms.get(bpms.len() / 2).copied(),
            rating_avg: self.db.query_row(&format!("SELECT AVG(rating) FROM tracks WHERE rating > 0 AND {main}"), [], |r| r.get(0))?,
            published: self.db.query_row(&format!("SELECT COUNT(*) FROM tracks WHERE (IFNULL(published_at, '') <> '' OR IFNULL(sc_url, '') <> '') AND {main}"), [], |r| r.get(0))?,
        })
    }

    /// Un réglage de la Carrière (table `settings`, comme Studio Vault).
    pub fn setting(&self, key: &str) -> Option<String> {
        self.db.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.db.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
        Ok(())
    }

    /// Les réglages dont la clé commence par `prefix`, valeur non vide (presets personnels, favoris…).
    pub fn settings_with_prefix(&self, prefix: &str) -> Vec<(String, String)> {
        let Ok(mut stmt) = self.db.prepare("SELECT key, value FROM settings WHERE key LIKE ?1 || '%' AND value <> '' ORDER BY key") else { return Vec::new() };
        stmt.query_map([prefix], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }
}

/// Recopie la première Carrière d'OTOKO (tables en français) dans le schéma de Studio Vault, puis
/// retire les anciennes tables.
fn migrate_first_version(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "INSERT OR IGNORE INTO tracks (id, path, filename, title, artist, genre, year, duration_s, bpm, music_key, lufs, rating, status, nature,
             notes, added_at, published_at, sc_url)
         SELECT id, COALESCE(NULLIF(chemin, ''), 'otoko:morceau/' || id), COALESCE(NULLIF(chemin, ''), titre), titre, artiste, genre, annee, duree, bpm,
             tonalite, lufs, note, CASE statut WHEN 'fini' THEN 'done' WHEN 'sorti' THEN 'released' ELSE 'idea' END, IFNULL(nature, 'original'),
             remarques, ajoute, IFNULL(publie, ''), IFNULL(lien, '')
         FROM morceaux;
         INSERT OR IGNORE INTO releases (id, title, rtype, release_date, cover_path, description, sc_link, notes, steps)
         SELECT id, titre, type, IFNULL(date, ''), IFNULL(pochette, ''), IFNULL(description, ''), IFNULL(lien, ''), IFNULL(remarques, ''), IFNULL(etapes, '{}')
         FROM sorties;
         INSERT OR IGNORE INTO release_tracks (release_id, track_id, position) SELECT sortie, morceau, position FROM sortie_morceaux;
         INSERT OR IGNORE INTO clients (id, name, contact, notes) SELECT id, nom, IFNULL(contact, ''), IFNULL(remarques, '') FROM ancien_clients;
         DROP TABLE sortie_morceaux; DROP TABLE sorties; DROP TABLE ancien_clients; DROP TABLE morceaux;",
    )?;
    if db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'publications'", [], |_| Ok(())).is_ok() {
        db.execute_batch(
            "INSERT INTO post_drafts (track_id, platform, tone, body, created_at, posted_at)
             SELECT morceau, plateforme, 'neutre', texte, cree, publie FROM publications;
             DROP TABLE publications;",
        )?;
    }
    if db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ancien_prompts'", [], |_| Ok(())).is_ok() {
        db.execute_batch(
            "INSERT INTO prompts (name, genre_tags, mood_tags, instrument_tags, vocals, production, era, anti_tags, bpm_target, music_key, lyrics,
                 prompt_final, use_count, created_at, modified_at)
             SELECT nom, style, ambiance, instruments, voix, production, epoque, eviter, bpm, tonalite, paroles, final, utilise, cree, modifie FROM ancien_prompts;
             DROP TABLE ancien_prompts;",
        )?;
    }
    Ok(())
}
