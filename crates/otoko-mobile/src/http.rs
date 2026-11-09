//! Un petit serveur HTTP/1.1 : un fil par connexion, connexions gardées ouvertes (le lecteur du
//! téléphone enchaîne les demandes par morceaux), lecture partielle des fichiers (`Range`).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

pub struct Request {
    pub method: String,
    /// Les morceaux du chemin, décodés (`/api/tracks/12` → ["api", "tracks", "12"]).
    pub parts: Vec<String>,
    pub path: String,
    pub query: HashMap<String, String>,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub peer: IpAddr,
    /// L'adresse de cet ordinateur sur laquelle la connexion est arrivée.
    pub local: IpAddr,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn q(&self, k: &str) -> &str {
        self.query.get(k).map(|s| s.as_str()).unwrap_or("")
    }
}

/// `%xx` et (dans une requête) `+` décodés.
pub fn decode(s: &str, plus: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' && plus { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `%` pour un en-tête `filename*=UTF-8''…`.
pub fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// La connexion d'une requête : où écrire la réponse.
pub struct Conn<'a> {
    stream: &'a TcpStream,
    pub head_only: bool,
    /// En-têtes ajoutés à toutes les réponses (sécurité du serveur public).
    pub always: &'a [(&'static str, String)],
    pub keep_alive: bool,
}

impl<'a> Conn<'a> {
    fn out(&self) -> &'a TcpStream {
        self.stream
    }

    fn head(&mut self, status: u16, ctype: &str, len: u64, extra: &[(&str, String)]) -> std::io::Result<()> {
        let mut h = format!("HTTP/1.1 {status} {}\r\nContent-Type: {ctype}\r\nContent-Length: {len}\r\nServer: Otoko Vault\r\n", reason(status));
        for (k, v) in extra {
            h.push_str(&format!("{k}: {v}\r\n"));
        }
        for (k, v) in self.always {
            h.push_str(&format!("{k}: {v}\r\n"));
        }
        h.push_str(if self.keep_alive { "Connection: keep-alive\r\n\r\n" } else { "Connection: close\r\n\r\n" });
        self.out().write_all(h.as_bytes())
    }

    pub fn send(&mut self, status: u16, ctype: &str, extra: &[(&str, String)], body: &[u8]) -> std::io::Result<()> {
        self.head(status, ctype, body.len() as u64, extra)?;
        if !self.head_only {
            self.out().write_all(body)?;
        }
        Ok(())
    }

    pub fn json(&mut self, status: u16, v: &serde_json::Value) -> std::io::Result<()> {
        self.send(status, "application/json; charset=utf-8", &[("Cache-Control", "no-store".into())], v.to_string().as_bytes())
    }

    pub fn error(&mut self, status: u16, message: &str) -> std::io::Result<()> {
        self.json(status, &serde_json::json!({ "error": message }))
    }

    /// Un fichier, entier ou la partie demandée (`Range: bytes=a-b`).
    pub fn file(&mut self, req: &Request, path: &Path, ctype: &str, mut extra: Vec<(&str, String)>) -> std::io::Result<()> {
        let mut f = std::fs::File::open(path)?;
        let size = f.metadata()?.len();
        extra.push(("Accept-Ranges", "bytes".into()));
        if !extra.iter().any(|(k, _)| *k == "Cache-Control") {
            extra.push(("Cache-Control", "no-store".into()));
        }
        if ctype.starts_with("audio/") {
            // Lecteurs réseau UPnP / DLNA (Mu-so) : flux de longueur connue, avance rapide possible.
            extra.push(("transferMode.dlna.org", "Streaming".into()));
            extra.push(("contentFeatures.dlna.org", "DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000".into()));
        }
        let range = req.header("Range");
        let parsed = range.and_then(|r| parse_range(r, size));
        if range.is_some() && parsed.is_none() {
            extra.push(("Content-Range", format!("bytes */{size}")));
            return self.send(416, ctype, &extra, b"");
        }
        let (start, end) = parsed.unwrap_or((0, size.saturating_sub(1)));
        let len = if size == 0 { 0 } else { end - start + 1 };
        if parsed.is_some() {
            extra.push(("Content-Range", format!("bytes {start}-{end}/{size}")));
        }
        self.head(if parsed.is_some() { 206 } else { 200 }, ctype, len, &extra)?;
        if self.head_only {
            return Ok(());
        }
        f.seek(SeekFrom::Start(start))?;
        let mut left = len;
        let mut buf = vec![0u8; 256 * 1024];
        while left > 0 {
            let want = left.min(buf.len() as u64) as usize;
            let n = f.read(&mut buf[..want])?;
            if n == 0 {
                break;
            }
            self.out().write_all(&buf[..n])?;
            left -= n as u64;
        }
        Ok(())
    }

    pub fn redirect(&mut self, to: &str) -> std::io::Result<()> {
        self.send(302, "text/plain", &[("Location", to.to_string()), ("Cache-Control", "no-store".into())], b"")
    }
}

