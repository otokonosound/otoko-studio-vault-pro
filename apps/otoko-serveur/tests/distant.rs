//! L'autre PC et le PC principal, de bout en bout : la copie de la base, une écriture envoyée, l'état qui change, les mises à
//! jour proposées. Le serveur tourne sur une base temporaire.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use otoko_library::career::{Career, distant};
use serde_json::{Value as Json, json};

struct Res {
    status: u16,
    body: Vec<u8>,
}

/// `head` : les en-têtes en plus, chacun terminé par « \r\n ».
fn call(port: u16, method: &str, path: &str, head: &str, body: Option<&Json>) -> Res {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let data = body.map(|b| b.to_string()).unwrap_or_default();
    s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n{head}Content-Length: {}\r\n\r\n{data}", data.len()).as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap_or_else(|e| panic!("{method} {path} : {e} ({} octets reçus)", raw.len()));
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let status = String::from_utf8_lossy(&raw[..split]).split_whitespace().nth(1).unwrap().parse().unwrap();
    Res { status, body: raw[split + 4..].to_vec() }
}

fn json_of(r: &Res) -> Json {
    serde_json::from_slice(&r.body).unwrap_or(Json::Null)
}

/// Une requête écrite à la main, pour envoyer ce qu'un navigateur n'enverrait pas.
fn raw(port: u16, request: &str) -> u16 {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
}

