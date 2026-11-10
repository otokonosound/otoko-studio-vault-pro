//! Le serveur privé de l'app mobile (comme `composer_vault/mobile/server.py`, mêmes adresses,
//! même JSON). N'accepte que cet ordinateur, les appareils Tailscale (100.64.0.0/10) et, si on l'a
//! autorisé, ceux du Wi-Fi de la maison (192.168.x.x, 10.x.x.x, 172.16-31.x.x) ; jamais visible
//! depuis Internet. Les partages se gèrent ici (`/api/shares`), depuis cet ordinateur ou Tailscale
//! seulement.

use std::collections::HashMap;
use std::net::IpAddr;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use otoko_library::career::release_color;
use rusqlite::Connection;
use serde_json::{Value as Json, json};

use crate::http::{Conn, Request, encode};
use crate::library::{self, List, Sources};
use crate::media::{self, Cache};
use crate::shares::{ShareError, ShareStore};
use crate::{Config, log, public, web};

/// Une erreur qui devient une réponse 500, ou la connexion coupée par le téléphone.
pub(crate) enum Fail {
    Io(std::io::Error),
    Other(String),
}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Fail {
        Fail::Io(e)
    }
}

impl From<rusqlite::Error> for Fail {
    fn from(e: rusqlite::Error) -> Fail {
        Fail::Other(e.to_string())
    }
}

pub(crate) type Out = Result<(), Fail>;

/// Tout ce que partagent les deux serveurs.
pub struct State {
    pub cfg: Config,
    pub cache: Cache,
    pub shares: Option<ShareStore>,
    pub colors: Colors,
    pub https: Https,
    pub(crate) port: AtomicU16,
    lan: Mutex<Option<(Instant, Option<String>)>>,
    lan_ok: Mutex<Option<(Instant, bool)>>,
    pub(crate) public: public::PublicState,
    /// Pour confier l'état aux fils d'arrière-plan.
    pub(crate) me: OnceLock<Weak<State>>,
}

impl State {
    pub(crate) fn new(cfg: Config, shares: Option<ShareStore>) -> State {
        let cache = Cache::new(cfg.cache.clone()).borrowing(cfg.borrow_cache.clone());
        let https = Https::new(cfg.port, cfg.public_port.unwrap_or(0), cfg.funnel_port);
        State {
            port: AtomicU16::new(cfg.port),
            cfg,
            cache,
            shares,
            colors: Colors::default(),
            https,
            lan: Mutex::new(None),
            lan_ok: Mutex::new(None),
            public: public::PublicState::default(),
            me: OnceLock::new(),
        }
    }

    pub(crate) fn arc(&self) -> Option<Arc<State>> {
        self.me.get().and_then(Weak::upgrade)
    }

    pub fn con(&self) -> rusqlite::Result<Connection> {
        library::connect(&self.cfg.db)
    }

    pub fn port(&self) -> u16 {
        self.port.load(Ordering::Relaxed)
    }

    /// « http://<IP locale>:<port> » : lecture directe à la maison et diffusion vers la Mu-so.
    pub fn lan_url(&self) -> Option<String> {
        let mut g = self.lan.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, url)) = g.as_ref() {
            if at.elapsed() < Duration::from_secs(60) {
                return url.clone();
            }
        }
        let url = lan_ip().map(|ip| format!("http://{ip}:{}", self.port()));
        *g = Some((Instant::now(), url.clone()));
        url
    }

    /// Le Wi-Fi de la maison est-il autorisé ? Relu toutes les 5 secondes : le choix s'applique sans redémarrer.
    fn lan_allowed(&self) -> bool {
        let mut g = self.lan_ok.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, ok)) = *g {
            if at.elapsed() < Duration::from_secs(5) {
                return ok;
            }
        }
        let ok = self.cfg.db.parent().is_some_and(crate::lan_get);
        *g = Some((Instant::now(), ok));
        ok
    }
}

// ---------------------------------------------------------------------- réseau

/// En-têtes de sécurité du serveur privé. La page mobile a son script dans le fichier et sa police chez Google.
pub fn headers() -> Vec<(&'static str, String)> {
    const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
        font-src https://fonts.gstatic.com; img-src 'self' data:; media-src 'self'; connect-src 'self'; \
        base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
    vec![
        ("X-Content-Type-Options", "nosniff".into()),
        ("Referrer-Policy", "no-referrer".into()),
        ("X-Frame-Options", "DENY".into()),
        ("Content-Security-Policy", CSP.into()),
    ]
}

pub fn is_tailscale(ip: IpAddr) -> bool {
    match canonical(ip) {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (o[1] & 0xC0) == 64
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    }
}

/// Le nom demandé par le navigateur. Un site piégé peut faire pointer son propre domaine vers cet ordinateur et
/// lire ensuite ce serveur comme s'il était chez lui (DNS rebinding) : seuls passent une adresse IP, un nom de machine
/// sans point (« localhost », « mon-pc ») et les noms locaux ou Tailscale, qu'un site public ne peut pas détourner.
pub fn host_allowed(host: &str) -> bool {
    let h = host.trim().to_lowercase();
    let name = match h.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => h.rsplit_once(':').filter(|(_, port)| port.bytes().all(|b| b.is_ascii_digit())).map_or(h.as_str(), |(n, _)| n),
    };
    let name = name.trim_end_matches('.');
    name.parse::<IpAddr>().is_ok() || !name.contains('.') || name.ends_with(".ts.net") || name.ends_with(".local")
}

