//! Carrière › Publier (comme l'assistant de publication de Studio Vault) : les textes pour
//! SoundCloud (description, genre et tags), Instagram et les stories, écrits par le moteur de
//! Studio Vault (`career::textes`, six tons, autre variante, sans répéter les phrases récentes),
//! l'historique des textes, l'univers de marque ; et, propre à OTOKO, le contrôle du fichier avant
//! de l'envoyer (sonie, crête, écrêtage, silences, mono, format), l'envoi préparé, puis le morceau
//! marqué « sorti » avec son lien. Rien ne part sur Internet : les textes se copient.

use std::cell::RefCell;
use std::rc::Rc;

use otoko_library::career::mes_textes::{self, Mine};
use otoko_library::career::{Song, textes, textes_fr};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::state::{Done, State};
use crate::{AppWindow, BrandFieldUi, HistUi, MineUi, Pub, PubCheckUi, PubTrackUi};

/// Une ligne du contrôle : 0 bon, 1 à voir, 2 à corriger.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub level: i32,
    pub text: String,
}

fn check(level: i32, text: impl Into<String>) -> Check {
    Check { level, text: text.into() }
}

/// Crête entre les échantillons, estimée par interpolation (Catmull-Rom) à 4 points par intervalle.
pub fn true_peak(x: &[f32]) -> f32 {
    let mut peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    for w in x.windows(4) {
        let (p0, p1, p2, p3) = (w[0], w[1], w[2], w[3]);
        // Seulement là où ça frôle déjà le plafond : le reste ne peut pas dépasser.
        if p1.abs().max(p2.abs()) < peak * 0.85 {
            continue;
        }
        for t in [0.25f32, 0.5, 0.75] {
            let v = 0.5 * (2.0 * p1 + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t);
            peak = peak.max(v.abs());
        }
    }
    peak
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-9).log10()
}

