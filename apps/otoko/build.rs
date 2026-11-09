fn main() {
    slint_build::compile("ui/app.slint").expect("interface Slint");
    // L'icône du programme, intégrée à l'exe.
    println!("cargo:rerun-if-changed=otoko.rc");
    println!("cargo:rerun-if-changed=../../installer/otoko.ico");
    embed_resource::compile("otoko.rc", embed_resource::NONE).manifest_optional().expect("icône du programme");
    // Le guide à côté du programme compilé, comme après une installation (target/<profil>/ est trois dossiers au-dessus).
    println!("cargo:rerun-if-changed=../../docs/guide.html");
    if let Some(dir) = std::env::var_os("OUT_DIR").map(std::path::PathBuf::from).and_then(|d| d.ancestors().nth(3).map(std::path::Path::to_path_buf)) {
        let _ = std::fs::copy("../../docs/guide.html", dir.join("Guide Otoko Studio Vault Pro.html"));
    }
}
