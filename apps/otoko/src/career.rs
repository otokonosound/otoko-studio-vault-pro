//! La base de ta carrière : tes morceaux (idées, finis, sortis), tes sorties et leur avancement. Elle vit dans
//! `%APPDATA%\OTOKO\carriere.db`, vide au départ ; la base Studio Vault s'y importe sur demande, lue sur une copie et
//! jamais modifiée.

use otoko_library::career::Career;

use crate::AppWindow;
use crate::state::{Done, State};

/// L'emplacement de la base Carrière de l'utilisateur.
pub fn default_path() -> std::path::PathBuf {
    crate::donnees::data_dir().join("carriere.db")
}

pub fn open(path: &std::path::Path) -> Option<Career> {
    Career::open(path).ok()
}

/// Les liens de partage de cette Carrière : `partages.db` à côté de `carriere.db` (une Carrière
/// d'essai a les siens, à son nom).
pub fn shares_path(career: &std::path::Path) -> std::path::PathBuf {
    if career.file_name().is_some_and(|n| n == "carriere.db") { career.with_file_name("partages.db") } else { career.with_extension("partages.db") }
}

/// La date du jour (UTC), « AAAA-MM-JJ ».
pub fn today() -> String {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86400).unwrap_or(0) as i64;
    // Jours depuis 1970 → date civile (algorithme de H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// « 2026-12-04 » → « 4 décembre 2026 » (le reste tel quel).
pub fn fr_date(iso: &str) -> String {
    const MONTHS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
    let parts: Vec<&str> = iso.get(..10).unwrap_or(iso).split('-').collect();
    match parts.as_slice() {
        [y, m, d] => match (m.parse::<usize>(), d.parse::<u32>()) {
            (Ok(m @ 1..=12), Ok(d)) => format!("{d}{} {} {y}", if d == 1 { "er" } else { "" }, MONTHS[m - 1]),
            _ => iso.to_string(),
        },
        _ => iso.to_string(),
    }
}

pub fn status_label(s: &str) -> &'static str {
    otoko_library::career::status_label(s)
}

/// La base a changé : toutes les pages seront relues, celle à l'écran tout de suite. Les autres attendent qu'on y aille
/// (`page`) : un import ou un lot de modifications ne recharge pas six écrans que personne ne regarde.
pub fn fill(app: &AppWindow, st: &mut State) {
    if st.career.is_none() {
        app.set_career_text("La base n'a pas pu s'ouvrir.".into());
        return;
    }
    app.set_career_vault(otoko_library::vault::default_dir().is_some());
    st.pages_lues = [false; 8];
    page(app, st, app.get_career_tab());
}

/// Remplit une page, si la base a changé depuis la dernière fois. L'accueil, la production, les statistiques et le
/// mastering se relisent à chaque visite : ils résument ce qui a pu bouger ailleurs, et ils sont rapides.
pub fn page(app: &AppWindow, st: &mut State, tab: i32) {
    let i = tab.clamp(0, 7) as usize;
    if st.pages_lues[i] && ![0, 5, 6, 7].contains(&tab) {
        return;
    }
    st.pages_lues[i] = true;
    match tab {
        0 => crate::accueil::fill_home(app, st),
        1 => crate::bibliotheque::fill(app, st),
        2 => crate::sorties::fill(app, st),
        3 => crate::publish::fill(app, st),
        4 => crate::suno::fill(app, st),
        5 => {
            crate::accueil::fill_charts(app, st);
            if let Some(c) = st.career.as_ref() {
                app.set_stats_facts(stats_facts(c).into());
            }
        }
        6 => crate::accueil::fill_board(app, st),
        _ => crate::mastrage::fill(app, st),
    }
}

/// Les faits de l'onglet Statistiques, en une ligne.
fn stats_facts(c: &otoko_library::career::Career) -> String {
    let s = c.stats().unwrap_or_default();
    let mut facts = Vec::new();
    if let Some(l) = s.lufs_released {
        facts.push(format!("Sonie moyenne de tes morceaux sortis : {l:.1} LUFS"));
    }
    if let Some(b) = s.bpm_median {
        facts.push(format!("tempo médian : {b:.0} BPM"));
    }
    if let Some(r) = s.rating_avg {
        facts.push(format!("note moyenne : {r:.1} / 5"));
    }
    facts.push(format!("{} morceau{} publié{} avec date ou lien", s.published, if s.published > 1 { "x" } else { "" }, if s.published > 1 { "s" } else { "" }));
    facts.join(" · ")
}

