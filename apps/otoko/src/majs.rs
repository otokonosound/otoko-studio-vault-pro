//! Les mises à jour en un clic. Le PC principal garde les installateurs dans un dossier ; le logiciel propose la nouvelle version
//! et l'installe sans questions. L'autre PC la demande au PC principal, le téléphone aussi (`/api/maj`).

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use slint::ComponentHandle;

use crate::state::{Done, State};
use crate::{AppWindow, CPrefs};

/// La version en trois nombres, comparable.
pub fn parse(v: &str) -> (u32, u32, u32) {
    let mut p = v.trim().split('.').map(|x| x.parse::<u32>().unwrap_or(0));
    (p.next().unwrap_or(0), p.next().unwrap_or(0), p.next().unwrap_or(0))
}

/// Le dossier des installateurs de ce PC.
pub fn folder() -> Option<PathBuf> {
    let file = crate::library::data_dir().join(otoko_mobile::UPDATES_FILE);
    std::fs::read_to_string(file).ok().map(|d| PathBuf::from(d.trim())).filter(|d| !d.as_os_str().is_empty())
}

pub fn set_folder(dir: Option<&Path>) {
    let file = crate::library::data_dir().join(otoko_mobile::UPDATES_FILE);
    let _ = match dir {
        Some(d) => std::fs::write(file, d.to_string_lossy().as_bytes()),
        None => std::fs::remove_file(file),
    };
}

/// D'où vient une nouvelle version.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Le dossier des installateurs de ce PC.
    Folder(PathBuf),
    /// Le PC principal, pour un autre PC qui lui est relié, avec l'empreinte SHA-256 qu'il donne de son installateur (un
    /// serveur plus ancien n'en donne pas).
    MainPc(Option<String>),
    /// La page des versions sur GitHub : l'adresse de l'installateur et de quoi le vérifier.
    Web(Release),
}

/// Un installateur publié sur GitHub. Il ne se lance qu'après vérification de son empreinte SHA-256 : celle que GitHub
/// calcule à l'envoi (« digest »), sinon celle du fichier « ….exe.sha256 » publié à côté.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub url: String,
    pub sha256: Option<String>,
    pub sha256_url: Option<String>,
}

