use super::*;

/// Une petite base Studio Vault (schéma réel), écrite dans un fichier.
pub(crate) fn vault_file(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("otoko-sv-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let v = Connection::open(&path).unwrap();
    v.execute_batch(schema::SCHEMA).unwrap();
    v.execute_batch(
        "INSERT INTO projects (id, name) VALUES (3, 'Album');
         INSERT INTO tracks (id, path, filename, title, artist, genre, year, duration_s, bpm, music_key, lufs, status, role, nature, notes, added_at,
             published_at, sc_url, archived, pick, project_id, mood_auto, energy)
         VALUES (11, 'Z:/cj.wav', 'cj.wav', 'Concrete Jungle', 'Otoko', 'UK Garage', 2025, 260, 140.6, 'A', -9.3, 'released', 'mixdown', 'original', '',
             '2025-01-01', '', '', 0, 1, 3, 'sombre, hypnotique', 0.7);
         INSERT INTO tracks (id, path, filename, title, status, role, parent_id, added_at) VALUES (12, 'Z:/cj (master).wav', 'cj (master).wav', '', 'done', 'render', 11, '2025-01-02');
         INSERT INTO tracks (id, path, filename, title, status, added_at) VALUES (13, 'Z:/idee 3.wav', 'idee 3.wav', '', 'idea', '2025-02-01');
         INSERT INTO tracks (id, path, filename, title, status, archived) VALUES (14, 'Z:/vieux.wav', 'vieux.wav', 'Vieux', 'done', 1);
         INSERT INTO tags (id, name) VALUES (5, 'club');
         INSERT INTO track_tags VALUES (11, 5);
         INSERT INTO markers (track_id, time_s, label) VALUES (11, 32.5, 'drop');
         INSERT INTO releases (id, title, rtype, release_date, steps) VALUES (7, 'Esprit Autonome', 'album', '2099-05-01', '{\"master\": true, \"pochette\": true, \"infos\": false}');
         INSERT INTO release_tracks VALUES (7, 11, 1);
         INSERT INTO clients (id, name, contact) VALUES (1, 'Label', 'contact@label');
         INSERT INTO post_drafts (track_id, platform, tone, body, hashtags, created_at) VALUES (11, 'soundcloud', 'mysterious', 'Sortie !', '#ukgarage', '2025-01-02');
         INSERT INTO prompts (id, name, custom_extra, prompt_final, use_count, created_at, modified_at)
         VALUES (2, 'Jungle', '{\"genre\": \"jungle, dnb\"}', 'jungle, dnb, dark', 3, '2025-01-01', '2025-01-02');
         INSERT INTO prompt_versions (prompt_id, version_no, snapshot) VALUES (2, 1, '{}');
         INSERT INTO saved_views (id, name, where_sql) VALUES (4, 'Club', 't.bpm > 120');
         INSERT INTO watch_folders (path) VALUES ('Z:/Drive/02_AUDIO');
         INSERT INTO set_entries (set_id, time_s, artist, title, ref_track_id) VALUES (13, 0, 'Otoko', 'Intro', 11);
         INSERT INTO settings VALUES ('brand_name', 'Otoko');",
    )
    .unwrap();
    path
}

fn temp_career(name: &str) -> (Career, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("otoko-carriere-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    (Career::open(&path).unwrap(), path)
}

/// Les formes d'onde s'écrivent en fond : un import ou une autre transaction qui tombe au même moment attend son tour.
#[test]
fn a_transaction_waits_for_a_background_writer() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let sv = vault_file("concurrence");
    let (mut c, path) = temp_career("concurrence");
    c.import_vault(&sv).unwrap();
    let id: i64 = c.conn().query_row("SELECT id FROM tracks LIMIT 1", [], |r| r.get(0)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (stop, path) = (stop.clone(), path.clone());
        std::thread::spawn(move || {
            let w = Career::open(&path).unwrap();
            while !stop.load(Ordering::Relaxed) {
                let _ = library::save_wave(w.conn(), id, &[1, 2, 3]);
                // Le vrai écrivain s'arrête 150 ms entre deux morceaux. Sans aucune pause, aucune attente ne serait
                // garantie d'aboutir : SQLite ne sert pas les demandes dans l'ordre.
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        })
    };
    let results: Vec<Result<ImportReport, String>> = (0..150).map(|_| c.import_vault(&sv)).collect();
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    let failed: Vec<&String> = results.iter().filter_map(|r| r.as_ref().err()).collect();
    assert!(failed.is_empty(), "{} imports sur 150 ont échoué : {:?}", failed.len(), failed.first());
}

#[test]
fn importing_studio_vault_keeps_everything_and_its_ids() {
    let (mut c, path) = temp_career("import");
    assert_eq!(c.overview("2026-01-01").unwrap().songs, 0, "une Carrière neuve est vide");
    let sv = vault_file("import");
    let r = c.import_vault(&sv).unwrap();
    assert_eq!((r.songs, r.releases, r.clients, r.drafts, r.prompts, r.tags, r.markers, r.views, r.sets, r.projects, r.folders), (4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1));
    // Identifiants gardés : les vues et les liens de partage restent valides.
    assert_eq!(r.track_ids.get(&11), Some(&11));
    assert_eq!(c.conn().query_row("SELECT parent_id FROM tracks WHERE id = 12", [], |x| x.get::<_, i64>(0)).unwrap(), 11);
    assert_eq!(c.conn().query_row("SELECT COUNT(*) FROM track_tags WHERE track_id = 11 AND tag_id = 5", [], |x| x.get::<_, i64>(0)).unwrap(), 1);
    // Un réimport n'ajoute rien.
    let again = c.import_vault(&sv).unwrap();
    assert_eq!((again.songs, again.releases, again.drafts, again.skipped), (0, 0, 0, 4));
    // Les morceaux principaux : ni la version (parent), ni l'archivé.
    let songs = c.songs("", "", 100).unwrap();
    assert_eq!(songs.len(), 2);
    assert!(songs.iter().any(|s| s.title == "idee 3"), "sans titre : le nom du fichier, sans extension");
    let cj = c.songs("jungle", "", 1).unwrap().remove(0);
    assert_eq!((cj.status.as_str(), cj.pick, cj.mood.as_str()), ("released", 1, "sombre, hypnotique"));
    let rel = c.releases().unwrap();
    assert_eq!((rel[0].songs.clone(), rel[0].steps_done), (vec!["Concrete Jungle".to_string()], 2));
    assert_eq!(c.release_of(11).unwrap().map(|x| x.0), Some("Esprit Autonome".into()));
    let o = c.overview("2026-01-01").unwrap();
    assert_eq!((o.ideas, o.released, o.next_release.map(|n| n.0)), (1, 1, Some("Esprit Autonome".into())));
    assert_eq!(c.drafts(11).unwrap()[0].text, "Sortie !\n\n#ukgarage");
    assert_eq!(c.setting("brand_name").as_deref(), Some("Otoko"));
    // Le style d'un prompt Studio Vault vient de `custom_extra.genre`.
    let p = c.prompts().unwrap();
    assert_eq!((p[0].name.as_str(), p[0].style.as_str(), p[0].uses), ("Jungle", "jungle, dnb", 3));
    drop(c);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&sv);
}

#[test]
fn drafts_prompts_and_stats_live_in_the_vault_schema() {
    let (mut c, path) = temp_career("usage");
    let sv = vault_file("usage");
    c.import_vault(&sv).unwrap();
    c.conn()
        .execute(
            "INSERT INTO tracks (path, filename, title, duration_s, bpm, music_key, lufs, status, role, nature, added_at, file_format, daw_project)
             VALUES ('C:/Exports/Nuit.wav', 'Nuit.wav', 'Nuit', 188, 124, 'Am', -9, 'done', 'mixdown', 'original', '2026-11-03', 'wav', 'C:/Projets/Nuit.flp')",
            [],
        )
        .unwrap();
    let id = c.conn().last_insert_rowid();
    c.save_draft(id, "soundcloud", "premier jet", "2026-11-03").unwrap();
    c.save_draft(id, "soundcloud", "texte final", "2026-11-03").unwrap();
    c.save_draft(id, "instagram", "légende", "2026-11-03").unwrap();
    assert_eq!(c.drafts(id).unwrap().iter().map(|d| d.text.as_str()).collect::<Vec<_>>(), vec!["légende", "texte final"]);
    c.mark_published(id, "2026-11-04", "https://soundcloud.com/otoko/nuit").unwrap();
    assert_eq!(c.song(id).unwrap().unwrap().status, "released");
    c.set_pick(id, -1).unwrap();
    assert_eq!(c.song(id).unwrap().unwrap().pick, -1);
    let mut p = Prompt { name: "Garage nocturne".into(), style: "uk garage, 2-step".into(), bpm: Some(132), ..Default::default() };
    p.id = c.save_prompt(&p, "2026-11-03").unwrap();
    p.final_text = "uk garage, 2-step, 132 bpm".into();
    assert_eq!(c.save_prompt(&p, "2026-11-04").unwrap(), p.id);
    c.use_prompt(p.id).unwrap();
    let all = c.prompts().unwrap();
    assert_eq!((all[0].style.as_str(), all[0].uses), ("uk garage, 2-step", 1));
    assert_eq!(c.conn().query_row("SELECT COUNT(*) FROM prompt_versions WHERE prompt_id = ?1", [p.id], |x| x.get::<_, i64>(0)).unwrap(), 2, "une version par enregistrement");
    let st = c.stats().unwrap();
    assert_eq!(st.kinds, vec![("album".to_string(), 1)]);
    assert!(st.genres.iter().any(|g| g.0 == "UK Garage"));
    assert_eq!(st.published, 1);
    drop(c);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&sv);
}

