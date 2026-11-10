//! L'état de l'application : la base, ce que montre chaque module, et les travaux de fond qui reviennent à l'interface.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;

use slint::Timer;

use crate::AppWindow;

/// Ce que les threads d'arrière-plan rendent à l'interface.
pub enum Done {
    /// Import de la base Studio Vault.
    Vault(Result<otoko_library::career::ImportReport, String>),
    /// Contrôle d'un morceau avant publication.
    Check { song: i64, secs: f64, result: Result<Vec<crate::publish::Check>, String> },
    /// L'état du serveur mobile : le texte et ses adresses, s'il répond, si la tâche du PC est installée.
    Mobile { text: String, on: bool, task: bool, steps: Vec<crate::mobile::Step>, address: Option<String> },
    /// Un message en cours de travail.
    Say(String),
    /// Les compléments du téléphone sont installés (ou l'erreur).
    AddonsDone(Result<(), String>),
    /// Le moteur de mastering est installé (ou l'erreur).
    EngineDone(Result<(), String>),
    /// Bibliothèque : dossiers surveillés relus.
    LibScan(Result<otoko_library::career::scan::ScanStats, String>),
    /// Où en est l'analyse audio des morceaux ; `last` à la fin.
    Analysed { done: usize, total: usize, ok: usize, failed: usize, last: bool, quiet: bool },
    /// La forme d'onde du morceau ouvert dans le lecteur.
    Waveform { id: i64, peaks: Vec<f32> },
    /// Un paquet de pochettes ; le dernier dit que tout est arrivé.
    Covers(Vec<(i64, Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>)>, bool),
    /// Les pochettes des sorties (bandeau de l'accueil, galerie des sorties).
    ReleaseCovers(Vec<(i64, Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>)>),
    /// Pochette écrite dans les fichiers d'une sortie : réussis, échecs.
    CoverEmbedded(usize, Vec<String>),
    /// Discographie exportée.
    Discography(Result<std::path::PathBuf, String>),
    /// Mastering : l'avancement d'un morceau, puis son résultat.
    Mastering(i32, crate::mastrage::Event),
    /// Un lien de partage créé, modifié ou retiré : le message, ou l'erreur.
    Partage(Result<String, String>),
    /// Où les liens sont publiés (adresse, ouverte sur Internet), si le serveur répond.
    PartageBase(Option<(String, bool)>),
    /// Ce qui vient d'être rangé depuis le dossier d'arrivée.
    Arrivals(Vec<otoko_library::career::arrivee::Landing>),
    /// Les liens par morceau : (reliés, titres inconnus, erreurs).
    Links(Result<(usize, Vec<String>, Vec<String>), String>),
    /// Le contrôle des copies.
    Copies(Result<otoko_library::career::copies::Report, String>),
    /// Des mini formes d'onde sont prêtes ; vrai au dernier paquet.
    Waves(bool),
    /// Une nouvelle version : (numéro, installateur s'il est sur ce PC).
    /// (la version trouvée, GitHub a-t-il répondu : rien si on ne lui a pas demandé).
    Update(Option<(String, crate::majs::Source)>, Option<bool>),
    /// L'installateur est téléchargé (ou l'erreur).
    UpdateFetched(Result<PathBuf, String>),
    /// Le tour du lien avec le PC principal : (état du PC principal, faut-il reprendre sa base).
    Distant(Result<(String, bool), String>),
    DistantCopy(Result<String, String>),
    RemoteTest { url: String, code: String, connect: bool, result: Result<String, String> },
    /// Un morceau arrivé du PC principal.
    Audio { id: i64, at: f64, result: Result<(), String> },
}

pub struct State {
    pub jobs: mpsc::Sender<Done>,
    pub pending: usize,
    pub poll: Rc<Timer>,
    /// La sortie d'écoute, ouverte seulement pendant la lecture.
    pub ecoute: crate::audio::Ecoute,
    /// La base (vide au départ).
    pub career: Option<otoko_library::career::Career>,
    pub career_path: PathBuf,
    /// Les pages à jour (par numéro d'onglet) : une page se relit quand on y va après un changement de la base.
    pub pages_lues: [bool; 8],
    /// Le morceau préparé dans Publier.
    pub career_pick: Option<i64>,
    pub suno: crate::suno::Suno,
    /// L'autotest et les captures tournent : rien n'est écrit dans les dossiers de l'utilisateur.
    pub selftest: bool,
    /// Le serveur de l'app mobile, quand le logiciel le fait tourner lui-même.
    pub mobile: Option<otoko_mobile::Server>,
    pub lib: crate::bibliotheque::View,
    pub player: crate::lecteur::Player,
    /// Accueil, Production, Statistiques.
    pub home: crate::accueil::View,
    pub rel: crate::sorties::View,
    /// Fiche projet, clients, constructeur de set.
    pub proj: crate::projets::View,
    pub publ: crate::publish::View,
    pub mast: crate::mastrage::View,
    /// Les liens de partage.
    pub partage: crate::partage::View,
    /// La palette de commandes (Ctrl+K).
    pub palette: crate::palette::View,
    /// Réglages : « copier aussi dans… » coché, dossier pas encore choisi.
    pub prefs_extra_pending: bool,
    /// Les travaux de fond en cours (un seul de chaque à la fois).
    pub arrivals_busy: bool,
    pub copies_busy: bool,
    pub waves_busy: bool,
    /// La minuterie du dossier d'arrivée.
    pub arrival_timer: Option<Timer>,
    /// Une nouvelle version prête, et d'où vient son installateur.
    pub update_ready: Option<String>,
    pub update_source: Option<crate::majs::Source>,
    /// Les fichiers déposés depuis l'Explorateur, traités ensemble (voir `glisser`).
    pub dropped: Vec<std::path::PathBuf>,
    /// La dernière recherche : l'heure, et si GitHub a répondu (rien : pas demandé, le dossier ou le PC principal a suffi).
    pub update_checked: Option<(String, Option<bool>)>,
    /// Ce PC travaille sur une copie de la base du PC principal.
    pub distant: Option<crate::distant::Distant>,
}

