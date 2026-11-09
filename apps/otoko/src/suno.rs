//! Carrière › Atelier Suno (comme l'atelier de prompts Suno v6 de Studio Vault) : la bibliothèque
//! de prompts (dernier utilisé d'abord, copie, suppression, versions gardées) ; le champ Style en six
//! cases avec étiquettes à cliquer ; « à exclure » ; les réglages créatifs et leurs préréglages ;
//! les paroles et leurs tags ; l'aperçu, les compteurs et le vérificateur avec correction en un clic ;
//! l'import des fichiers rapportés de Suno (renommés, rangés, liés au prompt).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use otoko_library::career::suno::{self, SunoPrompt};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::state::State;
use crate::{AppWindow, ChipGroupUi, ImportFileUi, IssueUi, SPresetUi, SPromptUi, SlotUi, SunoUi};

macro_rules! suno_on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.global::<SunoUi>().$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

/// Un fichier à importer : (chemin, titre, version, tags, prompt).
#[derive(Clone, Default)]
pub struct ImportFile {
    pub path: PathBuf,
    pub title: String,
    pub version: String,
    pub tags: String,
    pub prompt: String,
}

#[derive(Default)]
pub struct Suno {
    pub prompt: SunoPrompt,
    pub list: Vec<SunoPrompt>,
    pub search: String,
    pub chip_tab: String,
    pub tag_tab: String,
    pub import: Vec<ImportFile>,
    pub import_at: usize,
    pub import_copy: bool,
    pub import_group: bool,
    pub import_ids: Vec<Option<i64>>,
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

fn strings(v: impl IntoIterator<Item = impl Into<SharedString>>) -> ModelRc<SharedString> {
    model(v.into_iter().map(Into::into).collect())
}

fn say(app: &AppWindow, msg: &str) {
    app.global::<SunoUi>().set_msg(msg.into());
    app.set_message(msg.into());
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    suno_on!(app, state, on_search, |app, st, q| {
        st.suno.search = q.to_string();
        push_list(&app, st);
    });
    suno_on!(app, state, on_load, |app, st, id| {
        if let Some(p) = st.career.as_ref().and_then(|c| suno::get(c.conn(), id as i64)) {
            st.suno.prompt = p;
            push_all(&app, st);
        }
    });
    suno_on!(app, state, on_new, |app, st| {
        st.suno.prompt = SunoPrompt { name: "Nouveau prompt".into(), ..Default::default() };
        push_all(&app, st);
    });
    suno_on!(app, state, on_go_production, |app, st| { crate::raccourcis::tab(&app, st, 6); });
    suno_on!(app, state, on_fork, |app, st| {
        let id = st.suno.prompt.id;
        if id == 0 {
            say(&app, "Enregistre d'abord le prompt pour en faire une copie.");
            return;
        }
        if let Some(new) = st.career.as_ref().and_then(|c| suno::fork(c.conn(), id, "").ok()).filter(|n| *n > 0) {
            st.suno.prompt = st.career.as_ref().and_then(|c| suno::get(c.conn(), new)).unwrap_or_default();
            push_all(&app, st);
            say(&app, "Copie créée : modifie-la sans toucher au prompt d'origine.");
        }
    });
    suno_on!(app, state, on_delete, |app, st| {
        let (id, name) = (st.suno.prompt.id, st.suno.prompt.name.clone());
        if id == 0 {
            return;
        }
        let yes = st.selftest
            || rfd::MessageDialog::new().set_title("Supprimer").set_description(format!("Supprimer le prompt « {name} » et ses versions ?")).set_buttons(rfd::MessageButtons::YesNo).show()
                == rfd::MessageDialogResult::Yes;
        if !yes {
            return;
        }
        if let Some(c) = st.career.as_ref() {
            let _ = suno::delete(c.conn(), id);
        }
        st.suno.prompt = SunoPrompt::default();
        push_all(&app, st);
    });
    suno_on!(app, state, on_edited, |app, st, key, value| {
        match key.as_str() {
            "duration" => st.suno.prompt.duration_target_s = value.trim().parse().unwrap_or(st.suno.prompt.duration_target_s).clamp(30, 480),
            k => st.suno.prompt.set(k, &value),
        }
        refresh(&app, st);
    });
    suno_on!(app, state, on_set_model, |_app, st, i| {
        if let Some(m) = suno::models(&st.suno.prompt.model).get(i.max(0) as usize) {
            st.suno.prompt.model = m.0.clone();
        }
    });
    suno_on!(app, state, on_set_chip_tab, |app, st, t| {
        st.suno.chip_tab = t.to_string();
        push_chips(&app, st);
    });
    suno_on!(app, state, on_chip, |app, st, slot, text| {
        // Ajoutée à la bonne case, sans doublon.
        let p = &mut st.suno.prompt;
        let current = p.slot(slot.as_str()).trim().to_string();
        let parts: Vec<&str> = current.split(',').map(str::trim).collect();
        if !parts.contains(&text.as_str()) {
            let v = if current.is_empty() { text.to_string() } else { format!("{current}, {text}") };
            p.set(slot.as_str(), &v);
        }
        push_slots(&app, st);
        refresh(&app, st);
    });
    suno_on!(app, state, on_set_slider, |app, st, key, v| {
        let v = v.clamp(0, 100) as i64;
        match key.as_str() {
            "weirdness" => st.suno.prompt.weirdness = v,
            "style_influence" => st.suno.prompt.style_influence = v,
            _ => {
                if st.suno.prompt.audio_influence.is_some() {
                    st.suno.prompt.audio_influence = Some(v);
                }
            }
        }
        let _ = app;
    });
    suno_on!(app, state, on_toggle_audio, |app, st| {
        let g = app.global::<SunoUi>();
        st.suno.prompt.audio_influence = if st.suno.prompt.audio_influence.is_some() { None } else { Some(g.get_audio_influence().round() as i64) };
        g.set_audio_on(st.suno.prompt.audio_influence.is_some());
    });
    suno_on!(app, state, on_preset, |app, st, k| {
        let Some(p) = suno::caps()["slider_presets"].as_array().and_then(|a| a.get(k.max(0) as usize)).cloned() else { return };
        st.suno.prompt.weirdness = p["weirdness"].as_i64().unwrap_or(40);
        st.suno.prompt.style_influence = p["style_influence"].as_i64().unwrap_or(70);
        st.suno.prompt.audio_influence = p["audio_influence"].as_i64();
        push_sliders(&app, st);
        say(&app, &format!("Préréglage « {} » : étrangeté {}, influence du style {}.", p["label"].as_str().unwrap_or(""), st.suno.prompt.weirdness, st.suno.prompt.style_influence));
    });
    suno_on!(app, state, on_set_tag_tab, |app, st, t| {
        st.suno.tag_tab = t.to_string();
        push_tags(&app, st);
    });
    suno_on!(app, state, on_tag, |app, st, tag| {
        // Ajouté sur une nouvelle ligne à la fin des paroles.
        let l = &mut st.suno.prompt.lyrics;
        if !l.is_empty() && !l.ends_with('\n') {
            l.push('\n');
        }
        l.push_str(tag.as_str());
        l.push('\n');
        app.global::<SunoUi>().set_lyrics(st.suno.prompt.lyrics.as_str().into());
        refresh(&app, st);
    });
    suno_on!(app, state, on_anchor, |app, st| {
        st.suno.prompt.lyrics = format!("{}{}", suno::VERSION_ANCHOR, st.suno.prompt.lyrics);
        app.global::<SunoUi>().set_lyrics(st.suno.prompt.lyrics.as_str().into());
        refresh(&app, st);
    });
    suno_on!(app, state, on_section_example, |app, st, k| {
        if let Some(e) = suno::caps_list("section_edit_examples").get(k.max(0) as usize) {
            st.suno.prompt.section_edit = e.clone();
            app.global::<SunoUi>().set_section_edit(e.as_str().into());
        }
    });
    suno_on!(app, state, on_fix, |app, st, id| {
        st.suno.prompt = suno::corriger(&st.suno.prompt, id.as_str());
        push_slots(&app, st);
        push_texts(&app, st);
        refresh(&app, st);
        say(&app, "Correction appliquée.");
    });
    suno_on!(app, state, on_fix_all, |app, st| {
        st.suno.prompt = suno::fix_all(&st.suno.prompt);
        push_slots(&app, st);
        push_texts(&app, st);
        refresh(&app, st);
        say(&app, "Corrections appliquées.");
    });
    suno_on!(app, state, on_copy, |app, st, what| {
        let p = &st.suno.prompt;
        let (text, msg) = match what.as_str() {
            "style" => (suno::assemble_style_field(p), "Champ Style copié."),
            "lyrics" => (p.lyrics.clone(), "Paroles copiées."),
            _ => (suno::copy_all(p), "Prompt complet copié."),
        };
        if p.id != 0 {
            if let Some(c) = st.career.as_ref() {
                let _ = suno::mark_used(c.conn(), p.id);
            }
        }
        if !st.selftest {
            crate::publish::copy(&text);
        }
        say(&app, msg);
    });
    suno_on!(app, state, on_open_suno, |app, st| {
        if !st.selftest {
            let _ = std::process::Command::new("explorer.exe").arg("https://suno.com/create").spawn();
        }
        let _ = app;
    });
    suno_on!(app, state, on_save, |app, st| { save(&app, st); });
    suno_on!(app, state, on_import_show, |app, st| { import_show(&app, st); });
    suno_on!(app, state, on_import_dest_pick, |app, st| {
        let mut dlg = rfd::FileDialog::new().set_title("Dossier de destination des imports Suno");
        let cur = st.career.as_ref().and_then(|c| c.setting("suno_import_folder")).unwrap_or_default();
        if !cur.is_empty() {
            dlg = dlg.set_directory(&cur);
        }
        if let Some(d) = dlg.pick_folder() {
            if let Some(c) = st.career.as_ref() {
                let _ = c.set_setting("suno_import_folder", &d.to_string_lossy());
            }
            app.global::<SunoUi>().set_import_dest(d.to_string_lossy().as_ref().into());
            import_push(&app, st);
        }
    });
    suno_on!(app, state, on_import_toggle, |app, st, k| {
        if k == "copy" {
            st.suno.import_copy = !st.suno.import_copy;
        } else {
            st.suno.import_group = !st.suno.import_group;
        }
        import_push(&app, st);
    });
    suno_on!(app, state, on_import_select, |app, st, i| {
        import_store(&app, st);
        st.suno.import_at = i.max(0) as usize;
        import_fields(&app, st);
        import_push(&app, st);
    });
    suno_on!(app, state, on_import_edited, |app, st| {
        import_store(&app, st);
        import_preview(&app, st);
    });
    suno_on!(app, state, on_import_apply_all, |app, st| {
        import_store(&app, st);
        let Some(cur) = st.suno.import.get(st.suno.import_at).cloned() else { return };
        for f in st.suno.import.iter_mut() {
            f.prompt = cur.prompt.clone();
            if !cur.tags.trim().is_empty() {
                let mut merged: Vec<String> = f.tags.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
                for t in cur.tags.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                    if !merged.iter().any(|m| m == t) {
                        merged.push(t.to_string());
                    }
                }
                f.tags = merged.join(", ");
            }
        }
        say(&app, &format!("Prompt et tags appliqués à {} fichier(s).", st.suno.import.len()));
    });
    suno_on!(app, state, on_import_run, |app, st| { import_run(&app, st); });
    suno_on!(app, state, on_import_close, |app, st| {
        st.suno.import.clear();
        app.global::<SunoUi>().set_import_open(false);
    });
}

