//! Ce PC connecté au PC principal : il travaille sur une copie de la base (`career::distant`), envoie ses écritures et reprend la
//! nouvelle copie quand le PC principal a changé. La musique et les pochettes restent sur le PC principal : elles arrivent à la demande
//! et sont gardées dans un cache.

use std::path::{Path, PathBuf};

use otoko_library::career::{Career, distant};
use serde_json::{Value as Json, json};

use slint::ComponentHandle;

use crate::fond::{fetch_file, fetch_json, main_file, main_json, post_json};
use crate::{AppWindow, CPrefs};
use crate::state::{Done, State};

/// Tous les combien de secondes regarder s'il y a à faire.
pub const TICK_SECS: u64 = 5;
/// Le PC principal n'est interrogé que toutes les douze fois (une minute).
const PULL_EVERY: u32 = 12;
/// La place laissée au cache de musique.
const AUDIO_CACHE: u64 = 3 * 1024 * 1024 * 1024;

/// Le lien avec le PC principal.
#[derive(Default)]
pub struct Distant {
    pub url: String,
    /// Le code de liaison du PC principal : sans lui, il ne donne pas sa base.
    pub code: String,
    pub stamp: String,
    pub online: bool,
    /// Le PC principal a refusé le code de liaison : le dire, pas « injoignable ».
    pub refused: bool,
    pub busy: bool,
    pub ticks: u32,
}

fn file() -> PathBuf {
    crate::library::data_dir().join("distant.json")
}

/// Le dossier de ce mode : la copie de la base et les caches.
pub fn dir() -> PathBuf {
    crate::library::data_dir().join("distant")
}

pub fn base_path() -> PathBuf {
    dir().join("carriere.db")
}

/// L'adresse telle qu'on l'écrit (« mon-pc.xxxx.ts.net », « 100.64.1.2 ») devient une adresse complète, avec le
/// port du serveur. Une adresse en https garde le sien : c'est Tailscale qui le choisit.
pub fn normalize(addr: &str) -> String {
    let a = addr.trim();
    let (scheme, rest) = a.split_once("://").map(|(s, r)| (s.to_lowercase(), r)).unwrap_or(("http".to_string(), a));
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() {
        return String::new();
    }
    let has_port = host.rsplit_once(':').is_some_and(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if has_port || scheme == "https" { format!("{scheme}://{host}") } else { format!("{scheme}://{host}:{}", otoko_mobile::PORT) }
}

/// L'adresse du PC principal, si ce PC y est relié.
pub fn address() -> Option<String> {
    let t = std::fs::read_to_string(file()).ok()?;
    let v: Json = serde_json::from_str(&t).ok()?;
    v["adresse"].as_str().map(normalize).filter(|a| !a.is_empty())
}

/// Le code de liaison du PC principal, tel que recopié ici.
pub fn code() -> String {
    std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str::<Json>(&t).ok()).and_then(|v| v["code"].as_str().map(str::to_string)).unwrap_or_default()
}

/// Relie ce PC au PC principal (adresse et code de liaison), ou l'en détache.
pub fn set_address(link: Option<(&str, &str)>) {
    let _ = match link {
        Some((a, code)) => std::fs::write(file(), json!({ "adresse": normalize(a), "code": code.trim() }).to_string()),
        None => std::fs::remove_file(file()),
    };
}

/// Un serveur d'avant la 1.1 ne connaît pas ce mode.
fn too_old(version: &str) -> bool {
    let mut n = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    (n.next().unwrap_or(0), n.next().unwrap_or(0)) < (1, 1)
}