/// Importe Studio Vault en arrière-plan (copie de la base, puis lecture de la copie).
pub fn import(app: &AppWindow, st: &mut State) {
    let Some(dir) = otoko_library::vault::default_dir() else {
        app.set_career_text("Pas de base Studio Vault sur ce PC.".into());
        return;
    };
    app.set_career_text("Import de Studio Vault…".into());
    let path = st.career_path.clone();
    st.spawn(move || {
        let result = otoko_library::vault::snapshot(&dir).and_then(|(vault, tmp)| {
            // La copie a rejoué son journal : on la referme et l'import la relit en lecture seule.
            drop(vault);
            let mut c = open(&path).ok_or("base indisponible")?;
            let mut r = c.import_vault(&tmp.join("vault.db"));
            drop(c);
            // Les liens de partage (base à part, à côté de la Carrière), mêmes jetons.
            if let Ok(rep) = r.as_mut() {
                let shares = otoko_library::vault::copy_db(&dir, "partages.db", &tmp).and_then(|copy| match copy {
                    Some(copy) => {
                        let store = otoko_mobile::shares::ShareStore::open(&shares_path(&path)).map_err(|e| e.to_string())?;
                        store.import(&copy, &rep.track_ids, &rep.release_ids)
                    }
                    None => Ok(0),
                });
                rep.shares = shares.unwrap_or(0);
            }
            let _ = std::fs::remove_dir_all(tmp);
            r
        });
        Done::Vault(result)
    });
}

pub fn imported(app: &AppWindow, st: &mut State, result: Result<otoko_library::career::ImportReport, String>) {
    // La base a été écrite par un autre accès : on la rouvre pour relire. Les dossiers surveillés
    // venus de Studio Vault sont relus.
    st.career = open(&st.career_path);
    st.lib.last_scan = None;
    st.lib.covers_asked.clear();
    // Les préférences de Studio Vault (couleur d'accent, tableau) viennent avec la base.
    crate::preferences::apply(app, st);
    fill(app, st);
    let text = match result {
        Ok(r) => {
            let s = |n: usize| if n > 1 { "s" } else { "" };
            format!(
                "Studio Vault importé : {} morceaux, {} sorties, {} client{}, {} texte{} de publication, {} prompt{} Suno, {} tag{}, {} marqueur{}, {} vue{}, {} dossier{} surveillé{}, {} lien{} de partage ajoutés{}. Ta base Studio Vault n'a pas été modifiée.",
                r.songs,
                r.releases,
                r.clients,
                s(r.clients),
                r.drafts,
                s(r.drafts),
                r.prompts,
                s(r.prompts),
                r.tags,
                s(r.tags),
                r.markers,
                s(r.markers),
                r.views,
                s(r.views),
                r.folders,
                s(r.folders),
                s(r.folders),
                r.shares,
                s(r.shares),
                if r.skipped > 0 { format!(" ({} déjà présents)", r.skipped) } else { String::new() }
            )
        }
        Err(e) => format!("Import impossible : {e}"),
    };
    app.set_career_text(text.as_str().into());
    app.set_message(text.into());
}

/// Ouvre le projet d'un morceau (FL Studio, Ableton…) dans son logiciel.
pub fn open_project(app: &AppWindow, st: &mut State, id: i64) {
    let Some((title, project)) = st.career.as_ref().and_then(|c| otoko_library::career::projet::project_of(c.conn(), id)) else { return };
    let Some(path) = project else {
        app.set_message(format!("« {title} » n'a pas de projet retrouvé sur le disque : indique-le dans son panneau de détails.").into());
        return;
    };
    // Le chemin vient de la base : l'explorateur lancerait un programme aussi bien qu'un projet.
    let ext = std::path::Path::new(&path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ["exe", "com", "bat", "cmd", "ps1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "scr", "msi", "msp", "lnk", "pif", "cpl", "hta", "reg", "url"].contains(&ext.as_str()) {
        app.set_message(format!("« {title} » : ce chemin mène à un programme, pas à un projet. Il n'a pas été ouvert.").into());
        return;
    }
    if !st.selftest {
        let _ = std::process::Command::new("explorer.exe").arg(&path).spawn();
    }
    app.set_message(format!("Projet de « {title} » ouvert.").into());
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_read_in_french() {
        assert_eq!(super::fr_date("2026-12-04"), "4 décembre 2026");
        assert_eq!(super::fr_date("2027-01-01T10:00"), "1er janvier 2027");
        assert_eq!(super::fr_date("bientôt"), "bientôt");
        assert_eq!(super::today().len(), 10);
    }
}