#[test]
fn the_first_otoko_career_is_carried_over() {
    let path = std::env::temp_dir().join(format!("otoko-carriere-v1-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE morceaux (id INTEGER PRIMARY KEY, titre TEXT NOT NULL, artiste TEXT, chemin TEXT, genre TEXT, annee INTEGER, duree REAL,
                 bpm REAL, tonalite TEXT, lufs REAL, note INTEGER, statut TEXT NOT NULL DEFAULT 'idee', nature TEXT, remarques TEXT, ajoute TEXT,
                 publie TEXT, lien TEXT, source TEXT, source_id INTEGER);
             CREATE TABLE sorties (id INTEGER PRIMARY KEY, titre TEXT NOT NULL, type TEXT, date TEXT, pochette TEXT, description TEXT, lien TEXT,
                 remarques TEXT, etapes TEXT, source TEXT, source_id INTEGER);
             CREATE TABLE sortie_morceaux (sortie INTEGER, morceau INTEGER, position INTEGER);
             CREATE TABLE clients (id INTEGER PRIMARY KEY, nom TEXT NOT NULL, contact TEXT, remarques TEXT, source TEXT, source_id INTEGER);
             INSERT INTO morceaux (id, titre, chemin, statut) VALUES (1, 'Nuit', 'C:/Nuit.wav', 'fini');
             INSERT INTO clients (id, nom) VALUES (1, 'Label');",
        )
        .unwrap();
    }
    let c = Career::open(&path).unwrap();
    let s = c.song(1).unwrap().unwrap();
    assert_eq!((s.title.as_str(), s.status.as_str(), s.path.as_str()), ("Nuit", "done", "C:/Nuit.wav"));
    assert_eq!(c.overview("2026-01-01").unwrap().clients, 1);
    drop(c);
    let _ = std::fs::remove_file(&path);
}

