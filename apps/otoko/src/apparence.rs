//! L'apparence : le thème suit Windows, la taille de l'interface se choisit, les messages passent en toast, et le bouton
//! « ? » ouvre le guide à la page de l'écran en cours.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode};

use crate::{AppWindow, Theme, settings};

// ---------------------------------------------------------------------- thème

/// Windows en mode clair ? `None` si la clé n'est pas lisible (Windows 7, profil sans réglage).
pub fn windows_light() -> Option<bool> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(hkey: usize, sub: *const u16, value: *const u16, flags: u32, kind: *mut u32, data: *mut u8, len: *mut u32) -> i32;
    }
    const HKEY_CURRENT_USER: usize = 0x8000_0001;
    const RRF_RT_REG_DWORD: u32 = 0x10;
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let sub = w("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let name = w("AppsUseLightTheme");
    let mut data = [0u8; 4];
    let mut len = 4u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, sub.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), data.as_mut_ptr(), &mut len) };
    (r == 0).then(|| u32::from_le_bytes(data) != 0)
}

/// Le mode choisi : 0 comme Windows (par défaut), 1 sombre, 2 clair. Les anciens réglages
/// « sombre » / « clair » restent compris.
pub fn theme_mode() -> i32 {
    match settings::get("theme").and_then(|v| v.as_str().map(str::to_string)).as_deref() {
        Some("sombre") => 1,
        Some("clair") => 2,
        _ => 0,
    }
}

pub fn dark_for(mode: i32) -> bool {
    match mode {
        1 => true,
        2 => false,
        _ => !windows_light().unwrap_or(false),
    }
}

/// Applique le mode (sans l'enregistrer).
pub fn apply_theme(app: &AppWindow) {
    let mode = theme_mode();
    app.set_theme_mode(mode);
    app.global::<Theme>().set_dark(dark_for(mode));
}

pub fn set_mode(app: &AppWindow, mode: i32) {
    settings::set("theme", serde_json::json!(["systeme", "sombre", "clair"][mode.clamp(0, 2) as usize]));
    apply_theme(app);
}

/// Suit le réglage de Windows (Paramètres › Personnalisation › Couleurs) pendant que le logiciel tourne :
/// une lecture de registre toutes les trois secondes, rien de plus.
pub fn watch(app: &AppWindow) -> Timer {
    let weak = app.as_weak();
    let t = Timer::default();
    t.start(TimerMode::Repeated, Duration::from_secs(3), move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_theme_mode() != 0 {
            return;
        }
        let dark = dark_for(0);
        let theme = app.global::<Theme>();
        if theme.get_dark() != dark {
            theme.set_dark(dark);
        }
    });
    t
}

// ---------------------------------------------------------------------- taille de l'interface

/// 0 normale, 1 grande (110 %), 2 très grande (125 %).
pub fn scale_choice() -> i32 {
    settings::get("ui_scale").and_then(|v| v.as_i64()).unwrap_or(0).clamp(0, 2) as i32
}

const SCALES: [f32; 3] = [1.0, 1.1, 1.25];

/// Avant la création de la fenêtre : Slint lit `SLINT_SCALE_FACTOR` une fois au démarrage, d'où
/// « au prochain lancement ». La mise à l'échelle de Windows (125 %, 150 %…) reste comprise.
pub fn prepare_scale() {
    let choice = scale_choice();
    if choice == 0 || std::env::var_os("SLINT_SCALE_FACTOR").is_some() {
        return;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDpiForSystem() -> u32;
    }
    let dpi = unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0;
    // SAFETY : avant tout fil secondaire (appelé au tout début de `main`).
    unsafe { std::env::set_var("SLINT_SCALE_FACTOR", format!("{:.3}", dpi * SCALES[choice as usize])) };
}

pub fn set_scale(app: &AppWindow, choice: i32) {
    settings::set("ui_scale", serde_json::json!(choice.clamp(0, 2)));
    app.set_ui_scale(choice.clamp(0, 2));
    app.set_message(if choice == scale_choice() && choice == 0 { "Taille normale au prochain lancement.".into() } else { "La nouvelle taille s'appliquera au prochain lancement.".into() });
}

// ---------------------------------------------------------------------- aide de l'écran

/// L'ancre du guide pour l'écran en cours.
pub fn help_anchor(space: i32, tab: i32) -> &'static str {
    match (space, tab) {
        (6, _) => "reglages",
        (7, _) => "mobile",
        (_, 1) => "bibliotheque",
        (_, 2) => "sorties",
        (_, 3) => "publier",
        (_, 4) => "suno",
        (_, 5) => "statistiques",
        (_, 6) => "production",
        (_, 7) => "mastering",
        _ => "accueil",
    }
}

/// Le guide, à côté du programme (l'installateur l'y met ; `build.rs` aussi, pour une compilation).
pub fn guide_path() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_default().with_file_name("Guide Otoko Studio Vault Pro.html")
}

/// L'adresse `file:///…#ancre` du guide ; `start` l'ouvre dans le navigateur, ancre comprise.
pub fn guide_url(path: &std::path::Path, anchor: &str) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    let p = p.trim_start_matches("//?/");
    let mut enc = String::new();
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => enc.push(b as char),
            _ => enc.push_str(&format!("%{b:02X}")),
        }
    }
    format!("file:///{enc}#{anchor}")
}

/// Ouvre le guide à l'ancre demandée (rien pendant un autotest).
pub fn open_help(app: &AppWindow, anchor: &str, selftest: bool) {
    let path = guide_path();
    if !path.exists() {
        app.set_message(format!("Guide introuvable : {}", path.display()).into());
        return;
    }
    if selftest {
        app.set_message("Guide ouvert dans ton navigateur.".into());
        return;
    }
    let url = guide_url(&path, anchor);
    match std::process::Command::new("cmd").args(["/c", "start", "", &url]).spawn() {
        Ok(_) => app.set_message("Guide ouvert dans ton navigateur.".into()),
        Err(e) => app.set_message(format!("Guide impossible à ouvrir ({e}).").into()),
    }
}

// ---------------------------------------------------------------------- toasts

/// Chaque nouveau message s'affiche quatre secondes puis s'efface ; la barre d'état le garde en gris.
pub fn wire_toast(app: &AppWindow) {
    let weak = app.as_weak();
    let timer: Rc<RefCell<Timer>> = Rc::new(RefCell::new(Timer::default()));
    app.on_toasted(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_toast_on(true);
        let w2 = weak.clone();
        timer.borrow().start(TimerMode::SingleShot, Duration::from_millis(4000), move || {
            if let Some(app) = w2.upgrade() {
                app.set_toast_on(false);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_anchors_follow_the_screen() {
        assert_eq!(help_anchor(4, 7), "mastering");
        assert_eq!(help_anchor(4, 0), "accueil");
        assert_eq!(help_anchor(6, 0), "reglages");
        assert_eq!(help_anchor(7, 0), "mobile");
        let url = guide_url(std::path::Path::new("Z:\\Claude\\OTOKO\\docs\\guide é.html"), "mobile");
        assert_eq!(url, "file:///Z:/Claude/OTOKO/docs/guide%20%C3%A9.html#mobile");
    }
}
