//! Les vrais téléchargements, dans un dossier temporaire : les empreintes attendues sont les bonnes, et ce qui est
//! installé est bien reconnu par le logiciel. Rien de téléchargé n'est exécuté. À lancer à la main (160 Mo) :
//! cargo test -p otoko-mobile --test complements -- --ignored

use otoko_library::career::mastering::Engine;
use otoko_mobile::complements;

#[test]
#[ignore = "télécharge 160 Mo"]
fn ffmpeg_and_the_engine_install_from_their_official_sources() {
    let tmp = std::env::temp_dir().join(format!("otoko-essai-complements-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // Seul fil de ce test : rien d'autre ne lit l'environnement en même temps.
    unsafe { std::env::set_var("APPDATA", &tmp) };
    let say = |t: String| println!("{t}");

    let ffmpeg = complements::install_ffmpeg(&say).unwrap();
    assert!(ffmpeg.starts_with(&tmp), "installé dans les données : {}", ffmpeg.display());
    assert!(std::fs::metadata(&ffmpeg).unwrap().len() > 50_000_000, "un vrai ffmpeg");

    let root = complements::install_engine(&say).unwrap();
    assert_eq!(root, complements::engine_root());
    let engine = Engine { root: root.clone() };
    assert!(engine.ok(), "le logiciel reconnaît le moteur installé : {}", root.display());

    let left: Vec<String> = std::fs::read_dir(tmp.join("OTOKO").join("complements")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!left.iter().any(|n| n.contains(".part")), "aucun fichier de travail ne traîne : {left:?}");
    let _ = std::fs::remove_dir_all(&tmp);
}