/// D'où vient vraiment la requête : derrière « tailscale serve » la connexion vient de cet ordinateur, et Tailscale
/// écrit la véritable adresse dans X-Forwarded-For.
fn origin(req: &Request) -> IpAddr {
    if is_loopback(req.peer) {
        if let Some(ip) = req.header("X-Forwarded-For").and_then(|f| f.split(',').next()).and_then(|f| f.trim().parse().ok()) {
            return ip;
        }
    }
    req.peer
}

/// Le message de refus, ou rien si la requête peut passer.
fn refused(st: &State, req: &Request) -> Option<&'static str> {
    let from = origin(req);
    // Ce serveur n'est jamais publié : même si quelqu'un l'expose par Funnel, Internet reste dehors.
    if !client_allowed(from) || req.header("Tailscale-Funnel-Request").is_some() {
        return Some("Accès réservé à tes appareils Tailscale.");
    }
    // Une adresse 100.x peut aussi être celle d'un voisin chez un fournisseur d'accès : elle doit arriver par Tailscale.
    if is_tailscale(req.peer) && !is_tailscale(req.local) {
        return Some("Accès réservé à tes appareils Tailscale.");
    }
    if is_lan(from) && !st.lan_allowed() {
        return Some("Le Wi-Fi de la maison n'est pas autorisé sur ce PC (page Mobile et partage). Passe par Tailscale.");
    }
    if !host_allowed(req.header("Host").unwrap_or("")) {
        return Some("Adresse refusée.");
    }
    None
}

fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    }
}

/// Wi-Fi de la maison (adresses privées), ni Tailscale ni cet ordinateur.
pub fn is_lan(ip: IpAddr) -> bool {
    matches!(canonical(ip), IpAddr::V4(v4) if v4.is_private())
}

pub fn is_loopback(ip: IpAddr) -> bool {
    canonical(ip).is_loopback()
}

/// Cet ordinateur, Tailscale, le Wi-Fi de la maison.
pub fn client_allowed(ip: IpAddr) -> bool {
    match canonical(ip) {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback() || v4.is_private() || (o[0] == 100 && (o[1] & 0xC0) == 64)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            v6.is_loopback() || (s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0)
        }
    }
}

/// IPv4 de cet ordinateur sur le réseau local (celle de la route par défaut, pas Tailscale).
pub fn lan_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // Aucun paquet envoyé : sert seulement à choisir l'interface.
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    is_lan(ip).then(|| ip.to_string())
}

/// Lance un petit programme sans fenêtre et rend sa sortie (rien s'il dépasse `timeout`).
fn run_quiet(exe: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    use std::io::Read;
    let mut child = std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = out.read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    reader.join().ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub fn tailscale_exe() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            let p = d.join("tailscale.exe");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    [r"C:\Program Files\Tailscale\tailscale.exe", r"C:\Program Files (x86)\Tailscale\tailscale.exe"].iter().map(PathBuf::from).find(|p| p.is_file())
}

/// Tailscale sur ce PC : (installé, le nom de ce PC s'il est connecté).
pub fn tailscale_state() -> (bool, Option<String>) {
    let Some(exe) = tailscale_exe() else { return (false, None) };
    let st: Json = run_quiet(&exe, &["status", "--json"], Duration::from_secs(10)).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Json::Null);
    let name = st["Self"]["DNSName"].as_str().map(|n| n.trim_end_matches('.').to_string()).filter(|n| !n.is_empty());
    (true, name.filter(|_| st["BackendState"] == "Running"))
}

#[derive(Default, Clone)]
struct HttpsInfo {
    /// https://<mini>.ts.net si « tailscale serve » publie ce serveur.
    url: Option<String>,
    /// mon-pc.xxxx.ts.net
    name: Option<String>,
    /// Adresse publique (Funnel) qui mène au serveur des partages.
    public: Option<String>,
    checked: Option<Instant>,
}

/// Les adresses que donne Tailscale (HTTPS privé, partage public par Funnel), relues toutes
/// les 5 minutes.
pub struct Https {
    info: Mutex<HttpsInfo>,
    port: u16,
    public_port: u16,
    funnel_port: u16,
}

/// Ce que publie « tailscale serve » : (adresse HTTPS privée, adresse publique Funnel).
pub fn read_serve(serve: &Json, port: u16, public_port: u16) -> (Option<String>, Option<String>) {
    let (mut url, mut public) = (None, None);
    let Some(web) = serve["Web"].as_object() else { return (None, None) };
    for (hostport, cfg) in web {
        let Some(proxy) = cfg["Handlers"]["/"]["Proxy"].as_str() else { continue };
        let target: u16 = proxy.trim_end_matches('/').rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let base = format!("https://{}", hostport.strip_suffix(":443").unwrap_or(hostport));
        if target == port && port != 0 {
            url = Some(base);
        } else if target == public_port && public_port != 0 && serve["AllowFunnel"][hostport].as_bool() == Some(true) {
            public = Some(base);
        }
    }
    (url, public)
}

impl Https {
    fn new(port: u16, public_port: u16, funnel_port: u16) -> Https {
        Https { info: Mutex::new(HttpsInfo::default()), port, public_port, funnel_port }
    }