/// « sha256:ABC… », « abc…  fichier.exe » ou « abc… » : les 64 chiffres hexadécimaux, en minuscules.
pub fn sha256_in(text: &str) -> Option<String> {
    let t = text.trim();
    let t = t.strip_prefix("sha256:").unwrap_or(t);
    let hex = t.split_whitespace().next()?.to_ascii_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

/// Le fichier est-il exactement celui attendu ? Sinon il est effacé : rien ne lance un installateur douteux.
fn verified(path: &Path, expected: &str) -> Result<(), String> {
    match otoko_mobile::complements::sha256_of(path) {
        Ok(h) if h == expected => Ok(()),
        Ok(_) => {
            let _ = std::fs::remove_file(path);
            Err("l'installateur reçu n'est pas celui publié (empreinte différente) : il a été effacé. Réessaie plus tard, ou installe la version à la main depuis la page Releases.".into())
        }
        Err(e) => Err(format!("installateur illisible : {e}")),
    }
}

/// La dernière version publiée. Rien n'est envoyé : c'est une simple lecture de la page publique.
const RELEASES: &str = "https://api.github.com/repos/otokonosound/otoko-studio-vault-pro/releases/latest";

/// (version, installateur) d'après la réponse de GitHub.
fn read_release(v: &serde_json::Value) -> Option<(String, Release)> {
    let version = v["tag_name"].as_str()?.trim_start_matches('v').to_string();
    let name = format!("Otoko-Studio-Vault-Pro-{version}-installation.exe");
    let ours = |u: &str| u.starts_with("https://github.com/otokonosound/");
    let assets = v["assets"].as_array()?;
    let asset = assets.iter().find(|a| a["name"] == name.as_str())?;
    let url = asset["browser_download_url"].as_str().filter(|u| ours(u))?;
    let sha256 = asset["digest"].as_str().and_then(sha256_in);
    let sha256_url = assets
        .iter()
        .find(|a| a["name"] == format!("{name}.sha256").as_str())
        .and_then(|a| a["browser_download_url"].as_str())
        .filter(|u| ours(u))
        .map(str::to_string);
    Some((version, Release { url: url.to_string(), sha256, sha256_url }))
}

/// Cherche une version plus récente, en fond.
pub fn check(st: &mut State) {
    if st.selftest {
        return;
    }
    let remote = st.distant.as_ref().map(|d| d.url.clone());
    st.spawn(move || {
        let current = parse(env!("CARGO_PKG_VERSION"));
        let found = match remote {
            Some(url) => crate::fond::fetch_json(&format!("{url}/api/maj"))
                .ok()
                .and_then(|v| Some((v["pc"].as_str()?.to_string(), v["pc_sha256"].as_str().and_then(sha256_in))))
                .filter(|(v, _)| parse(v) > current)
                .map(|(v, sha)| (v, Source::MainPc(sha))),
            None => folder()
                .and_then(|d| otoko_mobile::newest_in(&d))
                .filter(|(v, _)| parse(v) > current)
                .map(|(v, p)| (v, Source::Folder(p)))
                .or_else(|| crate::fond::fetch_json(RELEASES).ok().and_then(|v| read_release(&v)).filter(|(v, _)| parse(v) > current).map(|(v, u)| (v, Source::Web(u)))),
        };
        Done::Update(found)
    });
}

pub fn found(app: &AppWindow, st: &mut State, r: Option<(String, Source)>) {
    match r {
        Some((v, p)) => {
            st.update_ready = Some(v);
            st.update_source = Some(p);
            app.set_message(format!("Une nouvelle version est prête : {}.", st.update_ready.as_deref().unwrap_or("")).into());
        }
        None => {
            st.update_ready = None;
            st.update_source = None;
        }
    }
    push(app, st);
    if app.get_career_tab() == 0 && app.get_space() == 4 {
        crate::accueil::fill_home(app, st);
    }
}

/// Montre l'état dans Réglages › À propos.
pub fn push(app: &AppWindow, st: &State) {
    let g = app.global::<CPrefs>();
    g.set_update_dir(folder().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default().into());
    g.set_update_ready(st.update_ready.is_some());
    g.set_update_text(
        match st.update_ready.as_ref() {
            Some(v) => format!("La version {v} est prête. Le logiciel se ferme, s'installe et se relance."),
            None if st.distant.is_some() => "Les mises à jour viennent du PC principal.".to_string(),
            None => format!("Tu as la dernière version ({}).", env!("CARGO_PKG_VERSION")),
        }
        .into(),
    );
}

/// Un installateur s'exécute sur ce PC : on ne le prend qu'à une adresse Tailscale, où le réseau garantit à quel
/// appareil on parle. Un nom court ou une adresse du Wi-Fi pourrait répondre à sa place.
fn trusted_source(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or("");
    let host = match rest.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => rest.split(['/', ':']).next().unwrap_or(""),
    }
    .to_lowercase();
    host.trim_end_matches('.').ends_with(".ts.net") || host.parse().is_ok_and(otoko_mobile::server::is_tailscale)
}

