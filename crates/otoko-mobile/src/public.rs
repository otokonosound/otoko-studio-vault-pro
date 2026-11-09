//! Serveur public des liens de partage, publié sur Internet par Tailscale Funnel (comme
//! `composer_vault/mobile/public.py`). Il écoute seulement sur 127.0.0.1 et ne connaît que les
//! adresses /s/<jeton>/… : pas de liste de la bibliothèque, pas de recherche, pas de chemins de
//! fichiers. Sans jeton valide, on ne voit rien. Les essais de mot de passe et de jetons au
//! hasard sont limités par adresse.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::{Value as Json, json};

use crate::http::{Conn, Request};
use crate::library::{self, List};
use crate::media;
use crate::server::{Out, State, cover, digits, is_loopback, send_file, tagged};
use crate::shares::{Share, ShareStore};
use crate::{log, web};

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
    font-src https://fonts.gstatic.com; img-src 'self' data:; media-src 'self'; connect-src 'self'; \
    manifest-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// En-têtes de sécurité ajoutés à toutes les réponses du serveur public.
pub fn headers() -> Vec<(&'static str, String)> {
    vec![
        ("X-Content-Type-Options", "nosniff".into()),
        ("Referrer-Policy", "no-referrer".into()),
        ("X-Frame-Options", "DENY".into()),
        ("Content-Security-Policy", CSP.into()),
        ("Permissions-Policy", "camera=(), microphone=(), geolocation=()".into()),
    ]
}

fn quality_label(q: &str) -> &'static str {
    match q {
        "lossless" => "FLAC sans perte",
        "high" => "AAC 256 kb/s",
        _ => "Fichier d'origine (WAV / FLAC)",
    }
}

/// Compteur glissant : au plus `limit` événements par `window` et par clé.
#[derive(Default)]
struct Limiter {
    hits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Limiter {
    fn blocked(&self, key: &str, limit: usize, window: u64) -> bool {
        let mut g = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let Some(q) = g.get_mut(key) else { return false };
        while q.front().is_some_and(|t| t.elapsed() > Duration::from_secs(window)) {
            q.pop_front();
        }
        q.len() >= limit
    }

    fn add(&self, key: &str) {
        let mut g = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        g.entry(key.to_string()).or_default().push_back(Instant::now());
        if g.len() > 5000 {
            g.retain(|_, q| q.back().is_some_and(|t| t.elapsed() < Duration::from_secs(3600)));
        }
    }
}

/// L'état propre au serveur public.
#[derive(Default)]
pub struct PublicState {
    limiter: Limiter,
    /// Contenu de chaque partage, gardé 30 s (une écoute fait beaucoup de requêtes).
    content: Mutex<HashMap<i64, (Instant, Json)>>,
    pending: Mutex<HashSet<PathBuf>>,
}

impl PublicState {
    /// Oublie le contenu gardé d'un partage (modifié, supprimé, téléchargé).
    pub fn forget(&self, id: i64) {
        self.content.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    }
}

fn pub_track(st: &State, t: &Json, tok: &str, owner: &str) -> Json {
    let id = t["id"].as_i64().unwrap_or(0);
    let artist = t["artist"].as_str().filter(|a| !a.is_empty()).unwrap_or(owner);
    json!({
        "id": id, "title": t["title"], "artist": artist, "duration": t["duration"], "bpm": t["bpm"],
        "key": t["camelot"].as_str().unwrap_or(""), "version": t["version_label"],
        "release": t["release"]["title"].as_str().unwrap_or(""),
        "cover": format!("/s/{tok}/t/{id}/cover"), "color": st.colors.get(id),
    })
}

/// Ce que le lien montre : sorties (avec leurs morceaux) et/ou morceaux, sans donnée interne.
pub fn share_content(st: &State, con: &Connection, sh: &Share) -> rusqlite::Result<Json> {
    let tok = &sh.token;
    let owner = library::artist_name(con);
    let mut rels = Vec::new();
    let mut tracks = Vec::new();
    if sh.kind == "releases" || sh.kind == "discography" {
        let all = library::list_releases(con)?;
        let every: HashMap<i64, &Json> = all.iter().map(|r| (r["id"].as_i64().unwrap_or(0), r)).collect();
        let mut ids: Vec<i64> = if sh.kind == "discography" {
            all.iter().filter(|r| r["upcoming"] == false && r["tracks"].as_i64().unwrap_or(0) > 0).filter_map(|r| r["id"].as_i64()).collect()
        } else {
            sh.items.iter().copied().filter(|i| every.contains_key(i)).collect()
        };
        if sh.kind == "releases" {
            // La plus récente d'abord, comme dans la Carrière.
            ids.sort_by_key(|i| std::cmp::Reverse((every[i]["date"].as_str().unwrap_or("").to_string(), *i)));
        }
        for rid in ids {
            let r = every[&rid];
            let list: Vec<Json> = library::release_tracks(con, rid)?.iter().map(|t| pub_track(st, t, tok, &owner)).collect();
            rels.push(json!({
                "id": rid, "title": r["title"], "type": r["type"], "type_label": r["type_label"], "date": r["date"],
                "upcoming": r["upcoming"], "description": r["description"], "cover": format!("/s/{tok}/r/{rid}/cover"),
                "color": st.colors.release(&st.cache, con, rid), "tracks": list,
            }));
        }
    } else {
        let order: HashMap<i64, usize> = sh.items.iter().enumerate().map(|(n, t)| (*t, n)).collect();
        let mut found = library::list_tracks(con, &List { ids: Some(sh.items.clone()), scope: "all".into(), sort: "added".into(), ..Default::default() })?;
        found.sort_by_key(|t| order.get(&t["id"].as_i64().unwrap_or(0)).copied().unwrap_or(0));
        tracks = found.iter().map(|t| pub_track(st, t, tok, &owner)).collect();
    }
    Ok(json!({ "artist": owner, "releases": rels, "tracks": tracks }))
}

pub fn allowed_tracks(content: &Json) -> HashSet<i64> {
    let mut ids: HashSet<i64> = content["tracks"].as_array().into_iter().flatten().filter_map(|t| t["id"].as_i64()).collect();
    for r in content["releases"].as_array().into_iter().flatten() {
        ids.extend(r["tracks"].as_array().into_iter().flatten().filter_map(|t| t["id"].as_i64()));
    }
    ids
}

/// Prépare, un fichier à la fois et en priorité basse, les versions Opus et AAC 128 kb/s.
fn convert_later(st: &State, paths: Vec<PathBuf>) {
    let Some(st) = st.arc() else { return };
    let todo: Vec<PathBuf> = {
        let mut p = st.public.pending.lock().unwrap_or_else(|e| e.into_inner());
        paths.into_iter().filter(|x| x.exists() && p.insert(x.clone())).collect()
    };
    if todo.is_empty() {
        return;
    }
    let _ = std::thread::Builder::new().name("partage-conv".into()).spawn(move || {
        for p in todo {
            for q in ["eco", "eco_aac"] {
                st.cache.transcoded(&p, q);
            }
            st.public.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&p);
        }
    });
}

