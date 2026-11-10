//! Préférences de la Carrière, comme `preferences_dialog.py`, `shortcuts.py` et `theme.py` de Studio
//! Vault : mêmes réglages (table `settings`, mêmes clés : ceux de Studio Vault sont repris à
//! l'import), mêmes actions et raccourcis par défaut, même couleur tirée d'une pochette.

use rusqlite::Connection;
use serde_json::Value as Json;

/// Les couleurs d'accent proposées (« Thème » : la braise d'OTOKO).
pub const ACCENTS: [(&str, &str); 8] =
    [("Lavande", "#b48cff"), ("Bleu", "#3d8bfd"), ("Turquoise", "#1fb5a5"), ("Vert", "#43b96a"), ("Ambre", "#e8a33d"), ("Corail", "#f2695c"), ("Rose", "#e05fa8"), ("Argent", "#9aa3b5")];

/// Une action de la Carrière : identifiant (celui de Studio Vault), libellé, raccourci par défaut, explication.
pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub key: &'static str,
    pub tip: &'static str,
}

const fn a(id: &'static str, label: &'static str, key: &'static str, tip: &'static str) -> Action {
    Action { id, label, key, tip }
}

/// Les actions de Studio Vault (même ordre, mêmes raccourcis), plus celles propres à OTOKO.
pub const ACTIONS: &[Action] = &[
    a("add_folder", "Ajouter un dossier…", "Ctrl+O", "Scanner un dossier de musiques"),
    a("rescan", "Rescanner les dossiers", "F5", ""),
    a("watch_folders", "Dossiers surveillés…", "", ""),
    a("relocate", "Changer l'emplacement de la bibliothèque…", "", "Fichiers déplacés vers un autre disque ou dossier : tout le travail suit les morceaux"),
    a("import_suno", "Importer des fichiers Suno…", "Ctrl+I", ""),
    a("export_csv", "Exporter la vue en CSV…", "", ""),
    a("backup", "Sauvegarder la base maintenant", "", ""),
    a("preferences", "Préférences…", "Ctrl+,", ""),
    a("undo", "Annuler la dernière opération", "Ctrl+Z", "Défait la dernière modification de fiches faite sur ce PC (un champ, une édition en lot, un genre, un statut…) ou remet en place les fichiers du dernier renommage ou de la dernière réorganisation"),
    a("edit_tags", "Éditer les tags…", "Ctrl+E", ""),
    a("genre", "Genre et ambiance…", "Ctrl+G", ""),
    a("rename", "Renommer en lot…", "Ctrl+R", ""),
    a("project", "Fiche projet (remix, mashup, set, prod)…", "Ctrl+Shift+P", ""),
    a("detect_nature", "Détecter la nature (remix, mashup, set…)", "", ""),
    a("clients", "Carnet de clients…", "", ""),
    a("complete_library", "Compléter genres et artistes…", "Ctrl+Shift+G", ""),
    a("dupe_folders", "Dossiers en double…", "", ""),
    a("set_builder", "Construire un set…", "Ctrl+Shift+B", "Ordre de passage qui enchaîne bien (sélection, ou toute la vue affichée)"),
    a("discography", "Exporter la discographie…", "", ""),
    a("planning", "Planning des sorties", "", ""),
    a("open_soundcloud", "Ouvrir sur SoundCloud", "", ""),
    a("rename_one", "Renommer le fichier…", "F2", ""),
    a("reorganize", "Réorganiser les fichiers…", "Ctrl+Shift+R", ""),
    // Studio Vault lui donnait Ctrl+Shift+A, déjà pris par « Analyser la sélection » (les deux restaient
    // muets) : ici l'analyse garde la combinaison, l'archivage n'en a pas d'origine.
    a("archive_tracks", "Archiver / sortir des archives", "", "Cache les morceaux sélectionnés (vue « Archives ») sans toucher aux fichiers"),
    a("remove", "Retirer de la bibliothèque", "Del", ""),
    a("home", "Accueil", "Ctrl+1", ""),
    a("library", "Bibliothèque", "Ctrl+2", ""),
    a("details", "Panneau de détails", "I", ""),
    a("focus_search", "Rechercher", "Ctrl+F", ""),
    a("save_view", "Enregistrer la vue actuelle…", "Ctrl+S", ""),
    a("play", "Lecture / pause", "Space", ""),
    a("stop", "Stop", "Ctrl+.", ""),
    a("prev", "Morceau précédent", "Ctrl+Left", ""),
    a("next", "Morceau suivant", "Ctrl+Right", ""),
    a("queue_add", "Ajouter à la file de lecture", "Q", ""),
    a("queue_show", "Voir la file de lecture…", "Ctrl+Shift+Q", ""),
    a("loop_clear", "Effacer la boucle A-B", "Escape", ""),
    a("pick_keep", "Garder", "P", ""),
    a("pick_reject", "Rejeter", "X", ""),
    a("pick_neutral", "Neutre", "N", ""),
    a("analyze", "Analyser la sélection", "Ctrl+Shift+A", "Tempo, tonalité, niveau sonore, énergie, ambiance, crêtes, silences"),
    a("analyze_missing", "Analyser tout ce qui ne l'est pas encore", "", ""),
    a("master", "Ajouter au mastering", "Ctrl+Shift+M", "Ajoute les morceaux sélectionnés à l'écran Mastering (le mastering ne démarre pas tout seul)"),
    a("qc", "Contrôle avant publication…", "Ctrl+T", ""),
    a("compare", "Comparer A/B à volume égal…", "Ctrl+D", ""),
    a("dupes", "Doublons…", "", ""),
    a("compatible", "Morceaux compatibles (Camelot)", "Ctrl+Shift+K", ""),
    a("palette", "Palette de commandes…", "Ctrl+K", "Rechercher une action, un morceau ou une page"),
    a("prompt_builder", "Atelier de prompts Suno…", "Ctrl+B", ""),
    a("prompt_library", "Bibliothèque de prompts…", "Ctrl+L", ""),
    a("publishing", "Assistant de publication…", "Ctrl+6", ""),
    a("releases", "Sorties (singles, EP, albums)…", "Ctrl+5", ""),
    a("stats", "Statistiques…", "Ctrl+7", ""),
    a("shortcuts", "Raccourcis clavier…", "", ""),
    a("help_search", "Syntaxe de recherche", "F1", ""),
    a("about", "À propos", "", ""),
    // Propres à OTOKO.
    a("mastering", "Mastering", "Ctrl+4", "L'écran Mastering (PhaseLimiter)"),
    a("production", "Production", "Ctrl+3", "Le tableau de production"),
    a("mobile", "Mobile et partage", "Ctrl+8", ""),
    a("camelot", "Roue de Camelot", "", ""),
    a("open_studio", "Ouvrir le projet", "", "Le projet du morceau dans son logiciel (FL Studio, Ableton Live, Cubase, Reaper…)"),
    a("batches", "Genres proposés par lots…", "", "Les morceaux sans genre, regroupés, à valider d'un clic"),
    a("statuses", "Statuts automatiques…", "", "Ce que la base prouve sur l'avancement des morceaux"),
    a("link_tracks", "Relier les morceaux à SoundCloud", "", "Retrouve le lien de chaque titre à partir de celui de sa sortie"),
    a("check_copies", "Contrôler les copies", "", "Ce qui n'existe qu'à un seul endroit"),
];

