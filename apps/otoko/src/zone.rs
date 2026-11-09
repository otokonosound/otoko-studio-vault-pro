//! L'icône près de l'horloge. Fermer la fenêtre laisse le logiciel ouvert (la musique continue) ; un clic rouvre la
//! fenêtre, un clic droit propose Ouvrir, Vérifier une mise à jour et Fermer le logiciel. Lancer le logiciel une seconde
//! fois rouvre aussi la fenêtre, au lieu d'en démarrer un autre.
//!
//! Une fenêtre invisible, sur son propre fil, reçoit les messages de Windows ; les commandes reviennent au fil de
//! l'interface par `tray-command`. Aucune bibliothèque de plus : quelques fonctions de Windows suffisent.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use slint::ComponentHandle;

use crate::state::State;
use crate::{AppWindow, settings};

const CLASS: &str = "OtokoStudioVaultPro.Zone";
const TITLE: &str = "Otoko Studio Vault Pro";
const SETTING: &str = "close_to_tray";
const TOLD: &str = "tray_told";

const WM_NULL: u32 = 0x0000;
const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_QUERYENDSESSION: u32 = 0x0011;
const WM_ENDSESSION: u32 = 0x0016;
const WM_COMMAND: u32 = 0x0111;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_APP: u32 = 0x8000;
const WM_ICON_CLICK: u32 = WM_APP + 1;
const WM_OPEN: u32 = WM_APP + 2;
const WM_ICON_SET: u32 = WM_APP + 3;
const WM_BALLOON: u32 = WM_APP + 4;

const CMD_OPEN: usize = 1;
const CMD_UPDATE: usize = 2;
const CMD_QUIT: usize = 3;

const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 0x01;
const NIF_ICON: u32 = 0x02;
const NIF_TIP: u32 = 0x04;
const NIF_INFO: u32 = 0x10;

type Hwnd = isize;

#[repr(C)]
struct WndClassW {
    style: u32,
    proc: unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
    cls_extra: i32,
    wnd_extra: i32,
    instance: isize,
    icon: isize,
    cursor: isize,
    brush: isize,
    menu: *const u16,
    name: *const u16,
}

#[repr(C)]
#[derive(Default)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct NotifyIcon {
    size: u32,
    hwnd: Hwnd,
    id: u32,
    flags: u32,
    callback: u32,
    icon: isize,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    timeout: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: [u32; 4],
    balloon_icon: isize,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(ex: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: Hwnd, menu: isize, instance: isize, param: *const u8) -> Hwnd;
    fn DefWindowProcW(h: Hwnd, m: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(m: *mut Msg, h: Hwnd, min: u32, max: u32) -> i32;
    fn TranslateMessage(m: *const Msg) -> i32;
    fn DispatchMessageW(m: *const Msg) -> isize;
    fn PostMessageW(h: Hwnd, m: u32, w: usize, l: isize) -> i32;
    fn PostQuitMessage(code: i32);
    fn CreatePopupMenu() -> isize;
    fn AppendMenuW(menu: isize, flags: u32, id: usize, text: *const u16) -> i32;
    fn SetMenuDefaultItem(menu: isize, item: u32, by_position: u32) -> i32;
    fn TrackPopupMenu(menu: isize, flags: u32, x: i32, y: i32, reserved: i32, h: Hwnd, rect: *const u8) -> i32;
    fn DestroyMenu(menu: isize) -> i32;
    fn GetCursorPos(p: *mut Point) -> i32;
    fn SetForegroundWindow(h: Hwnd) -> i32;
    fn AllowSetForegroundWindow(pid: u32) -> i32;
    fn LoadImageW(instance: isize, name: *const u16, kind: u32, cx: i32, cy: i32, flags: u32) -> isize;
    fn GetSystemMetrics(index: i32) -> i32;
    fn RegisterWindowMessageW(name: *const u16) -> u32;
    fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
    fn ShowWindow(h: Hwnd, cmd: i32) -> i32;
    fn IsIconic(h: Hwnd) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn Shell_NotifyIconW(message: u32, data: *const NotifyIcon) -> i32;
}

/// La fenêtre invisible, une fois créée.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Le message de Windows quand la barre des tâches redémarre : l'icône est à remettre.
static TASKBAR_BACK: AtomicU32 = AtomicU32::new(0);
/// La zone tourne (pas pendant un autotest, une capture ou une mesure).
static ACTIVE: AtomicBool = AtomicBool::new(false);
static ICON_ON: AtomicBool = AtomicBool::new(false);
static APP: Mutex<Option<slint::Weak<AppWindow>>> = Mutex::new(None);
static THREAD: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copie un texte dans un champ de taille fixe, en le coupant s'il le faut.
fn fill<const N: usize>(field: &mut [u16; N], s: &str) {
    for (i, c) in s.encode_utf16().take(N - 1).enumerate() {
        field[i] = c;
    }
}