/// Prépare (en fond) les versions d'écoute des morceaux des partages donnés, ou de tous les
/// partages actifs, pour que la lecture démarre tout de suite chez l'auditeur.
pub fn prewarm(st: &State, shares: Option<Vec<Share>>) {
    let Some(st) = st.arc() else { return };
    let _ = std::thread::Builder::new().name("partage-prep".into()).spawn(move || {
        let Some(store) = st.shares.as_ref() else { return };
        let items = shares.unwrap_or_else(|| store.all().into_iter().filter(|s| ShareStore::state(s) == "active").collect());
        let Ok(con) = st.con() else { return };
        let mut paths: Vec<PathBuf> = Vec::new();
        for sh in &items {
            let Ok(content) = share_content(&st, &con, sh) else { continue };
            let mut ids: Vec<i64> = allowed_tracks(&content).into_iter().collect();
            ids.sort_unstable();
            for tid in ids {
                if let Some(p) = library::track_path(&con, tid) {
                    if !paths.contains(&p) {
                        paths.push(p);
                    }
                }
            }
        }
        convert_later(&st, paths);
    });
}

// ---------------------------------------------------------------------- routes

fn ip(req: &Request) -> String {
    // Tailscale Funnel transmet l'adresse du visiteur dans X-Forwarded-For.
    req.header("X-Forwarded-For").and_then(|f| f.split(',').next()).map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).unwrap_or_else(|| req.peer.to_string())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

fn page(c: &mut Conn, status: u16, title: &str, text: &str) -> Out {
    let html = format!(
        "<!doctype html><html lang=fr><meta charset=utf-8><meta name=viewport content='width=device-width'><title>{title}</title>\
         <link rel=stylesheet href='/p/share.css'><body class=msg><main><h1>{title}</h1><p>{text}</p></main></body></html>"
    );
    c.send(status, "text/html; charset=utf-8", &[("Cache-Control", "no-store".into())], html.as_bytes())?;
    Ok(())
}