impl State {
    /// Lance un travail d'arrière-plan et s'assure que son résultat sera relevé.
    pub fn spawn(&mut self, work: impl FnOnce() -> Done + Send + 'static) {
        let tx = self.jobs.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        self.pending += 1;
        if !self.poll.running() {
            self.poll.restart();
        }
    }
}

/// « Bonjour » le jour, « Bonsoir » le soir, d'après l'heure locale (UTC moins le décalage de Windows, heure d'été
/// comprise).
pub fn salutation() -> &'static str {
    #[repr(C)]
    struct Tz {
        bias: i32,
        rest: [u8; 168],
    }
    unsafe extern "system" {
        fn GetTimeZoneInformation(tz: *mut Tz) -> u32;
    }
    let mut tz = Tz { bias: 0, rest: [0; 168] };
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let dst = if id == 2 { -60 } else { 0 };
    let utc = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let hour = ((utc / 60 - tz.bias as i64 - dst) / 60).rem_euclid(24);
    if (5..18).contains(&hour) { "Bonjour" } else { "Bonsoir" }
}

/// Relève les travaux d'arrière-plan terminés.
pub fn collect(app: &AppWindow, st: &mut State, rx: &mpsc::Receiver<Done>) {
    while let Ok(done) = rx.try_recv() {
        st.pending = st.pending.saturating_sub(1);
        match done {
            Done::Vault(result) => crate::career::imported(app, st, result),
            Done::Check { song, secs, result } => crate::publish::checked(app, st, song, secs, result),
            Done::Mobile { text, on, task, steps, address } => crate::mobile::landed(app, text, on, task, steps, address),
            Done::LibScan(r) => crate::bibliotheque::scanned(app, st, r),
            Done::Waveform { id, peaks } => crate::lecteur::waveform(app, st, id, peaks),
            Done::Analysed { done, total, ok, failed, last, quiet } => crate::analyse::progress(app, st, done, total, ok, failed, last, quiet),
            Done::Covers(list, last) => crate::bibliotheque::covers_landed(app, st, list, last),
            Done::ReleaseCovers(list) => crate::accueil::covers_landed(app, st, list),
            Done::CoverEmbedded(done, failed) => crate::sorties::embedded(app, st, done, failed),
            Done::Discography(r) => crate::sorties::exported(app, st, r),
            Done::Mastering(id, ev) => crate::mastrage::event(app, st, id, ev),
            Done::Partage(r) => crate::partage::done(app, st, r),
            Done::PartageBase(b) => crate::partage::base_landed(app, st, b),
            Done::Arrivals(list) => crate::fond::landed(app, st, list),
            Done::Links(r) => crate::fond::linked(app, st, r),
            Done::Copies(r) => crate::fond::copies_done(app, st, r),
            Done::Waves(last) => crate::fond::waves_landed(app, st, last),
            Done::Update(r, github) => crate::majs::found(app, st, r, github),
            Done::UpdateFetched(r) => crate::majs::fetched(app, r),
            Done::Say(text) => app.set_message(text.into()),
            Done::AddonsDone(r) => crate::mobile::addons_done(app, st, r),
            Done::EngineDone(r) => crate::mastrage::engine_done(app, st, r),
            Done::Distant(r) => crate::distant::ticked(app, st, r),
            Done::DistantCopy(r) => crate::distant::copied(app, st, r),
            Done::RemoteTest { url, code, connect, result } => crate::distant::tried(app, &url, &code, connect, result),
            Done::Audio { id, at, result } => crate::distant::audio_landed(app, st, id, at, result),
        }
    }
    if st.pending == 0 {
        st.poll.stop();
    }
}
