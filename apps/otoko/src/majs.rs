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
#[derive(Clone)]
pub enum Source {
    /// Le dossier des installateurs de ce PC.
    Folder(PathBuf),
    /// Le PC principal, pour un autre PC qui lui est relié.
    MainPc,
    /// La page des versions sur GitHub : l'adresse de l'installateur.
    Web(String),
}

/// La dernière version publiée. Rien n'est envoyé : c'est une simple lecture de la page publique.
const RELEASES: &str = "https://api.github.com/repos/otokonosound/otoko-studio-vault-pro/releases/latest";

/// (version, adresse de l'installateur) d'après la réponse de GitHub.
fn read_release(v: &serde_json::Value) -> Option<(String, String)> {
    let version = v["tag_name"].as_str()?.trim_start_matches('v').to_string();
    let name = format!("Otoko-Studio-Vault-Pro-{version}-installation.exe");
    let asset = v["assets"].as_array()?.iter().find(|a| a["name"] == name.as_str())?;
    let url = asset["browser_download_url"].as_str()?;
    url.starts_with("https://github.com/otokonosound/").then(|| (version, url.to_string()))
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
            Some(url) => crate::fond::fetch_json(&format!("{url}/api/maj")).ok().and_then(|v| v["pc"].as_str().map(str::to_string)).filter(|v| parse(v) > current).map(|v| (v, Source::MainPc)),
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
    let url = match st.update_source.clone() {
        Some(Source::Folder(p)) => return launch(app, &p),
        Some(Source::Web(u)) => u,
        Some(Source::MainPc) => {
            let Some(d) = st.distant.as_ref() else { return };
            if !trusted_source(&d.url) {
                app.set_message("Mise à jour refusée : pour la recevoir, relie ce PC avec son adresse Tailscale (….ts.net), ou installe-la à la main.".into());
                return;
            }
            format!("{}/maj/pc", d.url)
        }
        None => return,
    };
    app.set_message("Téléchargement de la mise à jour…".into());
    st.spawn(move || {
        let out = std::env::temp_dir().join("Otoko-Studio-Vault-Pro-installation.exe");
        Done::UpdateFetched(crate::fond::fetch_file(&url, &out).map(|()| out))
    });
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
        let (v, url) = read_release(&good).unwrap();
        assert_eq!(v, "1.2.3");
        assert!(url.ends_with("a.exe"));
        let elsewhere = serde_json::json!({ "tag_name": "v1.2.3", "assets": [{ "name": "Otoko-Studio-Vault-Pro-1.2.3-installation.exe", "browser_download_url": "https://example.com/a.exe" }] });
        assert!(read_release(&elsewhere).is_none());
        assert!(read_release(&serde_json::json!({ "message": "Not Found" })).is_none());
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