/// Contrôle d'un fichier audio, tel qu'une plateforme le recevra.
pub fn analyze(path: &str, song: &Song) -> Result<Vec<Check>, String> {
    let (secs, rate, channels) = crate::samples::probe(path).ok_or("fichier introuvable ou illisible")?;
    let data = crate::samples::load(path, rate as f64, None)?;
    let (l, r) = (&data.left, &data.right);
    let mut out = Vec::new();
    let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
    let lossless = matches!(ext.as_str(), "WAV" | "FLAC" | "AIFF" | "AIF");
    out.push(if !lossless {
        check(1, format!("Fichier compressé ({ext}) : les plateformes le recompressent ; envoie plutôt le WAV ou le FLAC."))
    } else if rate < 44_100 {
        check(2, format!("{ext} à {:.1} kHz : en dessous de 44,1 kHz, les aigus sont coupés.", rate as f64 / 1000.0))
    } else {
        check(0, format!("{ext} sans perte, {:.1} kHz, {}.", rate as f64 / 1000.0, if channels >= 2 { "stéréo" } else { "mono" }))
    });
    if secs < 30.0 {
        out.push(check(1, format!("{secs:.0} s : trop court pour compter comme une écoute sur la plupart des plateformes.")));
    }
    let mut meter = crate::loudness::Meter::new(rate as f64);
    meter.add(l, r);
    let lufs = meter.integrated();
    let (to_spotify, sc) = (lufs + 14.0, lufs);
    out.push(if lufs > -7.0 {
        check(1, format!("{lufs:.1} LUFS : très fort. Spotify et YouTube le baisseront de {to_spotify:.0} dB, et la dynamique perdue ne reviendra pas."))
    } else if lufs >= -11.0 {
        check(0, format!("{lufs:.1} LUFS : niveau club. Spotify et YouTube le baisseront de {to_spotify:.0} dB ; SoundCloud le jouera tel quel."))
    } else if lufs >= -16.0 {
        check(0, format!("{lufs:.1} LUFS : niveau streaming, joué tel quel partout."))
    } else {
        check(1, format!("{sc:.1} LUFS : faible. Il sonnera plus bas que les autres sur SoundCloud ; Masteriser le remonte."))
    });
    let tp = db(true_peak(l).max(true_peak(r)) as f64);
    out.push(if tp > -0.1 {
        check(2, format!("Crête réelle {tp:+.1} dBTP : il saturera après la conversion en MP3 ou AAC. Vise -1 dBTP (limiteur du master)."))
    } else if tp > -1.0 {
        check(1, format!("Crête réelle {tp:+.1} dBTP : un peu juste, -1 dBTP est plus sûr pour l'encodage."))
    } else {
        check(0, format!("Crête réelle {tp:+.1} dBTP : de la marge pour l'encodage."))
    });
    // Écrêtage : trois échantillons de suite au plafond.
    let clipped = [l, r]
        .iter()
        .map(|x| {
            let (mut runs, mut run) = (0, 0);
            for v in x.iter() {
                if v.abs() >= 0.999 {
                    run += 1;
                    if run == 3 {
                        runs += 1;
                    }
                } else {
                    run = 0;
                }
            }
            runs
        })
        .max()
        .unwrap_or(0);
    if clipped > 0 {
        out.push(check(if clipped > 20 { 2 } else { 1 }, format!("{clipped} passage{} écrêté{} : le son tape le plafond.", if clipped > 1 { "s" } else { "" }, if clipped > 1 { "s" } else { "" })));
    }
    let loud = |i: &usize| l[*i].abs().max(r[*i].abs()) > 0.001;
    let n = l.len().min(r.len());
    let head = (0..n).find(loud).unwrap_or(n) as f64 / rate as f64;
    let tail = (n - (0..n).rev().find(loud).map(|i| i + 1).unwrap_or(0)) as f64 / rate as f64;
    if head > 1.0 {
        out.push(check(1, format!("{head:.1} s de silence au début : l'auditeur croit que rien ne joue.")));
    }
    if tail > 4.0 {
        out.push(check(1, format!("{tail:.1} s de silence à la fin.")));
    }
    if head <= 1.0 && tail <= 4.0 {
        out.push(check(0, "Début et fin nets."));
    }
    // Compatibilité mono : un son en opposition de phase se creuse sur un téléphone.
    let (mut lr, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
    for i in (0..n).step_by(4) {
        let (a, b) = (l[i] as f64, r[i] as f64);
        (lr, ll, rr) = (lr + a * b, ll + a * a, rr + b * b);
    }
    let corr = lr / (ll * rr).sqrt().max(1e-12);
    if channels >= 2 {
        out.push(if corr < 0.0 {
            check(2, format!("Corrélation {corr:+.2} : en mono (téléphone, enceinte de club), une partie du son disparaît."))
        } else if corr < 0.3 {
            check(1, format!("Corrélation {corr:+.2} : très large ; vérifie qu'il tient en mono."))
        } else {
            check(0, format!("Corrélation {corr:+.2} : il tient en mono."))
        });
    }
    if song.genre.trim().is_empty() {
        out.push(check(1, "Pas de genre dans la fiche : il sert aux textes et aux tags des plateformes."));
    }
    Ok(out)
}

/// Copie un texte dans le presse-papiers de Windows (UTF-16, comme `clip` l'attend).
pub fn copy(text: &str) -> bool {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    let Ok(mut child) = std::process::Command::new("clip").stdin(std::process::Stdio::piped()).creation_flags(0x0800_0000).spawn() else { return false };
    let bytes: Vec<u8> = [0xFFFEu16].into_iter().chain(text.encode_utf16()).flat_map(|u| u.to_le_bytes()).collect();
    let ok = child.stdin.take().is_some_and(|mut s| s.write_all(&bytes).is_ok());
    child.wait().is_ok_and(|s| s.success()) && ok
}

// ---------------------------------------------------------------------- l'assistant de publication

macro_rules! pub_on {
    ($app:expr, $state:expr, $setter:ident, |$a:ident, $st:ident $(, $arg:ident)*| $body:block) => {{
        let weak = $app.as_weak();
        let state = $state.clone();
        $app.global::<Pub>().$setter(move |$($arg),*| {
            let $a = weak.unwrap();
            let Ok(mut guard) = state.try_borrow_mut() else { return };
            let $st: &mut State = &mut guard;
            $body
        });
    }};
}

/// L'état de l'onglet (le morceau choisi est `State::career_pick`).
#[derive(Default)]
pub struct View {
    pub platform: String,
    pub tone: String,
    pub variant: u32,
    pub filter: String,
    pub last: textes::Generated,
    pub history: Vec<textes::HistoryRow>,
    pub hist_one: bool,
    pub hist_sel: Option<usize>,
    /// Le modèle utilisé pour écrire (0 : le moteur).
    pub model: i64,
    /// Le texte ouvert dans « Mes textes ».
    pub mine: Option<Mine>,
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

fn say(app: &AppWindow, msg: &str) {
    app.global::<Pub>().set_msg(msg.into());
    app.set_message(msg.into());
}

pub fn wire(app: &AppWindow, state: &Rc<RefCell<State>>) {
    pub_on!(app, state, on_select, |app, st, id| {
        st.career_pick = Some(id as i64);
        st.publ.variant = 0;
        fill(&app, st);
    });
    pub_on!(app, state, on_filter, |app, st, q| {
        st.publ.filter = q.to_string();
        push_tracks(&app, st);
    });
    pub_on!(app, state, on_set_tab, |app, st, t| {
        app.global::<Pub>().set_tab(t.clone());
        if t == "history" {
            push_history(&app, st);
        }
        if t == "brand" {
            push_brand(&app, st);
        }
        if t == "mine" {
            push_mine(&app, st);
        }
    });
    pub_on!(app, state, on_set_model, |app, st, id| {
        st.publ.model = id as i64;
        st.publ.variant = 0;
        generate(&app, st);
    });
    pub_on!(app, state, on_keep_model, |app, st| { keep_model(&app, st); });
    pub_on!(app, state, on_mine_new, |app, st, kind| {
        let name = if kind == "modele" { "Nouveau modèle" } else { "" };
        new_mine(&app, st, Mine { kind: kind.to_string(), name: name.into(), ..Default::default() });
    });
    pub_on!(app, state, on_mine_select, |app, st, id| {
        st.publ.mine = st.career.as_ref().and_then(|c| mes_textes::list(c.conn()).into_iter().find(|m| m.id == id as i64));
        push_mine(&app, st);
    });
    pub_on!(app, state, on_mine_edited, |app, st| {
        let g = app.global::<Pub>();
        let (name, body) = (g.get_mine_name().to_string(), g.get_mine_body().to_string());
        change_mine(&app, st, |m| {
            m.name = name;
            m.body = body;
        });
    });
    pub_on!(app, state, on_mine_set_kind, |app, st, k| { change_mine(&app, st, |m| m.kind = k.to_string()); });
    pub_on!(app, state, on_mine_set_platform, |app, st, p| { change_mine(&app, st, |m| m.platform = p.to_string()); });
    pub_on!(app, state, on_mine_insert, |app, st, f| {
        let g = app.global::<Pub>();
        g.set_mine_body(format!("{}{f}", g.get_mine_body()).into());
        let body = g.get_mine_body().to_string();
        change_mine(&app, st, |m| m.body = body);
    });
    pub_on!(app, state, on_mine_delete, |app, st| {
        let Some(id) = st.publ.mine.as_ref().map(|m| m.id) else { return };
        if let Some(c) = st.career.as_ref() {
            let _ = mes_textes::delete(c.conn(), id);
        }
        if st.publ.model == id {
            st.publ.model = 0;
        }
        st.publ.mine = None;
        push_mine(&app, st);
    });
    pub_on!(app, state, on_go_releases, |app, st| { crate::sorties::gallery(&app, st); });
    pub_on!(app, state, on_set_platform, |app, st, p| {
        st.publ.platform = p.to_string();
        generate(&app, st);
    });
    pub_on!(app, state, on_set_tone, |app, st, t| {
        st.publ.tone = t.to_string();
        generate(&app, st);
    });
    pub_on!(app, state, on_variant, |app, st| {
        st.publ.variant += 1;
        generate(&app, st);
    });
    pub_on!(app, state, on_copy_text, |app, st| {
        let text = app.global::<Pub>().get_text().to_string();
        let ok = !st.selftest && copy(&text);
        say(&app, if ok { "Texte copié : colle-le avec Ctrl+V." } else { "Texte prêt." });
    });
    pub_on!(app, state, on_copy_genre, |app, st| {
        if !st.selftest {
            copy(app.global::<Pub>().get_sc_genre().as_str());
        }
        say(&app, "Genre copié.");
    });
    pub_on!(app, state, on_copy_tags, |app, st| {
        if !st.selftest {
            copy(app.global::<Pub>().get_sc_tags().as_str());
        }
        say(&app, "Tags copiés.");
    });
    pub_on!(app, state, on_save, |app, st, posted| {
        save(&app, st, posted);
    });
    pub_on!(app, state, on_send, |app, st| { send(&app, st); });
    pub_on!(app, state, on_check, |app, st| { run_check(&app, st); });
    pub_on!(app, state, on_master, |app, st| {
        if let Some(id) = st.career_pick {
            crate::mastrage::add_songs(&app, st, &[id]);
        }
    });
    pub_on!(app, state, on_published, |app, st| { published(&app, st); });
    pub_on!(app, state, on_hist_scope, |app, st, one| {
        st.publ.hist_one = one;
        push_history(&app, st);
    });
    pub_on!(app, state, on_hist_select, |app, st, i| {
        st.publ.hist_sel = Some(i.max(0) as usize);
        push_history_text(&app, st);
    });
    pub_on!(app, state, on_hist_copy, |app, st| {
        let text = app.global::<Pub>().get_hist_text().to_string();
        if !text.is_empty() && !st.selftest {
            copy(&text);
        }
        say(&app, "Texte copié.");
    });
    pub_on!(app, state, on_hist_posted, |app, st| {
        let Some(d) = st.publ.hist_sel.and_then(|i| st.publ.history.get(i)).map(|d| d.id) else { return };
        if let Some(c) = st.career.as_ref() {
            let _ = textes::mark_posted(c.conn(), d);
        }
        push_history(&app, st);
    });
    pub_on!(app, state, on_hist_delete, |app, st| {
        let Some(d) = st.publ.hist_sel.and_then(|i| st.publ.history.get(i)).map(|d| d.id) else { return };
        let yes = st.selftest
            || rfd::MessageDialog::new().set_title("Supprimer").set_description("Supprimer ce texte de l'historique ?").set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes;
        if !yes {
            return;
        }
        if let Some(c) = st.career.as_ref() {
            let _ = textes::delete_text(c.conn(), d);
        }
        st.publ.hist_sel = None;
        push_history(&app, st);
    });
    pub_on!(app, state, on_brand_edited, |app, st, key, value| {
        let Some(c) = st.career.as_ref() else { return };
        let mut b = textes::brand_load(c.conn());
        b.set(key.as_str(), value.trim());
        let _ = textes::brand_save(c.conn(), &b);
        let _ = app;
    });
    pub_on!(app, state, on_brand_tone_set, |app, st, t| {
        if let Some(c) = st.career.as_ref() {
            let mut b = textes::brand_load(c.conn());
            b.default_tone = t.to_string();
            let _ = textes::brand_save(c.conn(), &b);
        }
        app.global::<Pub>().set_brand_tone(t);
    });
    pub_on!(app, state, on_brand_save, |app, st| {
        say(&app, "Univers de marque enregistré.");
        generate(&app, st);
    });
}

/// L'onglet Publier : les morceaux, puis le texte du morceau choisi.
pub fn fill(app: &AppWindow, st: &mut State) {
    let g = app.global::<Pub>();
    g.set_platforms(model(textes::PLATEFORMES.iter().map(|p| SharedString::from(p.0)).collect()));
    g.set_platform_labels(model(textes::PLATEFORMES.iter().map(|p| SharedString::from(p.1)).collect()));
    g.set_tones(model(textes_fr::TONS.iter().map(|t| SharedString::from(t.0)).collect()));
    g.set_tone_labels(model(textes_fr::TONS.iter().map(|t| SharedString::from(t.1)).collect()));
    let Some(c) = st.career.as_ref() else { return };
    let brand = textes::brand_load(c.conn());
    if st.publ.platform.is_empty() {
        st.publ.platform = "soundcloud".into();
    }
    if st.publ.tone.is_empty() {
        st.publ.tone = if brand.default_tone.is_empty() { "mysterieux".into() } else { brand.default_tone.clone() };
    }
    push_tracks(app, st);
    let Some(s) = current(st) else {
        g.set_current(-1);
        g.set_title("".into());
        g.set_meta("".into());
        g.set_text("".into());
        g.set_info("".into());
        return;
    };
    let release = st.career.as_ref().and_then(|c| c.release_of(s.id).ok().flatten());
    let mut meta = vec![crate::career::status_label(&s.status).to_string()];
    if let Some(r) = &release {
        meta.push(format!("{} « {} »", otoko_library::career::release_label(&r.1), r.0));
    }
    if !s.published.is_empty() {
        meta.push(format!("publié le {}", crate::career::fr_date(&s.published)));
    }
    if s.path.is_empty() || !std::path::Path::new(&s.path).exists() {
        meta.push("fichier introuvable".into());
    }
    g.set_current(s.id as i32);
    g.set_title(s.title.as_str().into());
    g.set_meta(meta.join(" · ").into());
    g.set_link(s.link.as_str().into());
    g.set_checks(model(Vec::new()));
    g.set_check_text("Le contrôle lit le fichier comme une plateforme le recevra.".into());
    generate(app, st);
    if g.get_tab() == "history" {
        push_history(app, st);
    }
    if g.get_tab() == "mine" {
        push_mine(app, st);
    }
    if brand.artist_name.is_empty() && brand.sc_url.is_empty() && brand.city.is_empty() {
        g.set_msg("Commence par remplir ton univers de marque : le moteur s'en sert pour écrire.".into());
    }
}

fn push_tracks(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let q = st.publ.filter.trim().to_lowercase();
    let all = textes::publishable(c.conn());
    if st.career_pick.is_none() {
        st.career_pick = all.first().map(|t| t.0);
    }
    let rows: Vec<PubTrackUi> = all.into_iter().filter(|t| q.is_empty() || t.1.to_lowercase().contains(&q)).map(|t| PubTrackUi { id: t.0 as i32, label: t.1.into() }).collect();
    app.global::<Pub>().set_tracks(model(rows));
}

/// Écrit le texte du morceau choisi (plateforme, ton, variante).
fn generate(app: &AppWindow, st: &mut State) {
    let (Some(c), Some(id)) = (st.career.as_ref(), st.career_pick) else { return };
    let Some(tr) = textes::load_track(c.conn(), id) else { return };
    let brand = textes::brand_load(c.conn());
    let recents = textes::cles_recentes(c.conn(), 12);
    let mine = mes_textes::phrases(c.conn(), &st.publ.platform);
    // Les modèles de cette plateforme ; celui choisi peut ne plus en faire partie.
    let models = mes_textes::models(c.conn(), &st.publ.platform);
    let model = models.iter().find(|m| m.id == st.publ.model).cloned();
    let out = match model.as_ref() {
        Some(m) => mes_textes::fill(&m.body, &tr, &st.publ.platform, &st.publ.tone, &brand, &mes_textes::extra_of(c.conn(), id), st.publ.variant, &recents, &mine),
        None => textes::generer_avec(&tr, &st.publ.platform, &st.publ.tone, &brand, st.publ.variant, &recents, &mine),
    };
    let g = app.global::<Pub>();
    g.set_model(model.as_ref().map(|m| m.id as i32).unwrap_or(0));
    g.set_models(model_rows(&models));
    g.set_platform(st.publ.platform.as_str().into());
    g.set_tone(st.publ.tone.as_str().into());
    g.set_text(out.texte.as_str().into());
    g.set_sc_genre(out.genre_sc.as_str().into());
    g.set_sc_tags(textes::format_tags_soundcloud(&out.tags_sc).into());
    let manque = textes::manque(&tr, &brand);
    let mut info = format!("{} caractères · variante {}", out.texte.chars().count(), st.publ.variant + 1);
    if !manque.is_empty() {
        info.push_str(&format!("  ·  Infos manquantes : {} — lance l'analyse audio ou renseigne le genre pour des textes plus précis.", manque.join(", ")));
    }
    g.set_info(info.into());
    st.publ.last = out;
}

fn model_rows(list: &[Mine]) -> ModelRc<MineUi> {
    model(list.iter().map(|m| MineUi { id: m.id as i32, title: if m.name.trim().is_empty() { "Sans nom".into() } else { m.name.as_str().into() }, sub: SharedString::new() }).collect())
}

fn platform_short(p: &str) -> &'static str {
    match p {
        "soundcloud" => "SoundCloud",
        "instagram" => "Instagram",
        "story" => "Story",
        _ => "Partout",
    }
}

/// La liste de « Mes textes » et le texte ouvert, avec son aperçu sur le morceau choisi.
fn push_mine(app: &AppWindow, st: &State) {
    let Some(c) = st.career.as_ref() else { return };
    let g = app.global::<Pub>();
    let list = mes_textes::list(c.conn());
    g.set_mines(model(
        list.iter()
            .map(|m| {
                let kind = mes_textes::KINDS.iter().find(|k| k.0 == m.kind).map(|k| k.1).unwrap_or("");
                let head = m.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("Vide").to_string();
                let (title, sub) = if m.kind == "modele" { (if m.name.trim().is_empty() { "Sans nom".to_string() } else { m.name.clone() }, format!("{kind} · {}", platform_short(&m.platform))) } else { (head, format!("{kind} · {}", platform_short(&m.platform))) };
                MineUi { id: m.id as i32, title: title.into(), sub: sub.into() }
            })
            .collect(),
    ));
    g.set_mine_kinds(model(mes_textes::KINDS.iter().map(|k| SharedString::from(k.0)).collect()));
    g.set_mine_kind_labels(model(mes_textes::KINDS.iter().map(|k| SharedString::from(k.1)).collect()));
    let Some(m) = st.publ.mine.as_ref() else {
        g.set_mine_sel(-1);
        return;
    };
    g.set_mine_sel(m.id as i32);
    g.set_mine_kind(m.kind.as_str().into());
    g.set_mine_platform(m.platform.as_str().into());
    if g.get_mine_name() != m.name.as_str() {
        g.set_mine_name(m.name.as_str().into());
    }
    if g.get_mine_body() != m.body.as_str() {
        g.set_mine_body(m.body.as_str().into());
    }
    let fields = mes_textes::FIELDS.iter().filter(|f| m.kind == "modele" || !["accroche", "corps", "appel", "hashtags", "signature", "lien", "date"].contains(&f.0));
    g.set_mine_fields(model(fields.map(|f| SharedString::from(format!("{{{}}}", f.0))).collect()));
    g.set_mine_preview(preview(st, m).into());
}

/// Ce que donne ce texte sur le morceau choisi.
fn preview(st: &State, m: &Mine) -> String {
    let (Some(c), Some(id)) = (st.career.as_ref(), st.career_pick) else { return String::new() };
    let Some(tr) = textes::load_track(c.conn(), id) else { return String::new() };
    let platform = if m.platform.is_empty() { st.publ.platform.as_str() } else { m.platform.as_str() };
    let g = mes_textes::fill(&m.body, &tr, platform, &st.publ.tone, &textes::brand_load(c.conn()), &mes_textes::extra_of(c.conn(), id), st.publ.variant, &Default::default(), &mes_textes::phrases(c.conn(), platform));
    g.texte
}

fn new_mine(app: &AppWindow, st: &mut State, mut m: Mine) {
    let Some(c) = st.career.as_ref() else { return };
    match mes_textes::add(c.conn(), &m) {
        Ok(id) => {
            m.id = id;
            st.publ.mine = Some(m);
            push_mine(app, st);
            generate(app, st);
        }
        Err(e) => say(app, &format!("Enregistrement impossible : {e}")),
    }
}

/// Modifie le texte ouvert et l'enregistre.
fn change_mine(app: &AppWindow, st: &mut State, f: impl FnOnce(&mut Mine)) {
    let Some(m) = st.publ.mine.as_mut() else { return };
    f(m);
    if let (Some(c), Some(m)) = (st.career.as_ref(), st.publ.mine.as_ref()) {
        if let Err(e) = mes_textes::save(c.conn(), m) {
            say(app, &format!("Enregistrement impossible : {e}"));
        }
    }
    push_mine(app, st);
}

/// Le texte affiché devient un modèle : ce qui appartient au morceau y devient un champ.
fn keep_model(app: &AppWindow, st: &mut State) {
    let (Some(c), Some(id)) = (st.career.as_ref(), st.career_pick) else { return };
    let Some(tr) = textes::load_track(c.conn(), id) else { return };
    let g = app.global::<Pub>();
    let text = g.get_text().trim().to_string();
    if text.is_empty() {
        return;
    }
    let link = mes_textes::extra_of(c.conn(), id).link;
    let body = mes_textes::to_model(&text, &tr, &textes::brand_load(c.conn()), &link);
    let name = format!("Modèle {}", platform_short(&st.publ.platform));
    let platform = st.publ.platform.clone();
    new_mine(app, st, Mine { kind: "modele".into(), platform, name, body, ..Default::default() });
    g.set_tab("mine".into());
    say(app, "Modèle créé : donne-lui un nom et retouche-le.");
}

fn save(app: &AppWindow, st: &mut State, posted: bool) {
    let (Some(c), Some(id)) = (st.career.as_ref(), st.career_pick) else { return };
    let g = app.global::<Pub>();
    let text = g.get_text().trim().to_string();
    if text.is_empty() {
        return;
    }
    let extra = if st.publ.platform == "soundcloud" { g.get_sc_tags().to_string() } else { String::new() };
    let _ = textes::save_text(c.conn(), id, &st.publ.platform, &st.publ.tone, &text, &extra, &st.publ.last.cles, posted);
    say(app, if posted { "Enregistré comme publié." } else { "Enregistré dans l'historique." });
}

fn platform_label(p: &str) -> &'static str {
    textes::PLATEFORMES.iter().find(|x| x.0 == p).map(|x| x.1).unwrap_or("")
}