    fn detect(&self) -> HttpsInfo {
        let mut info = HttpsInfo { checked: Some(Instant::now()), ..Default::default() };
        let Some(exe) = tailscale_exe() else { return info };
        let t = Duration::from_secs(10);
        let st: Json = run_quiet(&exe, &["status", "--json"], t).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Json::Null);
        info.name = st["Self"]["DNSName"].as_str().map(|n| n.trim_end_matches('.').to_string()).filter(|n| !n.is_empty());
        let serve: Json = run_quiet(&exe, &["serve", "status", "--json"], t).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Json::Null);
        (info.url, info.public) = read_serve(&serve, self.port, self.public_port);
        info
    }

    /// Relit Tailscale si c'est trop ancien (hors verrou : les autres requêtes ne l'attendent pas).
    fn current(&self) -> HttpsInfo {
        let stale = {
            let mut g = self.info.lock().unwrap_or_else(|e| e.into_inner());
            let stale = g.checked.is_none_or(|c| c.elapsed() > Duration::from_secs(300));
            if stale {
                g.checked = Some(Instant::now());
            }
            stale
        };
        if stale {
            let fresh = self.detect();
            *self.info.lock().unwrap_or_else(|e| e.into_inner()) = fresh;
        }
        self.info.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn refresh(&self) {
        self.info.lock().unwrap_or_else(|e| e.into_inner()).checked = None;
    }

    pub fn get(&self) -> Option<String> {
        self.current().url
    }

    pub fn name(&self) -> Option<String> {
        self.current().name
    }

    /// Le partage public (Funnel) mène bien à ce serveur.
    pub fn funnel(&self) -> bool {
        self.current().public.is_some()
    }

    /// Début des liens de partage : l'adresse Funnel active, sinon celle prévue.
    pub fn public_base(&self) -> String {
        let i = self.current();
        i.public.unwrap_or_else(|| match i.name {
            Some(n) => format!("https://{n}:{}", self.funnel_port),
            None => format!("http://127.0.0.1:{}", self.public_port),
        })
    }
}

/// Couleur marquante de la pochette de chaque morceau, calculée en arrière-plan.
#[derive(Default)]
pub struct Colors {
    by_track: Mutex<HashMap<i64, Option<String>>>,
    by_release: Mutex<HashMap<i64, Option<String>>>,
}

impl Colors {
    pub fn get(&self, tid: i64) -> Option<String> {
        self.by_track.lock().unwrap_or_else(|e| e.into_inner()).get(&tid).cloned().flatten()
    }

    pub fn release(&self, cache: &Cache, con: &Connection, rid: i64) -> Option<String> {
        if let Some(c) = self.by_release.lock().unwrap_or_else(|e| e.into_inner()).get(&rid) {
            return c.clone();
        }
        let c = cache.cover_color(&library::release_cover_sources(con, rid));
        self.by_release.lock().unwrap_or_else(|e| e.into_inner()).insert(rid, c.clone());
        c
    }

    pub(crate) fn warm(st: std::sync::Arc<State>) {
        let _ = std::thread::Builder::new().name("pochettes".into()).spawn(move || {
            let Ok(con) = st.con() else { return };
            let rids: Vec<i64> = con.prepare("SELECT id FROM releases").and_then(|mut s| s.query_map([], |r| r.get(0))?.collect()).unwrap_or_default();
            for rid in rids {
                st.colors.release(&st.cache, &con, rid);
            }
            let ids: Vec<i64> = con
                .prepare("SELECT id FROM tracks WHERE archived = 0 AND missing = 0 ORDER BY added_at DESC")
                .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
                .unwrap_or_default();
            for tid in &ids {
                let c = st.cache.cover_color(&library::cover_sources(&con, *tid));
                st.colors.by_track.lock().unwrap_or_else(|e| e.into_inner()).insert(*tid, c);
            }
            log(&st, &format!("Pochettes prêtes ({} morceaux)", ids.len()));
        });
    }
}

/// Prépare en arrière-plan les versions FLAC des morceaux principaux, les plus récents d'abord.
pub(crate) fn warm_lossless(st: std::sync::Arc<State>) {
    let _ = std::thread::Builder::new().name("flac".into()).spawn(move || {
        let paths: Vec<PathBuf> = match st.con() {
            Ok(con) => library::list_tracks(&con, &List { sort: "added".into(), ..Default::default() })
                .unwrap_or_default()
                .iter()
                .filter_map(|t| library::track_path(&con, t["id"].as_i64()?))
                .collect(),
            Err(_) => return,
        };
        let done = paths.iter().filter(|p| media::is_lossless_source(p) && p.exists() && st.cache.transcoded(p, "lossless").is_some()).count();
        log(&st, &format!("Versions FLAC prêtes ({done} morceaux)"));
    });
}

const WARM_LOSSY_TRACKS: usize = 30;
const WARM_LOSSY_MAX_S: f64 = 20.0 * 60.0;

/// Versions Haute et Éco (Opus et AAC) des 30 morceaux les plus récents (pas les longs sets).
pub(crate) fn warm_lossy(st: std::sync::Arc<State>) {
    let _ = std::thread::Builder::new().name("allege".into()).spawn(move || {
        let paths: Vec<PathBuf> = match st.con() {
            Ok(con) => library::list_tracks(&con, &List { sort: "added".into(), ..Default::default() })
                .unwrap_or_default()
                .iter()
                .filter(|t| t["duration"].as_f64().is_some_and(|d| d <= WARM_LOSSY_MAX_S))
                .take(WARM_LOSSY_TRACKS)
                .filter_map(|t| library::track_path(&con, t["id"].as_i64()?))
                .collect(),
            Err(_) => return,
        };
        let mut done = 0;
        for p in paths.iter().filter(|p| p.exists()) {
            for q in ["high", "eco", "high_aac", "eco_aac"] {
                done += st.cache.transcoded(p, q).is_some() as usize;
            }
        }
        log(&st, &format!("Versions Haute / Éco prêtes ({done} fichiers)"));
    });
}

// ---------------------------------------------------------------------- réponses communes

