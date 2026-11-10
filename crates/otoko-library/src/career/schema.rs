//! Le schéma de la Carrière : celui de Studio Vault (même tables, mêmes colonnes, même sens), pour
//! que l'import soit une copie fidèle et que le serveur mobile parle la même langue que l'app.

/// Toutes les tables, dans leur dernière version (Studio Vault 2.3).
pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS tracks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    path          TEXT UNIQUE NOT NULL,
    filename      TEXT NOT NULL,
    title         TEXT,
    artist        TEXT,
    album         TEXT,
    genre         TEXT,
    year          INTEGER,
    track_no      INTEGER,
    duration_s    REAL,
    bitrate_kbps  INTEGER,
    samplerate    INTEGER,
    channels      INTEGER,
    file_format   TEXT,
    file_size     INTEGER,
    bpm           REAL,
    music_key     TEXT,
    lufs          REAL,
    rating        INTEGER DEFAULT 0,
    status        TEXT DEFAULT 'idea',
    role          TEXT DEFAULT 'mixdown',
    parent_id     INTEGER,
    project_id    INTEGER,
    version_label TEXT,
    isrc          TEXT DEFAULT '',
    notes         TEXT,
    daw_project   TEXT,
    fingerprint   TEXT,
    added_at      TEXT DEFAULT (datetime('now')),
    modified_at   TEXT DEFAULT (datetime('now')),
    last_played   TEXT,
    play_count    INTEGER DEFAULT 0,
    missing       INTEGER DEFAULT 0,
    suno_prompt   TEXT,
    suno_id       TEXT,
    flp_project   TEXT,
    prompt_id     INTEGER,
    prompt_version_no INTEGER,
    generation_group TEXT,
    pick          INTEGER DEFAULT 0,
    release_type  TEXT,
    energy        REAL,
    brightness    REAL,
    dynamics      REAL,
    mode          TEXT,
    mood_auto     TEXT,
    true_peak     REAL,
    clip_ratio    REAL,
    silence_start REAL,
    silence_end   REAL,
    analyzed_at   TEXT,
    audio_fp      TEXT,
    nature        TEXT DEFAULT 'original',
    nature_meta   TEXT DEFAULT '{}',
    file_date     TEXT,
    sc_url        TEXT DEFAULT '',
    published_at  TEXT DEFAULT '',
    archived      INTEGER DEFAULT 0,
    cover_path    TEXT DEFAULT '',
    to_publish    INTEGER DEFAULT 0,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL,
    FOREIGN KEY (parent_id)  REFERENCES tracks(id)   ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_tracks_path        ON tracks(path);
CREATE INDEX IF NOT EXISTS idx_tracks_status      ON tracks(status);
CREATE INDEX IF NOT EXISTS idx_tracks_bpm         ON tracks(bpm);
CREATE INDEX IF NOT EXISTS idx_tracks_key         ON tracks(music_key);
CREATE INDEX IF NOT EXISTS idx_tracks_project     ON tracks(project_id);
CREATE INDEX IF NOT EXISTS idx_tracks_parent      ON tracks(parent_id);
CREATE INDEX IF NOT EXISTS idx_tracks_fingerprint ON tracks(fingerprint);
CREATE INDEX IF NOT EXISTS idx_tracks_audio_fp    ON tracks(audio_fp);
CREATE INDEX IF NOT EXISTS idx_tracks_nature      ON tracks(nature);

