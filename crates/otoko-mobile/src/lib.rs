//! Serveur de l'app mobile d'OTOKO : la Carrière servie au téléphone (Tailscale, Wi-Fi de la
//! maison) et les liens de partage publics (Tailscale Funnel). Mêmes adresses et même JSON que le
//! serveur de Studio Vault, pour que l'app Android et la page web s'y retrouvent à l'identique.
//!
//! Deux serveurs : le privé (`PORT`, toutes les interfaces, filtré par adresse) et le public
//! (`PUBLIC_PORT`, 127.0.0.1 seulement, que Funnel publie sur Internet).

pub mod complements;
pub mod edits;
pub mod http;
pub mod library;
pub mod media;
pub mod public;
pub mod server;
pub mod shares;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub use server::State;
use shares::ShareStore;

/// Ports d'OTOKO, à côté de ceux de Studio Vault (8765, 8766, 8443) tant que les deux tournent.
pub const PORT: u16 = 8775;
pub const PUBLIC_PORT: u16 = 8776;
/// Port HTTPS public de Tailscale Funnel prévu pour les partages d'OTOKO (443, 8443 ou 10000).
pub const FUNNEL_PORT: u16 = 10000;

#[derive(Clone, Debug)]
pub struct Config {
    /// La Carrière (`carriere.db`).
    pub db: PathBuf,
    /// Les liens de partage (`partages.db`).
    pub shares_db: PathBuf,
    /// Pochettes, formes d'onde, versions allégées, journal.
    pub cache: PathBuf,
    /// Versions allégées déjà faites par Studio Vault, servies sans reconvertir.
    pub borrow_cache: Option<PathBuf>,
    pub host: String,
    pub port: u16,
    /// Rien : pas de serveur public ; 0 : un port libre.
    pub public_port: Option<u16>,
    pub funnel_port: u16,
    /// Préparer en fond pochettes et versions d'écoute des partages au démarrage.
    pub warm: bool,
    /// Convertir d'avance toute la bibliothèque (FLAC, Opus, AAC) : pour le serveur du PC, pas
    /// pour celui qui tourne dans OTOKO pendant qu'on compose.
    pub convert_ahead: bool,
    /// L'application Android proposée en téléchargement (`/otoko.apk`).
    pub apk: Option<PathBuf>,
    pub version: String,
}

impl Config {
    /// Les réglages habituels, données dans `data` (`%APPDATA%\OTOKO`).
    pub fn new(data: &Path, version: &str) -> Config {
        let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("Otoko Vault.apk")));
        let apk = beside.filter(|p| p.is_file()).unwrap_or_else(|| data.join("Otoko Vault.apk"));
        Config {
            db: data.join("carriere.db"),
            shares_db: data.join("partages.db"),
            cache: data.join("mobile"),
            borrow_cache: std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("ComposerVault").join("mobile_cache").join("allege")),
            host: "0.0.0.0".into(),
            port: PORT,
            public_port: Some(PUBLIC_PORT),
            funnel_port: FUNNEL_PORT,
            warm: true,
            convert_ahead: true,
            apk: Some(apk),
            version: version.into(),
        }
    }
}

/// Le nom du dossier des mises à jour, écrit par le logiciel, lu par le serveur.
pub const UPDATES_FILE: &str = "dossier_maj.txt";

/// Le choix d'ouvrir le serveur au Wi-Fi de la maison, dans le dossier de données : « 1 » ou « 0 ».
pub const LAN_FILE: &str = "wifi_autorise.txt";

/// Tout appareil du Wi-Fi écoute sans mot de passe : c'est un choix explicite, refusé par défaut.
pub fn lan_get(data: &Path) -> bool {
    std::fs::read_to_string(data.join(LAN_FILE)).is_ok_and(|t| t.trim() == "1")
}

pub fn lan_set(data: &Path, on: bool) -> std::io::Result<()> {
    std::fs::write(data.join(LAN_FILE), if on { "1" } else { "0" })
}