pub(crate) fn handle(st: &State, req: &Request, c: &mut Conn) -> std::io::Result<()> {
    // Seul Tailscale (sur cet ordinateur) doit pouvoir nous parler.
    if !is_loopback(req.peer) {
        return c.error(403, "Accès refusé.");
    }
    let r = match req.method.as_str() {
        "GET" | "HEAD" => public_get(st, req, c),
        "POST" => match req.parts.as_slice() {
            [s, tok, a] if s == "s" && a == "auth" => auth(st, req, c, tok),
            _ => Ok(c.error(404, "Adresse inconnue.")?),
        },
        _ => {
            c.keep_alive = false;
            Ok(c.error(405, "Méthode refusée.")?)
        }
    };
    match r {
        Ok(()) => Ok(()),
        Err(crate::server::Fail::Io(e)) => Err(e),
        Err(crate::server::Fail::Other(e)) => {
            log(st, &format!("[public] erreur : {e}"));
            c.error(500, "Erreur du serveur.")
        }
    }
}

/// Le partage du jeton s'il est valide ; sinon répond (404, 410, 429) et rend rien.
fn share(st: &State, req: &Request, c: &mut Conn, token: &str) -> std::io::Result<Option<Share>> {
    let ip = ip(req);
    let lim = &st.public.limiter;
    if lim.blocked(&format!("bad:{ip}"), 20, 600) {
        c.error(429, "Trop d'essais. Réessaie dans quelques minutes.")?;
        return Ok(None);
    }
    let Some(sh) = st.shares.as_ref().and_then(|s| s.by_token(token)) else {
        lim.add(&format!("bad:{ip}"));
        c.error(404, "Ce lien n'existe pas.")?;
        return Ok(None);
    };
    match ShareStore::state(&sh) {
        "revoked" => c.error(410, "Ce lien a été désactivé.")?,
        "expired" => c.error(410, "Ce lien a expiré.")?,
        _ => return Ok(Some(sh)),
    }
    Ok(None)
}

fn cookie<'a>(req: &'a Request, sh: &Share) -> Option<&'a str> {
    let name = format!("svp{}", sh.id);
    req.header("Cookie")?.split(';').filter_map(|kv| kv.trim().split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v.trim_matches('"'))
}

fn unlocked(st: &State, req: &Request, sh: &Share) -> bool {
    st.shares.as_ref().is_some_and(|s| s.cookie_ok(sh, cookie(req, sh)))
}

fn content(st: &State, sh: &Share) -> Result<Json, crate::server::Fail> {
    if let Some((at, v)) = st.public.content.lock().unwrap_or_else(|e| e.into_inner()).get(&sh.id) {
        if at.elapsed() < Duration::from_secs(30) {
            return Ok(v.clone());
        }
    }
    let con = st.con()?;
    let v = share_content(st, &con, sh)?;
    st.public.content.lock().unwrap_or_else(|e| e.into_inner()).insert(sh.id, (Instant::now(), v.clone()));
    Ok(v)
}

fn public_get(st: &State, req: &Request, c: &mut Conn) -> Out {
    let p: Vec<&str> = req.parts.iter().map(String::as_str).collect();
    let Some(store) = st.shares.as_ref() else { return page(c, 503, "Otoko Vault", "Les partages sont indisponibles pour le moment.") };
    match p.as_slice() {
        [] => return page(c, 404, "Otoko Vault", "Il faut un lien de partage pour écouter quelque chose ici."),
        ["p", name] => {
            let rel = if *name == "icon.png" { "icon.png".to_string() } else { format!("partage/{name}") };
            return match web::get(&rel) {
                Some((body, ctype)) => Ok(c.send(200, ctype, &[("Cache-Control", "max-age=3600".into())], body)?),
                None => page(c, 404, "Page introuvable", "Cette adresse n'existe pas."),
            };
        }
        ["s", token] => return share_page(st, req, c, token),
        ["s", _, ..] => {}
        _ => return page(c, 404, "Page introuvable", "Cette adresse n'existe pas."),
    }
    let (token, rest) = (p[1], &p[2..]);
    let Some(sh) = share(st, req, c, token)? else { return Ok(()) };
    if rest == ["api"] {
        if !unlocked(st, req, &sh) {
            let label = if sh.label.is_empty() { "Partage privé" } else { sh.label.as_str() };
            return Ok(c.json(401, &json!({ "password": true, "label": label, "artist": library::artist_name(&st.con()?) }))?);
        }
        let mut v = content(st, &sh)?;
        store.record(sh.id, "view", None);
        if let Some(o) = v.as_object_mut() {
            o.insert("label".into(), json!(sh.label));
            o.insert("message".into(), json!(sh.message));
            o.insert("kind".into(), json!(sh.kind));
            o.insert("expires_at".into(), json!(sh.expires_at));
            o.insert("can_download".into(), json!(ShareStore::can_download(&sh)));
            o.insert("download_quality".into(), json!(if sh.can_download { quality_label(&sh.download_quality) } else { "" }));
            o.insert("downloads_left".into(), json!(sh.max_downloads.map(|m| (m - sh.downloads).max(0))));
        }
        return Ok(c.json(200, &v)?);
    }
    if !unlocked(st, req, &sh) {
        return Ok(c.error(401, "Mot de passe requis.")?);
    }
    let ct = content(st, &sh)?;
    match rest {
        ["cover"] => {
            let con = st.con()?;
            let src = if let Some(r) = ct["releases"].get(0) {
                library::release_cover_sources(&con, r["id"].as_i64().unwrap_or(0))
            } else if let Some(t) = ct["tracks"].get(0) {
                library::cover_sources(&con, t["id"].as_i64().unwrap_or(0))
            } else {
                Vec::new()
            };
            drop(con);
            cover(st, req, c, &src)
        }
        ["r", rid, "cover"] if digits(rid).is_some() => {
            let rid = digits(rid).unwrap_or(0);
            if !ct["releases"].as_array().into_iter().flatten().any(|r| r["id"] == rid) {
                return Ok(c.error(404, "Introuvable.")?);
            }
            let src = library::release_cover_sources(&st.con()?, rid);
            cover(st, req, c, &src)
        }
        ["t", tid, what] if digits(tid).is_some() => {
            let tid = digits(tid).unwrap_or(0);
            if !allowed_tracks(&ct).contains(&tid) {
                return Ok(c.error(404, "Introuvable.")?);
            }
            track_public(st, req, c, &sh, tid, what)
        }
        _ => Ok(c.error(404, "Adresse inconnue.")?),
    }
}