/// Le PC principal répond-il ? Rend de quoi l'afficher.
pub fn test(url: &str, code: &str) -> Result<String, String> {
    let ping = fetch_json(&format!("{url}/api/ping")).map_err(|_| format!("Rien ne répond à {url}. Vérifie que Tailscale est connecté sur les deux PC et que le serveur tourne sur le PC principal."))?;
    let version = ping["version"].as_str().unwrap_or("");
    if too_old(version) {
        return Err(format!("Le serveur du PC principal est en version {version}, trop ancienne pour ce mode. Sur le PC principal, lance « Installer le serveur mobile » (dossier mobile du logiciel), puis réessaie."));
    }
    if otoko_mobile::link_normalize(code).len() < 16 {
        return Err("Recopie aussi le code de liaison du PC principal : il l'affiche dans Réglages › Général, sous « Travailler depuis un autre PC ».".to_string());
    }
    let state = main_json(&format!("{url}/api/distant/etat"), code).map_err(|e| {
        if e == crate::fond::REFUSED_CODE { e } else { "Le PC principal répond, mais il refuse cette adresse : passe par Tailscale, et vérifie que son serveur est à jour (« Installer le serveur mobile »).".to_string() }
    })?;
    let here = std::env::var("COMPUTERNAME").unwrap_or_default();
    if !here.is_empty() && state["machine"].as_str().is_some_and(|m| m.eq_ignore_ascii_case(&here)) {
        return Err("C'est l'adresse de ce PC : le PC principal travaille sur sa propre base. Ce réglage sert sur un autre ordinateur.".to_string());
    }
    Ok(format!("Le PC principal répond : {} morceaux, {} sorties.", ping["tracks"], ping["releases"]))
}

/// « Tester » et « Se connecter » : le réseau peut tarder à répondre, l'interface n'attend pas.
pub fn try_address(app: &AppWindow, st: &mut State, connect: bool) {
    let g = app.global::<CPrefs>();
    let url = normalize(&g.get_remote_address());
    if url.is_empty() {
        g.set_remote_info("Indique d'abord l'adresse du PC principal.".into());
        return;
    }
    let code = g.get_remote_code().trim().to_string();
    g.set_remote_address(url.as_str().into());
    g.set_remote_info("Recherche du PC principal…".into());
    st.spawn(move || {
        let result = test(&url, &code);
        Done::RemoteTest { url, code, connect, result }
    });
}

pub fn tried(app: &AppWindow, url: &str, code: &str, connect: bool, r: Result<String, String>) {
    let g = app.global::<CPrefs>();
    match r {
        Ok(_) if connect => {
            set_address(Some((url, code)));
            g.set_remote_on(true);
            g.set_remote_info("Relié au PC principal : relance le logiciel pour travailler sur sa base.".into());
        }
        Ok(text) | Err(text) => g.set_remote_info(text.into()),
    }
}

fn new_copy() -> PathBuf {
    dir().join("carriere.nouvelle.db")
}

/// Rapatrie la base du PC principal à côté de la copie en place : une coupure ne casse rien, et l'interface continue pendant ce temps.
fn fetch_copy(url: &str, code: &str) -> Result<String, String> {
    let _ = std::fs::create_dir_all(dir());
    let stamp = main_json(&format!("{url}/api/distant/etat"), code)?["stamp"].as_str().unwrap_or("").to_string();
    main_file(&format!("{url}/api/distant/base"), &new_copy(), code)?;
    prepare(&new_copy())?;
    Ok(stamp)
}

/// Met la nouvelle copie à la place de l'ancienne. La base doit être fermée.
fn swap_copy() -> Result<(), String> {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", base_path().display()));
    }
    std::fs::rename(new_copy(), base_path()).map_err(|e| e.to_string())
}

/// Met le journal et les déclencheurs dans la copie.
fn prepare(path: &Path) -> Result<(), String> {
    let c = Career::open(path).map_err(|e| e.to_string())?;
    distant::install(c.conn()).map_err(|e| e.to_string())?;
    c.conn().execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;").map_err(|e| e.to_string())
}

/// Au lancement : la base du PC principal, sinon la dernière copie (hors ligne, les écritures attendront).
pub fn boot(url: &str, code: &str) -> Distant {
    // La copie en place reprend les déclencheurs de cette version (colonnes changées seulement) avant toute écriture.
    let waiting = base_path().is_file()
        && Career::open(&base_path())
            .map(|c| {
                let _ = distant::install(c.conn());
                distant::waiting(c.conn())
            })
            .unwrap_or(0)
            > 0;
    let (online, refused, stamp) = if waiting {
        // Des écritures attendent : la copie est gardée, elles partiront d'abord.
        (true, false, String::new())
    } else {
        match fetch_copy(url, code).and_then(|s| swap_copy().map(|()| s)) {
            Ok(s) => (true, false, s),
            Err(e) => (false, e == crate::fond::REFUSED_CODE, String::new()),
        }
    };
    Distant { url: url.to_string(), code: code.to_string(), stamp, online, refused, ..Default::default() }
}