fn push_history(app: &AppWindow, st: &mut State) {
    let Some(c) = st.career.as_ref() else { return };
    let one = if st.publ.hist_one { st.career_pick } else { None };
    st.publ.history = textes::history(c.conn(), one, 200);
    let g = app.global::<Pub>();
    g.set_hist_one(st.publ.hist_one);
    g.set_history(model(
        st.publ
            .history
            .iter()
            .map(|r| HistUi {
                id: r.id as i32,
                date: r.created.chars().take(16).collect::<String>().into(),
                title: r.title.as_str().into(),
                platform: platform_label(&r.platform).split(" —").next().unwrap_or("").into(),
                posted: r.posted,
                preview: r.body.replace('\n', " ").chars().take(90).collect::<String>().into(),
            })
            .collect(),
    ));
    if st.publ.hist_sel.is_some_and(|i| i >= st.publ.history.len()) {
        st.publ.hist_sel = None;
    }
    push_history_text(app, st);
}

fn push_history_text(app: &AppWindow, st: &State) {
    let g = app.global::<Pub>();
    g.set_hist_sel(st.publ.hist_sel.map(|i| i as i32).unwrap_or(-1));
    let text = st.publ.hist_sel.and_then(|i| st.publ.history.get(i)).map(|d| {
        let mut t = d.body.clone();
        if !d.hashtags.is_empty() && d.platform == "soundcloud" {
            t.push_str(&format!("\n\nTags : {}", d.hashtags));
        }
        t
    });
    g.set_hist_text(text.unwrap_or_default().into());
}