pub(crate) fn digits(s: &str) -> Option<i64> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// Une pochette carrée (160 ou 600 px) ; 204 si le morceau n'en a pas.
pub(crate) fn cover(st: &State, req: &Request, c: &mut Conn, sources: &Sources) -> Out {
    let size = digits(req.q("size")).unwrap_or(600).clamp(1, 4096) as u32;
    match st.cache.cover_jpeg(sources, size) {
        None => c.send(204, "image/jpeg", &[("Cache-Control", "max-age=600".into())], b"")?,
        Some((jpeg, color)) => {
            let mut extra = vec![("Cache-Control", "max-age=86400".to_string())];
            if let Some(col) = color {
                extra.push(("X-Cover-Color", col));
            }
            c.send(200, "image/jpeg", &extra, &jpeg)?
        }
    }
    Ok(())
}

/// Un fichier (lecture partielle possible), avec son nom s'il s'enregistre.
pub(crate) fn send_file(c: &mut Conn, req: &Request, path: &Path, ctype: &str, name: Option<&str>, attachment: bool) -> Out {
    let mut extra = Vec::new();
    if let Some(n) = name {
        // Ni guillemet ni retour à la ligne : le nom ne doit pas pouvoir sortir de cet en-tête.
        let ascii: String = n.chars().map(|ch| if ch == '"' { '\'' } else if ch.is_ascii() && !ch.is_ascii_control() { ch } else { '?' }).collect();
        extra.push(("Content-Disposition", format!("{}; filename=\"{ascii}\"; filename*=UTF-8''{}", if attachment { "attachment" } else { "inline" }, encode(n))));
    }
    c.file(req, path, ctype, extra)?;
    Ok(())
}

/// Le fichier avec titre, artiste, sortie et pochette dedans (sans réencodage), dans une copie
/// temporaire supprimée après l'envoi.
pub(crate) fn tagged(st: &State, req: &Request, c: &mut Conn, tid: i64, path: &Path, mime: &str, attachment: bool) -> Out {
    let (t, sources, owner) = {
        let con = st.con()?;
        (library::get_track(&con, tid)?.unwrap_or(Json::Null), library::cover_sources(&con, tid), library::artist_name(&con))
    };
    let txt = |v: &Json| v.as_str().unwrap_or("").to_string();
    let rel = &t["release"];
    let title = Some(txt(&t["title"])).filter(|s| !s.is_empty()).unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let artist = Some(txt(&t["artist"])).filter(|s| !s.is_empty()).unwrap_or(owner);
    let album = txt(&rel["title"]);
    let date: String = Some(txt(&rel["date"])).filter(|s| !s.is_empty()).unwrap_or_else(|| txt(&t["published_date"])).chars().take(4).collect();
    let tags = [("title", title.clone()), ("artist", artist.clone()), ("album", album), ("date", date)];
    let cover = st.cache.cover_jpeg(&sources, 600);
    let out = st.cache.tagged_copy(path, &tags, cover.as_ref().map(|x| x.0.as_slice()));
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
    let name = media::safe_name(&if artist.is_empty() { title.clone() } else { format!("{artist} - {title}") }) + &ext;
    let r = send_file(c, req, out.as_deref().unwrap_or(path), mime, Some(&name), attachment);
    if let Some(o) = out {
        let _ = std::fs::remove_file(o);
    }
    r
}

// ---------------------------------------------------------------------- routes

pub(crate) fn handle(st: &State, req: &Request, c: &mut Conn) -> std::io::Result<()> {
    if let Some(why) = refused(st, req) {
        c.keep_alive = false;
        return c.error(403, why);
    }
    match route(st, req, c) {
        Ok(()) => Ok(()),
        Err(Fail::Io(e)) => Err(e),
        Err(Fail::Other(e)) => {
            log(st, &format!("Erreur sur {} {} : {e}", req.method, req.path));
            c.error(500, &format!("Erreur du serveur : {e}"))
        }
    }
}