/// Le code de liaison de l'autre PC, dans le dossier de données. Sans lui, personne ne lit ni n'écrit la base par
/// `/api/distant/…` : être sur le même Tailscale ne suffit plus (un téléphone perdu, un appareil partagé).
pub const LINK_FILE: &str = "code_autre_pc.txt";

/// Lettres et chiffres sans ambiguïté à recopier (ni 0/O, ni 1/I) : 32 signes, 5 bits chacun.
const LINK_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// Un nouveau code : 16 signes (80 bits), en quatre groupes, « ABCD-EFGH-JKMN-PQRS ».
fn new_link_code() -> String {
    let mut raw = [0u8; 16];
    getrandom::getrandom(&mut raw).expect("aléa du système indisponible");
    let signs: Vec<char> = raw.iter().map(|b| LINK_ALPHABET[(b & 31) as usize] as char).collect();
    signs.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-")
}

/// Le code tel qu'on le compare : sans espaces ni tirets, en majuscules.
pub fn link_normalize(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// Le code de liaison de ce PC, créé la première fois. Le logiciel et le serveur peuvent le demander en même temps : le
/// premier qui l'écrit gagne, l'autre relit le sien.
pub fn link_code(data: &Path) -> String {
    let file = data.join(LINK_FILE);
    for _ in 0..3 {
        if let Ok(t) = std::fs::read_to_string(&file) {
            if link_normalize(&t).len() >= 16 {
                return t.trim().to_string();
            }
        }
        let code = new_link_code();
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&file) {
            Ok(mut f) => {
                use std::io::Write;
                if f.write_all(code.as_bytes()).is_ok() {
                    return code;
                }
            }
            // Déjà là (écrit à l'instant par l'autre) ou illisible : on le relit, ou on le remplace s'il est vide.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if std::fs::read_to_string(&file).is_ok_and(|t| link_normalize(&t).len() < 16) {
                    let _ = std::fs::remove_file(&file);
                }
            }
            Err(_) => return code,
        }
    }
    std::fs::read_to_string(&file).map(|t| t.trim().to_string()).unwrap_or_default()
}

/// Un nouveau code : l'autre PC devra recopier celui-ci.
pub fn link_renew(data: &Path) -> String {
    let code = new_link_code();
    let _ = std::fs::write(data.join(LINK_FILE), &code);
    code
}

/// Le code reçu est-il celui de ce PC ? Comparé en temps constant.
pub fn link_matches(data: &Path, given: &str) -> bool {
    let (a, b) = (link_normalize(&link_code(data)), link_normalize(given));
    a.len() >= 16 && a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Avant ce réglage, ouvrir le pare-feu suffisait : qui l'a déjà fait garde son Wi-Fi. Le choix est ensuite écrit une
/// fois pour toutes.
fn lan_first_run(data: &Path) {
    use std::os::windows::process::CommandExt;
    if data.join(LAN_FILE).exists() {
        return;
    }
    let rule = std::process::Command::new("netsh.exe")
        .args(["advfirewall", "firewall", "show", "rule", "name=OTOKO mobile (Wi-Fi)"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .status()
        .is_ok_and(|s| s.success());
    let _ = lan_set(data, rule);
}

/// « Otoko-Studio-Vault-Pro-1.2.0-installation.exe » → (1, 2, 0).
pub fn installer_version(name: &str) -> Option<(u32, u32, u32)> {
    let v = name.strip_prefix("Otoko-Studio-Vault-Pro-")?.strip_suffix("-installation.exe")?;
    let mut p = v.split('.').map(|x| x.parse::<u32>().ok());
    Some((p.next()??, p.next()??, p.next()??))
}

/// L'installateur le plus récent d'un dossier : (version, fichier).
pub fn newest_in(dir: &Path) -> Option<(String, PathBuf)> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let v = installer_version(&e.file_name().to_string_lossy())?;
            Some((v, e.path()))
        })
        .max_by_key(|x| x.0)
        .map(|((a, b, c), p)| (format!("{a}.{b}.{c}"), p))
}

