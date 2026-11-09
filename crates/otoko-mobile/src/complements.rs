//! Ce qui n'est pas livré avec le logiciel et s'installe d'un clic, sans rien avoir à chercher : ffmpeg, le moteur de
//! mastering et Tailscale. Tout vient de la source officielle, et rien n'est lancé sans contrôle : empreinte connue
//! d'avance pour ffmpeg et le moteur, signature de l'éditeur pour Tailscale (ses versions changent trop souvent pour
//! figer une empreinte).

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const NO_WINDOW: u32 = 0x0800_0000;

const FFMPEG_URL: &str = "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip";
const FFMPEG_SHA256: &str = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
const FFMPEG_IN_ZIP: &str = "ffmpeg-9.0.2-essentials_build/bin/ffmpeg.exe";

const ENGINE_URL: &str = "https://github.com/ai-mastering/phaselimiter/releases/download/v0.2.0/phaselimiter-win.zip";
const ENGINE_SHA256: &str = "cab2d30ad8d993a383749b30d9f6dc1911198d3aa309d5f631b70206e0162145";

const TAILSCALE_INDEX: &str = "https://pkgs.tailscale.com/stable/?mode=json";
const TAILSCALE_FILES: &str = "https://pkgs.tailscale.com/stable/";
/// Le temps laissé pour se connecter à Tailscale dans le navigateur.
const LOGIN_WAIT: Duration = Duration::from_secs(300);

/// Le dossier des compléments, à côté des données.
fn root() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("OTOKO").join("complements")
}

/// ffmpeg, une fois installé par le logiciel.
pub fn ffmpeg_path() -> PathBuf {
    root().join("ffmpeg.exe")
}

/// Le moteur de mastering, une fois installé par le logiciel.
pub fn engine_root() -> PathBuf {
    root().join("moteur")
}

fn quiet(c: &mut Command) -> std::io::Result<std::process::ExitStatus> {
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(NO_WINDOW).status()
}

fn download(url: &str, out: &Path) -> Result<(), String> {
    let ok = quiet(Command::new("curl.exe").args(["-sL", "--fail", "--retry", "2", "--max-time", "3600", "-o"]).arg(out).arg(url)).map_err(|e| format!("téléchargement impossible : {e}"))?;
    if ok.success() { Ok(()) } else { Err("téléchargement impossible : vérifie ta connexion à Internet, puis réessaie.".into()) }
}