/// Toutes les quelques secondes : envoyer ce qui attend, puis regarder si le PC principal a changé.
pub fn tick(st: &mut State) {
    let Some(d) = st.distant.as_mut() else { return };
    if d.busy {
        return;
    }
    d.ticks += 1;
    let waiting = st.career.as_ref().map(|c| distant::waiting(c.conn())).unwrap_or(0);
    let Some(d) = st.distant.as_mut() else { return };
    let pull = d.ticks % PULL_EVERY == 0;
    if waiting == 0 && !pull {
        return;
    }
    d.busy = true;
    let (url, code, stamp, path) = (d.url.clone(), d.code.clone(), d.stamp.clone(), st.career_path.clone());
    st.spawn(move || {
        if waiting > 0 {
            // Le PC principal a pu changer de son côté : on le regarde avant d'écrire, après ce ne serait plus visible.
            let stale = main_json(&format!("{url}/api/distant/etat"), &code).map(|v| v["stamp"].as_str().unwrap_or("") != stamp).unwrap_or(false);
            return Done::Distant(push(&url, &code, &path).map(|s| (s, stale)));
        }
        Done::Distant(match main_json(&format!("{url}/api/distant/etat"), &code) {
            Ok(v) => {
                let now = v["stamp"].as_str().unwrap_or("").to_string();
                Ok((now.clone(), now != stamp))
            }
            Err(e) => Err(e),
        })
    });
}

/// Envoie le journal. Rend le nouvel état du PC principal.
fn push(url: &str, code: &str, path: &Path) -> Result<String, String> {
    let c = Career::open(path).map_err(|e| e.to_string())?;
    let (changes, last) = distant::pending(c.conn(), 500).map_err(|e| e.to_string())?;
    let v = post_json(&format!("{url}/api/distant/ecrire"), &json!({ "changes": changes }), code)?;
    distant::sent(c.conn(), last).map_err(|e| e.to_string())?;
    Ok(v["stamp"].as_str().unwrap_or("").to_string())
}

/// Ce que le tour a rendu : (état du PC principal, faut-il reprendre sa base).
pub fn ticked(app: &AppWindow, st: &mut State, r: Result<(String, bool), String>) {
    let Some(d) = st.distant.as_mut() else { return };
    d.busy = false;
    d.refused = r.as_ref().is_err_and(|e| e == crate::fond::REFUSED_CODE);
    match r {
        Ok((stamp, stale)) => {
            d.online = true;
            d.stamp = stamp;
            if stale {
                reload(app, st);
                return;
            }
        }
        Err(_) => d.online = false,
    }
    show(app, st);
}

/// Une écriture attend ici : la reprise est remise à plus tard, sinon elle l'effacerait.
fn later(st: &mut State) -> bool {
    let waiting = st.career.as_ref().is_some_and(|c| distant::waiting(c.conn()) > 0);
    if let (true, Some(d)) = (waiting, st.distant.as_mut()) {
        d.stamp.clear();
    }
    waiting
}

/// Le PC principal a changé et rien n'attend ici : on reprend sa base, en arrière-plan.
fn reload(app: &AppWindow, st: &mut State) {
    if later(st) {
        show(app, st);
        return;
    }
    let Some(d) = st.distant.as_mut() else { return };
    let (url, code) = (d.url.clone(), d.code.clone());
    d.busy = true;
    st.spawn(move || Done::DistantCopy(fetch_copy(&url, &code)));
}

/// La nouvelle copie est là : la base se ferme le temps de la remplacer.
pub fn copied(app: &AppWindow, st: &mut State, r: Result<String, String>) {
    let Some(d) = st.distant.as_mut() else { return };
    d.busy = false;
    d.online = r.is_ok();
    d.refused = r.as_ref().is_err_and(|e| e == crate::fond::REFUSED_CODE);
    if let Ok(stamp) = r
        && !later(st)
    {
        st.career = None;
        let swapped = swap_copy().is_ok();
        st.career = crate::career::open(&st.career_path);
        // Une copie en place encore tenue : on recommencera au prochain tour.
        if let Some(d) = st.distant.as_mut() {
            d.stamp = if swapped { stamp } else { String::new() };
        }
        if swapped {
            crate::career::fill(app, st);
        }
    }
    show(app, st);
}

