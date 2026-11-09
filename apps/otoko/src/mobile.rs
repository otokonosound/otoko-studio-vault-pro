//! Le serveur d'Otoko Vault (l'app mobile) vu depuis la page Mobile et partage : le logiciel le fait tourner lui-même quand
//! on le demande (si le PC ne le fait pas déjà par la tâche planifiée « OTOKO mobile »), montre les adresses à taper sur
//! le téléphone, et lance les scripts du PC principal (démarrage avec le PC, partage public, passage depuis Studio Vault).

use std::os::windows::process::CommandExt;
use std::path::PathBuf;

use crate::state::{Done, State};
use crate::{AppWindow, settings};

const STUDIO_VAULT_PORT: u16 = 8765;

/// Les réglages du serveur quand il tourne dans le logiciel : pas de conversion de toute la bibliothèque (la tâche
/// planifiée du PC s'en charge).
fn config(st: &State) -> otoko_mobile::Config {
    let mut cfg = otoko_mobile::Config::new(&crate::library::data_dir(), env!("CARGO_PKG_VERSION"));
    cfg.db = st.career_path.clone();
    cfg.shares_db = crate::career::shares_path(&st.career_path);
    cfg.convert_ahead = false;
    cfg
}

/// Au lancement : le logiciel sert l'app mobile si on le lui a demandé et que le PC ne le fait pas déjà.
pub fn autostart(st: &mut State) {
    if st.selftest {
        return;
    }
    // La tâche est prévue mais son serveur s'est arrêté : il repart sans qu'on ait à s'en occuper.
    if st.career_path == crate::career::default_path() {
        let _ = std::thread::Builder::new().name("serveur-relance".into()).spawn(|| {
            if task_installed() && !otoko_mobile::answering(otoko_mobile::PORT) {
                let _ = schtasks(&["/Run", "/TN", "OTOKO mobile"]);
            }
        });
    }
    if settings::get("mobile_server").and_then(|v| v.as_bool()) != Some(true) {
        return;
    }
    let cfg = config(st);
    if !otoko_mobile::answering(cfg.port) {
        st.mobile = otoko_mobile::start(cfg).ok();
    }
}

/// `otoko-serveur.exe` : à côté du programme (installé), sinon la compilation locale.
fn server_exe() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("otoko-serveur.exe"))).filter(|p| p.is_file())
}

fn schtasks(args: &[&str]) -> bool {
    std::process::Command::new("schtasks.exe").args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).creation_flags(0x0800_0000).status().is_ok_and(|s| s.success())
}

/// « Démarrer avec le PC » sans script : la tâche planifiée « OTOKO mobile » à l'ouverture de
/// session, créée (ou retirée) directement. Le pare-feu pour le Wi-Fi de la maison, lui, demande
/// les droits d'administrateur : c'est le script qui s'en charge.
pub fn task(app: &AppWindow, st: &mut State, install: bool) {
    if st.selftest {
        return;
    }
    if install {
        let Some(exe) = server_exe() else {
            app.set_message("otoko-serveur.exe introuvable : réinstalle le logiciel.".into());
            return;
        };
        // La tâche prend le relais : le logiciel libère le port.
        if let Some(s) = st.mobile.take() {
            s.stop();
        }
        let tr = format!("\"{}\"", exe.display());
        let ok = schtasks(&["/Create", "/SC", "ONLOGON", "/TN", "OTOKO mobile", "/TR", &tr, "/RL", "LIMITED", "/F"]);
        let started = ok && schtasks(&["/Run", "/TN", "OTOKO mobile"]);
        app.set_message(
            if started {
                "Le serveur mobile démarre maintenant avec le PC (tâche « OTOKO mobile »), même le logiciel fermé."
            } else if ok {
                "Tâche « OTOKO mobile » créée ; elle démarrera à la prochaine ouverture de session."
            } else {
                "Windows a refusé de créer la tâche : lance « Installer le serveur (admin)… », qui s'en charge."
            }
            .into(),
        );
    } else {
        let _ = schtasks(&["/End", "/TN", "OTOKO mobile"]);
        let ok = schtasks(&["/Delete", "/TN", "OTOKO mobile", "/F"]);
        app.set_message(if ok { "Le serveur mobile ne démarre plus avec le PC (« Servir depuis ce logiciel » reste possible)." } else { "La tâche « OTOKO mobile » n'a pas pu être retirée (elle a peut-être été créée en administrateur : relance le script)." }.into());
    }
    refresh(app, st);
}

