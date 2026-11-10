//! Carrière › Bibliothèque › Outils (comme Studio Vault) : renommer en lot, réorganiser en
//! dossiers, annuler la dernière opération sur les fichiers, écrire les tags dans les fichiers,
//! compléter genres et artistes, doublons, rattacher une version. Tout ce qui touche aux
//! fichiers est demandé par l'utilisateur et montré avant d'être fait ; l'autotest ne touche
//! jamais aux fichiers.

use std::collections::BTreeSet;
use std::path::PathBuf;

use otoko_library::career::files::{self, DupRow, Move, Suggestion, TagValues};
use otoko_library::career::folders;
use otoko_library::career::journal;
use otoko_library::career::library as lib;
use otoko_library::career::releases::{self, SyncAction};
use otoko_library::career::{nature, nature_short, prefs, statuts};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::state::State;
use crate::{AppWindow, DlgBtnUi, DlgItemUi, Lib};

/// Le dialogue ouvert.
#[derive(Default)]
pub enum Dlg {
    #[default]
    None,
    Rename { ids: Vec<i64>, moves: Vec<Move> },
    Reorganize { ids: Vec<i64>, root: PathBuf, copy: bool, moves: Vec<Move> },
    Dupes { groups: Vec<Vec<DupRow>>, checked: BTreeSet<i64> },
    Complete { items: Vec<Suggestion>, checked: BTreeSet<usize>, write: bool },
    Attach { id: i64, all: Vec<(i64, String, String)>, shown: Vec<usize>, chosen: Option<i64>, stem: bool },
    Nature { found: Vec<(i64, String, &'static str)>, checked: BTreeSet<i64> },
    Sync { actions: Vec<SyncAction>, checked: BTreeSet<usize> },
    /// Ajouter la sélection à une sortie : une existante (choisie), ou une nouvelle (titre tapé).
    AddToRelease { ids: Vec<i64>, all: Vec<(i64, String, String)>, chosen: Option<i64> },
    /// Genre et ambiance en lot (le genre est le champ du dialogue).
    Genre { ids: Vec<i64>, genres: Vec<String>, moods: BTreeSet<&'static str>, replace: bool, write: bool },
    /// Dossiers en double : pour chaque paire, le dossier à retirer.
    DupeFolders { pairs: Vec<folders::Dupe>, trash: bool },
    /// Changer l'emplacement de la bibliothèque (l'ancien début de chemin est le champ du dialogue).
    Relocate { new: PathBuf, suggestions: Vec<(String, usize)>, preview: folders::Preview },
    /// Les dossiers surveillés ; le champ : les noms de dossiers d'archives.
    Watch { list: Vec<folders::Watch>, chosen: Option<i64> },
    /// Au démarrage : des dossiers surveillés introuvables.
    Missing { list: Vec<folders::Watch>, checked: BTreeSet<i64>, quiet: bool },
    /// La file de lecture.
    Queue { chosen: Option<usize> },
    /// Les genres proposés par lots ; `editing` : le lot dont le champ montre le genre.
    Batches { list: Vec<files::Batch>, checked: BTreeSet<usize>, editing: Option<usize> },
    /// Les statuts que les données prouvent, à valider la première fois.
    Statuses { list: Vec<statuts::Change>, checked: BTreeSet<usize> },
}

const RENAME_DEFAULT: &str = "{artist} - {title}";
const REORG_DEFAULT: &str = "{artist}/{album}/{track_no} - {title}";

fn btn(id: &str, label: &str, primary: bool) -> DlgBtnUi {
    DlgBtnUi { id: id.into(), label: label.into(), primary }
}

fn item(id: i64, text: String, sub: String, checkable: bool, checked: bool) -> DlgItemUi {
    DlgItemUi { id: id as i32, text: text.into(), sub: sub.into(), checkable, checked, ..Default::default() }
}

fn selection(st: &State) -> Vec<i64> {
    st.lib.rows.iter().filter(|r| st.lib.selected.contains(&r.id)).map(|r| r.id).collect()
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn reason_text(r: &str) -> &'static str {
    match r {
        "unchanged" => "inchangé",
        "missing" => "fichier introuvable",
        "invalid" => "nom vide",
        "outside_root" => "hors du dossier choisi",
        _ => "",
    }
}

/// Ouvre un outil sur la sélection (ou sur toute la bibliothèque pour doublons et compléter).
pub fn open(app: &AppWindow, st: &mut State, what: &str) {
    let ids = selection(st);
    let Some(c) = st.career.as_ref() else { return };
    let conn = c.conn();
    match what {
        "rename" | "reorganize" if ids.is_empty() => {
            app.set_message("Sélectionne d'abord les morceaux (clic, Ctrl+clic, Maj+clic).".into());
            return;
        }
        "rename" => {
            let pattern = crate::settings::get("career_rename").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| RENAME_DEFAULT.into());
            app.global::<Lib>().set_dialog_input(pattern.as_str().into());
            st.lib.dlg = Dlg::Rename { moves: files::rename_preview(&files::file_rows(conn, &ids), &pattern), ids };
        }
        "reorganize" => {
            let pattern = crate::settings::get("career_reorganize").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| REORG_DEFAULT.into());
            let root = otoko_library::career::scan::watch_folders(conn).first().map(|f| PathBuf::from(&f.1)).unwrap_or_default();
            app.global::<Lib>().set_dialog_input(pattern.as_str().into());
            st.lib.dlg = Dlg::Reorganize { moves: files::reorganize_preview(&files::file_rows(conn, &ids), &root, &pattern), ids, root, copy: false };
        }
        "dupes" => {
            st.lib.dlg = Dlg::Dupes { groups: files::dupe_groups(conn), checked: BTreeSet::new() };
        }
        "complete" => {
            let artist = c.setting("brand_name").filter(|a| !a.is_empty()).unwrap_or_else(|| "Otoko".into());
            app.global::<Lib>().set_dialog_input(artist.as_str().into());
            let items = files::suggest(conn, &artist);
            // Le tempo seul est un repère large : décoché, à vérifier (comme Studio Vault).
            let checked = items.iter().enumerate().filter(|(_, s)| !s.reason.starts_with("tempo")).map(|(i, _)| i).collect();
            st.lib.dlg = Dlg::Complete { items, checked, write: false };
        }
        "attach" => {
            let Some(id) = ids.first().copied().or(st.lib.details) else { return };
            let all: Vec<(i64, String, String)> = conn
                .prepare(
                    "SELECT id, COALESCE(NULLIF(title, ''), filename), IFNULL(album, '') FROM tracks
                     WHERE missing = 0 AND id <> ?1 AND (parent_id IS NULL OR parent_id <> ?1) ORDER BY title COLLATE NOCASE",
                )
                .and_then(|mut s| s.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
                .unwrap_or_default();
            app.global::<Lib>().set_dialog_input("".into());
            let shown = (0..all.len()).collect();
            st.lib.dlg = Dlg::Attach { id, all, shown, chosen: None, stem: false };
        }
        // Toute la bibliothèque, pas la sélection en cours (comme depuis l'accueil de Studio Vault).
        "nature" => {
            let found = nature::suggestions(conn, &[]);
            let checked = found.iter().map(|f| f.0).collect();
            st.lib.dlg = Dlg::Nature { found, checked };
        }
        "sync" => {
            let actions = releases::sync_plan(conn);
            let checked = (0..actions.len()).collect();
            st.lib.dlg = Dlg::Sync { actions, checked };
        }
        "add-to-release" => {
            if ids.is_empty() {
                app.set_message("Sélectionne d'abord les morceaux.".into());
                return;
            }
            let all = releases::gallery(conn).into_iter().map(|g| (g.id, g.title, g.sub)).collect();
            app.global::<Lib>().set_dialog_input("".into());
            st.lib.dlg = Dlg::AddToRelease { ids, all, chosen: None };
        }
        "genre" => {
            if ids.is_empty() {
                app.set_message("Sélectionne d'abord les morceaux (un album, un dossier…).".into());
                return;
            }
            app.global::<Lib>().set_dialog_input("".into());
            let moods = lib::common_moods(conn, &ids).into_iter().collect();
            st.lib.dlg = Dlg::Genre { genres: lib::genre_choices(conn), ids, moods, replace: true, write: false };
        }
        "dupe-folders" => {
            let mut pairs: Vec<folders::Dupe> = Vec::new();
            for d in folders::dupe_folders(conn) {
                if !pairs.iter().any(|p| (p.folder == d.folder && p.other == d.other) || (p.folder == d.other && p.other == d.folder)) {
                    pairs.push(d);
                }
            }
            st.lib.dlg = Dlg::DupeFolders { pairs, trash: false };
        }
        "relocate" => {
            let suggestions = folders::suggestions(conn);
            let old = suggestions.first().map(|s| s.0.clone()).unwrap_or_default();
            app.global::<Lib>().set_dialog_input(old.as_str().into());
            st.lib.dlg = Dlg::Relocate { new: PathBuf::new(), suggestions, preview: folders::Preview::default() };
        }
        "watch" => {
            app.global::<Lib>().set_dialog_input(lib::archive_patterns(conn).join(", ").into());
            st.lib.dlg = Dlg::Watch { list: folders::watch_list(conn), chosen: None };
        }
        "missing" => {
            let list = folders::missing_watch(conn);
            if list.is_empty() {
                return;
            }
            st.lib.dlg = Dlg::Missing { list, checked: BTreeSet::new(), quiet: false };
        }
        "queue" => st.lib.dlg = Dlg::Queue { chosen: None },
        "batches" => {
            let mut own = prefs::aliases(conn);
            own.push(c.setting("artist_alias").unwrap_or_default());
            let list = files::batches(conn, &own);
            let checked = list.iter().enumerate().filter(|(_, b)| b.sure && !b.genre.is_empty()).map(|(i, _)| i).collect();
            app.global::<Lib>().set_dialog_input("".into());
            st.lib.dlg = Dlg::Batches { list, checked, editing: None };
        }
        "statuses" => {
            let list = statuts::plan(conn);
            let checked = (0..list.len()).collect();
            st.lib.dlg = Dlg::Statuses { list, checked };
        }
        _ => return,
    }
    push(app, st);
}

/// Montre le dialogue tel qu'il est.
pub fn push(app: &AppWindow, st: &State) {
    let g = app.global::<Lib>();
    let set = |title: &str, text: &str, input: &str, input2: (&str, String), flag: (&str, bool), items: Vec<DlgItemUi>, buttons: Vec<DlgBtnUi>| {
        g.set_dialog_title(title.into());
        g.set_dialog_text(text.into());
        g.set_dialog_input_label(input.into());
        g.set_dialog_input2_label(input2.0.into());
        g.set_dialog_input2(input2.1.into());
        g.set_dialog_flag_label(flag.0.into());
        g.set_dialog_flag(flag.1);
        g.set_dialog_items(ModelRc::new(VecModel::from(items)));
        g.set_dialog_buttons(ModelRc::new(VecModel::from(buttons)));
    };
    let moves_items = |moves: &[Move]| -> Vec<DlgItemUi> {
        moves
            .iter()
            .map(|m| {
                let sub = if m.reason == "ok" { format!("→ {}", m.dst.display()) } else { reason_text(m.reason).to_string() };
                item(m.id, file_name(&m.src), sub, false, false)
            })
            .collect()
    };
    match &st.lib.dlg {
        Dlg::None => {
            g.set_dialog("".into());
            return;
        }
        Dlg::Rename { moves, .. } => {
            let n = moves.iter().filter(|m| m.reason == "ok").count();
            set(
                "Renommer les fichiers",
                "Modèle : {title} {artist} {album} {genre} {year} {track_no} {bpm} {key} {status} {orig}. Les fichiers restent dans leur dossier ; Ctrl+Z annule.",
                "MODÈLE DE NOM",
                ("", String::new()),
                ("", false),
                moves_items(moves),
                vec![btn("apply", &format!("Renommer {n} fichier{}", if n > 1 { "s" } else { "" }), true)],
            );
        }
        Dlg::Reorganize { moves, root, copy, .. } => {
            let n = moves.iter().filter(|m| m.reason == "ok").count();
            set(
                "Réorganiser en dossiers",
                "Modèle, avec « / » pour les sous-dossiers : {artist} {album} {genre} {year} {track_no} {bpm} {key} {status_fr} {role} {version} {format} {title} {orig}. Ctrl+Z annule un déplacement.",
                "MODÈLE DE CHEMIN",
                ("DOSSIER DE DESTINATION", root.display().to_string()),
                ("Copier au lieu de déplacer (les originaux restent en place)", *copy),
                moves_items(moves),
                vec![btn("apply", &format!("{} {n} fichier{}", if *copy { "Copier" } else { "Déplacer" }, if n > 1 { "s" } else { "" }), true)],
            );
        }
        Dlg::Dupes { groups, checked } => {
            let mut items = Vec::new();
            for (k, g) in groups.iter().enumerate() {
                let best = files::best_of(g);
                items.push(DlgItemUi { text: format!("Groupe {} — {} fichiers", k + 1, g.len()).into(), header: true, ..Default::default() });
                let mut sorted: Vec<&DupRow> = g.iter().collect();
                sorted.sort_by_key(|r| r.id != best);
                for r in sorted {
                    let sub = format!(
                        "{} · {} · {:.1} Mo · {}",
                        r.format.to_uppercase(),
                        lib::duration_text(r.duration),
                        r.size as f64 / 1_048_576.0,
                        std::path::Path::new(&r.path).parent().map(|p| p.display().to_string()).unwrap_or_default()
                    );
                    let mut it = item(r.id, r.filename.clone(), sub, true, checked.contains(&r.id));
                    it.star = r.id == best;
                    items.push(it);
                }
            }
            let n: usize = groups.iter().map(|g| g.len()).sum();
            set(
                if groups.is_empty() { "Aucun doublon" } else { "Doublons" },
                &if groups.is_empty() {
                    "Aucune copie du même fichier dans la bibliothèque.".to_string()
                } else {
                    format!(
                        "{} groupe{} — {n} fichiers. Repérés par le contenu : une copie renommée est reconnue. Dans chaque groupe, ★ marque le fichier conseillé à garder. Coche ceux à traiter.",
                        groups.len(),
                        if groups.len() > 1 { "s" } else { "" }
                    )
                },
                "",
                ("", String::new()),
                ("", false),
                items,
                if groups.is_empty() {
                    Vec::new()
                } else {
                    vec![btn("check-all", "Cocher tous sauf ★", false), btn("forget", "Retirer les cochés de la bibliothèque", false), btn("trash", "Mettre les cochés à la corbeille", true)]
                },
            );
        }
        Dlg::Complete { items, checked, write } => {
            let list: Vec<DlgItemUi> = items
                .iter()
                .enumerate()
                .map(|(i, s)| item(i as i64, format!("{} → {} : {}", s.title, if s.field == "genre" { "genre" } else { "artiste" }, s.value), s.reason.clone(), true, checked.contains(&i)))
                .collect();
            let n = checked.len();
            set(
                "Compléter genres et artistes",
                "Propositions pour les morceaux sans genre ou sans artiste : style écrit dans le titre, autres morceaux de la sortie ou du dossier, nom du dossier, puis tempo (décoché, à vérifier). Rien n'est écrit sans ton accord.",
                "TON NOM D'ARTISTE (pour les morceaux rangés avec les tiens)",
                ("", String::new()),
                ("Écrire aussi dans les fichiers (tags)", *write),
                list,
                vec![btn("apply", &format!("Appliquer {n} proposition{}", if n > 1 { "s" } else { "" }), true)],
            );
        }
        Dlg::Batches { list, checked, editing } => {
            let items: Vec<DlgItemUi> = list
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let n = b.ids.len();
                    let genre = if b.genre.is_empty() { "à choisir".to_string() } else { b.genre.clone() };
                    item(i as i64, format!("{} : {n} morceau{} → {genre}", b.label, if n > 1 { "x" } else { "" }), b.why.clone(), true, checked.contains(&i))
                })
                .collect();
            let tracks: usize = checked.iter().filter_map(|i| list.get(*i)).map(|b| b.ids.len()).sum();
            set(
                "Genres proposés par lots",
                "Les morceaux sans genre, regroupés. Coche les lots qui te vont ; clique un lot puis écris dans le champ pour changer son genre.",
                if editing.is_some() { "GENRE DU LOT CHOISI" } else { "GENRE (CLIQUE D'ABORD UN LOT)" },
                ("", String::new()),
                ("", false),
                items,
                vec![btn("apply", &format!("Appliquer à {tracks} morceau{}", if tracks > 1 { "x" } else { "" }), true)],
            );
        }
        Dlg::Statuses { list, checked } => {
            let items: Vec<DlgItemUi> = list.iter().enumerate().map(|(i, c)| item(i as i64, format!("{} → {}", c.title, otoko_library::career::status_label(c.to)), c.why.to_string(), true, checked.contains(&i))).collect();
            let n = checked.len();
            set(
                "Statuts automatiques",
                "Le tableau de production peut suivre ton travail : voici ce que tes données prouvent. Un morceau n'avance que sur une preuve et ne recule jamais. Valide cette première liste ; ensuite, ça se fait seul et une carte déplacée à la main n'est plus touchée.",
                "",
                ("", String::new()),
                ("", false),
                items,
                vec![btn("apply", &format!("Appliquer à {n} morceau{}", if n > 1 { "x" } else { "" }), true)],
            );
        }
        Dlg::Attach { all, shown, chosen, stem, .. } => {
            let list: Vec<DlgItemUi> =
                shown.iter().take(300).map(|&k| item(all[k].0, all[k].1.clone(), all[k].2.clone(), true, *chosen == Some(all[k].0))).collect();
            set(
                "Rattacher à un morceau principal",
                "Choisis le morceau principal (tape pour chercher). Le fichier devient une de ses versions, ou une piste séparée.",
                "CHERCHER",
                ("", String::new()),
                ("C'est une piste séparée (stem), pas une version", *stem),
                list,
                vec![btn("apply", "Rattacher", true)],
            );
        }
        Dlg::Nature { found, checked } => {
            let list: Vec<DlgItemUi> = found.iter().map(|(id, path, nat)| item(*id, file_name(std::path::Path::new(path)), nature_short(nat), true, checked.contains(id))).collect();
            let n = checked.len();
            set(
                if found.is_empty() { "Rien à classer" } else { "Classer remix, edits et mashups" },
                &if found.is_empty() {
                    "Aucun remix, edit, mashup, set ou prod repéré dans les noms de fichiers.".to_string()
                } else {
                    format!("{} morceau(x) encore « originaux » dont le nom dit autre chose. Décoche ce qui est faux : la nature (et l'artiste d'origine d'un remix) est écrite dans ta base, pas dans les fichiers.", found.len())
                },
                "",
                ("", String::new()),
                ("", false),
                list,
                if found.is_empty() { Vec::new() } else { vec![btn("apply", &format!("Appliquer à {n} morceau{}", if n > 1 { "x" } else { "" }), true)] },
            );
        }
        Dlg::Sync { actions, checked } => {
            let list: Vec<DlgItemUi> = actions
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let mut sub = Vec::new();
                    if a.copies > 0 {
                        sub.push(format!("{} copie(s) laissée(s) de côté", a.copies));
                    }
                    if a.elsewhere > 0 {
                        sub.push(format!("{} dans un autre dossier, laissé(s) de côté", a.elsewhere));
                    }
                    item(i as i64, releases::describe(a), sub.join(" · "), true, checked.contains(&i))
                })
                .collect();
            let n: usize = checked.iter().filter_map(|&i| actions.get(i)).map(|a| a.track_ids.len()).sum();
            set(
                if actions.is_empty() { "Tout est rangé" } else { "Rattacher les albums" },
                if actions.is_empty() {
                    "Tout est déjà rangé : chaque album de tes fichiers a sa fiche de sortie."
                } else {
                    "D'après l'album écrit dans tes fichiers. Décoche ce qui ne doit pas être rangé. Les copies d'un morceau déjà dans la fiche et les morceaux d'un autre dossier (démos…) sont laissés de côté. Les fichiers ne sont pas modifiés."
                },
                "",
                ("", String::new()),
                ("", false),
                list,
                if actions.is_empty() { Vec::new() } else { vec![btn("apply", &format!("Ranger {n} morceau{}", if n > 1 { "x" } else { "" }), true)] },
            );
        }
        Dlg::AddToRelease { ids, all, chosen } => {
            let list: Vec<DlgItemUi> = all.iter().map(|(id, title, sub)| item(*id, title.clone(), sub.clone(), true, *chosen == Some(*id))).collect();
            set(
                &format!("Ajouter {} morceau(x) à une sortie", ids.len()),
                "Choisis une sortie, ou tape le titre d'une nouvelle. Les morceaux s'ajoutent à la fin ; l'ordre se règle dans la fiche.",
                "TITRE D'UNE NOUVELLE SORTIE (sinon, choisis-en une ci-dessous)",
                ("", String::new()),
                ("", false),
                list,
                vec![btn("apply", "Ajouter", true)],
            );
        }
        Dlg::Genre { ids, genres, moods, replace, write } => {
            let mut list = vec![DlgItemUi { text: "AMBIANCE (deux au maximum conseillé)".into(), header: true, ..Default::default() }];
            list.extend(otoko_library::career::humeur::ADJECTIFS.iter().enumerate().map(|(k, a)| item(20_000 + k as i64, a.0.to_string(), String::new(), true, moods.contains(a.0))));
            list.push(DlgItemUi { text: "OPTIONS".into(), header: true, ..Default::default() });
            list.push(item(30_000, "Remplacer les ambiances existantes (sinon, on ajoute)".into(), String::new(), true, *replace));
            list.push(DlgItemUi { text: "GENRES (un clic le met dans le champ)".into(), header: true, ..Default::default() });
            list.extend(genres.iter().enumerate().map(|(k, g)| item(10_000 + k as i64, g.clone(), String::new(), false, false)));
            set(
                "Genre et ambiance",
                &format!("{} morceau(x). Le genre et l'ambiance servent au moteur de textes, aux filtres et aux statistiques. Laisse le genre vide pour garder l'existant.", ids.len()),
                "GENRE (vide : ne pas changer)",
                ("", String::new()),
                ("Écrire aussi le genre dans les fichiers audio", *write),
                list,
                vec![btn("apply", "Appliquer", true)],
            );
        }
        Dlg::DupeFolders { pairs, trash } => {
            let short = |f: &str| {
                let parts: Vec<&str> = f.split(['\\', '/']).filter(|p| !p.is_empty()).collect();
                if parts.len() > 3 { parts[parts.len() - 3..].join("\\") } else { f.to_string() }
            };
            let name = |f: &str| std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.to_string());
            let mut list = Vec::new();
            for (k, d) in pairs.iter().enumerate() {
                list.push(DlgItemUi { text: format!("{} ({} morceaux) et {} : {} identique(s)", short(&d.folder), d.n, short(&d.other), d.common).into(), header: true, ..Default::default() });
                list.push(item((k * 2) as i64, format!("Retirer « {} »", name(&d.folder)), format!("{} — on garde {}", d.folder, d.other), false, false));
                list.push(item((k * 2 + 1) as i64, format!("Retirer « {} »", name(&d.other)), format!("{} — on garde {}", d.other, d.folder), false, false));
            }
            set(
                if pairs.is_empty() { "Aucun dossier en double" } else { "Dossiers en double" },
                if pairs.is_empty() {
                    "Aucun dossier en double."
                } else {
                    "Ces dossiers contiennent les mêmes fichiers audio (même contenu, pas seulement le même nom). Pour chaque paire, choisis le dossier à retirer de la bibliothèque : il ne sera plus réindexé. Ses fichiers restent sur le disque, sauf si tu coches la corbeille."
                },
                "",
                ("", String::new()),
                ("Mettre aussi les fichiers en double à la corbeille (seulement ceux présents dans l'autre dossier)", *trash),
                list,
                Vec::new(),
            );
        }
        Dlg::Relocate { new, suggestions, preview } => {
            let mut list = vec![DlgItemUi { text: "EMPLACEMENTS CONNUS (un clic le met dans le champ)".into(), header: true, ..Default::default() }];
            list.extend(suggestions.iter().enumerate().map(|(k, s)| item(10_000 + k as i64, s.0.clone(), format!("{} morceau(x)", s.1), false, false)));
            let old = g.get_dialog_input().to_string();
            let mut lines: Vec<String> = Vec::new();
            if !old.trim().is_empty() && !new.as_os_str().is_empty() {
                if !new.is_dir() {
                    lines.push("Le nouvel emplacement n'existe pas (disque branché ? dossier copié ?).".into());
                } else if preview.tracks == 0 {
                    lines.push("Aucun morceau de la bibliothèque n'est dans l'ancien emplacement.".into());
                } else {
                    lines.push(format!("{} morceau(x) concerné(s) : {} retrouvé(s) au nouvel emplacement, {} introuvable(s).", preview.tracks, preview.found, preview.absent.len()));
                    if preview.conflicts > 0 {
                        lines.push(format!("{} morceau(x) déjà indexé(s) au nouvel emplacement : ces entrées récentes seront remplacées par les anciennes, qui gardent ton travail.", preview.conflicts));
                    }
                    if preview.found * 2 < preview.tracks {
                        lines.push("Moins de la moitié des fichiers sont retrouvés : vérifie le nouvel emplacement (il doit correspondre à l'ancien dossier, pas à un de ses parents).".into());
                    }
                }
            }
            if !preview.absent.is_empty() && new.is_dir() {
                list.push(DlgItemUi { text: "PAR EXEMPLE, INTROUVABLE".into(), header: true, ..Default::default() });
                list.extend(preview.absent.iter().take(5).map(|a| item(-1, a.clone(), String::new(), false, false)));
            }
            let ready = new.is_dir() && preview.tracks > 0 && preview.found * 2 >= preview.tracks;
            set(
                "Changer l'emplacement de la bibliothèque",
                &format!(
                    "Tu as déplacé ou copié tes fichiers ailleurs (autre disque, autre dossier, Google Drive vers un disque local…). Indique l'ancien et le nouvel emplacement : analyses, tags, fiches de sortie et statuts suivent les morceaux. Les fichiers ne sont pas touchés, et la base est sauvegardée juste avant.{}",
                    if lines.is_empty() { String::new() } else { format!("\n\n{}", lines.join("\n")) }
                ),
                "ANCIEN EMPLACEMENT",
                ("NOUVEL EMPLACEMENT", if new.as_os_str().is_empty() { "(à choisir)".into() } else { new.display().to_string() }),
                ("", false),
                list,
                if ready { vec![btn("apply", "Changer l'emplacement", true)] } else { Vec::new() },
            );
        }
        Dlg::Watch { list, chosen } => {
            let items: Vec<DlgItemUi> = list
                .iter()
                .map(|w| {
                    let state = if !w.exists { "introuvable" } else if w.enabled { "surveillé" } else { "désactivé" };
                    let mut it = item(w.id, w.path.clone(), format!("{} · {} morceau(x)", state, w.tracks), true, *chosen == Some(w.id));
                    it.star = w.enabled && w.exists;
                    it
                })
                .collect();
            set(
                "Dossiers surveillés",
                "OTOKO relit ces dossiers à l'ouverture et toutes les 10 minutes : tout nouveau fichier audio déposé dedans (ou dans un sous-dossier) entre dans la bibliothèque, un fichier supprimé est noté introuvable. ★ : surveillé. Choisis un dossier pour l'activer, le désactiver ou le retirer.",
                "DOSSIERS D'ARCHIVES (noms séparés par des virgules, * pour joker) : gardés mais hors des vues normales",
                ("", String::new()),
                ("", false),
                items,
                vec![btn("add", "+ Ajouter un dossier…", true), btn("enable", "Activer / désactiver", false), btn("remove", "Retirer", false), btn("rescan", "Rescanner", false)],
            );
        }
        Dlg::Missing { list, checked, quiet } => {
            let items: Vec<DlgItemUi> = list.iter().map(|w| item(w.id, w.path.clone(), format!("{} morceau(x) indexé(s)", w.tracks), true, checked.contains(&w.id))).collect();
            set(
                &format!("{} dossier(s) surveillé(s) introuvable(s)", list.len()),
                "C'est normal si un disque (par exemple P:) n'est pas branché : dans ce cas, garde-les. Coche ceux que tu veux retirer de la surveillance. Les morceaux déjà indexés restent dans la bibliothèque. Dossiers déplacés ? « Changer l'emplacement… ».",
                "",
                ("", String::new()),
                ("Ne plus me prévenir pour ces dossiers", *quiet),
                items,
                vec![btn("keep", "Garder tout", false), btn("relocate", "Changer l'emplacement…", false), btn("apply", "Retirer les dossiers cochés", true)],
            );
        }
        Dlg::Queue { chosen } => {
            let items: Vec<DlgItemUi> = st
                .player
                .queue
                .iter()
                .enumerate()
                .map(|(k, id)| {
                    let r = st.career.as_ref().and_then(|c| lib::row_by_id(c.conn(), *id).ok().flatten());
                    let (title, sub) = r.map(|r| (r.title.clone(), [r.artist.clone(), lib::duration_text(r.duration)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "))).unwrap_or_default();
                    item(k as i64, format!("{}. {title}", k + 1), sub, true, *chosen == Some(k))
                })
                .collect();
            set(
                "File de lecture",
                if st.player.queue.is_empty() { "La file est vide : Q ajoute les morceaux sélectionnés." } else { "Les morceaux joués après celui en cours, dans cet ordre. Choisis-en un pour le déplacer, le retirer ou l'écouter tout de suite." },
                "",
                ("", String::new()),
                ("", false),
                items,
                if st.player.queue.is_empty() {
                    Vec::new()
                } else {
                    vec![btn("play", "Écouter maintenant", true), btn("up", "Monter", false), btn("down", "Descendre", false), btn("remove", "Retirer", false), btn("clear", "Vider la file", false)]
                },
            );
        }
    }
    let kind = match &st.lib.dlg {
        Dlg::Rename { .. } => "rename",
        Dlg::Reorganize { .. } => "reorganize",
        Dlg::Dupes { .. } => "dupes",
        Dlg::Complete { .. } => "complete",
        Dlg::Attach { .. } => "attach",
        Dlg::Nature { .. } => "nature",
        Dlg::Sync { .. } => "sync",
        Dlg::AddToRelease { .. } => "add-to-release",
        Dlg::Genre { .. } => "genre",
        Dlg::DupeFolders { .. } => "dupe-folders",
        Dlg::Relocate { .. } => "relocate",
        Dlg::Watch { .. } => "watch",
        Dlg::Missing { .. } => "missing",
        Dlg::Queue { .. } => "queue",
        Dlg::Batches { .. } => "batches",
        Dlg::Statuses { .. } => "statuses",
        Dlg::None => "",
    };
    g.set_dialog(kind.into());
}

/// Le texte du champ a changé : l'aperçu suit.
pub fn input_edited(app: &AppWindow, st: &mut State, text: &str) {
    let Some(c) = st.career.as_ref() else { return };
    match &mut st.lib.dlg {
        Dlg::Rename { ids, moves } => *moves = files::rename_preview(&files::file_rows(c.conn(), ids), text),
        Dlg::Reorganize { ids, root, moves, .. } => *moves = files::reorganize_preview(&files::file_rows(c.conn(), ids), root, text),
        Dlg::Complete { items, checked, .. } => {
            *items = files::suggest(c.conn(), text.trim());
            *checked = items.iter().enumerate().filter(|(_, s)| !s.reason.starts_with("tempo")).map(|(i, _)| i).collect();
        }
        Dlg::Attach { all, shown, .. } => {
            let q = text.trim().to_lowercase();
            *shown = (0..all.len()).filter(|&k| q.is_empty() || all[k].1.to_lowercase().contains(&q) || all[k].2.to_lowercase().contains(&q)).collect();
        }
        Dlg::Relocate { new, preview, .. } => *preview = folders::preview(c.conn(), text, &new.to_string_lossy()),
        Dlg::Batches { list, checked, editing } => {
            let Some(k) = *editing else { return };
            if let Some(b) = list.get_mut(k) {
                b.genre = text.trim().to_string();
                if b.genre.is_empty() {
                    checked.remove(&k);
                } else {
                    checked.insert(k);
                }
            }
        }
        Dlg::Watch { .. } => {
            // Les noms de dossiers d'archives, enregistrés à chaque frappe ; les vues suivent à la fermeture.
            let list: Vec<String> = text.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let _ = folders::set_archive_patterns(c.conn(), &list);
            return;
        }
        _ => return,
    }
    push(app, st);
}

pub fn toggle(app: &AppWindow, st: &mut State, i: usize) {
    let items = app.global::<Lib>().get_dialog_items();
    let Some(it) = slint::Model::row_data(&items, i) else { return };
    match &mut st.lib.dlg {
        Dlg::Dupes { checked, .. } => {
            let id = it.id as i64;
            if !checked.remove(&id) {
                checked.insert(id);
            }
        }
        Dlg::Complete { checked, .. } => {
            let k = it.id as usize;
            if !checked.remove(&k) {
                checked.insert(k);
            }
        }
        Dlg::Attach { chosen, .. } => *chosen = Some(it.id as i64),
        Dlg::Nature { checked, .. } => {
            let id = it.id as i64;
            if !checked.remove(&id) {
                checked.insert(id);
            }
        }
        Dlg::Sync { checked, .. } => {
            let k = it.id as usize;
            if !checked.remove(&k) {
                checked.insert(k);
            }
        }
        Dlg::AddToRelease { chosen, .. } => *chosen = if *chosen == Some(it.id as i64) { None } else { Some(it.id as i64) },
        Dlg::Genre { genres, moods, replace, .. } => match it.id {
            30_000 => *replace = !*replace,
            id if id >= 20_000 => {
                if let Some(a) = otoko_library::career::humeur::ADJECTIFS.get(id as usize - 20_000).map(|a| a.0) {
                    if !moods.remove(a) {
                        moods.insert(a);
                    }
                }
            }
            id if id >= 10_000 => {
                if let Some(g) = genres.get(id as usize - 10_000) {
                    app.global::<Lib>().set_dialog_input(g.as_str().into());
                }
            }
            _ => {}
        },
        Dlg::Relocate { suggestions, .. } => {
            if let Some(s) = (it.id >= 10_000).then(|| suggestions.get(it.id as usize - 10_000)).flatten() {
                let old = s.0.clone();
                app.global::<Lib>().set_dialog_input(old.as_str().into());
                return input_edited(app, st, &old);
            }
            return;
        }
        Dlg::DupeFolders { .. } => return remove_dupe_folder(app, st, it.id as usize),
        Dlg::Watch { chosen, .. } => *chosen = Some(it.id as i64),
        Dlg::Missing { checked, .. } => {
            let id = it.id as i64;
            if !checked.remove(&id) {
                checked.insert(id);
            }
        }
        Dlg::Queue { chosen } => *chosen = Some(it.id as usize),
        Dlg::Batches { list, checked, editing } => {
            let k = it.id as usize;
            // Un lot sans genre attend qu'on l'écrive : il n'est coché qu'ensuite.
            if !checked.remove(&k) && list.get(k).is_some_and(|b| !b.genre.is_empty()) {
                checked.insert(k);
            }
            *editing = Some(k);
            let genre = list.get(k).map(|b| b.genre.clone()).unwrap_or_default();
            app.global::<Lib>().set_dialog_input(genre.as_str().into());
        }
        Dlg::Statuses { checked, .. } => {
            let k = it.id as usize;
            if !checked.remove(&k) {
                checked.insert(k);
            }
        }
        _ => return,
    }
    push(app, st);
}

/// Dossiers en double : retire le dossier choisi (paire `k / 2`, côté `k % 2`), après confirmation.
fn remove_dupe_folder(app: &AppWindow, st: &mut State, k: usize) {
    let Dlg::DupeFolders { pairs, trash } = &st.lib.dlg else { return };
    let Some(d) = pairs.get(k / 2) else { return };
    let (folder, keep) = if k % 2 == 0 { (d.folder.clone(), d.other.clone()) } else { (d.other.clone(), d.folder.clone()) };
    let trash = *trash;
    let what = if trash { "retiré de la bibliothèque et ses fichiers en double mis à la corbeille" } else { "retiré de la bibliothèque (les fichiers restent sur le disque)" };
    if !confirm(st, "Dossiers en double", &format!("Le dossier\n{folder}\nsera {what}.\n\nOn garde :\n{keep}\n\nContinuer ?")) {
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    let mut failed = 0;
    if trash {
        // Seulement les morceaux de ce dossier qu'on retrouve dans l'autre.
        let ids = folders::dupe_folders(c.conn()).into_iter().find(|x| x.folder == folder && x.other == keep).map(|x| x.ids).unwrap_or_default();
        let paths: Vec<PathBuf> = files::file_rows(c.conn(), &ids).into_iter().map(|r| PathBuf::from(r.path)).filter(|p| p.exists()).collect();
        if to_recycle_bin(&paths).is_err() {
            failed = paths.len();
        }
    }
    let n = folders::ignore_folder(c.conn(), &folder).unwrap_or(0);
    let mut msg = format!("{n} morceau(x) retiré(s) de la bibliothèque.");
    if failed > 0 {
        msg.push_str(&format!(" {failed} fichier(s) n'ont pas pu aller à la corbeille (sur Google Drive, supprime-les depuis l'Explorateur)."));
    }
    app.set_message(msg.into());
    open(app, st, "dupe-folders");
    crate::bibliotheque::fill(app, st);
}

fn confirm(st: &State, title: &str, text: &str) -> bool {
    if st.selftest {
        return false;
    }
    rfd::MessageDialog::new().set_title(title).set_description(text).set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes
}

pub fn button(app: &AppWindow, st: &mut State, id: &str) {
    match id {
        "close" => {
            // Les noms de dossiers d'archives ont pu changer : les vues suivent.
            let refill = matches!(st.lib.dlg, Dlg::Watch { .. });
            st.lib.dlg = Dlg::None;
            push(app, st);
            if refill {
                crate::bibliotheque::fill(app, st);
            }
            return;
        }
        "flag" => {
            match &mut st.lib.dlg {
                Dlg::Reorganize { copy, .. } => *copy = !*copy,
                Dlg::Complete { write, .. } => *write = !*write,
                Dlg::Attach { stem, .. } => *stem = !*stem,
                Dlg::Genre { write, .. } => *write = !*write,
                Dlg::DupeFolders { trash, .. } => *trash = !*trash,
                Dlg::Missing { quiet, .. } => *quiet = !*quiet,
                _ => {}
            }
            push(app, st);
            return;
        }
        "choose-root" => {
            if st.selftest {
                return;
            }
            let relocating = matches!(st.lib.dlg, Dlg::Relocate { .. });
            if let Some(dir) = rfd::FileDialog::new().set_title(if relocating { "Nouvel emplacement" } else { "Dossier de destination" }).pick_folder() {
                match &mut st.lib.dlg {
                    Dlg::Reorganize { root, .. } => *root = dir,
                    Dlg::Relocate { new, .. } => *new = dir,
                    _ => {}
                }
                let text = app.global::<Lib>().get_dialog_input().to_string();
                input_edited(app, st, &text);
            }
            return;
        }
        _ if matches!(st.lib.dlg, Dlg::Watch { .. } | Dlg::Queue { .. }) => return list_button(app, st, id),
        "keep" | "relocate" if matches!(st.lib.dlg, Dlg::Missing { .. }) => {
            if let Dlg::Missing { list, quiet: true, .. } = &st.lib.dlg {
                let paths: Vec<String> = list.iter().map(|w| w.path.clone()).collect();
                if let Some(c) = st.career.as_ref() {
                    let _ = folders::quiet_missing(c.conn(), &paths);
                }
            }
            let first = if let Dlg::Missing { list, .. } = &st.lib.dlg { list.first().map(|w| w.path.clone()) } else { None };
            st.lib.dlg = Dlg::None;
            if id == "relocate" {
                open(app, st, "relocate");
                if let Some(p) = first {
                    app.global::<Lib>().set_dialog_input(p.as_str().into());
                    input_edited(app, st, &p);
                }
            } else {
                push(app, st);
            }
            return;
        }
        "check-all" => {
            if let Dlg::Dupes { groups, checked } = &mut st.lib.dlg {
                checked.clear();
                for g in groups.iter() {
                    let best = files::best_of(g);
                    checked.extend(g.iter().filter(|r| r.id != best).map(|r| r.id));
                }
            }
            push(app, st);
            return;
        }
        _ => {}
    }
    let pattern = app.global::<Lib>().get_dialog_input().to_string();
    let dlg = std::mem::take(&mut st.lib.dlg);
    let Some(c) = st.career.as_ref() else { return };
    let conn = c.conn();
    let msg = match (&dlg, id) {
        (Dlg::Rename { moves, .. }, "apply") => {
            if st.selftest {
                "Autotest : aucun fichier n'est renommé.".to_string()
            } else {
                let (ok, err) = files::apply(conn, moves, "Renommage", false);
                crate::settings::set("career_rename", serde_json::json!(pattern));
                format!("{ok} fichier{} renommé{}{} — Ctrl+Z pour annuler.", if ok > 1 { "s" } else { "" }, if ok > 1 { "s" } else { "" }, if err > 0 { format!(", {err} échec(s)") } else { String::new() })
            }
        }
        (Dlg::Reorganize { moves, copy, .. }, "apply") => {
            let n = moves.iter().filter(|m| m.reason == "ok").count();
            if !confirm(st, "Réorganiser", &format!("{} {n} fichier(s) vers leur nouveau dossier ?", if *copy { "Copier" } else { "Déplacer" })) {
                st.lib.dlg = dlg;
                return;
            }
            let (ok, err) = files::apply(conn, moves, "Réorganisation", *copy);
            if !*copy {
                let srcs: Vec<PathBuf> = moves.iter().filter(|m| m.reason == "ok").map(|m| m.src.clone()).collect();
                files::clean_empty_dirs(&srcs);
            }
            crate::settings::set("career_reorganize", serde_json::json!(pattern));
            format!("{ok} fichier(s) {}{}.", if *copy { "copiés" } else { "déplacés — Ctrl+Z pour annuler" }, if err > 0 { format!(", {err} échec(s)") } else { String::new() })
        }
        (Dlg::Dupes { groups, checked }, "forget" | "trash") => {
            if checked.is_empty() {
                st.lib.dlg = dlg;
                app.set_message("Coche d'abord les fichiers à traiter.".into());
                return;
            }
            let trash = id == "trash";
            let verb = if trash { "Mettre à la corbeille" } else { "Retirer de la bibliothèque" };
            if !confirm(st, "Doublons", &format!("{verb} {} fichier(s) ?{}", checked.len(), if trash { "\n\nIls restent récupérables dans la corbeille de Windows." } else { "\n\nLes fichiers ne sont pas touchés." })) {
                st.lib.dlg = dlg;
                return;
            }
            let ids: Vec<i64> = checked.iter().copied().collect();
            let mut errors = 0;
            if trash {
                let paths: Vec<PathBuf> = groups.iter().flatten().filter(|r| checked.contains(&r.id)).map(|r| PathBuf::from(&r.path)).filter(|p| p.exists()).collect();
                if let Err(e) = to_recycle_bin(&paths) {
                    errors = paths.len();
                    app.set_message(format!("Corbeille impossible : {e}").into());
                }
            }
            if errors == 0 {
                files::forget(conn, &ids);
            }
            let n = ids.len();
            st.lib.dlg = Dlg::Dupes { groups: files::dupe_groups(conn), checked: BTreeSet::new() };
            push(app, st);
            crate::bibliotheque::fill(app, st);
            if errors == 0 {
                app.set_message(format!("{n} doublon(s) {}.", if trash { "mis à la corbeille" } else { "retirés de la bibliothèque" }).into());
            }
            return;
        }
        (Dlg::Complete { items, checked, write }, "apply") => {
            let chosen: Vec<Suggestion> = checked.iter().filter_map(|&i| items.get(i).cloned()).collect();
            let n = journal::record(conn, "Genres et artistes complétés", "", || files::apply_suggestions(conn, &chosen));
            let _ = c.set_setting("brand_name", pattern.trim());
            let mut written = (0, 0);
            if *write && !st.selftest {
                for s in &chosen {
                    let v = if s.field == "genre" { TagValues { genre: Some(s.value.clone()), ..Default::default() } } else { TagValues { artist: Some(s.value.clone()), ..Default::default() } };
                    match files::write_tags(std::path::Path::new(&s.path), &v) {
                        Ok(()) => written.0 += 1,
                        Err(_) => written.1 += 1,
                    }
                }
            }
            format!(
                "{n} proposition(s) appliquée(s){}.",
                if *write { format!(" ; {} fichier(s) mis à jour{}", written.0, if written.1 > 0 { format!(", {} impossible(s)", written.1) } else { String::new() }) } else { String::new() }
            )
        }
        (Dlg::Batches { list, checked, .. }, "apply") => {
            let n: usize = journal::record(conn, "Genres par lots", "", || checked.iter().filter_map(|i| list.get(*i)).map(|b| files::apply_genre(conn, &b.ids, &b.genre)).sum());
            format!("Genre donné à {n} morceau{}.", if n > 1 { "x" } else { "" })
        }
        (Dlg::Statuses { list, checked }, "apply") => {
            journal::record(conn, "Statuts automatiques", "", || {
                for c in checked.iter().filter_map(|i| list.get(*i)) {
                    let _ = otoko_library::career::board::set_status(conn, c.id, c.to);
                }
            });
            // Ce qui a été décoché reste où il est, pour de bon.
            for (i, c) in list.iter().enumerate() {
                if !checked.contains(&i) {
                    let _ = lib::lock(conn, c.id, "status", true);
                }
            }
            crate::fond::status_seen(st);
            format!("{} morceau{} déplacé{} dans Production.", checked.len(), if checked.len() > 1 { "x" } else { "" }, if checked.len() > 1 { "s" } else { "" })
        }
        (Dlg::Attach { id, chosen, stem, .. }, "apply") => {
            let Some(parent) = chosen else {
                st.lib.dlg = dlg;
                app.set_message("Choisis le morceau principal dans la liste.".into());
                return;
            };
            let _ = journal::record(conn, "Rattachement", "", || files::attach(conn, *id, *parent, *stem));
            st.lib.details = Some(*id);
            format!("Rattaché comme {}.", if *stem { "piste séparée" } else { "version" })
        }
        (Dlg::Nature { found, checked }, "apply") => {
            let chosen: Vec<(i64, String, &str)> = found.iter().filter(|f| checked.contains(&f.0)).map(|f| (f.0, f.1.clone(), f.2)).collect();
            match journal::record(conn, "Remix, edits et mashups classés", "", || nature::apply_suggestions(conn, &chosen)) {
                Ok(n) => format!("Nature appliquée à {n} morceau(x)."),
                Err(e) => format!("Écriture impossible : {e}"),
            }
        }
        (Dlg::Sync { actions, checked }, "apply") => {
            let chosen: Vec<SyncAction> = checked.iter().filter_map(|&i| actions.get(i).cloned()).collect();
            match releases::sync_apply(conn, &chosen) {
                Ok(n) => format!("{n} morceau(x) rattaché(s) à leur sortie."),
                Err(e) => format!("Écriture impossible : {e}"),
            }
        }
        (Dlg::Genre { ids, moods, replace, write, .. }, "apply") => {
            let genre = pattern.trim().to_string();
            let chosen: Vec<&str> = moods.iter().copied().collect();
            let n = journal::record(conn, "Genre et ambiance", "", || lib::apply_genre_mood(conn, ids, (!genre.is_empty()).then_some(genre.as_str()), &chosen, *replace).unwrap_or(0));
            let mut errors = 0;
            if *write && !genre.is_empty() && !st.selftest {
                for r in files::file_rows(conn, ids) {
                    if files::write_tags(std::path::Path::new(&r.path), &TagValues { genre: Some(genre.clone()), ..Default::default() }).is_err() {
                        errors += 1;
                    }
                }
            }
            if errors > 0 {
                format!("Genre et ambiance appliqués à {n} morceau(x) ; {errors} fichier(s) n'ont pas pu être modifiés.")
            } else {
                format!("Genre et ambiance appliqués à {n} morceau(x).")
            }
        }
        (Dlg::Relocate { new, .. }, "apply") => {
            let old = pattern.trim().to_string();
            let p = folders::preview(conn, &old, &new.to_string_lossy());
            if !p.absent.is_empty() && !confirm(st, "Changer l'emplacement", &format!("{} fichier(s) ne sont pas au nouvel emplacement : ils seront signalés introuvables. Continuer ?", p.absent.len())) {
                st.lib.dlg = dlg;
                return;
            }
            // La base est sauvegardée juste avant (comme Studio Vault).
            let _ = otoko_library::career::backup::now(conn, &crate::donnees::data_dir().join("sauvegardes"), None);
            match folders::relocate(conn, &old, &new.to_string_lossy()) {
                Ok((changed, settings, replaced)) => {
                    let _ = otoko_library::career::scan::refresh_missing(conn);
                    format!(
                        "Emplacement changé : {changed} chemin(s){}{}. Le travail suit les morceaux.",
                        if settings > 0 { format!(", {settings} réglage(s)") } else { String::new() },
                        if replaced > 0 { format!(", {replaced} entrée(s) récente(s) remplacée(s)") } else { String::new() }
                    )
                }
                Err(e) => format!("Opération impossible : {e} Rien n'a été modifié."),
            }
        }
        (Dlg::Missing { list, checked, quiet }, "apply") => {
            for id in checked {
                let _ = folders::unwatch(conn, *id);
            }
            if *quiet {
                let paths: Vec<String> = list.iter().filter(|w| !checked.contains(&w.id)).map(|w| w.path.clone()).collect();
                let _ = folders::quiet_missing(conn, &paths);
            }
            format!("{} dossier(s) retiré(s) de la surveillance ; leurs morceaux restent dans la bibliothèque.", checked.len())
        }
        (Dlg::AddToRelease { ids, chosen, .. }, "apply") => {
            let title = pattern.trim().to_string();
            let target = match chosen {
                Some(rid) => releases::add_tracks(conn, *rid, ids).map(|_| *rid),
                None if !title.is_empty() => releases::create(conn, &title, ids),
                None => {
                    st.lib.dlg = dlg;
                    app.set_message("Choisis une sortie, ou tape le titre d'une nouvelle.".into());
                    return;
                }
            };
            st.lib.dlg = Dlg::None;
            push(app, st);
            crate::career::fill(app, st);
            match target {
                Ok(rid) => crate::sorties::open(app, st, rid),
                Err(e) => app.set_message(format!("Ajout impossible : {e}").into()),
            }
            return;
        }
        _ => {
            st.lib.dlg = dlg;
            return;
        }
    };
    st.lib.dlg = Dlg::None;
    push(app, st);
    app.set_message(msg.into());
    // Le dialogue a pu s'ouvrir depuis l'accueil ou les sorties : toute la Carrière est relue.
    crate::career::fill(app, st);
}

/// Les boutons des dialogues qui restent ouverts : dossiers surveillés, file de lecture.
fn list_button(app: &AppWindow, st: &mut State, id: &str) {
    if let Dlg::Queue { chosen } = &mut st.lib.dlg {
        let q = &mut st.player.queue;
        let k = chosen.filter(|k| *k < q.len());
        match (id, k) {
            ("play", Some(k)) => {
                let track = q.remove(k);
                st.lib.dlg = Dlg::Queue { chosen: None };
                crate::lecteur::play_id(app, st, track, 0.0);
            }
            ("up", Some(k)) if k > 0 => {
                q.swap(k, k - 1);
                *chosen = Some(k - 1);
            }
            ("down", Some(k)) if k + 1 < q.len() => {
                q.swap(k, k + 1);
                *chosen = Some(k + 1);
            }
            ("remove", Some(k)) => {
                q.remove(k);
                *chosen = None;
            }
            ("clear", _) => {
                q.clear();
                *chosen = None;
            }
            _ => app.set_message("Choisis d'abord un morceau de la file.".into()),
        }
        crate::lecteur::push(app, st);
        push(app, st);
        return;
    }
    let chosen = if let Dlg::Watch { chosen, .. } = &st.lib.dlg { *chosen } else { None };
    match (id, chosen) {
        ("add", _) => crate::bibliotheque::add_folder(app, st),
        ("rescan", _) => crate::bibliotheque::scan(app, st),
        ("enable", Some(w)) => {
            if let Some(c) = st.career.as_ref() {
                let on = folders::watch_list(c.conn()).iter().find(|x| x.id == w).is_some_and(|x| !x.enabled);
                let _ = folders::set_watch_enabled(c.conn(), w, on);
            }
        }
        ("remove", Some(w)) => {
            let path = st.career.as_ref().and_then(|c| folders::watch_list(c.conn()).into_iter().find(|x| x.id == w)).map(|x| x.path).unwrap_or_default();
            if !confirm(st, "Dossiers surveillés", &format!("Retirer « {path} » ?\n\nSes morceaux sortent de la bibliothèque (pas du disque) ; ceux rangés dans une sortie restent.")) {
                return;
            }
            if let Some(c) = st.career.as_ref() {
                let _ = otoko_library::career::scan::remove_folder(c.conn(), w);
            }
            crate::bibliotheque::fill(app, st);
        }
        _ => {
            app.set_message("Choisis d'abord un dossier dans la liste.".into());
            return;
        }
    }
    if let (Dlg::Watch { list, .. }, Some(c)) = (&mut st.lib.dlg, st.career.as_ref()) {
        *list = folders::watch_list(c.conn());
    }
    push(app, st);
}

/// Ce que Ctrl+Z défait : le plus récent du dernier déplacement de fichiers et de la dernière modification de fiches faite
/// sur ce PC.
pub enum Undoable {
    Files { batch: String, label: String, n: i64 },
    Edits(journal::Last),
}

pub fn next_undo(c: &otoko_library::career::Career) -> Option<Undoable> {
    let conn = c.conn();
    let files = files::last_batch(conn).map(|(batch, label, n)| {
        let at: String = conn.query_row("SELECT IFNULL(MAX(created_at), '') FROM file_ops WHERE batch_id = ?1", [&batch], |r| r.get(0)).unwrap_or_default();
        (at, Undoable::Files { batch, label, n })
    });
    let edits = journal::last(conn, journal::PC).map(|l| (l.at.clone(), Undoable::Edits(l)));
    match (files, edits) {
        (Some(f), Some(e)) => Some(if f.0 > e.0 { f.1 } else { e.1 }),
        (f, e) => f.or(e).map(|x| x.1),
    }
}

/// Le libellé du menu Annuler (vide : rien à annuler).
pub fn undo_label(c: &otoko_library::career::Career) -> String {
    let plural = |n: i64, one: &str, many: &str| format!("{n} {}", if n > 1 { many } else { one });
    match next_undo(c) {
        Some(Undoable::Files { label, n, .. }) => format!("{label} ({})", plural(n, "fichier", "fichiers")),
        Some(Undoable::Edits(l)) => format!("{} ({})", l.label, plural(l.tracks, "morceau", "morceaux")),
        None => String::new(),
    }
}

/// Annule le dernier renommage, la dernière réorganisation ou la dernière modification de fiches.
pub fn undo(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let Some(next) = next_undo(c) else {
        app.set_message("Rien à annuler.".into());
        return;
    };
    let msg = match next {
        // L'autotest ne déplace pas de vrais fichiers.
        Undoable::Files { .. } if st.selftest => return,
        Undoable::Files { batch, label, n } => {
            let (ok, err) = files::undo(c.conn(), &batch);
            format!("{label} annulé : {ok} fichier(s) sur {n} remis en place{}.", if err > 0 { format!(", {err} impossible(s)") } else { String::new() })
        }
        Undoable::Edits(l) => match journal::undo(c.conn(), &l.batch) {
            Ok((_, 0)) => format!("Annulé : {}.", l.label),
            Ok((_, kept)) => format!("Annulé : {}. {kept} champ(s) modifié(s) depuis ont été laissés tels quels.", l.label),
            Err(e) => format!("Annulation impossible : {e}"),
        },
    };
    app.set_message(msg.into());
    crate::bibliotheque::fill(app, st);
    crate::bibliotheque::details(app, st);
}

/// Écrit dans les fichiers sélectionnés ce que la Carrière sait d'eux.
pub fn write_tags(app: &AppWindow, st: &mut State) {
    let ids = selection(st);
    if ids.is_empty() {
        app.set_message("Sélectionne d'abord les morceaux.".into());
        return;
    }
    if !confirm(st, "Écrire les tags", &format!("Écrire titre, artiste, album, genre, année, n° de piste, tempo et tonalité dans {} fichier(s) ?\n\nLe son n'est pas modifié.", ids.len())) {
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    let (mut ok, mut err) = (0, 0);
    for r in files::file_rows(c.conn(), &ids) {
        let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
        let v = TagValues {
            title: opt(&r.title),
            artist: opt(&r.artist),
            album: opt(&r.album),
            genre: opt(&r.genre),
            year: r.year.map(|y| y.to_string()),
            track_no: r.track_no.map(|n| n.to_string()),
            bpm: r.bpm.map(|b| format!("{b:.0}")),
            key: opt(&r.key),
            comment: None,
        };
        match files::write_tags(std::path::Path::new(&r.path), &v) {
            Ok(()) => ok += 1,
            Err(_) => err += 1,
        }
    }
    app.set_message(format!("Tags écrits dans {ok} fichier(s){}.", if err > 0 { format!(", {err} impossible(s) (fichier ouvert ailleurs ou format non pris en charge)") } else { String::new() }).into());
    crate::bibliotheque::fill(app, st);
}

/// Retire la sélection de la bibliothèque (les fichiers ne bougent pas).
pub fn forget(app: &AppWindow, st: &mut State) {
    let ids = selection(st);
    if ids.is_empty() || !confirm(st, "Retirer de la bibliothèque", &format!("Retirer {} morceau(x) de la bibliothèque ?\n\nLes fichiers ne sont pas touchés ; un dossier surveillé les remettra s'ils y sont encore (sinon, « Archiver » les cache sans les retirer).", ids.len())) {
        return;
    }
    if let Some(c) = st.career.as_ref() {
        files::forget(c.conn(), &ids);
    }
    crate::bibliotheque::fill(app, st);
}

pub fn detach(app: &AppWindow, st: &mut State) {
    let Some(id) = st.lib.details.or_else(|| selection(st).first().copied()) else { return };
    if let Some(c) = st.career.as_ref() {
        let _ = journal::record(c.conn(), "Détachement", "", || files::detach(c.conn(), id));
    }
    app.set_message("Détaché : c'est de nouveau un morceau principal.".into());
    crate::bibliotheque::fill(app, st);
}

/// Met des fichiers à la corbeille de Windows (récupérables).
pub fn to_recycle_bin(paths: &[PathBuf]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[repr(C)]
    struct ShFileOp {
        hwnd: isize,
        func: u32,
        from: *const u16,
        to: *const u16,
        flags: u16,
        aborted: i32,
        mappings: *mut std::ffi::c_void,
        title: *const u16,
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHFileOperationW(op: *mut ShFileOp) -> i32;
    }
    if paths.is_empty() {
        return Ok(());
    }
    // Liste terminée par deux zéros.
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(p.as_os_str().encode_wide());
        from.push(0);
    }
    from.push(0);
    const FO_DELETE: u32 = 3;
    const FOF_SILENT: u16 = 0x4;
    const FOF_NOCONFIRMATION: u16 = 0x10;
    const FOF_ALLOWUNDO: u16 = 0x40;
    const FOF_NOERRORUI: u16 = 0x400;
    let mut op = ShFileOp {
        hwnd: 0,
        func: FO_DELETE,
        from: from.as_ptr(),
        to: std::ptr::null(),
        flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
        aborted: 0,
        mappings: std::ptr::null_mut(),
        title: std::ptr::null(),
    };
    // SAFETY : `from` vit jusqu'à la fin de l'appel ; la structure suit SHFILEOPSTRUCTW (x64).
    let r = unsafe { SHFileOperationW(&mut op) };
    if r == 0 && op.aborted == 0 { Ok(()) } else { Err(format!("code {r}")) }
}