/// Sur une copie de la vraie base Studio Vault : import, puis un passage du scanner sur les vrais
/// dossiers surveillés, la navigation et une recherche. À lancer à la main :
/// `cargo test -p otoko-library --release -- --ignored --nocapture real_library`
#[test]
#[ignore]
fn real_library() {
    let Some(dir) = crate::vault::default_dir() else { return };
    let (conn, tmp) = crate::vault::snapshot(&dir).unwrap();
    drop(conn);
    let (mut c, path) = temp_career("reelle");
    let t = std::time::Instant::now();
    let rep = c.import_vault(&tmp.join("vault.db")).unwrap();
    println!("import : {} morceaux en {:?}", rep.songs, t.elapsed());
    let t = std::time::Instant::now();
    let stop = std::sync::atomic::AtomicBool::new(false);
    let st = scan::scan(c.conn_mut(), None, &stop, &mut |_| {}).unwrap();
    println!("scan : {st:?} en {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let st2 = scan::scan(c.conn_mut(), None, &stop, &mut |_| {}).unwrap();
    println!("second passage : {st2:?} en {:?}", t.elapsed());
    assert_eq!(st2.added + st2.updated, 0, "rien de nouveau au second passage");
    let t = std::time::Instant::now();
    let nav = library::nav(c.conn());
    println!("navigation : {} entrées en {:?}", nav.len(), t.elapsed());
    for n in nav.iter().filter(|n| n.depth == 0).take(40) {
        println!("  {:<12} {:<40} {}", n.section, n.label, n.count);
    }
    let all = &nav[0];
    let (w, p) = library::full_where(c.conn(), &all.where_sql, &all.params, false, &library::Chips::default(), "camelot:8A bpm:120-130");
    let rows = library::rows(c.conn(), &w, &p, library::DEFAULT_ORDER).unwrap();
    println!("camelot:8A bpm:120-130 → {} morceaux, ex. {:?}", rows.len(), rows.first().map(|r| (&r.title, r.cell("bpm"), r.cell("camelot"))));
    drop(c);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(tmp);
}
/// L'accueil et les statistiques sur une copie de la base Studio Vault, en JSON, pour les comparer
/// au code de Studio Vault (la copie est gardée dans `OTOKO_SV_COPY` si la variable est posée).
#[test]
#[ignore]
fn real_home() {
    let Some(dir) = crate::vault::default_dir() else { return };
    let (conn, tmp) = crate::vault::snapshot(&dir).unwrap();
    let t = std::time::Instant::now();
    let h = home::home(&conn);
    let c = home::charts(&conn);
    let plan = releases::sync_plan(&conn);
    eprintln!("accueil + statistiques + rattachement en {:?}", t.elapsed());
    let n_nat = h.todo.iter().find(|x| x.1 == "nature").map(|x| x.0.split(' ').next().unwrap_or("0").to_string()).unwrap_or("0".into());
    let out = serde_json::json!({
        "months": h.months.iter().map(|m| m.1).collect::<Vec<_>>(),
        "unfinished": h.unfinished, "released": h.released, "recent": h.recent, "total": h.total,
        "priority": h.priority.iter().map(|p| p.0).collect::<Vec<_>>(),
        "news": h.news.iter().map(|p| p.0).collect::<Vec<_>>(),
        "n_nat": n_nat.parse::<i64>().unwrap_or(0),
        "brand_empty": home::brand_is_empty(&conn),
        "sync": plan.iter().map(|a| serde_json::json!([a.kind, a.release_id, a.title, a.rtype, a.track_ids, a.copies, a.elsewhere])).collect::<Vec<_>>(),
        "alerts": releases::planning_alerts(&conn).iter().map(|a| a.1.clone()).collect::<Vec<_>>(),
        "tiles": h.releases.iter().map(|r| serde_json::json!([r.id, r.when, r.done])).collect::<Vec<_>>(),
        "deadlines": h.deadlines.len(), "unpaid": h.unpaid.len(), "unpaid_total": h.unpaid_total,
        "charts": {
            "hours": format!("{:.3}", c.hours), "keep": c.keep, "analyzed": c.analyzed, "avg_bpm": c.avg_bpm.map(|b| format!("{b:.3}")),
            "added": c.added_by_month, "types": c.release_types, "formats": c.formats, "tempo": c.tempo.iter().map(|x| x.1).collect::<Vec<_>>(),
            "keys": c.keys, "genres": c.genres, "moods": c.moods, "status": c.by_status.iter().map(|x| x.1).collect::<Vec<_>>(),
            "picks": c.picks.iter().map(|x| x.1).collect::<Vec<_>>(), "played": c.most_played.iter().map(|x| x.1).collect::<Vec<_>>(),
        },
    });
    println!("{out}");
    drop(conn);
    if let Some(keep) = std::env::var_os("OTOKO_SV_COPY") {
        std::fs::copy(tmp.join("vault.db"), keep).unwrap();
    }
    let _ = std::fs::remove_dir_all(tmp);
}

/// Le constructeur de set sur une copie de la base Studio Vault (`OTOKO_SV_COPY`), en JSON, pour
/// comparer l'ordre à celui de Studio Vault.
#[test]
#[ignore]
fn real_set_builder() {
    let Some(copy) = std::env::var_os("OTOKO_SV_COPY") else { return };
    let conn = Connection::open(copy).unwrap();
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM tracks WHERE missing = 0 AND COALESCE(nature, 'original') <> 'set' AND bpm IS NOT NULL ORDER BY id LIMIT 40")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .flatten()
        .collect();
    let pool = set_builder::tracks(&conn, &ids);
    let mut out = serde_json::Map::new();
    for (mode, _) in set_builder::MODES {
        let o = set_builder::build(&pool, mode, None, 0.0);
        out.insert(mode.into(), serde_json::json!(o.iter().map(|t| t.id).collect::<Vec<_>>()));
    }
    let o = set_builder::build(&pool, "plateau", Some(ids[7]), 60.0);
    out.insert("plateau_start_60".into(), serde_json::json!(o.iter().map(|t| t.id).collect::<Vec<_>>()));
    println!("{}", serde_json::Value::Object(out));
}

/// Changer l'emplacement et dossiers en double sur une copie de la base Studio Vault
/// (`OTOKO_SV_COPY`, copiée de nouveau avant d'être modifiée), en JSON, pour comparer à
/// `relocate.py` et `dupe_folders.py` de Studio Vault.
#[test]
#[ignore]
fn real_relocate() {
    let Some(copy) = std::env::var_os("OTOKO_SV_COPY") else { return };
    let work = std::env::temp_dir().join(format!("otoko-relocate-{}.db", std::process::id()));
    std::fs::copy(&copy, &work).unwrap();
    let conn = Connection::open(&work).unwrap();
    let sugg = folders::suggestions(&conn);
    let old = sugg.iter().find(|s| s.0.contains("02_AUDIO")).map(|s| s.0.clone()).unwrap_or_else(|| sugg[0].0.clone());
    let p = folders::preview(&conn, &old, r"Y:\Musique");
    let dupes: Vec<serde_json::Value> = folders::dupe_folders(&conn).iter().map(|d| serde_json::json!([d.folder, d.other, d.n, d.common, d.ids])).collect();
    let done = folders::relocate(&conn, &old, r"Y:\Musique").unwrap();
    let paths: Vec<String> = conn.prepare("SELECT path FROM tracks ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().flatten().collect();
    let settings: Vec<(String, String)> = conn.prepare("SELECT key, value FROM settings ORDER BY key").unwrap().query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default()))).unwrap().flatten().collect();
    drop(conn);
    let _ = std::fs::remove_file(&work);
    println!(
        "{}",
        serde_json::json!({
            "suggestions": sugg, "old": old, "tracks": p.tracks, "found": p.found, "absent": p.absent.len(), "conflicts": p.conflicts,
            "dupes": dupes, "changed": done.0, "settings_changed": done.1, "replaced": done.2, "paths": paths, "settings": settings,
        })
    );
}

