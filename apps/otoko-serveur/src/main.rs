//! Otoko Vault · serveur mobile, sans fenêtre : ta bibliothèque servie au téléphone (Tailscale, Wi-Fi de
//! la maison) et les liens de partage publics. Lancé au démarrage par la tâche planifiée
//! « OTOKO mobile » ; Otoko Studio Vault Pro peut aussi le faire tourner lui-même.
//!
//! otoko-serveur [--port 8775] [--port-public 8776] [--donnees DOSSIER] [--sans-preparation]

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let flag = |name: &str| args.iter().any(|a| a == name);
    if flag("--help") || flag("-h") {
        println!("otoko-serveur [--port 8775] [--port-public 8776] [--donnees DOSSIER] [--sans-preparation]");
        return;
    }
    let data = value("--donnees").map(PathBuf::from).unwrap_or_else(|| std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("OTOKO"));
    let mut cfg = otoko_mobile::Config::new(&data, env!("CARGO_PKG_VERSION"));
    let port = |v: Option<String>, d: u16| v.and_then(|p| p.parse().ok()).unwrap_or(d);
    cfg.port = port(value("--port").or(std::env::var("OTOKO_MOBILE_PORT").ok()), cfg.port);
    cfg.public_port = Some(port(value("--port-public").or(std::env::var("OTOKO_PUBLIC_PORT").ok()), otoko_mobile::PUBLIC_PORT));
    if flag("--sans-preparation") {
        cfg.warm = false;
    }
    // Le logiciel ouvert peut servir le téléphone lui-même : on attend qu'il se ferme pour prendre le relais.
    let mut said = false;
    loop {
        while otoko_mobile::answering(cfg.port) {
            if !said {
                note(&data, &format!("Otoko Studio Vault Pro sert déjà l'app mobile sur le port {} : relais à sa fermeture.", cfg.port));
                said = true;
            }
            std::thread::sleep(std::time::Duration::from_secs(15));
        }
        match otoko_mobile::start(cfg.clone()) {
            Ok(server) => {
                println!("Otoko Vault · serveur mobile sur le port {} (partages : {:?})", server.port(), server.public_port());
                loop {
                    std::thread::park();
                }
            }
            Err(e) => {
                note(&data, &e);
                if !cfg.db.is_file() {
                    // Pas encore de base : le logiciel la créera ; on réessaie dans une minute.
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    continue;
                }
                std::process::exit(1);
            }
        }
    }
}

fn note(data: &std::path::Path, msg: &str) {
    eprintln!("{msg}");
    let dir = data.join("mobile");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("serveur.log")).and_then(|mut f| {
        use std::io::Write;
        writeln!(f, "{} {msg}", otoko_mobile::shares::now_iso())
    });
}