/// Installe la version prête : le logiciel se ferme, l'installateur fait le reste et le relance.
pub fn install(app: &AppWindow, st: &mut State) {
    if st.selftest {
        return;
    }
    let (url, check) = match st.update_source.clone() {
        Some(Source::Folder(p)) => return launch(app, &p),
        Some(Source::Web(r)) => {
            let check = match (r.sha256, r.sha256_url) {
                (Some(h), _) => Check::Known(h),
                (None, Some(u)) => Check::At(u),
                (None, None) => Check::Missing,
            };
            (r.url, check)
        }
        Some(Source::MainPc(sha)) => {
            let Some(d) = st.distant.as_ref() else { return };
            if !trusted_source(&d.url) {
                app.set_message("Mise à jour refusée : pour la recevoir, relie ce PC avec son adresse Tailscale (….ts.net), ou installe-la à la main.".into());
                return;
            }
            // Un PC principal plus ancien ne donne pas d'empreinte : Tailscale garantit alors seul à qui on parle.
            (format!("{}/maj/pc", d.url), sha.map(Check::Known).unwrap_or(Check::Tailscale))
        }
        None => return,
    };
    app.set_message("Téléchargement de la mise à jour…".into());
    st.spawn(move || Done::UpdateFetched(fetch_installer(&url, check)));
}

/// Comment vérifier un installateur avant de le lancer.
#[derive(Debug)]
enum Check {
    /// L'empreinte SHA-256 attendue.
    Known(String),
    /// L'adresse du fichier « .sha256 » publié à côté.
    At(String),
    /// Un PC principal plus ancien, par Tailscale : pas d'empreinte, le réseau garantit l'appareil.
    Tailscale,
    /// Depuis Internet sans aucune empreinte : refusé.
    Missing,
}

/// Télécharge l'installateur dans un fichier à soi, puis le vérifie. Depuis Internet, sans empreinte, il n'est pas lancé.
fn fetch_installer(url: &str, check: Check) -> Result<PathBuf, String> {
    let expected = match check {
        Check::Known(h) => Some(h),
        Check::At(u) => Some(
            crate::fond::fetch_text(&u)
                .ok()
                .and_then(|t| sha256_in(&t))
                .ok_or("l'empreinte de l'installateur est illisible : installe la version à la main depuis la page Releases.")?,
        ),
        Check::Tailscale => None,
        Check::Missing => return Err("cette version n'a pas d'empreinte publiée : installe-la à la main depuis la page Releases.".into()),
    };
    let out = std::env::temp_dir().join(format!("Otoko-Studio-Vault-Pro-installation-{}.exe", std::process::id()));
    let _ = std::fs::remove_file(&out);
    crate::fond::fetch_file(url, &out)?;
    if let Some(h) = expected {
        verified(&out, &h)?;
    }
    Ok(out)
}

/// L'installateur est arrivé : on le lance.
pub fn fetched(app: &AppWindow, r: Result<PathBuf, String>) {
    match r {
        Ok(p) => launch(app, &p),
        Err(e) => app.set_message(format!("Mise à jour impossible : {e}").into()),
    }
}