CREATE TABLE IF NOT EXISTS projects (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT UNIQUE NOT NULL,
    color      TEXT,
    notes      TEXT,
    created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS tags (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL
);
CREATE TABLE IF NOT EXISTS track_tags (
    track_id INTEGER NOT NULL,
    tag_id   INTEGER NOT NULL,
    PRIMARY KEY (track_id, tag_id),
    FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id)   REFERENCES tags(id)   ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS watch_folders (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    path     TEXT UNIQUE NOT NULL,
    enabled  INTEGER DEFAULT 1
);
CREATE TABLE IF NOT EXISTS markers (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id INTEGER NOT NULL,
    time_s   REAL NOT NULL,
    label    TEXT NOT NULL,
    FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT
);
CREATE TABLE IF NOT EXISTS prompts (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    genre_tags      TEXT,
    bpm_target      INTEGER,
    music_key       TEXT,
    mood_tags       TEXT,
    instrument_tags TEXT,
    vocals          TEXT,
    production      TEXT,
    era             TEXT,
    references_text TEXT,
    anti_tags       TEXT,
    custom_extra    TEXT,
    lyrics          TEXT,
    prompt_final    TEXT,
    notes           TEXT,
    parent_id       INTEGER,
    use_count       INTEGER DEFAULT 0,
    last_used_at    TEXT,
    created_at      TEXT DEFAULT (datetime('now')),
    modified_at     TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (parent_id) REFERENCES prompts(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_prompts_name ON prompts(name);
CREATE TABLE IF NOT EXISTS prompt_versions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    prompt_id    INTEGER NOT NULL,
    version_no   INTEGER NOT NULL,
    snapshot     TEXT NOT NULL,
    prompt_final TEXT,
    created_at   TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (prompt_id) REFERENCES prompts(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_prompt_versions_prompt ON prompt_versions(prompt_id);
CREATE TABLE IF NOT EXISTS post_drafts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id INTEGER,
    platform TEXT NOT NULL DEFAULT 'instagram',
    tone TEXT NOT NULL DEFAULT 'mysterious',
    lang TEXT NOT NULL DEFAULT 'fr',
    body TEXT NOT NULL,
    hashtags TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now')),
    posted_at TEXT,
    used_keys TEXT DEFAULT '',
    FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS release_kits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id INTEGER UNIQUE,
    title_final TEXT DEFAULT '',
    cover_path TEXT DEFAULT '',
    description TEXT DEFAULT '',
    sc_link TEXT DEFAULT '',
    ig_link TEXT DEFAULT '',
    teaser_path TEXT DEFAULT '',
    release_date TEXT DEFAULT '',
    notes TEXT DEFAULT '',
    checklist_done TEXT DEFAULT '{}',
    updated_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS file_ops (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id   TEXT NOT NULL,
    label      TEXT NOT NULL,
    track_id   INTEGER,
    src        TEXT NOT NULL,
    dst        TEXT NOT NULL,
    created_at TEXT DEFAULT (datetime('now')),
    undone     INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_file_ops_batch ON file_ops(batch_id);
CREATE TABLE IF NOT EXISTS releases (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    title        TEXT NOT NULL,
    rtype        TEXT DEFAULT 'single',
    release_date TEXT DEFAULT '',
    cover_path   TEXT DEFAULT '',
    credits      TEXT DEFAULT '',
    description  TEXT DEFAULT '',
    sc_link      TEXT DEFAULT '',
    notes        TEXT DEFAULT '',
    steps        TEXT DEFAULT '{}',
    created_at   TEXT DEFAULT (datetime('now')),
    extra        TEXT DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS release_tracks (
    release_id INTEGER NOT NULL,
    track_id   INTEGER NOT NULL,
    position   INTEGER DEFAULT 0,
    PRIMARY KEY (release_id, track_id),
    FOREIGN KEY (release_id) REFERENCES releases(id) ON DELETE CASCADE,
    FOREIGN KEY (track_id)   REFERENCES tracks(id)   ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS saved_views (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    where_sql  TEXT NOT NULL,
    params     TEXT DEFAULT '[]',
    search     TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS clients (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    contact    TEXT DEFAULT '',
    notes      TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS set_entries (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    set_id       INTEGER NOT NULL,
    time_s       REAL NOT NULL DEFAULT 0,
    artist       TEXT DEFAULT '',
    title        TEXT DEFAULT '',
    ref_track_id INTEGER,
    FOREIGN KEY (set_id) REFERENCES tracks(id) ON DELETE CASCADE,
    FOREIGN KEY (ref_track_id) REFERENCES tracks(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_set_entries_set ON set_entries(set_id);
CREATE TABLE IF NOT EXISTS field_locks (
    track_id INTEGER NOT NULL,
    field    TEXT NOT NULL,
    PRIMARY KEY (track_id, field),
    FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS track_copies (
    track_id   INTEGER PRIMARY KEY,
    copies     INTEGER NOT NULL DEFAULT 0,
    checked_at TEXT,
    FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS track_waves (
    track_id INTEGER PRIMARY KEY,
    peaks    BLOB NOT NULL,
    FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS text_models (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT NOT NULL DEFAULT 'modele',
    platform   TEXT NOT NULL DEFAULT '',
    name       TEXT NOT NULL DEFAULT '',
    body       TEXT NOT NULL DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);
";

/// L'historique des fiches (voir `journal`) : une ligne par champ changé, avec sa valeur d'avant et d'après (sans type :
/// texte, nombre ou rien, tels quels).
pub const JOURNAL: &str = "
CREATE TABLE IF NOT EXISTS main.edit_log (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id  TEXT NOT NULL,
    label     TEXT NOT NULL,
    source    TEXT NOT NULL DEFAULT 'pc',
    grp       TEXT NOT NULL DEFAULT '',
    track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    field     TEXT NOT NULL,
    old_value,
    new_value,
    at        TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now')),
    undone    INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS main.idx_edit_log_track ON edit_log(track_id, id);
CREATE INDEX IF NOT EXISTS main.idx_edit_log_batch ON edit_log(batch_id);
";

/// Statuts d'un morceau, dans l'ordre de la production.
pub const STATUSES: [(&str, &str); 7] = [
    ("idea", "Idée"),
    ("draft", "Brouillon"),
    ("wip", "En cours"),
    ("to_mix", "À mixer"),
    ("to_master", "À masteriser"),
    ("done", "Terminé"),
    ("released", "Publié"),
];

pub fn status_label(s: &str) -> &'static str {
    STATUSES.iter().find(|x| x.0 == s).map(|x| x.1).unwrap_or("Idée")
}

/// Colonnes de l'écran Production : (titre, statuts qu'elle regroupe).
pub const BOARD: [(&str, &[&str]); 5] = [
    ("Idées", &["idea", "draft"]),
    ("En cours", &["wip"]),
    ("Mix et master", &["to_mix", "to_master"]),
    ("Prêts", &["done"]),
    ("Publiés", &["released"]),
];

pub const RELEASE_TYPES: [(&str, &str, &str); 7] = [
    ("single", "Single", "#5bc0eb"),
    ("ep", "EP", "#f5b942"),
    ("album", "Album", "#6d6bff"),
    ("remixes", "Remixes", "#d36bff"),
    ("compilation", "Compilation", "#59c17a"),
    ("mixtape", "Mixtape", "#ff7043"),
    ("other", "Autre", "#8f8f9a"),
];

pub fn release_label(t: &str) -> String {
    RELEASE_TYPES.iter().find(|x| x.0 == t).map(|x| x.1.to_string()).unwrap_or_else(|| {
        let mut c = t.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_else(|| "Sortie".into())
    })
}

pub fn release_color(t: &str) -> Option<&'static str> {
    RELEASE_TYPES.iter().find(|x| x.0 == t).map(|x| x.2)
}

/// Natures d'un morceau : (code, libellé, libellé court).
pub const NATURES: [(&str, &str, &str); 6] = [
    ("original", "Original", "Original"),
    ("remix", "Remix", "Remix"),
    ("edit", "Edit / bootleg", "Edit"),
    ("mashup", "Mashup", "Mashup"),
    ("set", "Set / mix DJ", "Set"),
    ("prod", "Prod pour un artiste", "Prod"),
];

pub fn nature_short(n: &str) -> String {
    NATURES.iter().find(|x| x.0 == n).map(|x| x.2.to_string()).unwrap_or_else(|| {
        let mut c = n.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    })
}

/// Rôles des morceaux « principaux » (les stems, démos et générations Suno sont rattachés).
pub const MAIN_ROLES: [&str; 2] = ["mixdown", "render"];

/// Toutes les étapes possibles d'une sortie (clés du JSON `steps`), dans l'ordre. Une sortie n'a que celles de ses
/// plateformes (`steps_for`).
pub const ALL_STEPS: [(&str, &str); 10] = [
    ("master", "Master"),
    ("pochette", "Pochette"),
    ("infos", "Infos"),
    ("controle", "Contrôle"),
    ("distrib", "Distributeur"),
    ("soundcloud", "SoundCloud"),
    ("bandcamp", "Bandcamp"),
    ("youtube", "YouTube"),
    ("textes", "Textes"),
    ("promo", "Promo"),
];

/// Où une sortie peut sortir : (clé, nom court, explication). « streaming » passe par un distributeur (DistroKid,
/// TuneCore, CD Baby, Believe…), qui l'envoie à Spotify, Apple Music, Deezer, Tidal, Amazon Music, YouTube Music…
pub const PLATFORMS: [(&str, &str, &str); 4] = [
    ("streaming", "Streaming", "Spotify, Apple Music, Deezer… par un distributeur"),
    ("soundcloud", "SoundCloud", "mis en ligne par toi"),
    ("bandcamp", "Bandcamp", "mis en ligne par toi"),
    ("youtube", "YouTube", "la vidéo, mise en ligne par toi"),
];

/// Les étapes d'une sortie qui sort sur ces plateformes.
pub fn steps_for(platforms: &[String]) -> Vec<(&'static str, &'static str)> {
    let on = |k: &str| platforms.iter().any(|p| p == k);
    ALL_STEPS
        .iter()
        .copied()
        .filter(|(k, _)| match *k {
            "distrib" => on("streaming"),
            "soundcloud" | "bandcamp" | "youtube" => on(k),
            _ => true,
        })
        .collect()
}

/// Les plateformes écrites dans le JSON `extra` d'une sortie. Une sortie d'avant (sans ce choix) sort sur SoundCloud,
/// comme avant.
pub fn platforms_of(extra: &serde_json::Value) -> Vec<String> {
    match extra["platforms"].as_array() {
        Some(a) => PLATFORMS.iter().map(|p| p.0).filter(|k| a.iter().any(|x| x == k)).map(String::from).collect(),
        None => vec!["soundcloud".to_string()],
    }
}

/// Le nom lisible des plateformes : « Streaming et SoundCloud ».
pub fn platforms_label(platforms: &[String]) -> String {
    let names: Vec<&str> = PLATFORMS.iter().filter(|p| platforms.iter().any(|x| x == p.0)).map(|p| p.1).collect();
    match names.as_slice() {
        [] => "aucune plateforme".to_string(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} et {last}", rest.join(", ")),
    }
}

/// (étapes faites, étapes en tout) d'une sortie, d'après ses colonnes `steps` et `extra`.
pub fn steps_count(steps: &str, extra: &str) -> (usize, usize) {
    let done = super::nature::load_meta(steps);
    let list = steps_for(&platforms_of(&super::nature::load_meta(extra)));
    (list.iter().filter(|(k, _)| done[*k].as_bool().unwrap_or(false)).count(), list.len())
}