/// L'état du lien, dans la barre du bas.
pub fn show(app: &AppWindow, st: &State) {
    let Some(d) = st.distant.as_ref() else {
        app.set_remote_text("".into());
        return;
    };
    let waiting = st.career.as_ref().map(|c| distant::waiting(c.conn())).unwrap_or(0);
    app.set_remote_text(
        if d.refused {
            format!("Code de liaison refusé par le PC principal (Réglages › Général){}", if waiting > 0 { format!(" · {waiting} écriture(s) en attente") } else { String::new() })
        } else if !d.online {
            format!("PC principal injoignable{}", if waiting > 0 { format!(" · {waiting} écriture(s) en attente") } else { String::new() })
        } else if waiting > 0 {
            format!("Connecté au PC principal · envoi de {waiting} écriture(s)…")
        } else {
            "Connecté au PC principal".to_string()
        }
        .into(),
    );
}

// ---------------------------------------------------------------------- la musique et les pochettes

fn audio_file(id: i64) -> PathBuf {
    dir().join("musique").join(format!("{id}.flac"))
}

/// Le morceau, s'il est déjà là.
pub fn cached_audio(id: i64) -> Option<PathBuf> {
    Some(audio_file(id)).filter(|p| p.is_file())
}

/// Va chercher un morceau chez le PC principal, puis le joue.
pub fn fetch_audio(app: &AppWindow, st: &mut State, id: i64, at: f64) {
    let Some(d) = st.distant.as_ref() else { return };
    let url = format!("{}/api/tracks/{id}/audio?quality=lossless", d.url);
    app.set_message("Le morceau arrive du PC principal…".into());
    st.spawn(move || {
        let out = audio_file(id);
        let _ = std::fs::create_dir_all(out.parent().unwrap_or(Path::new(".")));
        let tmp = out.with_extension("part");
        let r = fetch_file(&url, &tmp).and_then(|()| std::fs::rename(&tmp, &out).map_err(|e| e.to_string()));
        trim_cache();
        Done::Audio { id, at, result: r }
    });
}

pub fn audio_landed(app: &AppWindow, st: &mut State, id: i64, at: f64, r: Result<(), String>) {
    match r {
        Ok(()) => crate::lecteur::play_id(app, st, id, at),
        Err(e) => app.set_message(format!("Morceau indisponible : {e}").into()),
    }
}

/// Garde le cache sous sa limite : les morceaux les plus anciens partent d'abord.
fn trim_cache() {
    let Ok(list) = std::fs::read_dir(dir().join("musique")) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = list.flatten().filter_map(|e| { let m = e.metadata().ok()?; Some((m.modified().ok()?, m.len(), e.path())) }).collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, size, path) in files {
        if total <= AUDIO_CACHE {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            total -= size;
        }
    }
}

/// Une pochette du PC principal (`tracks` ou `releases`), gardée sur le disque.
pub fn cover(url: &str, kind: &str, id: i64, size: u32) -> Option<Vec<u8>> {
    let f = dir().join("pochettes").join(format!("{kind}-{id}-{size}.jpg"));
    if let Ok(b) = std::fs::read(&f) {
        return (!b.is_empty()).then_some(b);
    }
    let _ = std::fs::create_dir_all(f.parent()?);
    // Un morceau sans pochette laisse un fichier vide : il n'est pas redemandé.
    if fetch_file(&format!("{url}/api/{kind}/{id}/cover?size={size}"), &f).is_err() {
        let _ = std::fs::write(&f, b"");
    }
    std::fs::read(&f).ok().filter(|b| !b.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{normalize, too_old};

    #[test]
    fn an_address_gets_what_is_missing() {
        assert_eq!(normalize(" mon-pc.x.ts.net/ "), "http://mon-pc.x.ts.net:8775");
        assert_eq!(normalize("100.64.1.2"), "http://100.64.1.2:8775");
        assert_eq!(normalize("http://mini"), "http://mini:8775");
        assert_eq!(normalize("mini:9000"), "http://mini:9000");
        assert_eq!(normalize("https://mini.ts.net:8444/api/ping"), "https://mini.ts.net:8444");
        assert_eq!(normalize("HTTPS://mini.ts.net"), "https://mini.ts.net");
        assert_eq!(normalize("   "), "");
    }

    #[test]
    fn a_server_before_1_1_is_too_old() {
        assert!(too_old("0.4.0"));
        assert!(too_old("1.0.0"));
        assert!(too_old(""));
        assert!(!too_old("1.1.0"));
        assert!(!too_old("2.0.0"));
    }
}