/// Les raccourcis de page de Studio Vault : repris à l'import, ils laissent la place à Ctrl+1 à 8.
const OLD_PAGE_KEYS: [(&str, &str); 4] = [("home", "Ctrl+H"), ("library", "Ctrl+1"), ("releases", "Ctrl+J"), ("publishing", "Ctrl+M")];

pub fn action(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

fn setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get::<_, Option<String>>(0)).ok().flatten()
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", [key, value])?;
    Ok(())
}

/// Une combinaison écrite d'une seule façon : « Ctrl+Shift+A », « Escape », « Del », « Space ».
/// Accepte l'écriture de Qt (« Esc », « Delete », « Return ») et la casse libre.
pub fn normalize(seq: &str) -> String {
    let parts: Vec<String> = seq.split('+').map(|p| p.trim().to_string()).collect();
    // « Ctrl++ » : la touche plus.
    let mut keys: Vec<String> = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        if parts[i].is_empty() && i + 1 < parts.len() && parts[i + 1].is_empty() {
            keys.push("+".into());
            i += 2;
            continue;
        }
        if !parts[i].is_empty() {
            keys.push(parts[i].clone());
        }
        i += 1;
    }
    let (mut ctrl, mut shift, mut alt, mut key) = (false, false, false, String::new());
    for k in keys {
        match k.to_lowercase().as_str() {
            "ctrl" | "control" | "meta" => ctrl = true,
            "shift" | "maj" => shift = true,
            "alt" => alt = true,
            "esc" | "escape" | "échap" => key = "Escape".into(),
            "del" | "delete" | "suppr" => key = "Del".into(),
            "return" | "enter" | "entrée" => key = "Return".into(),
            "space" | "espace" | " " => key = "Space".into(),
            "left" => key = "Left".into(),
            "right" => key = "Right".into(),
            "up" => key = "Up".into(),
            "down" => key = "Down".into(),
            "backspace" => key = "Backspace".into(),
            "tab" => key = "Tab".into(),
            "home" => key = "Home".into(),
            "end" => key = "End".into(),
            other => {
                key = if other.len() > 1 && other.starts_with('f') && other[1..].chars().all(|c| c.is_ascii_digit()) { other.to_uppercase() } else { k.to_uppercase() }
            }
        }
    }
    if key.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if ctrl {
        out.push_str("Ctrl+");
    }
    if alt {
        out.push_str("Alt+");
    }
    if shift {
        out.push_str("Shift+");
    }
    out.push_str(&key);
    out
}