/// Une étape pour écouter sur le téléphone : où on en est, et le bouton qui fait la suite.
pub struct Step {
    pub done: bool,
    pub title: String,
    pub text: String,
    pub label: String,
    pub action: String,
}

fn step_row(done: bool, title: &str, text: String, label: &str, action: String) -> Step {
    Step { done, title: title.into(), text, label: label.into(), action }
}

/// Les étapes, d'après ce que répond le serveur et ce qu'il y a sur ce PC.
fn steps(ping: Option<&serde_json::Value>, task: bool) -> Vec<Step> {
    let (installed, name) = otoko_mobile::server::tailscale_state();
    let running_without = ping.is_some_and(|p| p["transcoding"] == false);
    step_rows(installed, name, ping.is_some(), task, otoko_mobile::media::find_ffmpeg().is_some(), running_without)
}

/// Pour la vidéo de présentation : la page ne regarde plus ce PC, dont l'adresse ne doit jamais se montrer.
static SHOWCASE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn hold_for_showcase() {
    SHOWCASE.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// La page telle que la voit un PC neuf, ou une fois tout installé. L'adresse est un exemple.
pub fn showcase(app: &AppWindow, ready: bool) {
    let name = ready.then(|| "ton-pc.exemple.ts.net".to_string());
    let rows = step_rows(ready, name, ready, ready, ready, false);
    let address = rows.last().and_then(|s| s.action.strip_prefix("copy:")).map(str::to_string);
    let text = if ready { "Serveur mobile actif, il démarre avec le PC." } else { "Serveur mobile arrêté." }.to_string();
    app.set_mobile_wifi(false);
    landed(app, text, ready, ready, rows, address);
}

/// L'état du serveur et les étapes, à l'écran.
pub fn landed(app: &AppWindow, text: String, on: bool, task: bool, steps: Vec<Step>, address: Option<String>) {
    app.set_mobile_text(text.into());
    app.set_mobile_on(on);
    app.set_mobile_task(task);
    // La dernière étape, le téléphone, ne se coche pas : le reste dit s'il manque quelque chose.
    app.set_mobile_missing(steps.iter().rev().skip(1).any(|s| !s.done));
    show_codes(app, address.as_deref());
    let rows: Vec<crate::MobileStep> = steps.into_iter().map(|s| crate::MobileStep { done: s.done, title: s.title.into(), text: s.text.into(), label: s.label.into(), action: s.action.into() }).collect();
    app.set_mobile_steps(slint::ModelRc::new(slint::VecModel::from(rows)));
}

fn step_rows(installed: bool, name: Option<String>, on: bool, task: bool, ffmpeg_here: bool, running_without: bool) -> Vec<Step> {
    let address = name.as_ref().map(|n| format!("http://{n}:{}", otoko_mobile::PORT));
    let tailscale = match (&name, installed) {
        (Some(_), _) => step_row(true, "Tailscale est connecté sur ce PC", "Il relie tes appareils directement, de façon chiffrée, sans rien ouvrir sur Internet.".into(), "", String::new()),
        (None, true) => step_row(false, "Connecte Tailscale sur ce PC", "Une page s'ouvre : connecte-toi avec un compte gratuit, Google ou Microsoft.".into(), "Me connecter", "login".into()),
        (None, false) => step_row(false, "Tailscale, sur ce PC", "Gratuit. Il relie ton téléphone à ton PC, de façon chiffrée, sans rien ouvrir sur Internet.".into(), "Installer Tailscale", "tailscale".into()),
    };
    let server = if on && task {
        step_row(true, "Le serveur démarre avec ce PC", "Ta musique est disponible même logiciel fermé.".into(), "", String::new())
    } else if task {
        step_row(false, "Le serveur est arrêté", "Il est prévu au démarrage du PC mais ne répond pas : relance-le.".into(), "Relancer le serveur", "restart".into())
    } else {
        step_row(false, "Fais démarrer le serveur avec ce PC", "Pour écouter même quand le logiciel est fermé.".into(), "Démarrer avec le PC", "task".into())
    };
    let ffmpeg = if ffmpeg_here && on && running_without {
        step_row(false, "ffmpeg est installé : redémarre le serveur", "Le serveur le prendra en compte au redémarrage.".into(), "Redémarrer le serveur", "restart".into())
    } else if ffmpeg_here {
        step_row(true, "ffmpeg est installé", "Écoute légère en données mobiles et liens de partage : c'est bon.".into(), "", String::new())
    } else {
        step_row(false, "ffmpeg, sur ce PC", "Pour écouter en qualité légère en données mobiles et faire écouter tes liens de partage.".into(), "Installer ffmpeg", "ffmpeg".into())
    };
    let phone = match address {
        Some(a) => step_row(false, "Sur ton téléphone : scanne les deux codes", format!("Le premier installe Tailscale (connecte-toi avec le même compte). Le second ouvre ta musique ({a}) : touche « App Android », puis « Ouvrir l'app »."), "Copier l'adresse", format!("copy:{a}")),
        None => step_row(false, "Sur ton téléphone", "Quand les étapes ci-dessus sont faites, les codes à scanner s'affichent ici.".into(), "", String::new()),
    };
    vec![tailscale, server, ffmpeg, phone]
}

/// Tailscale sur le Play Store.
const TAILSCALE_PLAY: &str = "https://play.google.com/store/apps/details?id=com.tailscale.ipn";

/// Un code à scanner.
fn code(text: &str) -> Option<slint::Image> {
    const SCALE: usize = 6;
    const BORDER: usize = 3;
    let qr = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let width = qr.width();
    let dark = qr.to_colors();
    let side = (width + 2 * BORDER) * SCALE;
    let mut buf = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(side as u32, side as u32);
    for (i, px) in buf.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = ((i % side) / SCALE, (i / side) / SCALE);
        let on = (BORDER..width + BORDER).contains(&x) && (BORDER..width + BORDER).contains(&y) && dark[(y - BORDER) * width + (x - BORDER)] == qrcode::Color::Dark;
        let v = if on { 0 } else { 255 };
        *px = slint::Rgb8Pixel { r: v, g: v, b: v };
    }
    Some(slint::Image::from_rgb8(buf))
}