/// L'installateur que ce serveur peut proposer.
pub fn newest_installer(cfg: &Config) -> Option<(String, PathBuf)> {
    let data = cfg.db.parent()?;
    let dir = std::fs::read_to_string(data.join(UPDATES_FILE)).ok()?;
    newest_in(Path::new(dir.trim()))
}

/// La page web et la page des liens de partage, dans l'exécutable.
pub mod web {
    const FILES: &[(&str, &[u8], &str)] = &[
        ("index.html", include_bytes!("../web/index.html"), "text/html; charset=utf-8"),
        ("manifest.webmanifest", include_bytes!("../web/manifest.webmanifest"), "application/manifest+json"),
        ("sw.js", include_bytes!("../web/sw.js"), "text/javascript; charset=utf-8"),
        ("icon.png", include_bytes!("../web/icon.png"), "image/png"),
        ("partage/share.html", include_bytes!("../web/partage/share.html"), "text/html; charset=utf-8"),
        ("partage/share.css", include_bytes!("../web/partage/share.css"), "text/css; charset=utf-8"),
        ("partage/share.js", include_bytes!("../web/partage/share.js"), "text/javascript; charset=utf-8"),
    ];

    pub fn get(name: &str) -> Option<(&'static [u8], &'static str)> {
        FILES.iter().find(|f| f.0 == name).map(|f| (f.1, f.2))
    }
}

/// Une ligne dans `serveur.log` (jamais de jeton de partage ni de mot de passe).
pub fn log(st: &State, msg: &str) {
    use std::io::Write;
    let line = format!("{} {msg}\n", shares::now_iso());
    eprint!("{line}");
    let f = st.cfg.cache.join("serveur.log");
    if std::fs::metadata(&f).is_ok_and(|m| m.len() > 2 * 1024 * 1024) {
        let _ = std::fs::rename(&f, f.with_extension("log.1"));
    }
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(&f) {
        let _ = out.write_all(line.as_bytes());
    }
}

/// Les deux serveurs en marche.
pub struct Server {
    pub state: Arc<State>,
    private: http::Running,
    public: Option<http::Running>,
}

impl Server {
    pub fn port(&self) -> u16 {
        self.private.port()
    }

    pub fn public_port(&self) -> Option<u16> {
        self.public.as_ref().map(|p| p.port())
    }

    pub fn stop(&self) {
        self.private.stop();
        if let Some(p) = &self.public {
            p.stop();
        }
        log(&self.state, "Serveur mobile arrêté");
    }
}

fn handler<F>(f: F) -> Arc<http::Handler>
where
    F: Fn(&http::Request, &mut http::Conn) -> std::io::Result<()> + Send + Sync + 'static,
{
    Arc::new(f)
}

/// Démarre le serveur privé et, si les partages sont lisibles, le serveur public.
pub fn start(cfg: Config) -> Result<Server, String> {
    if !cfg.db.is_file() {
        return Err(format!("Carrière introuvable : {}", cfg.db.display()));
    }
    let _ = std::fs::create_dir_all(&cfg.cache);
    if let Some(data) = cfg.db.parent() {
        lan_first_run(data);
    }
    let shares = ShareStore::open(&cfg.shares_db).map_err(|e| eprintln!("Partages illisibles ({}) : {e}", cfg.shares_db.display())).ok();
    let st = Arc::new(State::new(cfg, shares));
    let _ = st.me.set(Arc::downgrade(&st));
    let s = st.clone();
    let addr = format!("{}:{}", st.cfg.host, st.cfg.port);
    let private = http::serve(&addr, handler(move |r, c| server::handle(&s, r, c)), 256 * 1024, server::headers()).map_err(|e| format!("Port {} indisponible : {e}", st.cfg.port))?;
    st.port.store(private.port(), Ordering::Relaxed);
    let public = if let Some(pp) = st.cfg.public_port.filter(|_| st.shares.is_some()) {
        let s = st.clone();
        match http::serve(&format!("127.0.0.1:{pp}"), handler(move |r, c| public::handle(&s, r, c)), 4096, public::headers()) {
            Ok(p) => {
                log(&st, &format!("Serveur des partages prêt sur 127.0.0.1:{} (publié par Tailscale Funnel)", p.port()));
                Some(p)
            }
            Err(e) => {
                log(&st, &format!("Serveur des partages non démarré : {e}"));
                None
            }
        }
    } else {
        None
    };
    if st.cfg.warm {
        server::Colors::warm(st.clone());
        if st.cfg.convert_ahead {
            server::warm_lossless(st.clone());
            server::warm_lossy(st.clone());
        }
        public::prewarm(&st, None);
        let s = st.clone();
        let _ = std::thread::Builder::new().name("https".into()).spawn(move || {
            let url = s.https.get();
            log(&s, &format!("Adresse HTTPS : {}", url.as_deref().unwrap_or("aucune")));
        });
    }
    log(&st, &format!("Serveur mobile prêt sur le port {} (Carrière : {})", private.port(), st.cfg.db.display()));
    Ok(Server { state: st, private, public })
}