fn route(st: &State, req: &Request, c: &mut Conn) -> Out {
    let p: Vec<&str> = req.parts.iter().map(String::as_str).collect();
    match req.method.as_str() {
        "GET" | "HEAD" => {}
        "POST" | "DELETE" => {
            if p.len() >= 2 && p[0] == "api" && p[1] == "shares" {
                return shares_api(st, req, c, &p[2..]);
            }
            if p == ["api", "distant", "ecrire"] {
                return distant_write(st, req, c);
            }
            if let ["api", "tracks", id, rest @ ..] = p.as_slice() {
                if let Some(id) = digits(id) {
                    return track_write(st, req, c, id, rest);
                }
            }
            return Ok(c.error(404, "Adresse inconnue.")?);
        }
        _ => {
            c.keep_alive = false;
            return Ok(c.error(405, "Méthode refusée.")?);
        }
    }
    if p.is_empty() || p == ["index.html"] {
        if let Some(t) = wants_https(st, req) {
            return Ok(c.redirect(&format!("{t}/"))?);
        }
        return static_file(c, "index.html");
    }
    if p == ["otoko.apk"] {
        return match st.cfg.apk.as_ref().filter(|a| a.is_file()) {
            Some(apk) => send_file(c, req, apk, "application/vnd.android.package-archive", Some("Otoko Vault.apk"), true),
            None => Ok(c.error(404, "L'application Android n'est pas encore installée sur ce serveur.")?),
        };
    }
    if p == ["maj", "pc"] {
        return match crate::newest_installer(&st.cfg) {
            Some((_, f)) => send_file(c, req, &f, "application/octet-stream", Some("Otoko-Studio-Vault-Pro-installation.exe"), true),
            None => Ok(c.error(404, "Aucune mise à jour disponible.")?),
        };
    }
    if p[0] != "api" {
        return static_file(c, &p.join("/"));
    }
    match &p[1..] {
        ["maj"] => {
            let found = crate::newest_installer(&st.cfg);
            let pc_sha256 = found.as_ref().and_then(|(_, f)| installer_sha256(f));
            let pc = found.map(|x| x.0);
            let apk = st.cfg.apk.as_ref().and_then(|a| std::fs::read_to_string(a.with_extension("version")).ok()).and_then(|t| t.trim().parse::<i64>().ok());
            // L'empreinte de l'installateur : l'autre PC ne lance que le fichier qui a exactement celle-ci.
            Ok(c.json(200, &json!({ "pc": pc, "pc_sha256": pc_sha256, "apk_code": apk }))?)
        }
        ["distant", "etat"] => {
            if !distant_ok(st, req, c)? {
                return Ok(());
            }
            // Le nom de la machine : le logiciel ne doit pas se relier à lui-même.
            Ok(c.json(200, &json!({ "stamp": stamp(&st.cfg.db), "machine": std::env::var("COMPUTERNAME").unwrap_or_default() }))?)
        }
        ["distant", "base"] => {
            if !distant_ok(st, req, c)? {
                return Ok(());
            }
            // Une photo cohérente de la base, même si elle est en train d'être écrite.
            let tmp = std::env::temp_dir().join(format!("otoko-base-{}-{:?}.db", std::process::id(), std::thread::current().id()));
            let _ = std::fs::remove_file(&tmp);
            st.con()?.execute("VACUUM INTO ?1", [tmp.to_string_lossy()])?;
            let r = send_file(c, req, &tmp, "application/octet-stream", None, false);
            let _ = std::fs::remove_file(&tmp);
            r
        }
        ["ping"] => {
            let con = st.con()?;
            let mut v = json!({
                "name": "Otoko Vault", "version": st.cfg.version, "transcoding": st.cache.has_ffmpeg(),
                "https": st.https.get(), "lan": st.lan_url(),
                // L'adresse qui marche hors de la maison : le téléphone la retient.
                "tailscale": st.https.name().map(|n| format!("http://{n}:{}", st.port())),
                "wifi": st.lan_allowed(),
                "apk": st.cfg.apk.as_ref().is_some_and(|a| a.is_file()),
            });
            if let (Some(o), Json::Object(s)) = (v.as_object_mut(), library::summary(&con)) {
                o.extend(s);
            }
            Ok(c.json(200, &v)?)
        }
        ["tracks"] => tracks(st, req, c),
        ["tracks", id, rest @ ..] if digits(id).is_some() => track(st, req, c, digits(id).unwrap_or(0), rest),
        ["releases"] => {
            let con = st.con()?;
            let list: Vec<Json> = library::list_releases(&con)?.into_iter().map(|r| release_json(st, &con, r)).collect();
            Ok(c.json(200, &Json::Array(list))?)
        }
        ["releases", id, rest @ ..] if digits(id).is_some() => release(st, req, c, digits(id).unwrap_or(0), rest),
        ["shares", rest @ ..] => shares_api(st, req, c, rest),
        ["views"] => {
            let con = st.con()?;
            Ok(c.json(200, &Json::Array(library::list_views(&con)?))?)
        }
        ["views", id, "tracks"] if digits(id).is_some() => {
            let con = st.con()?;
            let Some(ids) = library::view_track_ids(&con, digits(id).unwrap_or(0)) else { return Ok(c.error(404, "Vue introuvable.")?) };
            let sort = Some(req.q("sort")).filter(|s| !s.is_empty()).unwrap_or("added").to_string();
            let mut list = library::list_tracks(&con, &List { ids: Some(ids), scope: "all".into(), sort, ..Default::default() })?;
            with_colors(st, &mut list);
            Ok(c.json(200, &Json::Array(list))?)
        }
        _ => Ok(c.error(404, "Adresse inconnue.")?),
    }
}

/// L'empreinte SHA-256 d'un installateur, recalculée seulement quand le fichier change (il pèse des dizaines de Mo, et
/// `/api/maj` est demandé par chaque appareil).
fn installer_sha256(f: &Path) -> Option<String> {
    type Known = Option<(PathBuf, std::time::SystemTime, u64, String)>;
    static LAST: Mutex<Known> = Mutex::new(None);
    let m = std::fs::metadata(f).ok()?;
    let (when, len) = (m.modified().ok()?, m.len());
    let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, w, l, h)) = g.as_ref() {
        if p == f && *w == when && *l == len {
            return Some(h.clone());
        }
    }
    let h = crate::complements::sha256_of(f).ok()?;
    *g = Some((f.to_path_buf(), when, len, h.clone()));
    Some(h)
}

/// L'état de la base : change à chaque écriture, sans la lire.
fn stamp(db: &Path) -> String {
    let t = |p: &Path| std::fs::metadata(p).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis()).unwrap_or(0);
    format!("{}-{}", t(db), t(&PathBuf::from(format!("{}-wal", db.display()))))
}

/// L'en-tête où l'autre PC donne le code de liaison du PC principal.
pub const LINK_HEADER: &str = "X-Otoko-Code";

/// L'autre PC passe par Tailscale ou cet ordinateur, jamais par le Wi-Fi, et donne le code de liaison de ce PC : c'est ce
/// qui autorise à lire toute la base et à y écrire. Tout appareil de Tailscale n'est pas l'autre PC.
fn distant_ok(st: &State, req: &Request, c: &mut Conn) -> std::io::Result<bool> {
    if is_lan(req.peer) {
        c.error(403, "L'autre PC se connecte par Tailscale, pas par le Wi-Fi.")?;
        return Ok(false);
    }
    if req.header("X-Studio-Vault") != Some("1") {
        c.error(403, "Requête refusée.")?;
        return Ok(false);
    }
    let Some(data) = st.cfg.db.parent() else { return Ok(false) };
    if !req.header(LINK_HEADER).is_some_and(|code| crate::link_matches(data, code)) {
        c.error(401, "Code de liaison absent ou faux : recopie celui du PC principal (Réglages › Général › Travailler depuis un autre PC).")?;
        return Ok(false);
    }
    Ok(true)
}