/// L'empreinte d'un fichier, sans le charger en mémoire.
pub fn sha256_of(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Télécharge et refuse tout fichier qui n'est pas exactement celui attendu.
fn fetch_checked(url: &str, sha256: &str, out: &Path) -> Result<(), String> {
    download(url, out)?;
    if sha256_of(out)? != sha256 {
        let _ = std::fs::remove_file(out);
        return Err("le fichier reçu n'est pas celui attendu : il a été écarté. Réessaie plus tard.".into());
    }
    Ok(())
}

fn unzip(zip: &Path, into: &Path, member: &str) -> Result<(), String> {
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let ok = quiet(Command::new("tar.exe").arg("-xf").arg(zip).arg("-C").arg(into).arg(member)).map_err(|e| format!("décompression impossible : {e}"))?;
    if ok.success() { Ok(()) } else { Err("décompression impossible.".into()) }
}

/// Installe ffmpeg. Rend son chemin.
pub fn install_ffmpeg(say: &dyn Fn(String)) -> Result<PathBuf, String> {
    let dir = root();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    say("Téléchargement de ffmpeg (110 Mo)…".into());
    let (zip, tmp) = (dir.join("ffmpeg.part.zip"), dir.join("ffmpeg.part"));
    let _ = std::fs::remove_dir_all(&tmp);
    let done = (|| {
        fetch_checked(FFMPEG_URL, FFMPEG_SHA256, &zip)?;
        say("Installation de ffmpeg…".into());
        unzip(&zip, &tmp, FFMPEG_IN_ZIP)?;
        std::fs::rename(tmp.join(FFMPEG_IN_ZIP), ffmpeg_path()).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_file(&zip);
    let _ = std::fs::remove_dir_all(&tmp);
    done.map(|()| ffmpeg_path()).map_err(|e| format!("ffmpeg : {e}"))
}

/// Installe le moteur de mastering (et ffmpeg, dont il a besoin, s'il manque).
pub fn install_engine(say: &dyn Fn(String)) -> Result<PathBuf, String> {
    let ffmpeg = match crate::media::find_ffmpeg() {
        Some(p) => p,
        None => install_ffmpeg(say)?,
    };
    let dir = root();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    say("Téléchargement du moteur de mastering (50 Mo)…".into());
    let (zip, tmp, engine) = (dir.join("moteur.part.zip"), dir.join("moteur.part"), engine_root());
    let _ = std::fs::remove_dir_all(&tmp);
    let done = (|| {
        fetch_checked(ENGINE_URL, ENGINE_SHA256, &zip)?;
        say("Installation du moteur de mastering…".into());
        unzip(&zip, &tmp, "phaselimiter")?;
        let _ = std::fs::remove_dir_all(&engine);
        std::fs::create_dir_all(&engine).map_err(|e| e.to_string())?;
        std::fs::rename(tmp.join("phaselimiter"), engine.join("phaselimiter")).map_err(|e| e.to_string())?;
        // Le moteur attend ffmpeg à côté de lui : un lien suffit, sinon une copie.
        let beside = engine.join("ffmpeg.exe");
        if std::fs::hard_link(&ffmpeg, &beside).is_err() {
            std::fs::copy(&ffmpeg, &beside).map_err(|e| e.to_string())?;
        }
        Ok::<(), String>(())
    })();
    let _ = std::fs::remove_file(&zip);
    let _ = std::fs::remove_dir_all(&tmp);
    done.map(|()| engine).map_err(|e| format!("moteur de mastering : {e}"))
}

/// Le nom du paquet Windows d'après la liste officielle.
fn tailscale_package(index: &serde_json::Value) -> Option<String> {
    let name = index["MSIs"]["amd64"].as_str()?;
    (name.starts_with("tailscale-setup-") && name.ends_with("-amd64.msi") && !name.contains(['/', '\\', '?', '#', '%'])).then(|| name.to_string())
}

/// Installe Tailscale. Windows demande une seule fois l'accord d'administrateur.
pub fn install_tailscale(say: &dyn Fn(String)) -> Result<(), String> {
    let dir = root();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    say("Téléchargement de Tailscale…".into());
    let list = dir.join("tailscale.json");
    download(TAILSCALE_INDEX, &list)?;
    let index: serde_json::Value = std::fs::read_to_string(&list).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let _ = std::fs::remove_file(&list);
    let name = tailscale_package(&index).ok_or("Tailscale : la liste officielle est illisible.")?;
    let msi = dir.join(&name);
    download(&format!("{TAILSCALE_FILES}{name}"), &msi)?;
    let done = (|| {
        // Signé par son éditeur, sinon il n'est pas lancé.
        let signed = quiet(
            Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", "$s = Get-AuthenticodeSignature -LiteralPath $env:OTOKO_FICHIER; if ($s.Status -eq 'Valid' -and $s.SignerCertificate.Subject -like '*Tailscale*') { exit 0 } else { exit 1 }"])
                .env("OTOKO_FICHIER", &msi),
        )
        .map_err(|e| e.to_string())?;
        if !signed.success() {
            return Err("le fichier reçu n'est pas signé par Tailscale : il a été écarté.".to_string());
        }
        say("Installation de Tailscale : accepte la demande de Windows.".into());
        let code = Command::new("msiexec.exe").arg("/i").arg(&msi).args(["/passive", "/norestart"]).status().map_err(|e| e.to_string())?;
        match code.code() {
            Some(0) | Some(3010) => Ok(()),
            Some(1602) => Err("installation annulée.".to_string()),
            _ => Err("l'installation a échoué.".to_string()),
        }
    })();
    let _ = std::fs::remove_file(&msi);
    done.map_err(|e| format!("Tailscale : {e}"))
}

/// Ouvre la connexion à Tailscale dans le navigateur et attend qu'elle soit faite. C'est la seule étape qui demande
/// quelque chose : un compte, personne ne peut le créer à ta place.
pub fn tailscale_login(say: &dyn Fn(String)) -> Result<(), String> {
    let exe = crate::server::tailscale_exe().ok_or("Tailscale n'est pas installé.")?;
    say("Connecte-toi à Tailscale dans la page qui s'ouvre (compte gratuit, Google ou Microsoft)…".into());
    let mut child = Command::new(&exe).arg("login").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).creation_flags(NO_WINDOW).spawn().map_err(|e| e.to_string())?;
    // Tailscale écrit l'adresse à ouvrir : on l'ouvre pour toi, une seule fois.
    let opened = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let readers: Vec<Box<dyn Read + Send>> = [child.stdout.take().map(|o| Box::new(o) as Box<dyn Read + Send>), child.stderr.take().map(|e| Box::new(e) as Box<dyn Read + Send>)].into_iter().flatten().collect();
    for r in readers {
        let opened = opened.clone();
        let _ = std::thread::Builder::new().name("tailscale".into()).spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(r).lines().map_while(Result::ok) {
                if let Some(url) = login_url(&line) {
                    if !opened.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        let _ = Command::new("explorer.exe").arg(url).spawn();
                    }
                }
            }
        });
    }
    let start = Instant::now();
    let connected = loop {
        if crate::server::tailscale_state().1.is_some() {
            break true;
        }
        if start.elapsed() > LOGIN_WAIT {
            break false;
        }
        std::thread::sleep(Duration::from_secs(3));
    };
    let _ = child.kill();
    let _ = child.wait();
    if connected { Ok(()) } else { Err("Tailscale n'est pas encore connecté : ouvre son icône près de l'horloge pour te connecter, puis « Actualiser ».".into()) }
}