fn track_public(st: &State, req: &Request, c: &mut Conn, sh: &Share, tid: i64, what: &str) -> Out {
    let store = st.shares.as_ref().expect("partages");
    let (path, sources) = {
        let con = st.con()?;
        (library::track_path(&con, tid), library::cover_sources(&con, tid))
    };
    if what == "cover" {
        return cover(st, req, c, &sources);
    }
    let Some(path) = path.filter(|p| p.exists()) else { return Ok(c.error(404, "Ce morceau n'est pas disponible pour le moment.")?) };
    let first = req.header("Range").unwrap_or("bytes=0-").trim() == "bytes=0-";
    let get = req.method == "GET";
    // « bytes=1- » ne doit pas permettre de télécharger sans être compté : toute demande de téléchargement compte.
    let counted = what == "download" && get;
    match what {
        "stream" => {
            // 128 kb/s : démarre vite, même par le relais Funnel.
            let q = if req.q("f") == "aac" { "eco_aac" } else { "eco" };
            let (mut alt, mut mime) = (st.cache.cached(&path, q), media::quality(q).map(|x| x.1).unwrap_or("audio/ogg"));
            if alt.is_none() {
                // Pas encore converti : on sert tout de suite une version prête (Funnel coupe une
                // requête qui ne répond pas assez vite) et on prépare la version légère en fond.
                convert_later(st, vec![path.clone()]);
                let flac = if media::is_lossless_source(&path) { st.cache.cached(&path, "lossless") } else { None };
                let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                if flac.is_some() {
                    (alt, mime) = (flac, "audio/flac");
                } else if ["mp3", "m4a", "ogg", "opus", "flac", "aac"].contains(&ext.as_str()) {
                    (alt, mime) = (Some(path.clone()), media::path_mime(&path));
                } else {
                    alt = st.cache.transcoded(&path, q);
                }
            }
            let Some(alt) = alt else { return Ok(c.error(503, "Lecture indisponible pour le moment.")?) };
            if first && get {
                store.record(sh.id, "play", Some(tid));
            }
            send_file(c, req, &alt, mime, None, false)
        }
        "download" => {
            let sh = store.by_token(&sh.token).unwrap_or_else(|| sh.clone()); // compteur à jour
            if !ShareStore::can_download(&sh) {
                return Ok(c.error(403, "Le téléchargement n'est pas autorisé pour ce lien (ou la limite est atteinte).")?);
            }
            let mut quality = sh.download_quality.as_str();
            if quality == "lossless" && !media::is_lossless_source(&path) {
                quality = "original";
            }
            let (file, mime) = if quality == "original" {
                (path.clone(), media::path_mime(&path))
            } else {
                let q = if quality == "lossless" { "lossless" } else { "high_aac" };
                let Some(f) = st.cache.transcoded(&path, q) else { return Ok(c.error(503, "Conversion indisponible pour le moment.")?) };
                (f, media::quality(q).map(|x| x.1).unwrap_or("application/octet-stream"))
            };
            if counted {
                store.record(sh.id, "download", Some(tid));
            }
            st.public.forget(sh.id);
            tagged(st, req, c, tid, &file, mime, true)
        }
        _ => Ok(c.error(404, "Adresse inconnue.")?),
    }
}