fn push_brand(app: &AppWindow, st: &State) {
    let Some(c) = st.career.as_ref() else { return };
    let b = textes::brand_load(c.conn());
    let g = app.global::<Pub>();
    g.set_brand(model(textes::BRAND_FIELDS.iter().map(|(k, l, p)| BrandFieldUi { key: (*k).into(), label: (*l).into(), placeholder: (*p).into(), value: b.get(k).into() }).collect()));
    g.set_brand_tone(if b.default_tone.is_empty() { "mysterieux".into() } else { b.default_tone.as_str().into() });
}

fn current(st: &State) -> Option<Song> {
    st.career.as_ref()?.song(st.career_pick?).ok().flatten()
}

pub fn run_check(app: &AppWindow, st: &mut State) {
    let Some(s) = current(st) else { return };
    if !std::path::Path::new(&s.path).exists() {
        app.global::<Pub>().set_check_text(format!("Fichier introuvable : {}", s.path).into());
        return;
    }
    app.global::<Pub>().set_check_text("Lecture du fichier…".into());
    st.spawn(move || {
        let t = std::time::Instant::now();
        let result = analyze(&s.path, &s);
        Done::Check { song: s.id, secs: t.elapsed().as_secs_f64(), result }
    });
}

pub fn checked(app: &AppWindow, st: &mut State, song: i64, secs: f64, result: Result<Vec<Check>, String>) {
    if st.career_pick != Some(song) {
        return;
    }
    let g = app.global::<Pub>();
    match result {
        Ok(list) => {
            let bad = list.iter().filter(|c| c.level == 2).count();
            let warn = list.iter().filter(|c| c.level == 1).count();
            g.set_check_text(
                match (bad, warn) {
                    (0, 0) => format!("Prêt à publier (contrôlé en {secs:.1} s)."),
                    (0, w) => format!("Publiable, {w} point{} à voir.", if w > 1 { "s" } else { "" }),
                    (b, _) => format!("{b} point{} à corriger avant de publier.", if b > 1 { "s" } else { "" }),
                }
                .into(),
            );
            g.set_checks(model(list.into_iter().map(|c| PubCheckUi { level: c.level, text: c.text.into() }).collect()));
        }
        Err(e) => g.set_check_text(format!("Contrôle impossible : {e}").into()),
    }
}