/// L'adresse de connexion dans ce qu'écrit Tailscale.
fn login_url(line: &str) -> Option<&str> {
    line.split_whitespace().find(|w| w.starts_with("https://login.tailscale.com/") || w.starts_with("https://controlplane.tailscale.com/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_expected_file_passes() {
        let dir = std::env::temp_dir().join(format!("otoko-complements-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("essai.bin");
        std::fs::write(&f, b"abc").unwrap();
        assert_eq!(sha256_of(&f).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tailscale_package_comes_from_the_official_list() {
        assert_eq!(tailscale_package(&serde_json::json!({ "MSIs": { "amd64": "tailscale-setup-1.104.5-amd64.msi" } })).as_deref(), Some("tailscale-setup-1.104.5-amd64.msi"));
        for bad in ["../evil-amd64.msi", "tailscale-setup-1/../../x-amd64.msi", "autre.msi", "tailscale-setup-1-amd64.msi?x=1"] {
            assert!(tailscale_package(&serde_json::json!({ "MSIs": { "amd64": bad } })).is_none(), "{bad}");
        }
        assert!(tailscale_package(&serde_json::json!({})).is_none());
    }

    #[test]
    fn the_login_address_is_found_in_what_tailscale_writes() {
        assert_eq!(login_url("\thttps://login.tailscale.com/a/abc123"), Some("https://login.tailscale.com/a/abc123"));
        assert_eq!(login_url("To authenticate, visit: https://login.tailscale.com/a/x"), Some("https://login.tailscale.com/a/x"));
        assert_eq!(login_url("https://evil.example.com/"), None);
    }
}