fn auth(st: &State, req: &Request, c: &mut Conn, token: &str) -> Out {
    let ip = ip(req);
    if req.body.is_empty() {
        return Ok(c.error(400, "Requête invalide.")?);
    }
    let Some(sh) = share(st, req, c, token)? else { return Ok(()) };
    let store = st.shares.as_ref().expect("partages");
    let lim = &st.public.limiter;
    if lim.blocked(&format!("pw:{ip}"), 8, 900) || lim.blocked(&format!("pws:{}", sh.id), 40, 3600) {
        return Ok(c.error(429, "Trop d'essais. Réessaie dans un quart d'heure.")?);
    }
    let Ok(b) = serde_json::from_slice::<Json>(&req.body) else { return Ok(c.error(400, "Requête invalide.")?) };
    let pw = match &b["password"] {
        Json::String(s) => s.clone(),
        Json::Null => String::new(),
        other => other.to_string(),
    };
    if !store.check_password(&sh, &pw) {
        lim.add(&format!("pw:{ip}"));
        lim.add(&format!("pws:{}", sh.id));
        std::thread::sleep(Duration::from_millis(400));
        return Ok(c.error(403, "Mot de passe incorrect.")?);
    }
    let ck = format!("svp{}={}; Path=/s/{token}; Max-Age=2592000; HttpOnly; Secure; SameSite=Strict", sh.id, store.cookie_value(&sh));
    c.send(200, "application/json; charset=utf-8", &[("Set-Cookie", ck), ("Cache-Control", "no-store".into())], br#"{"ok":true}"#)?;
    Ok(())
}

/// La page du lien, avec titre et pochette pour l'aperçu dans WhatsApp, Messenger, les mails…
/// (seulement si le lien est valide et sans mot de passe).
fn share_page(st: &State, req: &Request, c: &mut Conn, token: &str) -> Out {
    let mut html = String::from_utf8_lossy(web::get("partage/share.html").map(|x| x.0).unwrap_or_default()).into_owned();
    let sh = st.shares.as_ref().and_then(|s| s.by_token(token));
    if let Some(sh) = sh.filter(|s| ShareStore::state(s) == "active" && s.pw_hash.is_none()) {
        let ct = content(st, &sh)?;
        let rels = ct["releases"].as_array().cloned().unwrap_or_default();
        let title = if !sh.label.is_empty() {
            sh.label.clone()
        } else if rels.len() == 1 {
            rels[0]["title"].as_str().unwrap_or("").to_string()
        } else {
            "Écoute privée".to_string()
        };
        let n = ct["tracks"].as_array().map_or(0, |t| t.len()) + rels.iter().map(|r| r["tracks"].as_array().map_or(0, |t| t.len())).sum::<usize>();
        let owner = ct["artist"].as_str().unwrap_or("");
        let what = format!("{n} titre{} à écouter", if n > 1 { "s" } else { "" });
        let desc = if !sh.message.is_empty() {
            sh.message.clone()
        } else if owner.is_empty() {
            what
        } else {
            format!("{owner} · {what}")
        };
        let host = req.header("X-Forwarded-Host").or(req.header("Host")).unwrap_or("").trim();
        let base = if !host.is_empty() && !host.contains("127.0.0.1") { esc(&format!("https://{host}")) } else { String::new() };
        let short: String = desc.chars().take(200).collect();
        let meta = format!(
            "<meta property=\"og:type\" content=\"music.album\"><meta property=\"og:title\" content=\"{}\">\
             <meta property=\"og:description\" content=\"{}\"><meta property=\"og:image\" content=\"{base}/s/{token}/cover?size=600\">\
             <meta property=\"og:image:width\" content=\"600\"><meta property=\"og:image:height\" content=\"600\">\
             <meta name=\"twitter:card\" content=\"summary_large_image\">",
            esc(&title),
            esc(&short)
        );
        let page_title = if owner.is_empty() { esc(&title) } else { format!("{} · {}", esc(&title), esc(owner)) };
        html = html.replace("<title>Écoute privée</title>", &format!("<title>{page_title}</title>{meta}"));
    }
    c.send(200, "text/html; charset=utf-8", &[("Cache-Control", "no-store".into())], html.as_bytes())?;
    Ok(())
}