#[test]
fn a_website_or_a_strange_request_gets_nothing() {
    let tmp = std::env::temp_dir().join(format!("otoko-essai-securite-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let secret = tmp.join("secret.txt");
    std::fs::write(&secret, "ne doit jamais sortir").unwrap();
    {
        let c = Career::open(&tmp.join("carriere.db")).unwrap();
        c.conn().execute("INSERT INTO tracks (id, path, filename, title, status) VALUES (1, ?1, 'secret.txt', 'Piège', 'idea')", [secret.to_string_lossy()]).unwrap();
    }
    // Le choix écrit d'avance : ce PC a peut-être déjà une règle de pare-feu pour le Wi-Fi.
    otoko_mobile::lan_set(&tmp, false).unwrap();
    let mut cfg = otoko_mobile::Config::new(&tmp, "essai");
    cfg.port = 0;
    cfg.public_port = None;
    cfg.warm = false;
    let server = otoko_mobile::start(cfg).unwrap();
    let p = server.port();
    let get = |host: &str, path: &str, extra: &str| raw(p, &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra}\r\n"));

    // Les adresses normales passent.
    assert_eq!(get(&format!("127.0.0.1:{p}"), "/api/ping", ""), 200);
    assert_eq!(get("mon-pc:8775", "/api/ping", ""), 200);
    assert_eq!(get("mon-pc.tail1234.ts.net", "/api/ping", ""), 200);
    // Un site piégé dont le domaine pointe vers ce PC : refusé, en lecture comme en écriture.
    assert_eq!(get("evil.example.com:8775", "/api/tracks", ""), 403);
    assert_eq!(get("evil.example.com", "/api/distant/base", "X-Studio-Vault: 1\r\n"), 403);
    // Une demande venue d'Internet par un relais : refusée, même si elle arrive par cet ordinateur.
    assert_eq!(get("mon-pc.tail1234.ts.net", "/api/tracks", "X-Forwarded-For: 8.8.8.8\r\n"), 403);
    assert_eq!(get("mon-pc.tail1234.ts.net", "/api/tracks", "Tailscale-Funnel-Request: ?1\r\n"), 403);
    // Le Wi-Fi est refusé tant qu'on ne l'a pas autorisé.
    assert_eq!(get("mon-pc.tail1234.ts.net", "/api/tracks", "X-Forwarded-For: 192.168.1.20\r\n"), 403);
    otoko_mobile::lan_set(&tmp, true).unwrap();
    std::thread::sleep(Duration::from_millis(5100));
    assert_eq!(get("mon-pc.tail1234.ts.net", "/api/tracks", "X-Forwarded-For: 192.168.1.20\r\n"), 200);
    // Une requête ambiguë est refusée.
    assert_eq!(raw(p, &format!("POST /api/distant/ecrire HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n")), 400);
    assert_eq!(raw(p, &format!("POST /api/distant/ecrire HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nContent-Length: 3\r\nContent-Length: 4\r\n\r\nabcd")), 400);
    // Un chemin de base vers autre chose qu'un fichier audio ne sort jamais.
    assert_eq!(get(&format!("127.0.0.1:{p}"), "/api/tracks/1/audio", ""), 404);
    server.stop();
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Un programme lancé par le serveur (tailscale, ffmpeg) héritait de toutes les connexions ouvertes et les coupait net en se
/// terminant : le client lisait « connexion réinitialisée » après une réponse pourtant complète (3 % des demandes).
#[test]
fn a_program_started_meanwhile_does_not_cut_connections() {
    let tmp = std::env::temp_dir().join(format!("otoko-essai-coupure-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    drop(Career::open(&tmp.join("carriere.db")).unwrap());
    let mut cfg = otoko_mobile::Config::new(&tmp, "essai");
    cfg.port = 0;
    cfg.public_port = None;
    cfg.warm = false;
    let server = otoko_mobile::start(cfg).unwrap();
    let p = server.port();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    let programs = std::thread::spawn(move || {
        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = std::process::Command::new("cmd").args(["/c", "exit"]).output();
        }
    });
    let once = |p: u16| -> std::io::Result<()> {
        let mut s = TcpStream::connect(("127.0.0.1", p))?;
        s.set_read_timeout(Some(Duration::from_secs(30)))?;
        s.write_all(format!("GET /api/maj HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nConnection: close\r\n\r\n").as_bytes())?;
        s.read_to_end(&mut Vec::new()).map(|_| ())
    };
    let (mut total, mut cut) = (0, 0);
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        total += 1;
        cut += once(p).is_err() as usize;
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    programs.join().unwrap();
    server.stop();
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(total > 100, "assez de demandes pour conclure : {total}");
    assert!(cut * 200 < total, "{cut} connexions coupées sur {total}");
}

#[test]
fn the_other_pc_reads_and_writes_through_the_mini() {
    let tmp = std::env::temp_dir().join(format!("otoko-essai-distant-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    {
        let c = Career::open(&tmp.join("carriere.db")).unwrap();
        c.conn().execute_batch("INSERT INTO tracks (id, path, filename, title, status) VALUES (1, 'a.wav', 'a.wav', 'Nuit', 'idea');").unwrap();
    }
    let mut cfg = otoko_mobile::Config::new(&tmp, "essai");
    cfg.port = 0;
    cfg.public_port = None;
    cfg.warm = false;
    let server = otoko_mobile::start(cfg).unwrap();
    let p = server.port();
    let code = otoko_mobile::link_code(&tmp);
    let app = format!("X-Studio-Vault: 1\r\nX-Otoko-Code: {code}\r\n");
    let app = app.as_str();

    // Sans l'en-tête de l'application, rien.
    assert_eq!(call(p, "GET", "/api/distant/base", "", None).status, 403);
    assert_eq!(call(p, "POST", "/api/distant/ecrire", "", Some(&json!({ "changes": [] }))).status, 403);
    // Un appareil de Tailscale qui ne connaît pas le code de liaison : rien non plus, ni lecture ni écriture.
    assert_eq!(call(p, "GET", "/api/distant/base", "X-Studio-Vault: 1\r\n", None).status, 401);
    assert_eq!(call(p, "GET", "/api/distant/etat", "X-Studio-Vault: 1\r\nX-Otoko-Code: AAAA-AAAA-AAAA-AAAA\r\n", None).status, 401);
    assert_eq!(call(p, "POST", "/api/distant/ecrire", "X-Studio-Vault: 1\r\nX-Otoko-Code: \r\n", Some(&json!({ "changes": [] }))).status, 401);
    // Le code recopié à la main, en minuscules et sans tirets, passe.
    let typed = code.replace('-', "").to_lowercase();
    assert_eq!(call(p, "GET", "/api/distant/etat", &format!("X-Studio-Vault: 1\r\nX-Otoko-Code: {typed}\r\n"), None).status, 200);

    // La copie de la base.
    let state = json_of(&call(p, "GET", "/api/distant/etat", app, None));
    assert_eq!(state["machine"].as_str(), Some(std::env::var("COMPUTERNAME").unwrap_or_default().as_str()), "le serveur dit sur quelle machine il tourne");
    let stamp = state["stamp"].as_str().unwrap().to_string();
    let base = call(p, "GET", "/api/distant/base", app, None);
    assert_eq!(base.status, 200);
    let copy = tmp.join("copie.db");
    std::fs::write(&copy, &base.body).unwrap();
    let c = Career::open(&copy).unwrap();
    distant::install(c.conn()).unwrap();
    let title: String = c.conn().query_row("SELECT title FROM tracks WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(title, "Nuit");

    // Une écriture sur la copie arrive sur le PC principal.
    c.conn().execute("UPDATE tracks SET status = 'wip', notes = 'depuis l''autre PC' WHERE id = 1", []).unwrap();
    let (changes, last) = distant::pending(c.conn(), 100).unwrap();
    let r = call(p, "POST", "/api/distant/ecrire", app, Some(&json!({ "changes": changes })));
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(json_of(&r)["applied"], 1);
    distant::sent(c.conn(), last).unwrap();
    assert_eq!(distant::waiting(c.conn()), 0);
    let mini = Career::open(&tmp.join("carriere.db")).unwrap();
    let (status, notes): (String, String) = mini.conn().query_row("SELECT status, notes FROM tracks WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((status.as_str(), notes.as_str()), ("wip", "depuis l'autre PC"));
    let now = json_of(&call(p, "GET", "/api/distant/etat", app, None))["stamp"].as_str().unwrap().to_string();
    assert_ne!(now, stamp, "l'état change avec la base");

    // Une demande hors des tables suivies est refusée.
    assert_eq!(call(p, "POST", "/api/distant/ecrire", app, Some(&json!({ "changes": [{ "table": "sqlite_master", "op": "set", "row": { "name": "x" } }] }))).status, 400);

    // Les mises à jour proposées.
    assert_eq!(call(p, "GET", "/maj/pc", "", None).status, 404);
    let dir = tmp.join("majs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Otoko-Studio-Vault-Pro-1.2.0-installation.exe"), b"installateur").unwrap();
    std::fs::write(tmp.join(otoko_mobile::UPDATES_FILE), dir.to_string_lossy().as_bytes()).unwrap();
    assert_eq!(json_of(&call(p, "GET", "/api/maj", "", None))["pc"], "1.2.0");
    let f = call(p, "GET", "/maj/pc", "", None);
    assert_eq!((f.status, f.body.as_slice()), (200, b"installateur".as_slice()));

    server.stop();
    drop(c);
    drop(mini);
    let _ = std::fs::remove_dir_all(&tmp);
}