/// Une écriture venue d'une page ou d'une app à nous : l'en-tête que seul notre code envoie, et pas d'autre site
/// (un navigateur n'envoie pas cet en-tête vers une autre adresse sans le demander d'abord).
fn ours(req: &Request, c: &mut Conn) -> std::io::Result<bool> {
    if req.header("X-Studio-Vault") != Some("1") {
        c.error(403, "Requête refusée.")?;
        return Ok(false);
    }
    if let Some(origin) = req.header("Origin") {
        let o = origin.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("").split(':').next().unwrap_or("").to_lowercase();
        let h = req.header("Host").unwrap_or("").split(':').next().unwrap_or("").to_lowercase();
        if o != h {
            c.error(403, "Requête refusée.")?;
            return Ok(false);
        }
    }
    Ok(true)
}

/// Une connexion qui écrit dans la Carrière (les lectures, elles, restent en lecture seule).
fn writer(st: &State) -> rusqlite::Result<Connection> {
    let con = Connection::open(&st.cfg.db)?;
    con.busy_timeout(Duration::from_secs(5))?;
    con.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(con)
}

/// Ce que le téléphone change sur un morceau en l'écoutant (voir `edits`).
fn track_write(st: &State, req: &Request, c: &mut Conn, id: i64, rest: &[&str]) -> Out {
    if req.method != "POST" {
        return Ok(c.error(405, "Méthode refusée.")?);
    }
    if !ours(req, c)? {
        return Ok(());
    }
    let Ok(b) = body(req) else { return Ok(c.error(400, "Requête illisible.")?) };
    let con = writer(st)?;
    // (résultat, le PC doit-il relire son écran ?) : une simple écoute ne le dérange pas.
    let (done, shown) = match rest {
        ["edit"] => (crate::edits::edit(&con, id, &b), true),
        ["markers"] => (crate::edits::add_note(&con, id, &b), true),
        ["markers", "remove"] => (crate::edits::remove_note(&con, id, &b), true),
        ["played"] => (crate::edits::played(&con, id), false),
        _ => return Ok(c.error(404, "Adresse inconnue.")?),
    };
    if shown && done.is_ok() {
        crate::edits::stamp(&con, id);
    }
    drop(con);
    match done {
        Ok(()) => {
            let con = st.con()?;
            match library::get_track(&con, id)? {
                Some(t) => {
                    let mut l = [t];
                    with_colors(st, &mut l);
                    let [t] = l;
                    Ok(c.json(200, &t)?)
                }
                None => Ok(c.error(404, "Morceau introuvable.")?),
            }
        }
        Err(e) if e == "Morceau introuvable." => Ok(c.error(404, &e)?),
        Err(e) => Ok(c.error(400, &e)?),
    }
}

/// Les écritures de l'autre PC.
fn distant_write(st: &State, req: &Request, c: &mut Conn) -> Out {
    if !distant_ok(st, req, c)? {
        return Ok(());
    }
    let Ok(v) = serde_json::from_slice::<Json>(&req.body) else { return Ok(c.error(400, "Requête illisible.")?) };
    let changes = v["changes"].as_array().cloned().unwrap_or_default();
    // La seule écriture de ce serveur : les autres connexions sont en lecture seule.
    let mut con = Connection::open(&st.cfg.db)?;
    con.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);
    con.busy_timeout(Duration::from_secs(5))?;
    con.execute_batch("PRAGMA foreign_keys = ON;")?;
    match otoko_library::career::distant::apply(&con, &changes) {
        Ok(n) => {
            log(st, &format!("Autre PC : {n} écriture(s) appliquée(s)"));
            Ok(c.json(200, &json!({ "applied": n, "stamp": stamp(&st.cfg.db) }))?)
        }
        Err(e) => Ok(c.error(400, &format!("Écriture refusée : {e}"))?),
    }
}

/// Adresse HTTPS vers laquelle renvoyer la page ouverte en HTTP depuis un appareil Tailscale.
fn wants_https(st: &State, req: &Request) -> Option<String> {
    let host = req.header("Host").unwrap_or("").split(':').next().unwrap_or("").to_lowercase();
    if host.ends_with(".ts.net") || is_loopback(req.peer) {
        return None;
    }
    // Depuis le Wi-Fi de la maison (ou ouverte par l'IP locale) : on reste en HTTP direct.
    if is_lan(req.peer) || host.parse::<IpAddr>().is_ok_and(is_lan) {
        return None;
    }
    st.https.get()
}

fn static_file(c: &mut Conn, rel: &str) -> Out {
    match web::get(rel) {
        Some((body, ctype)) => c.send(200, ctype, &[("Cache-Control", if rel == "icon.png" { "max-age=86400" } else { "no-cache" }.into())], body)?,
        None => c.error(404, "Page introuvable.")?,
    }
    Ok(())
}

fn with_colors(st: &State, tracks: &mut [Json]) {
    for t in tracks.iter_mut() {
        let id = t["id"].as_i64().unwrap_or(0);
        t["color"] = json!(st.colors.get(id));
        t["cover"] = json!(format!("/api/tracks/{id}/cover"));
        if let Some(vs) = t.get_mut("versions").and_then(|v| v.as_array_mut()) {
            for v in vs {
                let vid = v["id"].as_i64().unwrap_or(0);
                v["color"] = json!(st.colors.get(vid));
            }
        }
    }
}

fn release_json(st: &State, con: &Connection, mut rel: Json) -> Json {
    let id = rel["id"].as_i64().unwrap_or(0);
    rel["cover"] = json!(format!("/api/releases/{id}/cover"));
    let color = st.colors.release(&st.cache, con, id).or_else(|| release_color(rel["type"].as_str().unwrap_or("")).map(str::to_string));
    rel["color"] = json!(color);
    rel
}