/// Les deux codes du téléphone, quand on connaît l'adresse.
pub fn show_codes(app: &AppWindow, address: Option<&str>) {
    let codes = address.and_then(|a| Some((code(TAILSCALE_PLAY)?, code(a)?)));
    app.set_mobile_codes(codes.is_some());
    if let Some((first, second)) = codes {
        app.set_mobile_code_tailscale(first);
        app.set_mobile_code_page(second);
    }
}

/// Une seule installation à la fois.
static INSTALLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Installe ce qui manque, en fond : ffmpeg, Tailscale, puis la connexion à Tailscale.
fn install(app: &AppWindow, st: &mut State, what: &'static [&'static str]) {
    use std::sync::atomic::Ordering;
    if INSTALLING.swap(true, Ordering::SeqCst) {
        app.set_message("Une installation est déjà en cours.".into());
        return;
    }
    let tx = st.jobs.clone();
    st.spawn(move || {
        let say = |text: String| {
            let _ = tx.send(Done::Say(text));
        };
        let result = (|| {
            for w in what {
                match *w {
                    "ffmpeg" if otoko_mobile::media::find_ffmpeg().is_none() => {
                        otoko_mobile::complements::install_ffmpeg(&say)?;
                    }
                    "tailscale" if otoko_mobile::server::tailscale_exe().is_none() => otoko_mobile::complements::install_tailscale(&say)?,
                    "login" if otoko_mobile::server::tailscale_state().1.is_none() => otoko_mobile::complements::tailscale_login(&say)?,
                    _ => {}
                }
            }
            Ok(())
        })();
        Done::AddonsDone(result)
    });
}