/// Demande `path` au serveur local (avec l'en-tête de gestion) ; rend le JSON si la réponse est 200.
pub fn ask(port: u16, path: &str) -> Option<serde_json::Value> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), std::time::Duration::from_millis(400)).ok()?;
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(15)));
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Studio-Vault: 1\r\nConnection: close\r\n\r\n").as_bytes()).ok()?;
    let mut buf = Vec::new();
    let _ = s.take(4 * 1024 * 1024).read_to_end(&mut buf);
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    if !buf.starts_with(b"HTTP/1.1 200") {
        return None;
    }
    serde_json::from_slice(&buf[split + 4..]).ok()
}

/// Envoie une demande (créer, modifier, retirer un partage) au serveur de ce PC. `None` s'il ne répond pas ; sinon sa
/// réponse, ou son message d'erreur.
pub fn send(port: u16, method: &str, path: &str, body: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), std::time::Duration::from_millis(400)).ok()?;
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(15)));
    let data = body.to_string();
    let head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Studio-Vault: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
    s.write_all(head.as_bytes()).ok()?;
    s.write_all(data.as_bytes()).ok()?;
    let mut buf = Vec::new();
    let _ = s.take(1024 * 1024).read_to_end(&mut buf);
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let v: serde_json::Value = serde_json::from_slice(&buf[split + 4..]).unwrap_or_default();
    Some(if buf.starts_with(b"HTTP/1.1 2") { Ok(v) } else { Err(v["error"].as_str().unwrap_or("Le serveur a refusé.").to_string()) })
}

/// Un serveur mobile (OTOKO ou Studio Vault) répond sur ce port ?
pub fn answering(port: u16) -> bool {
    ask(port, "/api/ping").is_some_and(|v| v.get("version").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_code_is_made_once_and_typed_loosely() {
        let tmp = std::env::temp_dir().join(format!("otoko-essai-code-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let code = link_code(&tmp);
        assert_eq!(code.len(), 19, "{code}");
        assert_eq!(link_normalize(&code).len(), 16);
        assert!(link_normalize(&code).bytes().all(|b| LINK_ALPHABET.contains(&b)), "{code}");
        assert_eq!(link_code(&tmp), code, "le même à chaque fois");
        assert!(link_matches(&tmp, &code));
        assert!(link_matches(&tmp, &format!("  {}  ", code.replace('-', " ").to_lowercase())), "espaces et minuscules acceptés");
        assert!(!link_matches(&tmp, ""));
        assert!(!link_matches(&tmp, &code[..14]));
        assert!(!link_matches(&tmp, "AAAA-AAAA-AAAA-AAAA"));
        // Un fichier vide ou abîmé : un code neuf.
        std::fs::write(tmp.join(LINK_FILE), "x").unwrap();
        let fresh = link_code(&tmp);
        assert_ne!(fresh, code);
        assert_eq!(link_normalize(&fresh).len(), 16);
        // Nouveau code : l'ancien ne passe plus.
        let renewed = link_renew(&tmp);
        assert!(link_matches(&tmp, &renewed) && !link_matches(&tmp, &fresh));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