fn tracks(st: &State, req: &Request, c: &mut Conn) -> Out {
    let or = |k: &str, d: &str| Some(req.q(k)).filter(|s| !s.is_empty()).unwrap_or(d).to_string();
    let num = |k: &str| req.q(k).trim().parse::<f64>().ok().filter(|x| x.is_finite());
    let a = List {
        q: req.q("q").to_string(),
        sort: or("sort", "added"),
        scope: or("scope", "main"),
        nature: req.q("nature").to_string(),
        status: req.q("status").to_string(),
        mood: req.q("mood").to_string(),
        key: req.q("key").to_string(),
        bpm_min: num("bpm_min"),
        bpm_max: num("bpm_max"),
        compatible_with: digits(req.q("compatible_with")),
        limit: digits(req.q("limit")).unwrap_or(5000),
        offset: digits(req.q("offset")).unwrap_or(0),
        ..Default::default()
    };
    let con = st.con()?;
    let mut list = library::list_tracks(&con, &a)?;
    drop(con);
    with_colors(st, &mut list);
    Ok(c.json(200, &Json::Array(list))?)
}

/// Les lecteurs réseau Naim (Mu-so) annoncent « audio/x-flac » et pas « audio/flac ».
fn dlna_mime(mime: &'static str, req: &Request) -> &'static str {
    if req.q("dlna") == "1" && mime == "audio/flac" { "audio/x-flac" } else { mime }
}

fn track(st: &State, req: &Request, c: &mut Conn, tid: i64, rest: &[&str]) -> Out {
    let (path, sources) = {
        let con = st.con()?;
        if rest.is_empty() {
            return match library::get_track(&con, tid)? {
                None => Ok(c.error(404, "Morceau introuvable.")?),
                Some(t) => {
                    let mut l = [t];
                    with_colors(st, &mut l);
                    let [t] = l;
                    Ok(c.json(200, &t)?)
                }
            };
        }
        (library::track_path(&con, tid), library::cover_sources(&con, tid))
    };
    let Some(path) = path else { return Ok(c.error(404, "Morceau introuvable.")?) };
    match rest {
        ["cover"] => cover(st, req, c, &sources),
        ["waveform"] => {
            if !path.exists() {
                return Ok(c.error(404, "Fichier introuvable sur le disque.")?);
            }
            let pts = digits(req.q("points")).unwrap_or(120).clamp(16, 4000) as usize;
            match st.cache.waveform(&path, pts) {
                Some(w) => Ok(c.json(200, &w)?),
                None => Ok(c.error(500, "Forme d'onde illisible pour ce fichier.")?),
            }
        }
        ["audio"] => {
            if !path.exists() {
                return Ok(c.error(404, "Fichier introuvable : le disque où il se trouve est-il branché ?")?);
            }
            let mut q = Some(req.q("quality")).filter(|s| !s.is_empty()).unwrap_or("original");
            if q == "lossless" && !media::is_lossless_source(&path) {
                q = "original"; // déjà compressé (FLAC, MP3…) : rien à gagner
            }
            // Sans ffmpeg sur le PC, on sert le fichier tel quel plutôt que rien (uniquement à tes propres appareils).
            if q != "original" && !st.cache.has_ffmpeg() && st.cache.cached(&path, q).is_none() {
                q = "original";
            }
            let tags = req.q("tags") == "1";
            if q != "original" {
                let Some((_, mime, _)) = media::quality(q) else { return Ok(c.error(400, "Qualité inconnue.")?) };
                let Some(alt) = st.cache.transcoded(&path, q) else { return Ok(c.error(503, "Conversion indisponible (ffmpeg introuvable).")?) };
                if tags {
                    return tagged(st, req, c, tid, &alt, mime, false);
                }
                return send_file(c, req, &alt, dlna_mime(mime, req), None, false);
            }
            let mime = dlna_mime(media::path_mime(&path), req);
            if tags {
                return tagged(st, req, c, tid, &path, mime, false);
            }
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            send_file(c, req, &path, mime, name.as_deref(), false)
        }
        _ => Ok(c.error(404, "Adresse inconnue.")?),
    }
}

fn release(st: &State, req: &Request, c: &mut Conn, rid: i64, rest: &[&str]) -> Out {
    let con = st.con()?;
    match rest {
        ["cover"] => {
            let s = library::release_cover_sources(&con, rid);
            drop(con);
            cover(st, req, c, &s)
        }
        [] => match library::get_release(&con, rid)? {
            None => Ok(c.error(404, "Sortie introuvable.")?),
            Some(r) => {
                let mut r = release_json(st, &con, r);
                let mut list = library::release_tracks(&con, rid)?;
                with_colors(st, &mut list);
                r["track_list"] = Json::Array(list);
                Ok(c.json(200, &r)?)
            }
        },
        ["tracks"] => {
            let mut list = library::release_tracks(&con, rid)?;
            with_colors(st, &mut list);
            Ok(c.json(200, &Json::Array(list))?)
        }
        _ => Ok(c.error(404, "Adresse inconnue.")?),
    }
}

// ---------------------------------------------------------------------- partages (gestion)

/// Cet ordinateur ou Tailscale seulement (pas le Wi-Fi), et seulement depuis la page ou l'app
/// OTOKO (en-tête que les autres sites ne peuvent pas envoyer sans autorisation).
fn manage_ok(st: &State, req: &Request, c: &mut Conn) -> std::io::Result<bool> {
    if is_lan(req.peer) {
        c.error(403, "Les partages se gèrent par Tailscale (adresse https://…ts.net), pas par le Wi-Fi.")?;
        return Ok(false);
    }
    if !ours(req, c)? {
        return Ok(false);
    }
    if st.shares.is_none() {
        c.error(503, "Partages indisponibles.")?;
        return Ok(false);
    }
    Ok(true)
}

fn share_out(st: &State, mut sh: Json) -> Json {
    if let Some(o) = sh.as_object_mut() {
        let tok = o.remove("token").and_then(|t| t.as_str().map(str::to_string)).unwrap_or_default();
        o.insert("url".into(), json!(format!("{}/s/{tok}", st.https.public_base())));
    }
    sh
}

fn body(req: &Request) -> Result<Json, ShareError> {
    if req.body.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_slice::<Json>(&req.body) {
        Ok(v @ Json::Object(_)) => Ok(v),
        _ => Err(ShareError("Requête illisible.".into())),
    }
}

fn shares_api(st: &State, req: &Request, c: &mut Conn, rest: &[&str]) -> Out {
    if !manage_ok(st, req, c)? {
        return Ok(());
    }
    let Some(store) = st.shares.as_ref() else { return Ok(()) };
    let m = if req.method == "HEAD" { "GET" } else { req.method.as_str() };
    let gone = |what: &str| (404, json!({ "error": what }));
    let id = rest.first().and_then(|s| digits(s)).unwrap_or(0);
    let res: Result<(u16, Json), ShareError> = (|| {
        Ok(match (m, rest) {
            ("GET", []) => {
                let list: Vec<Json> = store.list().into_iter().map(|s| share_out(st, s)).collect();
                (200, json!({ "public_base": st.https.public_base(), "funnel": st.https.funnel(), "shares": list }))
            }
            ("GET", ["status"]) => {
                st.https.refresh();
                (200, json!({ "public_base": st.https.public_base(), "funnel": st.https.funnel() }))
            }
            ("POST", []) => {
                let sh = store.create(&body(req)?)?;
                log(st, &format!("Partage créé : n°{} ({})", sh["id"], sh["kind"].as_str().unwrap_or("")));
                // Versions d'écoute prêtes avant que le lien soit ouvert.
                if let Some(full) = sh["token"].as_str().and_then(|t| store.by_token(t)) {
                    public::prewarm(st, Some(vec![full]));
                }
                (201, share_out(st, sh))
            }
            ("GET", [s, "events"]) if digits(s).is_some() => (200, Json::Array(store.events(id))),
            ("POST", [s]) if digits(s).is_some() => {
                let allowed = ["label", "message", "can_download", "download_quality", "expires_days", "password", "max_downloads", "revoked"];
                let b = body(req)?;
                let f: serde_json::Map<String, Json> = b.as_object().into_iter().flatten().filter(|(k, _)| allowed.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
                st.public.forget(id);
                match store.update(id, &Json::Object(f))? {
                    None => gone("Partage introuvable."),
                    Some(sh) => (200, share_out(st, sh)),
                }
            }
            ("DELETE", [s]) if digits(s).is_some() => {
                st.public.forget(id);
                if store.delete(id) { (200, json!({ "ok": true })) } else { gone("Partage introuvable.") }
            }
            _ => gone("Adresse inconnue."),
        })
    })();
    match res {
        Ok((status, v)) => c.json(status, &v)?,
        Err(ShareError(m)) => c.error(400, &m)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn who_may_connect() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        for ok in ["127.0.0.1", "::1", "100.101.102.103", "192.168.1.20", "10.0.0.5", "172.20.1.1", "fd7a:115c:a1e0::1", "::ffff:192.168.1.9"] {
            assert!(client_allowed(ip(ok)), "{ok}");
        }
        for no in ["8.8.8.8", "100.200.1.1", "2a01::1", "172.32.0.1"] {
            assert!(!client_allowed(ip(no)), "{no}");
        }
        assert!(is_lan(ip("192.168.1.20")) && !is_lan(ip("100.101.102.103")) && !is_lan(ip("127.0.0.1")));
        assert!(is_tailscale(ip("100.101.102.103")) && is_tailscale(ip("fd7a:115c:a1e0::1")) && !is_tailscale(ip("192.168.1.20")) && !is_tailscale(ip("100.200.1.1")));
    }

    #[test]
    fn a_website_cannot_pass_for_this_server() {
        for ok in ["127.0.0.1:8775", "localhost:8775", "localhost", "mon-pc", "mon-pc:8775", "100.65.1.2:8775", "192.168.1.20:8775", "[::1]:8775", "[fd7a:115c:a1e0::1]", "mini.tail1234.ts.net", "mini.tail1234.ts.net:8444", "MINI.TAIL1234.TS.NET.", "mon-pc.local:8775", ""] {
            assert!(host_allowed(ok), "{ok}");
        }
        for no in ["evil.example.com", "evil.example.com:8775", "127.0.0.1.evil.example.com", "mini.ts.net.evil.example.com", "evil.com:notaport"] {
            assert!(!host_allowed(no), "{no}");
        }
    }

    #[test]
    fn tailscale_addresses() {
        let serve: Json = serde_json::from_str(
            r#"{"Web":{"mini.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:8775"}}},
                       "mini.ts.net:10000":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:8776"}}},
                       "mini.ts.net:8443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:8766"}}}},
                "AllowFunnel":{"mini.ts.net:10000":true,"mini.ts.net:8443":true}}"#,
        )
        .unwrap();
        assert_eq!(read_serve(&serve, 8775, 8776), (Some("https://mini.ts.net".into()), Some("https://mini.ts.net:10000".into())));
        assert_eq!(read_serve(&serve, 8765, 8766), (None, Some("https://mini.ts.net:8443".into())));
    }
}