/// Tout est installé : le serveur repart pour prendre ffmpeg en compte.
pub fn addons_done(app: &AppWindow, st: &mut State, r: Result<(), String>) {
    INSTALLING.store(false, std::sync::atomic::Ordering::SeqCst);
    restart_server(st);
    app.set_message(match r {
        Ok(()) => "C'est prêt. Scanne les deux codes avec ton téléphone.".into(),
        Err(e) => e.into(),
    });
    refresh(app, st);
}

fn restart_server(st: &mut State) {
    if let Some(s) = st.mobile.take() {
        s.stop();
        st.mobile = otoko_mobile::start(config(st)).ok();
    } else {
        let _ = schtasks(&["/End", "/TN", "OTOKO mobile"]);
        let _ = schtasks(&["/Run", "/TN", "OTOKO mobile"]);
    }
}

/// Un bouton des étapes.
pub fn step(app: &AppWindow, st: &mut State, action: &str) {
    if st.selftest {
        return;
    }
    match action {
        // Le seul bouton à connaître : tout ce qui manque s'installe, dans l'ordre.
        "tout" => {
            if !task_installed() {
                task(app, st, true);
            }
            install(app, st, &["ffmpeg", "tailscale", "login"]);
        }
        "tailscale" => install(app, st, &["tailscale", "login"]),
        "login" => install(app, st, &["login"]),
        "ffmpeg" => install(app, st, &["ffmpeg"]),
        "task" => task(app, st, true),
        "restart" => {
            restart_server(st);
            app.set_message("Serveur redémarré.".into());
            refresh(app, st);
        }
        other => {
            if let Some(address) = other.strip_prefix("copy:") {
                crate::publish::copy(address);
                app.set_message(format!("Adresse copiée : {address}").into());
            }
        }
    }
}

/// Le dossier où le serveur lit son choix pour le Wi-Fi : celui de la base qu'il sert.
fn data(st: &State) -> PathBuf {
    st.career_path.parent().map(PathBuf::from).unwrap_or_else(crate::library::data_dir)
}

/// Ouvrir au Wi-Fi demande le pare-feu, donc les droits d'administrateur : le script explique et demande confirmation.
/// Refuser s'applique tout de suite, sans droits : le serveur ne répond plus au Wi-Fi.
pub fn wifi_set(app: &AppWindow, st: &mut State, on: bool) {
    if on {
        script(app, st, "wifi");
        return;
    }
    if st.selftest {
        return;
    }
    match otoko_mobile::lan_set(&data(st), false) {
        Ok(()) => app.set_message("Le Wi-Fi est refusé : seuls cet ordinateur et tes appareils Tailscale ont accès.".into()),
        Err(e) => app.set_message(format!("Réglage impossible à enregistrer : {e}").into()),
    }
    refresh(app, st);
}