fn reason(s: u16) -> &'static str {
    match s {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        413 => "Payload Too Large",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// « bytes=a-b » → (début, fin incluse), ou rien si absent ou illisible.
pub fn parse_range(h: &str, size: u64) -> Option<(u64, u64)> {
    let r = h.trim().strip_prefix("bytes=")?;
    let (a, b) = r.split_once('-')?;
    if size == 0 {
        return None;
    }
    let (start, end) = match (a.trim(), b.trim()) {
        ("", "") => return None,
        ("", n) => (size.saturating_sub(n.parse().ok()?), size - 1),
        (s, "") => (s.parse().ok()?, size - 1),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(size - 1)),
    };
    (start <= end && start < size).then_some((start, end))
}

pub type Handler = dyn Fn(&Request, &mut Conn) -> std::io::Result<()> + Send + Sync;

/// Une connexion à la fois ne doit pas pouvoir remplir la mémoire : ligne et en-têtes sont bornés.
const MAX_LINE: u64 = 32 * 1024;
const MAX_HEADERS: usize = 32 * 1024;
/// Au-delà, les nouvelles connexions sont fermées : chacune occupe un fil.
const MAX_CONNECTIONS: usize = 256;
/// Le lecteur du téléphone peut rester en pause longtemps : il reprend alors par une nouvelle demande.
const WRITE_TIMEOUT: Duration = Duration::from_secs(300);

/// Une ligne terminée par un retour, ou rien (connexion fermée, ligne trop longue).
fn read_line_limited(reader: &mut impl BufRead) -> Option<String> {
    let mut buf = Vec::new();
    match reader.by_ref().take(MAX_LINE).read_until(b'\n', &mut buf) {
        Ok(n) if n > 0 && buf.last() == Some(&b'\n') => Some(String::from_utf8_lossy(&buf).into_owned()),
        _ => None,
    }
}

/// La taille annoncée du corps. Un corps en morceaux, une taille illisible ou deux tailles sont refusés : un
/// relais et ce serveur pourraient ne pas lire la même chose, et une requête se glisser dans une autre.
fn body_length(headers: &[(String, String)]) -> Result<usize, ()> {
    if headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("Transfer-Encoding")) {
        return Err(());
    }
    let mut sizes = headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("Content-Length"));
    let Some((_, v)) = sizes.next() else { return Ok(0) };
    if sizes.next().is_some() {
        return Err(());
    }
    v.parse().map_err(|_| ())
}

/// Lit les requêtes d'une connexion jusqu'à sa fermeture.
fn connection(stream: TcpStream, handler: &Handler, max_body: usize, always: &[(&'static str, String)]) {
    let peer = match stream.peer_addr() {
        Ok(a) => a.ip(),
        Err(_) => return,
    };
    let local = stream.local_addr().map(|a| a.ip()).unwrap_or(peer);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = stream.set_nodelay(true);
    // Sans `try_clone` : sa copie serait héritée par chaque programme lancé par le serveur (voir `not_inherited`).
    let mut reader = BufReader::new(&stream);
    loop {
        let Some(line) = read_line_limited(&mut reader) else { return };
        let mut it = line.split_whitespace();
        let (Some(method), Some(target)) = (it.next(), it.next()) else { return };
        let version = it.next().unwrap_or("HTTP/1.0").to_string();
        let mut headers = Vec::new();
        let mut size = 0;
        loop {
            let Some(h) = read_line_limited(&mut reader) else { return };
            size += h.len();
            if size > MAX_HEADERS {
                return;
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                headers.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        let get = |k: &str| headers.iter().find(|(n, _): &&(String, String)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone());
        let keep = match get("Connection").map(|c| c.to_lowercase()) {
            Some(c) if c.contains("close") => false,
            Some(c) if c.contains("keep-alive") => true,
            _ => version == "HTTP/1.1",
        };
        let Ok(len) = body_length(&headers) else {
            let mut conn = Conn { stream: &stream, head_only: false, always, keep_alive: false };
            let _ = conn.error(400, "Requête refusée.");
            return;
        };
        let mut conn = Conn { stream: &stream, head_only: method == "HEAD", always, keep_alive: keep && len <= max_body };
        if len > max_body {
            // Trop gros : on refuse et on ferme (sans lire le reste).
            conn.keep_alive = false;
            let _ = conn.error(413, "Requête trop grosse.");
            return;
        }
        let mut body = vec![0u8; len];
        if len > 0 && reader.read_exact(&mut body).is_err() {
            return;
        }
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let parts: Vec<String> = path.split('/').filter(|p| !p.is_empty()).map(|p| decode(p, false)).collect();
        let query: HashMap<String, String> = query
            .split('&')
            .filter(|kv| !kv.is_empty())
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                (decode(k, true), decode(v, true))
            })
            .collect();
        let req = Request { method: method.to_string(), parts, path: path.to_string(), query, headers, body, peer, local };
        let keep_alive = conn.keep_alive;
        if handler(&req, &mut conn).is_err() || !keep_alive {
            return;
        }
    }
}

/// Un serveur qui tourne : `stop()` l'arrête.
pub struct Running {
    stop: Arc<AtomicBool>,
    addr: std::net::SocketAddr,
    thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Running {
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Arrête d'écouter et attend (une seconde au plus) que le port soit rendu.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        // Débloque l'attente de connexion.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(300));
        if let Some(t) = self.thread.lock().ok().and_then(|mut t| t.take()) {
            let start = std::time::Instant::now();
            while !t.is_finished() && start.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(5));
            }
            if t.is_finished() {
                let _ = t.join();
            }
        }
    }
}

