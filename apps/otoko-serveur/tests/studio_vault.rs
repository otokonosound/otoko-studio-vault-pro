//! Le serveur mobile sur une copie des données de Studio Vault (vault.db, partages.db), comme
//! après l'import dans OTOKO. Les originaux sont seulement lus. À lancer à la main :
//! `cargo test -p otoko-serveur --release -- --ignored --nocapture`

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

struct Res {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Res {
    fn json(&self) -> Json {
        serde_json::from_slice(&self.body).unwrap_or(Json::Null)
    }
}

fn call(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&Json>) -> Res {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
    let data = body.map(|b| b.to_string()).unwrap_or_default();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n", data.len());
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(&data);
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
    Res { status, headers, body: raw[split + 4..].to_vec() }
}

#[test]
#[ignore]
fn serves_studio_vault_data() {
    let sv = PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("ComposerVault");
    let tmp = std::env::temp_dir().join(format!("otoko-essai-mobile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::copy(sv.join("vault.db"), tmp.join("vault.db")).unwrap();
    std::fs::copy(sv.join("partages.db"), tmp.join("sv-partages.db")).unwrap();

    let t0 = Instant::now();
    let mut career = otoko_library::career::Career::open(&tmp.join("carriere.db")).unwrap();
    let rep = career.import_vault(&tmp.join("vault.db")).unwrap();
    drop(career);
    let store = otoko_mobile::shares::ShareStore::open(&tmp.join("partages.db")).unwrap();
    let n = store.import(&tmp.join("sv-partages.db"), &rep.track_ids, &rep.release_ids).unwrap();
    drop(store);
    println!("import : {} morceaux, {} sorties, {n} partages ({:?})", rep.songs, rep.releases, t0.elapsed());

    let mut cfg = otoko_mobile::Config::new(&tmp, "essai");
    cfg.port = 0;
    cfg.public_port = Some(0);
    cfg.warm = false;
    let server = otoko_mobile::start(cfg).unwrap();
    let (p, pp) = (server.port(), server.public_port().unwrap());

    let ping = call(p, "GET", "/api/ping", &[], None);
    assert_eq!(ping.status, 200);
    let v = ping.json();
    println!("ping : {v}");
    assert_eq!(v["name"], "OTOKO");
    assert!(v["tracks"].as_i64().unwrap() > 0);

    let t = Instant::now();
    let list = call(p, "GET", "/api/tracks?sort=added&limit=500", &[], None).json();
    let tracks = list.as_array().unwrap();
    println!("{} morceaux principaux en {:?}", tracks.len(), t.elapsed());
    assert!(!tracks.is_empty() && tracks[0]["cover"].as_str().unwrap().starts_with("/api/tracks/"));
    let search = call(p, "GET", "/api/tracks?q=8A", &[], None).json();
    println!("recherche « 8A » : {} morceaux", search.as_array().map_or(0, |a| a.len()));

    // Un morceau présent sur le disque, de préférence déjà converti par Studio Vault.
    let on_disk: Vec<&Json> = tracks.iter().filter(|t| {
        let full = call(p, "GET", &format!("/api/tracks/{}", t["id"]), &[], None).json();
        full["id"] == t["id"]
    }).take(40).collect();
    let id = on_disk.iter().map(|t| t["id"].as_i64().unwrap()).find(|id| {
        call(p, "HEAD", &format!("/api/tracks/{id}/audio"), &[], None).status == 200
    });
    let Some(id) = id else { panic!("aucun fichier audio accessible (le QNAP est-il branché ?)") };
    let one = call(p, "GET", &format!("/api/tracks/{id}"), &[], None).json();
    println!("morceau {id} : {} ({} versions, {} repères)", one["title"], one["versions"].as_array().unwrap().len(), one["markers"].as_array().unwrap().len());

    let part = call(p, "GET", &format!("/api/tracks/{id}/audio"), &[("Range", "bytes=0-99")], None);
    assert_eq!((part.status, part.body.len()), (206, 100));
    assert!(part.headers["content-range"].starts_with("bytes 0-99/"));
    println!("audio : {} {}", part.headers["content-type"], part.headers["content-range"]);

    let c = call(p, "GET", &format!("/api/tracks/{id}/cover?size=160"), &[], None);
    println!("pochette : {} ({} octets, couleur {:?})", c.status, c.body.len(), c.headers.get("x-cover-color"));
    assert!(c.status == 200 || c.status == 204);

    let t = Instant::now();
    let w = call(p, "GET", &format!("/api/tracks/{id}/waveform?points=64"), &[], None).json();
    println!("forme d'onde : {} points, {} s, en {:?}", w["peaks"].as_array().map_or(0, |a| a.len()), w["duration"], t.elapsed());
    assert_eq!(w["peaks"].as_array().unwrap().len(), 64);

    let path = PathBuf::from(otoko_mobile::library::track_path(&otoko_mobile::library::connect(&tmp.join("carriere.db")).unwrap(), id).unwrap());
    let borrowed = tracks.iter().filter_map(|t| t["id"].as_i64()).take(60).find_map(|tid| {
        let con = otoko_mobile::library::connect(&tmp.join("carriere.db")).ok()?;
        let pth = otoko_mobile::library::track_path(&con, tid)?;
        server.state.cache.cached(&pth, "lossless").map(|c| (tid, c))
    });
    match &borrowed {
        Some((tid, c)) => {
            println!("version FLAC de Studio Vault réutilisée pour {tid} : {}", c.display());
            let t = Instant::now();
            let r = call(p, "GET", &format!("/api/tracks/{tid}/audio?quality=lossless"), &[("Range", "bytes=0-3")], None);
            assert_eq!((r.status, r.body.as_slice()), (206, &b"fLaC"[..]));
            println!("  servie en {:?} sans conversion", t.elapsed());
        }
        None => println!("aucune version FLAC de Studio Vault trouvée pour les 60 premiers morceaux ({})", path.display()),
    }

    let rels = call(p, "GET", "/api/releases", &[], None).json();
    let r0 = &rels.as_array().unwrap()[0];
    let rel = call(p, "GET", &format!("/api/releases/{}", r0["id"]), &[], None).json();
    println!("sorties : {} ; « {} » ({}) : {} morceaux, couleur {}", rels.as_array().unwrap().len(), rel["title"], rel["type_label"], rel["track_list"].as_array().unwrap().len(), rel["color"]);
    let views = call(p, "GET", "/api/views", &[], None).json();
    println!("vues : {views}");
    assert_eq!(call(p, "GET", "/api/rien", &[], None).status, 404);
    let page = call(p, "GET", "/", &[], None);
    assert!(page.status == 200 && String::from_utf8_lossy(&page.body).contains("OTOKO"));
    assert_eq!(call(p, "GET", "/icon.png", &[], None).headers["content-type"], "image/png");

    // Gestion des partages : en-tête obligatoire.
    assert_eq!(call(p, "GET", "/api/shares", &[], None).status, 403);
    let h = [("X-Studio-Vault", "1")];
    let all = call(p, "GET", "/api/shares", &h, None).json();
    println!("partages importés : {} (adresse publique {}, Funnel {})", all["shares"].as_array().unwrap().len(), all["public_base"], all["funnel"]);
    let created = call(p, "POST", "/api/shares", &[("X-Studio-Vault", "1"), ("Content-Type", "application/json")], Some(&json!({ "kind": "tracks", "items": [id], "label": "Essai OTOKO", "can_download": true, "expires_days": 1 })));
    assert_eq!(created.status, 201);
    let sh = created.json();
    let tok = sh["url"].as_str().unwrap().rsplit("/s/").next().unwrap().to_string();
    assert!(sh.get("token").is_none() && sh.get("pw_hash").is_none());
    let bad = call(p, "POST", "/api/shares", &[("X-Studio-Vault", "1")], Some(&json!({ "kind": "tracks", "items": [] })));
    assert_eq!(bad.status, 400);
    println!("refus attendu : {}", bad.json()["error"]);

    // Serveur public.
    let api = call(pp, "GET", &format!("/s/{tok}/api"), &[], None);
    assert_eq!(api.status, 200);
    assert!(api.headers["content-security-policy"].contains("default-src 'none'"));
    let a = api.json();
    assert_eq!(a["tracks"][0]["id"], id);
    let pg = String::from_utf8_lossy(&call(pp, "GET", &format!("/s/{tok}"), &[("X-Forwarded-Host", "mini.ts.net:10000")], None).body).into_owned();
    assert!(pg.contains("og:title\" content=\"Essai OTOKO\"") && pg.contains("https://mini.ts.net:10000/s/"));
    let st = call(pp, "GET", &format!("/s/{tok}/t/{id}/stream"), &[("Range", "bytes=0-9")], None);
    println!("écoute publique : {} {}", st.status, st.headers.get("content-type").cloned().unwrap_or_default());
    assert_eq!(st.status, 206);
    // Le début du morceau compte comme une écoute (pas les sauts dans le morceau).
    assert_eq!(call(pp, "HEAD", &format!("/s/{tok}/t/{id}/stream"), &[], None).status, 200);
    assert_eq!(call(pp, "GET", &format!("/s/{tok}/t/{id}/stream"), &[("Range", "bytes=0-")], None).status, 206);
    assert_eq!(call(pp, "GET", &format!("/s/{tok}/t/999999/stream"), &[], None).status, 404);
    assert_eq!(call(pp, "GET", "/s/jeton-bidon/api", &[], None).status, 404);
    assert_eq!(call(pp, "GET", "/api/tracks", &[], None).status, 404);
    assert_eq!(call(pp, "GET", "/p/share.css", &[], None).status, 200);

    // Les liens déjà envoyés par Studio Vault marchent encore.
    for s in all["shares"].as_array().unwrap() {
        let t = s["url"].as_str().unwrap().rsplit("/s/").next().unwrap();
        let r = call(pp, "GET", &format!("/s/{t}/api"), &[], None);
        let v = r.json();
        println!(
            "  partage n°{} « {} » ({}) : {} → {} sorties, {} morceaux",
            s["id"],
            s["label"],
            s["state"],
            r.status,
            v["releases"].as_array().map_or(0, |a| a.len()),
            v["tracks"].as_array().map_or(0, |a| a.len())
        );
        match s["state"].as_str() {
            Some("active") if s["has_password"] == true => assert_eq!(r.status, 401),
            Some("active") => assert_eq!(r.status, 200),
            _ => assert_eq!(r.status, 410),
        }
    }

    let sid = sh["id"].as_i64().unwrap();
    let ev = call(p, "GET", &format!("/api/shares/{sid}/events"), &h, None).json();
    println!("événements du partage d'essai : {ev}");
    assert!(ev.as_array().unwrap().len() >= 2, "une vue et une écoute");
    let up = call(p, "POST", &format!("/api/shares/{sid}"), &h, Some(&json!({ "password": "motdepasse" }))).json();
    assert_eq!(up["has_password"], true);
    assert_eq!(call(pp, "GET", &format!("/s/{tok}/api"), &[], None).status, 401);
    let wrong = call(pp, "POST", &format!("/s/{tok}/auth"), &[], Some(&json!({ "password": "faux" })));
    assert_eq!(wrong.status, 403);
    let ok = call(pp, "POST", &format!("/s/{tok}/auth"), &[], Some(&json!({ "password": "motdepasse" })));
    assert_eq!(ok.status, 200);
    let cookie = ok.headers["set-cookie"].split(';').next().unwrap().to_string();
    assert_eq!(call(pp, "GET", &format!("/s/{tok}/api"), &[("Cookie", &cookie)], None).status, 200);
    assert_eq!(call(p, "DELETE", &format!("/api/shares/{sid}"), &h, None).json()["ok"], true);
    assert_eq!(call(pp, "GET", &format!("/s/{tok}/api"), &[], None).status, 404);

    compare_with_studio_vault(&tmp);

    server.stop();
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Les mêmes lectures faites par le code de Studio Vault sur la même base (copie de vault.db) :
/// le JSON d'OTOKO doit être identique, champ par champ.
fn compare_with_studio_vault(tmp: &std::path::Path) {
    use otoko_mobile::library::{self as l, List};
    let sv = PathBuf::from(r"Z:\Claude\Studio Vault");
    let py = sv.join("python").join("python.exe");
    if !py.is_file() {
        println!("Studio Vault absent : comparaison sautée");
        return;
    }
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("sv_reference.py");
    let out = std::process::Command::new(&py).arg("-I").arg("-B").arg(&script).arg(&sv).arg(tmp.join("vault.db")).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let r: Json = serde_json::from_slice(&out.stdout).unwrap();
    let con = l::connect(&tmp.join("carriere.db")).unwrap();
    let s = |x: &str| x.to_string();
    let lists: Vec<(&str, List)> = vec![
        ("added", List::default()),
        ("title_all", List { sort: s("title"), scope: s("all"), ..Default::default() }),
        ("published", List { sort: s("published"), ..Default::default() }),
        ("bpm", List { sort: s("bpm"), ..Default::default() }),
        ("key", List { sort: s("key"), ..Default::default() }),
        ("plays", List { sort: s("plays"), ..Default::default() }),
        ("q_8A", List { q: s("8A"), ..Default::default() }),
        ("q_word", List { q: s("love"), ..Default::default() }),
        ("q_bpm", List { q: s("124"), ..Default::default() }),
        ("compatible", List { compatible_with: Some(1837), ..Default::default() }),
        ("nature", List { nature: s("original,remix"), scope: s("all"), ..Default::default() }),
        ("status", List { status: s("done,released"), ..Default::default() }),
        ("keys", List { key: s("8A,9A"), ..Default::default() }),
        ("bpm_range", List { bpm_min: Some(120.0), bpm_max: Some(128.0), ..Default::default() }),
        ("page", List { limit: 20, offset: 40, ..Default::default() }),
    ];
    let mut diffs = Vec::new();
    for (name, a) in &lists {
        diff(&format!("liste {name}"), &r["lists"][*name], &json!(l::list_tracks(&con, a).unwrap()), &mut diffs);
    }
    let tracks = r["tracks"].as_object().unwrap();
    for (id, t) in tracks {
        diff(&format!("morceau {id}"), t, &json!(l::get_track(&con, id.parse().unwrap()).unwrap()), &mut diffs);
    }
    diff("sorties", &r["releases"], &json!(l::list_releases(&con).unwrap()), &mut diffs);
    for (rid, t) in r["release_tracks"].as_object().unwrap() {
        diff(&format!("sortie {rid}"), t, &json!(l::release_tracks(&con, rid.parse().unwrap()).unwrap()), &mut diffs);
    }
    let norm = |src: l::Sources| json!(src.iter().map(|(k, p)| json!([k, p.display().to_string().replace('/', "\\")])).collect::<Vec<_>>());
    let unslash = |v: &Json| json!(v.as_array().unwrap().iter().map(|x| json!([x[0], x[1].as_str().unwrap().replace('/', "\\")])).collect::<Vec<_>>());
    for (id, c) in r["covers"].as_object().unwrap() {
        diff(&format!("pochette {id}"), &unslash(c), &norm(l::cover_sources(&con, id.parse().unwrap())), &mut diffs);
    }
    for (rid, c) in r["release_covers"].as_object().unwrap() {
        diff(&format!("pochette de sortie {rid}"), &unslash(c), &norm(l::release_cover_sources(&con, rid.parse().unwrap())), &mut diffs);
    }
    diff("vues", &r["views"], &json!(l::list_views(&con).unwrap()), &mut diffs);
    diff("résumé", &r["summary"], &l::summary(&con), &mut diffs);
    println!("comparaison avec le code de Studio Vault : {} listes, {} morceaux, {} sorties, {} écarts", lists.len(), tracks.len(), r["releases"].as_array().unwrap().len(), diffs.len());
    for d in diffs.iter().take(40) {
        println!("  {d}");
    }
    assert!(diffs.is_empty());
}

fn diff(at: &str, a: &Json, b: &Json, out: &mut Vec<String>) {
    match (a, b) {
        (Json::Object(x), Json::Object(y)) => {
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in keys {
                diff(&format!("{at}.{k}"), x.get(k).unwrap_or(&Json::Null), y.get(k).unwrap_or(&Json::Null), out);
            }
        }
        (Json::Array(x), Json::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{at} : {} éléments (Studio Vault) / {} (OTOKO)", x.len(), y.len()));
            }
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(&format!("{at}[{i}]"), p, q, out);
            }
        }
        (Json::Number(x), Json::Number(y)) if (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-9 => {}
        _ if a == b => {}
        _ => out.push(format!("{at} : {a} (Studio Vault) / {b} (OTOKO)")),
    }
}