fn data(h: Hwnd) -> NotifyIcon {
    NotifyIcon {
        size: std::mem::size_of::<NotifyIcon>() as u32,
        hwnd: h,
        id: 1,
        flags: 0,
        callback: WM_ICON_CLICK,
        icon: 0,
        tip: [0; 128],
        state: 0,
        state_mask: 0,
        info: [0; 256],
        timeout: 0,
        info_title: [0; 64],
        info_flags: 0,
        guid: [0; 4],
        balloon_icon: 0,
    }
}

fn icon_add(h: Hwnd) {
    const IMAGE_ICON: u32 = 1;
    const SM_CXSMICON: i32 = 49;
    let mut d = data(h);
    d.flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    fill(&mut d.tip, TITLE);
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON);
        // L'icône numéro 1 du programme (otoko.rc).
        d.icon = LoadImageW(GetModuleHandleW(std::ptr::null()), 1 as *const u16, IMAGE_ICON, size, size, 0);
        Shell_NotifyIconW(NIM_ADD, &d);
    }
}

fn icon_remove(h: Hwnd) {
    let d = data(h);
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &d);
    }
}

fn balloon(h: Hwnd) {
    let mut d = data(h);
    d.flags = NIF_INFO;
    fill(&mut d.info_title, "Le logiciel reste ouvert ici");
    fill(&mut d.info, "Un clic rouvre la fenêtre. Clic droit : vérifier une mise à jour ou fermer le logiciel.");
    unsafe {
        Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}

fn send(cmd: &'static str) {
    if let Some(app) = APP.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = app.upgrade_in_event_loop(move |a| a.invoke_tray_command(cmd.into()));
    }
}

fn menu(h: Hwnd) {
    const MF_STRING: u32 = 0;
    const MF_SEPARATOR: u32 = 0x800;
    const TPM_BOTTOMALIGN: u32 = 0x20;
    const TPM_RIGHTBUTTON: u32 = 0x02;
    let (open, update, quit) = (wide("Ouvrir"), wide("Vérifier une mise à jour"), wide("Fermer le logiciel"));
    unsafe {
        let m = CreatePopupMenu();
        AppendMenuW(m, MF_STRING, CMD_OPEN, open.as_ptr());
        AppendMenuW(m, MF_STRING, CMD_UPDATE, update.as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(m, MF_STRING, CMD_QUIT, quit.as_ptr());
        SetMenuDefaultItem(m, CMD_OPEN as u32, 0);
        let mut p = Point::default();
        GetCursorPos(&mut p);
        // Sans cela, le menu ne se referme pas quand on clique ailleurs (règle de Windows).
        SetForegroundWindow(h);
        TrackPopupMenu(m, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, p.x, p.y, 0, h, std::ptr::null());
        PostMessageW(h, WM_NULL, 0, 0);
        DestroyMenu(m);
    }
}

unsafe extern "system" fn window_proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
    match m {
        WM_ICON_CLICK => {
            match l as u32 {
                WM_LBUTTONUP => send("open"),
                WM_RBUTTONUP => menu(h),
                _ => {}
            }
            0
        }
        WM_OPEN => {
            send("open");
            0
        }
        WM_ICON_SET => {
            if w != 0 {
                icon_add(h);
            } else {
                icon_remove(h);
            }
            0
        }
        WM_BALLOON => {
            balloon(h);
            0
        }
        WM_COMMAND => {
            match w & 0xffff {
                CMD_OPEN => send("open"),
                CMD_UPDATE => send("update"),
                CMD_QUIT => send("quit"),
                _ => {}
            }
            0
        }
        // Windows s'arrête, ou un installateur veut remplacer le programme : on ferme pour de bon.
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if w != 0 {
                send("quit");
            }
            0
        }
        WM_DESTROY => {
            icon_remove(h);
            unsafe { PostQuitMessage(0) };
            0
        }
        _ if m != 0 && m == TASKBAR_BACK.load(Ordering::Relaxed) => {
            if ICON_ON.load(Ordering::Relaxed) {
                icon_add(h);
            }
            0
        }
        _ => unsafe { DefWindowProcW(h, m, w, l) },
    }
}