/// Démarre ou arrête le serveur dans le logiciel.
pub fn toggle(app: &AppWindow, st: &mut State) {
    if let Some(s) = st.mobile.take() {
        s.stop();
        settings::set("mobile_server", serde_json::json!(false));
        app.set_message("Le logiciel ne sert plus l'app mobile (la tâche « OTOKO mobile » du PC prend le relais si elle est installée).".into());
    } else {
        let cfg = config(st);
        if otoko_mobile::answering(cfg.port) {
            app.set_message("Le PC sert déjà l'app mobile (tâche « OTOKO mobile ») : rien à faire.".into());
        } else {
            match otoko_mobile::start(cfg) {
                Ok(s) => {
                    st.mobile = Some(s);
                    if !st.selftest {
                        settings::set("mobile_server", serde_json::json!(true));
                    }
                    app.set_message("Le logiciel sert l'app mobile tant qu'il est ouvert.".into());
                }
                Err(e) => app.set_message(format!("Serveur mobile impossible : {e}").into()),
            }
        }
    }
    refresh(app, st);
}

/// L'état du serveur et les adresses, lus en arrière-plan (Tailscale peut mettre quelques
/// secondes à répondre).
pub fn refresh(app: &AppWindow, st: &mut State) {
    if SHOWCASE.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    app.set_mobile_here(st.mobile.is_some());
    app.set_mobile_switched(switched().is_some());
    app.set_mobile_wifi(otoko_mobile::lan_get(&data(st)));
    if app.get_mobile_text().is_empty() {
        app.set_mobile_text("Recherche du serveur mobile…".into());
    }
    let here = st.mobile.is_some();
    let port = st.mobile.as_ref().map(|s| s.port()).unwrap_or(otoko_mobile::PORT);
    st.spawn(move || {
        let task = task_installed();
        let (text, on) = status(here, port, task);
        let ping = otoko_mobile::ask(port, "/api/ping");
        let rows = steps(ping.as_ref(), task);
        let address = rows.last().and_then(|s| s.action.strip_prefix("copy:")).map(str::to_string);
        Done::Mobile { text, on, task, steps: rows, address }
    });
}