/// L'onglet : la bibliothèque, puis le prompt ouvert.
pub fn fill(app: &AppWindow, st: &mut State) {
    let g = app.global::<SunoUi>();
    g.set_presets(model(
        suno::caps()["slider_presets"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| SPresetUi { label: p["label"].as_str().unwrap_or("").into(), note: p["note"].as_str().unwrap_or("").into() })
            .collect(),
    ));
    g.set_section_examples(strings(suno::caps_list("section_edit_examples").into_iter().map(|e| if e.chars().count() > 38 { format!("{}…", e.chars().take(38).collect::<String>()) } else { e })));
    if st.suno.chip_tab.is_empty() {
        st.suno.chip_tab = "genres".into();
        st.suno.tag_tab = "structure".into();
    }
    push_all(app, st);
}

fn push_all(app: &AppWindow, st: &mut State) {
    push_list(app, st);
    push_slots(app, st);
    push_texts(app, st);
    push_sliders(app, st);
    push_chips(app, st);
    push_tags(app, st);
    refresh(app, st);
}

fn push_list(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    st.suno.list = suno::list(c.conn(), &st.suno.search);
    let cur = st.suno.prompt.id;
    let rows: Vec<SPromptUi> = st
        .suno
        .list
        .iter()
        .map(|p| {
            let (total, keep, ..) = suno::stats(c.conn(), p.id);
            let mut sub = vec![suno::assemble_style_field(p)];
            if p.use_count > 0 {
                sub.push(format!("servi {} fois", p.use_count));
            }
            if total > 0 {
                sub.push(format!("{total} morceau(x), {keep} gardé(s)"));
            }
            SPromptUi { id: p.id as i32, name: p.name.as_str().into(), sub: sub.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ").into(), current: p.id == cur && cur != 0 }
        })
        .collect();
    let g = app.global::<SunoUi>();
    g.set_lib_text(
        if cur != 0 {
            let v = suno::versions(c.conn(), cur).len();
            format!("{} prompt(s) · celui-ci : {v} version(s) gardée(s)", st.suno.list.len())
        } else {
            format!("{} prompt(s) — nouveau prompt, pas encore enregistré", st.suno.list.len())
        }
        .into(),
    );
    g.set_prompts(model(rows));
}

fn push_slots(app: &AppWindow, st: &State) {
    let p = &st.suno.prompt;
    app.global::<SunoUi>().set_slots(model(
        suno::slots().into_iter().map(|(k, l, ph, h)| SlotUi { value: p.slot(&k).into(), key: k.into(), label: l.into(), placeholder: ph.into(), hint: h.into() }).collect(),
    ));
}

fn push_texts(app: &AppWindow, st: &State) {
    let p = &st.suno.prompt;
    let g = app.global::<SunoUi>();
    g.set_name(p.name.as_str().into());
    g.set_custom_model(p.custom_model.as_str().into());
    g.set_voice(p.voice.as_str().into());
    g.set_duration(p.duration_target_s.to_string().into());
    g.set_hook(p.hook_instrument.as_str().into());
    g.set_exclude(p.exclude_field.as_str().into());
    g.set_lyrics(p.lyrics.as_str().into());
    g.set_section_edit(p.section_edit.as_str().into());
    g.set_notes(p.notes.as_str().into());
    let models = suno::models(&p.model);
    g.set_model_labels(strings(models.iter().map(|m| m.1.clone())));
    g.set_model_index(models.iter().position(|m| m.0 == p.model).unwrap_or(0) as i32);
}

fn push_sliders(app: &AppWindow, st: &State) {
    let p = &st.suno.prompt;
    let g = app.global::<SunoUi>();
    g.set_weirdness(p.weirdness as f32);
    g.set_style_influence(p.style_influence as f32);
    g.set_audio_on(p.audio_influence.is_some());
    g.set_audio_influence(p.audio_influence.unwrap_or(60) as f32);
}

fn push_chips(app: &AppWindow, st: &State) {
    let groups: Vec<ChipGroupUi> = match st.suno.chip_tab.as_str() {
        "moods" => vec![ChipGroupUi { title: "".into(), slot: "mood".into(), chips: strings(suno::caps_list("mood_seeds")) }],
        "production" => vec![ChipGroupUi { title: "".into(), slot: "production".into(), chips: strings(suno::caps_list("production_styles")) }],
        _ => suno::genre_seeds().into_iter().map(|(fam, g)| ChipGroupUi { title: fam.to_uppercase().into(), slot: "genre".into(), chips: strings(g) }).collect(),
    };
    let g = app.global::<SunoUi>();
    g.set_chip_tab(st.suno.chip_tab.as_str().into());
    g.set_chip_groups(model(groups));
}

fn push_tags(app: &AppWindow, st: &State) {
    let key = match st.suno.tag_tab.as_str() {
        "vocal" => "vocal_delivery_tags",
        "instrumental" => "instrumental_tags",
        "dynamic" => "dynamic_tags",
        "examples" => "parameterized_tag_examples",
        _ => "structural_tags",
    };
    let g = app.global::<SunoUi>();
    g.set_tag_tab(st.suno.tag_tab.as_str().into());
    g.set_tags(strings(suno::caps_list(key)));
}

/// L'aperçu, les compteurs et les vérifications.
fn refresh(app: &AppWindow, st: &State) {
    let p = &st.suno.prompt;
    let style = suno::assemble_style_field(p);
    let n = suno::count_descriptors(&style);
    let (tmin, tmax) = suno::descriptors_target();
    let (cap, cap_l) = (suno::style_max(), suno::lyrics_max());
    let (chars, lchars) = (style.chars().count(), p.lyrics.chars().count());
    let g = app.global::<SunoUi>();
    g.set_style(style.as_str().into());
    g.set_counters(format!("{n} descripteurs (cible {tmin}-{tmax}) · {chars}/{cap} caractères · paroles {lchars}/{cap_l}").into());
    g.set_counters_ok((tmin..=tmax).contains(&n) && chars <= cap && lchars <= cap_l);
    let secs = suno::sections(&p.lyrics);
    g.set_lyrics_status(
        if secs.is_empty() {
            "Aucune section détectée".to_string()
        } else {
            format!("{} section(s) : {}{}", secs.len(), secs.iter().take(8).cloned().collect::<Vec<_>>().join(", "), if secs.len() > 8 { format!(" (+{})", secs.len() - 8) } else { String::new() })
        }
        .into(),
    );
    let issues = suno::verifier(p);
    g.set_any_fixable(issues.iter().any(|i| i.fixable));
    g.set_issues(model(issues.into_iter().map(|i| IssueUi { id: i.id.into(), message: i.message.into(), fixable: i.fixable }).collect()));
}

fn save(app: &AppWindow, st: &mut State) {
    if st.suno.prompt.name.trim().is_empty() {
        say(app, "Donne un nom au prompt pour l'enregistrer dans la bibliothèque.");
        return;
    }
    let Some(c) = st.career.as_ref() else { return };
    match suno::save(c.conn(), &st.suno.prompt) {
        Ok(id) => {
            let _ = suno::mark_used(c.conn(), id);
            st.suno.prompt.id = id;
            push_list(app, st);
            say(app, &format!("Prompt « {} » enregistré dans la bibliothèque (une version de plus).", st.suno.prompt.name));
        }
        Err(e) => say(app, &format!("Prompt non enregistré : {e}")),
    }
}

// ---------------------------------------------------------------------- import de fichiers Suno

pub fn import_show(app: &AppWindow, st: &mut State) {
    let Some(files) = rfd::FileDialog::new().set_title("Fichiers téléchargés depuis Suno").add_filter("Audio", &["mp3", "wav", "flac", "m4a", "ogg", "aiff"]).pick_files() else { return };
    import_open(app, st, files);
}

/// Ouvre l'import sur ces fichiers (aussi utilisé par l'autotest, sans boîte de dialogue).
pub fn import_open(app: &AppWindow, st: &mut State, files: Vec<PathBuf>) {
    st.suno.import = files
        .into_iter()
        .filter(|f| otoko_library::career::scan::is_audio(f))
        .map(|f| {
            let t = otoko_library::career::scan::read_tags(&f);
            let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            ImportFile { title: t.title.filter(|x| !x.is_empty()).unwrap_or(stem), version: "v1".into(), path: f, ..Default::default() }
        })
        .collect();
    if st.suno.import.is_empty() {
        say(app, "Aucun fichier audio à importer.");
        return;
    }
    st.suno.import_at = 0;
    st.suno.import_group = true;
    let list = st.career.as_ref().map(|c| suno::list(c.conn(), "")).unwrap_or_default();
    st.suno.import_ids = std::iter::once(None).chain(list.iter().map(|p| Some(p.id))).collect();
    let g = app.global::<SunoUi>();
    g.set_import_prompts(strings(std::iter::once("— Aucun (juste le tag « suno ») —".to_string()).chain(list.iter().map(|p| p.name.clone()))));
    g.set_import_prompt(st.suno.import_ids.iter().position(|i| *i == Some(st.suno.prompt.id)).unwrap_or(0) as i32);
    g.set_import_dest(st.career.as_ref().and_then(|c| c.setting("suno_import_folder")).unwrap_or_default().into());
    import_fields(app, st);
    import_push(app, st);
    g.set_import_open(true);
}

fn import_store(app: &AppWindow, st: &mut State) {
    let g = app.global::<SunoUi>();
    if let Some(f) = st.suno.import.get_mut(st.suno.import_at) {
        f.title = g.get_imp_title().trim().to_string();
        f.version = g.get_imp_version().trim().to_string();
        f.tags = g.get_imp_tags().trim().to_string();
        f.prompt = g.get_imp_prompt().trim().to_string();
    }
}

fn import_fields(app: &AppWindow, st: &State) {
    let Some(f) = st.suno.import.get(st.suno.import_at) else { return };
    let g = app.global::<SunoUi>();
    g.set_imp_title(f.title.as_str().into());
    g.set_imp_version(f.version.as_str().into());
    g.set_imp_tags(f.tags.as_str().into());
    g.set_imp_prompt(f.prompt.as_str().into());
    import_preview(app, st);
}

fn import_preview(app: &AppWindow, st: &State) {
    let g = app.global::<SunoUi>();
    let Some(f) = st.suno.import.get(st.suno.import_at) else { return };
    let ext = f.path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let title = g.get_imp_title().to_string();
    let name = suno::import_name(if title.trim().is_empty() { "Sans titre" } else { &title }, g.get_imp_version().as_str(), &ext);
    let d = g.get_import_dest().to_string();
    let dest = if d.is_empty() { name } else { PathBuf::from(d).join(name).display().to_string() };
    g.set_imp_preview(format!("→ {dest}").into());
}

fn import_push(app: &AppWindow, st: &State) {
    let g = app.global::<SunoUi>();
    g.set_import_copy(st.suno.import_copy);
    g.set_import_group(st.suno.import_group);
    g.set_import_files(model(
        st.suno
            .import
            .iter()
            .enumerate()
            .map(|(i, f)| ImportFileUi { name: f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().into(), current: i == st.suno.import_at })
            .collect(),
    ));
    import_preview(app, st);
}

/// Range les fichiers (renommés « Titre - version »), les indexe dans la Carrière, les lie au prompt.
fn import_run(app: &AppWindow, st: &mut State) {
    import_store(app, st);
    let g = app.global::<SunoUi>();
    let dest = PathBuf::from(g.get_import_dest().to_string());
    if dest.as_os_str().is_empty() {
        say(app, "Choisis d'abord un dossier de destination.");
        return;
    }
    if std::fs::create_dir_all(&dest).is_err() {
        say(app, &format!("Dossier impossible à créer : {}", dest.display()));
        return;
    }
    let prompt_id = st.suno.import_ids.get(g.get_import_prompt().max(0) as usize).copied().flatten();
    let group = st.suno.import_group.then(suno::generation_group);
    let Some(c) = st.career.as_ref() else { return };
    let (mut ok, mut errors) = (0, Vec::new());
    for f in st.suno.import.clone() {
        let ext = f.path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let title = if f.title.is_empty() { f.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { f.title.clone() };
        let name = suno::import_name(&title, &f.version, "");
        let mut dst = dest.join(format!("{name}{ext}"));
        let mut n = 2;
        while dst.exists() && dst != f.path {
            dst = dest.join(format!("{name} ({n}){ext}"));
            n += 1;
        }
        let moved = if dst == f.path {
            Ok(())
        } else if st.suno.import_copy {
            std::fs::copy(&f.path, &dst).map(|_| ())
        } else {
            std::fs::rename(&f.path, &dst).or_else(|_| std::fs::copy(&f.path, &dst).and_then(|_| std::fs::remove_file(&f.path)))
        };
        if let Err(e) = moved {
            errors.push(format!("{} : {e}", f.path.display()));
            continue;
        }
        let ignored = otoko_library::career::scan::ignored_folders(c.conn());
        match otoko_library::career::scan::upsert(c.conn(), &dst, &ignored) {
            Ok((_, tid)) => {
                let _ = otoko_library::career::library::update(c.conn(), tid, "title", &title);
                match suno::tag_import(c.conn(), tid, &f.prompt, &f.version, prompt_id, group.as_deref(), &f.tags) {
                    Ok(()) => ok += 1,
                    Err(e) => errors.push(format!("{} : {e}", dst.display())),
                }
            }
            Err(e) => errors.push(format!("{} : {e}", dst.display())),
        }
    }
    let total = st.suno.import.len();
    st.suno.import.clear();
    g.set_import_open(false);
    say(
        app,
        &if errors.is_empty() { format!("{ok} fichier(s) importé(s) depuis Suno.") } else { format!("{ok}/{total} importé(s). Erreurs : {}", errors.iter().take(3).cloned().collect::<Vec<_>>().join(" ; ")) },
    );
    crate::career::fill(app, st);
}
