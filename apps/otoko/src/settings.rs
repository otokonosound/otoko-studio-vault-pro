//! Réglages de l'utilisateur dans `%APPDATA%\OTOKO\settings.json` : un objet JSON dont chaque
//! module lit et écrit ses clés sans toucher aux autres (« library_roots », « level »…).
//!
//! Ce fichier garde ce qui appartient à ce PC (thème, icône près de l'horloge, modèles de renommage) : il reste ici même
//! quand le PC travaille sur la base d'un autre. Ce qui suit la musique et l'artiste va dans la table `settings` de la base
//! (voir `career::distant` pour ce qui voyage vers le PC principal).

use serde_json::{Map, Value};

fn path() -> std::path::PathBuf {
    crate::donnees::data_dir().join("settings.json")
}

fn read() -> Map<String, Value> {
    std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

pub fn get(key: &str) -> Option<Value> {
    read().get(key).cloned()
}

pub fn set(key: &str, value: Value) {
    let mut all = read();
    all.insert(key.to_string(), value);
    let _ = std::fs::create_dir_all(crate::donnees::data_dir());
    // Écrit à côté puis remplace : un réglage n'est jamais à moitié écrit.
    let tmp = path().with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string_pretty(&Value::Object(all)).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, path());
    }
}