/// Une connexion acceptée est héritable sous Windows. Tout programme lancé pendant qu'elle est ouverte (tailscale, ffmpeg)
/// en garderait une copie : fermée par le serveur, elle resterait ouverte jusqu'à la fin de ce programme, puis serait
/// coupée net, et le client lirait « connexion réinitialisée » après sa réponse.
fn not_inherited(s: &TcpStream) {
    use std::os::windows::io::AsRawSocket;
    unsafe extern "system" {
        fn SetHandleInformation(handle: isize, mask: u32, flags: u32) -> i32;
    }
    const HANDLE_FLAG_INHERIT: u32 = 1;
    unsafe {
        SetHandleInformation(s.as_raw_socket() as isize, HANDLE_FLAG_INHERIT, 0);
    }
}

struct Closed(Arc<AtomicUsize>);

impl Drop for Closed {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Écoute sur `addr` et confie chaque requête à `handler` (un fil par connexion).
pub fn serve(addr: &str, handler: Arc<Handler>, max_body: usize, always: Vec<(&'static str, String)>) -> std::io::Result<Running> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let always = Arc::new(always);
    let open = Arc::new(AtomicUsize::new(0));
    let thread = std::thread::Builder::new().name(format!("http-{}", local.port())).spawn(move || {
        for s in listener.incoming() {
            if flag.load(Ordering::Relaxed) {
                break;
            }
            let Ok(s) = s else { continue };
            not_inherited(&s);
            if open.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                continue; // `s` se ferme ici
            }
            open.fetch_add(1, Ordering::Relaxed);
            let (h, a, count) = (handler.clone(), always.clone(), open.clone());
            let spawned = std::thread::Builder::new().name("http-conn".into()).spawn(move || {
                // Décompté même si la requête s'arrête sur une erreur interne.
                let _done = Closed(count);
                connection(s, &*h, max_body, &a);
            });
            if spawned.is_err() {
                open.fetch_sub(1, Ordering::Relaxed);
            }
        }
    })?;
    let connect_to = if local.ip().is_unspecified() { std::net::SocketAddr::new(std::net::Ipv4Addr::LOCALHOST.into(), local.port()) } else { local };
    Ok(Running { stop, addr: connect_to, thread: std::sync::Mutex::new(Some(thread)) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_and_decoding() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=200-", 100), None);
        assert_eq!(decode("Concrete%20Jungle", false), "Concrete Jungle");
        assert_eq!(decode("a+b%C3%A9", true), "a bé");
        assert_eq!(encode("Otoko - Nuit é.wav"), "Otoko%20-%20Nuit%20%C3%A9.wav");
    }

    #[test]
    fn an_ambiguous_body_size_is_refused() {
        let h = |k: &str, v: &str| (k.to_string(), v.to_string());
        assert_eq!(body_length(&[]), Ok(0));
        assert_eq!(body_length(&[h("content-length", "12")]), Ok(12));
        assert!(body_length(&[h("Content-Length", "abc")]).is_err(), "taille illisible");
        assert!(body_length(&[h("Content-Length", "1"), h("Content-Length", "2")]).is_err(), "deux tailles");
        assert!(body_length(&[h("Transfer-Encoding", "chunked")]).is_err(), "corps en morceaux");
    }
}