/// Prépare l'envoi : le texte copié et gardé dans l'historique, le fichier montré dans
/// l'Explorateur, la page d'envoi ouverte dans le navigateur. La publication elle-même reste un
/// geste de l'artiste : OTOKO ne publie rien à sa place.
pub fn send(app: &AppWindow, st: &mut State) {
    let Some(s) = current(st) else { return };
    save(app, st, false);
    let text = app.global::<Pub>().get_text().to_string();
    let (url, label) = if st.publ.platform == "soundcloud" { ("https://soundcloud.com/upload", "SoundCloud") } else { ("https://www.instagram.com/", "Instagram") };
    if st.selftest {
        return;
    }
    copy(&text);
    use std::os::windows::process::CommandExt;
    if std::path::Path::new(&s.path).exists() {
        let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", s.path)).spawn();
    }
    let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
    say(
        app,
        &format!("{label} s'ouvre dans ton navigateur, le fichier est montré dans l'Explorateur et le texte est copié : glisse le fichier, colle le texte (Ctrl+V), publie. Reviens ensuite sur « Publié aujourd'hui » avec le lien."),
    );
}

/// Le morceau est en ligne : publié aujourd'hui, avec son lien.
pub fn published(app: &AppWindow, st: &mut State) {
    let Some(id) = st.career_pick else { return };
    let link = app.global::<Pub>().get_link().trim().to_string();
    if let Some(c) = st.career.as_ref() {
        let _ = otoko_library::career::journal::record(c.conn(), "Marqué publié", "", || c.mark_published(id, &crate::career::today(), &link));
    }
    crate::career::fill(app, st);
    say(app, &format!("Morceau marqué publié aujourd'hui{}.", if link.is_empty() { String::new() } else { format!(" ({link})") }));
}