/// Le moteur de textes sur une copie de la base Studio Vault (`OTOKO_SV_COPY`) : 12 morceaux × 3
/// plateformes × 6 tons × 2 variantes, en JSON, pour comparer au moteur de Studio Vault.
#[test]
#[ignore]
fn real_texts() {
    let Some(copy) = std::env::var_os("OTOKO_SV_COPY") else { return };
    let conn = Connection::open(copy).unwrap();
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM tracks WHERE missing = 0 ORDER BY (nature = 'original'), id LIMIT 12")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .flatten()
        .collect();
    let brand = textes::Brand {
        artist_name: "Otoko No Sound".into(),
        city: "Marseille".into(),
        base_hashtags: "#otoko, otokonosound".into(),
        signature: "À très vite.".into(),
        emoji_prefs: "🌑".into(),
        banned_words: "rêve".into(),
        default_tone: "mysterieux".into(),
        ..Default::default()
    };
    let recents: std::collections::HashSet<String> = ["m01", "cs01", "ci01"].iter().map(|s| s.to_string()).collect();
    let mut out = serde_json::Map::new();
    for id in ids {
        let tr = textes::load_track(&conn, id).unwrap();
        for (p, _) in textes::PLATEFORMES {
            for (ton, _) in textes_fr::TONS {
                for v in 0..2 {
                    let g = textes::generer(&tr, p, ton, &brand, v, &recents);
                    out.insert(format!("{id}|{p}|{ton}|{v}"), serde_json::json!([g.texte, g.hashtags, g.tags_sc, g.genre_sc, g.cles]));
                }
            }
        }
    }
    println!("{}", serde_json::Value::Object(out));
}