/// La bascule depuis Studio Vault a été faite (date).
fn switched() -> Option<String> {
    let t = std::fs::read_to_string(crate::library::data_dir().join("bascule.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()?;
    Some(v["date"].as_str().unwrap_or("").chars().take(10).collect())
}

fn task_installed() -> bool {
    std::process::Command::new("schtasks.exe")
        .args(["/Query", "/TN", "OTOKO mobile"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .status()
        .is_ok_and(|s| s.success())
}

fn status(here: bool, port: u16, task: bool) -> (String, bool) {
    let mut lines = Vec::new();
    let ping = otoko_mobile::ask(port, "/api/ping");
    let on = ping.is_some();
    match ping {
        Some(p) => {
            let who = if here { "dans ce logiciel, tant qu'il est ouvert" } else if task { "tâche « OTOKO mobile » du PC" } else { "une autre fenêtre de ce PC" };
            lines.push(format!("Serveur mobile actif ({who}) · {} morceaux · {} sorties.", p["tracks"], p["releases"]));
            let host = std::env::var("COMPUTERNAME").unwrap_or_default().to_lowercase();
            let tailscale = p["tailscale"].as_str().map(str::to_string).unwrap_or_else(|| format!("http://{host}:{port}"));
            let mut ways = vec![format!("par Tailscale : {tailscale}")];
            if let Some(l) = p["lan"].as_str().filter(|_| p["wifi"] == true) {
                ways.push(format!("à la maison : {l}"));
            }
            if let Some(h) = p["https"].as_str() {
                ways.push(format!("en HTTPS : {h}"));
            }
            lines.push(format!("Sur le téléphone (Otoko Vault ou navigateur) · {}.", ways.join(" · ")));
            if p["apk"] == true {
                lines.push(format!("Otoko Vault pour Android : {tailscale}/otoko.apk (ou « App Android » sur la page mobile). À la première ouverture, l'app demande l'adresse ci-dessus."));
            }
            if p["transcoding"] == false {
                lines.push("ffmpeg n'est pas sur ce PC : le téléphone reçoit les fichiers d'origine (lourds en données mobiles) et les liens de partage ne peuvent pas être écoutés. Pour l'ajouter : « winget install Gyan.FFmpeg » dans un terminal, puis relance le serveur.".into());
            }
            if p["tracks"].as_i64() == Some(0) {
                lines.push("La bibliothèque est vide : ajoute tes dossiers de musique depuis l'accueil pour les retrouver sur le téléphone.".into());
            }
            if let Some(s) = otoko_mobile::ask(port, "/api/shares/status") {
                let base = s["public_base"].as_str().unwrap_or("");
                lines.push(if s["funnel"] == true {
                    format!("Liens de partage publics actifs : {base}/s/…")
                } else {
                    format!("Liens de partage : pas encore publiés sur Internet (« Partage public » les met sur {base}).")
                });
            }
        }
        None => lines.push(if task {
            "Serveur mobile arrêté : la tâche « OTOKO mobile » est installée mais ne répond pas (journal : %APPDATA%\\OTOKO\\mobile\\serveur.log).".into()
        } else {
            "Serveur mobile arrêté. « Servir depuis ce logiciel » l'allume tant qu'il est ouvert ; « Démarrer avec le PC » l'installe pour de bon, même logiciel fermé.".into()
        }),
    }
    if here && !task {
        lines.push("Pour que le téléphone y accède même logiciel fermé : « Démarrer avec le PC ».".into());
    }
    match switched() {
        Some(d) => lines.push(format!("Studio Vault remplacé le {d} (« Revenir à Studio Vault » annule).")),
        None if otoko_mobile::answering(STUDIO_VAULT_PORT) => {
            lines.push("Studio Vault sert encore l'app mobile à côté (port 8765) : « Remplacer Studio Vault » quand tu es prêt.".into())
        }
        None => {}
    }
    (lines.join("\n"), on)
}

/// Le dossier des scripts : à côté du programme, et seulement là (aucun autre script n'est lancé en administrateur).
fn scripts() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("mobile"))).filter(|d| d.is_dir())
}

/// Lance un script du PC principal dans sa fenêtre (il demande les droits d'administrateur).
pub fn script(app: &AppWindow, st: &mut State, which: &str) {
    let (file, what) = match which {
        "installer" => ("installer_serveur_mobile.ps1", "Installation du serveur mobile au démarrage du PC"),
        "wifi" => ("wifi_maison.ps1", "Ouverture au Wi-Fi de la maison"),
        "partage" => ("partage_public.ps1", "Publication des liens de partage"),
        "couper" => ("couper_partage_public.ps1", "Coupure des liens de partage publics"),
        "bascule" => ("bascule_studio_vault.ps1", "Remplacement de Studio Vault"),
        "retour" => ("retour_studio_vault.ps1", "Retour à Studio Vault"),
        _ => return,
    };
    if which == "installer" || which == "bascule" {
        // La tâche planifiée prend le relais : le logiciel libère le port.
        if let Some(s) = st.mobile.take() {
            s.stop();
        }
    }
    if st.selftest {
        return;
    }
    let Some(path) = scripts().map(|d| d.join(file)).filter(|p| p.is_file()) else {
        app.set_message("Les scripts du serveur mobile sont introuvables : réinstalle le logiciel.".into());
        return;
    };
    let r = std::process::Command::new("powershell.exe").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(&path).spawn();
    app.set_message(match r {
        Ok(_) => format!("{what} : suis la fenêtre qui s'ouvre (Windows demande l'accord d'administrateur). « Actualiser » ensuite."),
        Err(e) => format!("Script introuvable ({e}) : {}", path.display()),
    }
    .into());
}

pub fn open_page(app: &AppWindow, st: &State) {
    let port = st.mobile.as_ref().map(|s| s.port()).unwrap_or(otoko_mobile::PORT);
    let _ = std::process::Command::new("explorer.exe").arg(format!("http://127.0.0.1:{port}/")).spawn();
    app.set_message("Page mobile ouverte dans ton navigateur (la même que sur le téléphone).".into());
}