/// Les raccourcis : ceux d'origine, remplacés par ceux choisis (réglage `shortcuts`, JSON de Studio Vault).
pub fn shortcuts(conn: &Connection) -> Vec<(&'static str, String)> {
    let custom: serde_json::Map<String, Json> = setting(conn, "shortcuts").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    ACTIONS
        .iter()
        .map(|a| {
            let chosen = custom.get(a.id).and_then(|v| v.as_str()).filter(|k| !OLD_PAGE_KEYS.iter().any(|o| o.0 == a.id && normalize(k) == o.1));
            (a.id, normalize(chosen.unwrap_or(a.key)))
        })
        .collect()
}

/// L'action d'une combinaison (déjà normalisée).
pub fn lookup(map: &[(&'static str, String)], seq: &str) -> Option<&'static str> {
    if seq.is_empty() {
        return None;
    }
    map.iter().find(|(_, k)| k == seq).map(|(id, _)| *id)
}

/// Change un raccourci (vide : aucun). Refusé s'il sert déjà : rend le libellé de l'autre action.
pub fn set_shortcut(conn: &Connection, id: &str, seq: &str) -> Result<(), String> {
    let seq = normalize(seq);
    let mut map = shortcuts(conn);
    if let Some(other) = map.iter().find(|(o, k)| !seq.is_empty() && *k == seq && *o != id) {
        return Err(action(other.0).map(|a| a.label.trim_end_matches('…').to_string()).unwrap_or_default());
    }
    for (o, k) in map.iter_mut() {
        if *o == id {
            *k = seq.clone();
        }
    }
    save(conn, &map)
}

fn save(conn: &Connection, map: &[(&'static str, String)]) -> Result<(), String> {
    let obj: serde_json::Map<String, Json> = map.iter().map(|(id, k)| (id.to_string(), Json::String(k.clone()))).collect();
    set_setting(conn, "shortcuts", &Json::Object(obj).to_string()).map_err(|e| e.to_string())
}

pub fn reset_shortcuts(conn: &Connection) -> Result<(), String> {
    let map: Vec<(&'static str, String)> = ACTIONS.iter().map(|a| (a.id, normalize(a.key))).collect();
    save(conn, &map)
}

/// Les autres noms d'artiste (réglage `artist_aliases`, liste JSON).
pub fn aliases(conn: &Connection) -> Vec<String> {
    setting(conn, "artist_aliases").and_then(|s| serde_json::from_str::<Vec<Json>>(&s).ok()).unwrap_or_default().into_iter().filter_map(|v| v.as_str().map(str::to_string)).filter(|s| !s.is_empty()).collect()
}

pub fn set_aliases(conn: &Connection, list: &[String]) -> rusqlite::Result<()> {
    let clean: Vec<Json> = list.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).map(|a| Json::String(a.into())).collect();
    set_setting(conn, "artist_aliases", &Json::Array(clean).to_string())
}

/// La couleur marquante d'une pochette (RGBA, `w` × `h`) : moyenne pondérée par la saturation,
/// sur une vignette de 24 × 24 ; rien pour une pochette en noir et blanc. Comme Studio Vault.
pub fn dominant_color(rgba: &[u8], w: usize, h: usize) -> Option<(u8, u8, u8)> {
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    let (mut tot, mut r, mut g, mut b) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for y in 0..24 {
        for x in 0..24 {
            let (px, py) = ((x * w + w / 2) / 24, (y * h + h / 2) / 24);
            let i = (py.min(h - 1) * w + px.min(w - 1)) * 4;
            let (cr, cg, cb) = (rgba[i] as f64, rgba[i + 1] as f64, rgba[i + 2] as f64);
            let (s, l) = hsl(cr, cg, cb);
            let wgt = s * (1.0 - (l - 0.5).abs() * 2.0) + 0.02;
            tot += wgt;
            r += cr * wgt;
            g += cg * wgt;
            b += cb * wgt;
        }
    }
    if tot <= 0.0 {
        return None;
    }
    let c = ((r / tot) as u8, (g / tot) as u8, (b / tot) as u8);
    if hsl(c.0 as f64, c.1 as f64, c.2 as f64).0 < 0.15 {
        return None;
    }
    Some(c)
}

/// Saturation et luminosité TSL (0 à 1), comme `QColor.getHslF`.
fn hsl(r: f64, g: f64, b: f64) -> (f64, f64) {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    (s, l)
}

/// « #rrggbb » → (r, g, b).
pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_read_like_studio_vault() {
        assert_eq!(normalize("ctrl+shift+a"), "Ctrl+Shift+A");
        assert_eq!(normalize("Esc"), "Escape");
        assert_eq!(normalize("Delete"), "Del");
        assert_eq!(normalize("f2"), "F2");
        assert_eq!(normalize("Ctrl+,"), "Ctrl+,");
        assert_eq!(normalize("Ctrl+."), "Ctrl+.");
        assert_eq!(normalize(""), "");
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);").unwrap();
        let map = shortcuts(&conn);
        assert_eq!(lookup(&map, "Ctrl+K"), Some("palette"));
        assert_eq!(lookup(&map, "P"), Some("pick_keep"));
        // Un raccourci déjà pris est refusé ; un libre est gardé (format de Studio Vault).
        assert_eq!(set_shortcut(&conn, "master", "Ctrl+K"), Err("Palette de commandes".into()));
        set_shortcut(&conn, "master", "ctrl+alt+m").unwrap();
        assert_eq!(lookup(&shortcuts(&conn), "Ctrl+Alt+M"), Some("master"));
        // Le réglage de Studio Vault (écrit par Qt : « Esc ») est relu.
        conn.execute("UPDATE settings SET value = '{\"loop_clear\": \"Esc\", \"play\": \"\"}' WHERE key = 'shortcuts'", []).unwrap();
        let map = shortcuts(&conn);
        assert_eq!(lookup(&map, "Escape"), Some("loop_clear"));
        assert_eq!(lookup(&map, "Space"), None);
        reset_shortcuts(&conn).unwrap();
        assert_eq!(lookup(&shortcuts(&conn), "Space"), Some("play"));
        set_aliases(&conn, &[" Otoko No Sound ".into(), "".into(), "OTK".into()]).unwrap();
        assert_eq!(aliases(&conn), vec!["Otoko No Sound", "OTK"]);
    }

    #[test]
    fn cover_colours() {
        // Une pochette rouge vif : l'accent est rouge ; une grise : pas d'accent.
        let red: Vec<u8> = (0..64 * 64).flat_map(|_| [220u8, 30, 40, 255]).collect();
        let c = dominant_color(&red, 64, 64).unwrap();
        assert!(c.0 > 200 && c.1 < 50);
        let grey: Vec<u8> = (0..64 * 64).flat_map(|_| [120u8, 120, 120, 255]).collect();
        assert_eq!(dominant_color(&grey, 64, 64), None);
        assert_eq!(parse_hex("#3d8bfd"), Some((0x3d, 0x8b, 0xfd)));
    }
}