/// Le logiciel se ferme, l'installateur fait le reste et le relance.
fn launch(app: &AppWindow, path: &Path) {
    match std::process::Command::new(path).args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/RESTARTAPPLICATIONS"]).creation_flags(0x0000_0008).spawn() {
        Ok(_) => {
            let _ = slint::quit_event_loop();
        }
        Err(e) => app.set_message(format!("Mise à jour impossible : {e}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(parse("1.10.0") > parse("1.9.3"));
        assert!(parse("2.0.0") > parse("1.99.99"));
        assert_eq!(parse("1.1.0"), (1, 1, 0));
        assert_eq!(otoko_mobile::installer_version("Otoko-Studio-Vault-Pro-1.2.3-installation.exe"), Some((1, 2, 3)));
        assert_eq!(otoko_mobile::installer_version("OTOKO-Studio-Live-1.0.0-installation.exe"), None);
    }

    #[test]
    fn a_release_only_comes_from_this_project() {
        let good = serde_json::json!({ "tag_name": "v1.2.3", "assets": [
            { "name": "autre.zip", "browser_download_url": "https://github.com/otokonosound/x/autre.zip" },
            { "name": "Otoko-Studio-Vault-Pro-1.2.3-installation.exe", "browser_download_url": "https://github.com/otokonosound/otoko-studio-vault-pro/releases/download/v1.2.3/a.exe" },
        ] });
        let (v, r) = read_release(&good).unwrap();
        assert_eq!(v, "1.2.3");
        assert!(r.url.ends_with("a.exe"));
        assert_eq!((r.sha256, r.sha256_url), (None, None), "sans empreinte : rien à vérifier, donc pas d'installation automatique");
        let elsewhere = serde_json::json!({ "tag_name": "v1.2.3", "assets": [{ "name": "Otoko-Studio-Vault-Pro-1.2.3-installation.exe", "browser_download_url": "https://example.com/a.exe" }] });
        assert!(read_release(&elsewhere).is_none());
        assert!(read_release(&serde_json::json!({ "message": "Not Found" })).is_none());
    }

    #[test]
    fn a_release_says_how_to_check_its_installer() {
        let hex = "ab".repeat(32);
        let with_digest = serde_json::json!({ "tag_name": "v1.2.3", "assets": [
            { "name": "Otoko-Studio-Vault-Pro-1.2.3-installation.exe", "digest": format!("sha256:{}", hex.to_uppercase()),
              "browser_download_url": "https://github.com/otokonosound/otoko-studio-vault-pro/releases/download/v1.2.3/a.exe" },
        ] });
        assert_eq!(read_release(&with_digest).unwrap().1.sha256.as_deref(), Some(hex.as_str()));
        let with_file = serde_json::json!({ "tag_name": "v1.2.3", "assets": [
            { "name": "Otoko-Studio-Vault-Pro-1.2.3-installation.exe", "browser_download_url": "https://github.com/otokonosound/otoko-studio-vault-pro/releases/download/v1.2.3/a.exe" },
            { "name": "Otoko-Studio-Vault-Pro-1.2.3-installation.exe.sha256", "browser_download_url": "https://github.com/otokonosound/otoko-studio-vault-pro/releases/download/v1.2.3/a.exe.sha256" },
        ] });
        assert!(read_release(&with_file).unwrap().1.sha256_url.is_some_and(|u| u.ends_with(".sha256")));
        assert_eq!(sha256_in(&format!("{hex}  Otoko-Studio-Vault-Pro-1.2.3-installation.exe\r\n")).as_deref(), Some(hex.as_str()));
        assert_eq!(sha256_in("sha256:1234"), None);
        assert_eq!(sha256_in(""), None);
    }

    #[test]
    fn an_installer_is_launched_only_if_it_is_the_one_published() {
        let dir = std::env::temp_dir().join(format!("otoko-majs-sha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.exe");
        std::fs::write(&f, b"abc").unwrap();
        assert!(verified(&f, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").is_ok());
        assert!(verified(&f, &"00".repeat(32)).is_err());
        assert!(!f.exists(), "un installateur douteux est effacé");
        assert!(fetch_installer("https://github.com/otokonosound/x.exe", Check::Missing).is_err(), "depuis Internet, sans empreinte : refusé avant même le téléchargement");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_installer_only_comes_from_tailscale() {
        for ok in ["http://pc.tail1234.ts.net:8775", "https://pc.tail1234.ts.net:8444", "http://100.65.1.2:8775", "http://[fd7a:115c:a1e0::1]:8775"] {
            assert!(trusted_source(ok), "{ok}");
        }
        for no in ["http://pc:8775", "http://192.168.1.20:8775", "http://pc.ts.net.evil.example.com:8775", "http://100.200.1.1:8775", ""] {
            assert!(!trusted_source(no), "{no}");
        }
    }

    #[test]
    fn the_newest_installer_wins() {
        let dir = std::env::temp_dir().join(format!("otoko-majs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for v in ["1.9.0", "1.10.0", "1.2.0"] {
            std::fs::write(dir.join(format!("Otoko-Studio-Vault-Pro-{v}-installation.exe")), b"x").unwrap();
        }
        std::fs::write(dir.join("autre.exe"), b"x").unwrap();
        assert_eq!(otoko_mobile::newest_in(&dir).map(|x| x.0).as_deref(), Some("1.10.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