/// Le logiciel tourne déjà ? On lui demande de se montrer. Vrai s'il fallait s'arrêter là.
pub fn already_running() -> bool {
    let class = wide(CLASS);
    unsafe {
        let h = FindWindowW(class.as_ptr(), std::ptr::null());
        if h == 0 {
            return false;
        }
        // Le premier lancement a le droit de passer devant : c'est ce qu'on vient de demander.
        AllowSetForegroundWindow(u32::MAX);
        PostMessageW(h, WM_OPEN, 0, 0);
    }
    true
}

/// Fermer la fenêtre garde-t-il le logiciel près de l'horloge ? Oui par défaut.
pub fn enabled() -> bool {
    settings::get(SETTING).and_then(|v| v.as_bool()) != Some(false)
}

pub fn set_enabled(on: bool) {
    settings::set(SETTING, serde_json::json!(on));
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    ICON_ON.store(on, Ordering::Relaxed);
    unsafe {
        PostMessageW(WINDOW.load(Ordering::Relaxed), WM_ICON_SET, on as usize, 0);
    }
}

/// Démarre la fenêtre invisible et l'icône.
pub fn start(app: &AppWindow) {
    *APP.lock().unwrap_or_else(|e| e.into_inner()) = Some(app.as_weak());
    ACTIVE.store(true, Ordering::Relaxed);
    let show_icon = enabled();
    ICON_ON.store(show_icon, Ordering::Relaxed);
    let thread = std::thread::Builder::new().name("zone".into()).spawn(move || {
        let (class, empty, back) = (wide(CLASS), wide(""), wide("TaskbarCreated"));
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            TASKBAR_BACK.store(RegisterWindowMessageW(back.as_ptr()), Ordering::Relaxed);
            let def = WndClassW { style: 0, proc: window_proc, cls_extra: 0, wnd_extra: 0, instance, icon: 0, cursor: 0, brush: 0, menu: std::ptr::null(), name: class.as_ptr() };
            RegisterClassW(&def);
            let h = CreateWindowExW(0, class.as_ptr(), empty.as_ptr(), 0, 0, 0, 0, 0, 0, 0, instance, std::ptr::null());
            if h == 0 {
                return;
            }
            WINDOW.store(h, Ordering::Relaxed);
            if show_icon {
                icon_add(h);
            }
            let mut m = Msg::default();
            while GetMessageW(&mut m, 0, 0, 0) > 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
    });
    *THREAD.lock().unwrap_or_else(|e| e.into_inner()) = thread.ok();
}

/// Retire l'icône en quittant.
pub fn stop() {
    if !ACTIVE.swap(false, Ordering::Relaxed) {
        return;
    }
    unsafe {
        PostMessageW(WINDOW.load(Ordering::Relaxed), WM_CLOSE, 0, 0);
    }
    if let Some(t) = THREAD.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = t.join();
    }
}

/// Montre la fenêtre, devant les autres.
pub fn show(app: &AppWindow) {
    const SW_RESTORE: i32 = 9;
    let _ = app.show();
    app.window().set_minimized(false);
    let title = wide(TITLE);
    unsafe {
        let h = FindWindowW(std::ptr::null(), title.as_ptr());
        if h != 0 {
            if IsIconic(h) != 0 {
                ShowWindow(h, SW_RESTORE);
            }
            SetForegroundWindow(h);
        }
    }
}

/// Branche la croix de la fenêtre et les commandes du menu.
pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    app.window().on_close_requested(|| {
        if ACTIVE.load(Ordering::Relaxed) && enabled() {
            // La première fois, on explique où il est passé.
            if settings::get(TOLD).is_none() {
                settings::set(TOLD, serde_json::json!(true));
                unsafe {
                    PostMessageW(WINDOW.load(Ordering::Relaxed), WM_BALLOON, 0, 0);
                }
            }
        } else {
            let _ = slint::quit_event_loop();
        }
        slint::CloseRequestResponse::HideWindow
    });
    let weak = app.as_weak();
    let state = state.clone();
    app.on_tray_command(move |cmd| {
        let Some(app) = weak.upgrade() else { return };
        match cmd.as_str() {
            "open" => show(&app),
            "update" => {
                show(&app);
                if let Ok(mut st) = state.try_borrow_mut() {
                    crate::raccourcis::preferences(&app, &mut st, "about");
                    crate::majs::check(&mut st);
                    app.set_message("Recherche d'une nouvelle version…".into());
                }
            }
            "quit" => {
                let _ = slint::quit_event_loop();
            }
            _ => {}
        }
    });
}