/// Ouvre l'onglet Publier sur un morceau (depuis la bibliothèque, une sortie…).
pub fn open(app: &AppWindow, st: &mut State, id: i64) {
    st.career_pick = Some(id);
    st.publ.variant = 0;
    app.global::<Pub>().set_tab("write".into());
    st.pages_lues[3] = false;
    crate::raccourcis::tab(app, st, 3);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        Song { title: "Concrete Jungle".into(), artist: "Otoko".into(), genre: "UK Garage".into(), bpm: Some(140.6), key: "A".into(), nature: "original".into(), ..Default::default() }
    }

    #[test]
    fn the_check_reads_loudness_peaks_and_silence() {
        // 40 s de sinus à 0 dBFS, 2 s de silence devant : trop fort, crête au plafond, silence signalé.
        let rate = 48_000u32;
        let path = std::env::temp_dir().join(format!("otoko-controle-{}.wav", std::process::id()));
        let spec = hound::WavSpec { channels: 2, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..(rate * 42) {
            let v = if i < rate * 2 { 0.0 } else { (i as f32 * 0.0731).sin() };
            w.write_sample(v).unwrap();
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();
        let checks = analyze(&path.to_string_lossy(), &song()).unwrap();
        let _ = std::fs::remove_file(&path);
        let find = |w: &str| checks.iter().find(|c| c.text.contains(w)).cloned();
        assert_eq!(find("WAV").map(|c| c.level), Some(0));
        assert_eq!(find("dBTP").map(|c| c.level), Some(2), "{checks:?}");
        assert_eq!(find("silence au début").map(|c| c.level), Some(1));
        assert_eq!(find("Corrélation").map(|c| c.level), Some(0));
    }
}