#[test]
#[ignore]
fn real_scan_diff() {
    let Some(dir) = crate::vault::default_dir() else { return };
    let (conn, tmp) = crate::vault::snapshot(&dir).unwrap();
    let rows: Vec<(String, Option<i64>, Option<String>)> =
        conn.prepare("SELECT path, file_size, file_date FROM tracks WHERE missing = 0").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().flatten().collect();
    let mut shown = 0;
    let (mut size_diff, mut date_diff, mut no_date) = (0, 0, 0);
    for (p, size, date) in &rows {
        let Ok(m) = std::fs::metadata(p) else { continue };
        let d = scan::file_date(&m);
        if Some(m.len() as i64) != *size {
            size_diff += 1;
        }
        match date {
            None => no_date += 1,
            Some(x) if *x != d => {
                date_diff += 1;
                if shown < 6 {
                    println!("{p}\n  Studio Vault : {x}\n  OTOKO        : {d}  (créé {:?}, modifié {:?})", m.created().ok(), m.modified().ok());
                    shown += 1;
                }
            }
            _ => {}
        }
    }
    println!("{} morceaux : taille différente {size_diff}, date différente {date_diff}, sans date {no_date}", rows.len());
    let known: std::collections::HashSet<String> = rows.iter().map(|r| r.0.to_lowercase().replace('/', "\\")).collect();
    let folders: Vec<String> = conn.prepare("SELECT path FROM watch_folders").unwrap().query_map([], |r| r.get(0)).unwrap().flatten().collect();
    let mut new = 0;
    for f in folders {
        for e in walkdir::WalkDir::new(&f).into_iter().flatten() {
            if e.file_type().is_file() && scan::is_audio(e.path()) {
                let k = e.path().to_string_lossy().to_lowercase().replace('/', "\\");
                if !known.contains(&k) {
                    new += 1;
                    if new <= 5 {
                        println!("nouveau : {} ({:?})", e.path().display(), std::fs::metadata(e.path()).and_then(|m| m.modified()).ok());
                    }
                }
            }
        }
    }
    println!("{new} fichiers absents de Studio Vault (ou marqués introuvables)");
    drop(conn);
    let _ = std::fs::remove_dir_all(tmp);
}
/// Les nombres de la colonne de gauche (tags, genres, statuts, sorties…) sont ceux de la liste qu'un clic ouvre :
/// ni les morceaux archivés, ni ceux d'un dossier d'archives n'y sont comptés.
#[test]
fn nav_counts_match_the_list() {
    let (c, path) = temp_career("nombres");
    let conn = c.conn();
    conn.execute_batch(
        "INSERT INTO tracks (id, path, filename, title, genre, music_key, bpm, status, file_format, archived) VALUES
             (1, 'C:\\Musique\\Nuit.wav', 'Nuit.wav', 'Nuit', 'House', 'Am', 124, 'done', 'wav', 0),
             (2, 'C:\\Musique\\Jour.wav', 'Jour.wav', 'Jour', 'House', 'Am', 124, 'done', 'wav', 1),
             (3, 'C:\\Musique\\old\\Soir.wav', 'Soir.wav', 'Soir', 'House', 'Am', 124, 'done', 'wav', 0);
         INSERT INTO releases (id, title) VALUES (1, 'EP');
         INSERT INTO release_tracks (release_id, track_id, position) VALUES (1, 1, 1), (1, 2, 2), (1, 3, 3);",
    )
    .unwrap();
    for id in 1..=3 {
        library::set_tags(conn, id, "club, 2024").unwrap();
    }
    let nav = library::nav(conn);
    let mut seen = Vec::new();
    for n in nav.iter().filter(|n| n.section != "VUES" || n.label == "Tous les morceaux") {
        let (w, p) = library::full_where(conn, &n.where_sql, &n.params, false, &library::Chips::default(), "");
        let shown = library::rows(conn, &w, &p, library::DEFAULT_ORDER).unwrap().len() as i64;
        assert_eq!(n.count, shown, "{} / {}", n.section, n.label);
        seen.push(format!("{}:{}", n.section, n.label));
    }
    for want in ["TAGS:club", "TAGS:2024", "GENRES:House", "SORTIES:EP", "FORMATS:WAV", "STATUT:Terminé"] {
        assert!(seen.iter().any(|s| s == want), "{want} manque : {seen:?}");
    }
    assert_eq!(nav.iter().find(|n| n.label == "2024").map(|n| n.count), Some(1));
    drop(c);
    let _ = std::fs::remove_file(&path);
}
