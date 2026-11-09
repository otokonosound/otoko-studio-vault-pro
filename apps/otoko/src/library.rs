//! Où vivent les données de l'application.

use std::path::PathBuf;

/// Dossier des données : %APPDATA%\OTOKO (la base, les réglages, le cache du serveur mobile).
pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("OTOKO")
}
