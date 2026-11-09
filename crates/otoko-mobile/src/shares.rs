//! Liens de partage publics : ce que l'artiste choisit de montrer, avec quels droits (comme
//! `composer_vault/mobile/shares.py`, même base, mêmes jetons : une base Studio Vault s'importe).
//!
//! Un partage = un jeton secret (128 bits) qui donne accès à des sorties, à une sélection de
//! morceaux ou à toute la discographie déjà parue. Droits : écoute seule ou écoute et
//! téléchargement (qualité choisie), expiration, mot de passe, nombre maximal de téléchargements,
//! révocation à tout moment.

use std::path::Path;
use std::sync::Mutex;

use base64::Engine;
use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value as Json, json};
use sha2::Sha256;

pub const KINDS: [&str; 3] = ["releases", "tracks", "discography"];
pub const DOWNLOAD_QUALITIES: [&str; 3] = ["original", "lossless", "high"];
const PBKDF2_ROUNDS: u32 = 240_000;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS shares (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token TEXT NOT NULL UNIQUE,
    label TEXT NOT NULL DEFAULT '',
    message TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL,
    items TEXT NOT NULL DEFAULT '[]',
    can_download INTEGER NOT NULL DEFAULT 0,
    download_quality TEXT NOT NULL DEFAULT 'original',
    expires_at TEXT,
    pw_salt TEXT,
    pw_hash TEXT,
    max_downloads INTEGER,
    downloads INTEGER NOT NULL DEFAULT 0,
    views INTEGER NOT NULL DEFAULT 0,
    plays INTEGER NOT NULL DEFAULT 0,
    last_access TEXT,
    created_at TEXT NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS share_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    share_id INTEGER NOT NULL,
    at TEXT NOT NULL,
    event TEXT NOT NULL,
    track_id INTEGER
);
CREATE INDEX IF NOT EXISTS idx_share_events ON share_events(share_id, at);
CREATE TABLE IF NOT EXISTS secrets (k TEXT PRIMARY KEY, v TEXT NOT NULL);
";

/// Un message affichable tel quel (données refusées).
#[derive(Debug)]
pub struct ShareError(pub String);

fn err<T>(m: &str) -> Result<T, ShareError> {
    Err(ShareError(m.into()))
}

pub struct ShareStore {
    con: Mutex<Connection>,
    secret: Mutex<Vec<u8>>,
}

/// Un partage complet (avec l'empreinte du mot de passe : à ne jamais renvoyer tel quel).
#[derive(Clone, Debug)]
pub struct Share {
    pub id: i64,
    pub token: String,
    pub label: String,
    pub message: String,
    pub kind: String,
    pub items: Vec<i64>,
    pub can_download: bool,
    pub download_quality: String,
    pub expires_at: Option<String>,
    pub pw_salt: Option<String>,
    pub pw_hash: Option<String>,
    pub max_downloads: Option<i64>,
    pub downloads: i64,
    pub views: i64,
    pub plays: i64,
    pub last_access: Option<String>,
    pub created_at: String,
    pub revoked: bool,
}

pub fn now_iso() -> String {
    iso(std::time::SystemTime::now())
}

/// « AAAA-MM-JJTHH:MM:SSZ » (UTC), comme Studio Vault.
fn iso(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn hash_pw(password: &str, salt_hex: &str) -> String {
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &unhex(salt_hex), PBKDF2_ROUNDS, &mut out);
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// Sans vrai hasard, un jeton serait devinable : on s'arrête plutôt que d'en créer un.
fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).expect("le générateur aléatoire du système ne répond pas");
    b
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Comparaison en temps constant.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Share> {
    let items: String = r.get("items")?;
    Ok(Share {
        id: r.get("id")?,
        token: r.get("token")?,
        label: r.get("label")?,
        message: r.get("message")?,
        kind: r.get("kind")?,
        items: serde_json::from_str(&items).unwrap_or_default(),
        can_download: r.get::<_, i64>("can_download")? != 0,
        download_quality: r.get("download_quality")?,
        expires_at: r.get("expires_at")?,
        pw_salt: r.get("pw_salt")?,
        pw_hash: r.get("pw_hash")?,
        max_downloads: r.get("max_downloads")?,
        downloads: r.get("downloads")?,
        views: r.get("views")?,
        plays: r.get("plays")?,
        last_access: r.get("last_access")?,
        created_at: r.get("created_at")?,
        revoked: r.get::<_, i64>("revoked")? != 0,
    })
}

/// La durée de validité en date d'expiration (rien : jamais).
fn expiry(days: &Json) -> Result<Option<String>, ShareError> {
    let d = match days {
        Json::Null => return Ok(None),
        Json::String(s) if s.is_empty() || s == "0" || s == "jamais" => return Ok(None),
        Json::String(s) => s.parse::<f64>().map_err(|_| ShareError("Durée de validité invalide.".into()))?,
        Json::Number(n) => n.as_f64().unwrap_or(0.0),
        _ => return err("Durée de validité invalide."),
    };
    if d == 0.0 {
        return Ok(None);
    }
    // Écrit ainsi, « NaN » est refusé aussi.
    if !(d > 0.0 && d <= 3650.0) {
        return err("Durée de validité invalide.");
    }
    Ok(Some(iso(std::time::SystemTime::now() + std::time::Duration::from_secs_f64(d * 86400.0))))
}

impl ShareStore {
    pub fn open(path: &Path) -> rusqlite::Result<ShareStore> {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let con = Connection::open(path)?;
        // OTOKO et le serveur de la tâche planifiée peuvent l'ouvrir en même temps.
        con.busy_timeout(std::time::Duration::from_secs(5))?;
        let _ = con.pragma_update(None, "journal_mode", "WAL");
        con.execute_batch(SCHEMA)?;
        let secret = match con.query_row("SELECT v FROM secrets WHERE k = 'cookie'", [], |r| r.get::<_, String>(0)).optional()? {
            Some(h) => unhex(&h),
            None => {
                let s = random(32);
                con.execute("INSERT INTO secrets (k, v) VALUES ('cookie', ?1)", [hex(&s)])?;
                s
            }
        };
        Ok(ShareStore { con: Mutex::new(con), secret: Mutex::new(secret) })
    }

    fn con(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.con.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn clean(kind: &str, items: &Json, can_download: bool, quality: &str, max_downloads: &Json) -> Result<(Vec<i64>, i64, Option<i64>), ShareError> {
        if !KINDS.contains(&kind) {
            return err("Type de partage inconnu.");
        }
        let mut ids: Vec<i64> = if kind == "discography" {
            Vec::new()
        } else {
            match items {
                Json::Array(a) => a.iter().map(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).collect::<Option<Vec<_>>>().ok_or(ShareError("Liste de morceaux ou de sorties invalide.".into()))?,
                Json::Null => Vec::new(),
                _ => return err("Liste de morceaux ou de sorties invalide."),
            }
        };
        ids.sort_unstable();
        ids.dedup();
        if kind != "discography" && ids.is_empty() {
            return err("Choisis au moins un morceau ou une sortie.");
        }
        if ids.len() > 2000 {
            return err("Trop d'éléments dans un seul partage.");
        }
        if !DOWNLOAD_QUALITIES.contains(&quality) {
            return err("Qualité de téléchargement inconnue.");
        }
        let maxd = match max_downloads {
            Json::Null => None,
            Json::String(s) if s.is_empty() => None,
            Json::String(s) => Some(s.parse::<i64>().map_err(|_| ShareError("Limite de téléchargements invalide.".into()))?),
            Json::Number(n) => Some(n.as_i64().ok_or(ShareError("Limite de téléchargements invalide.".into()))?),
            _ => return err("Limite de téléchargements invalide."),
        };
        if maxd.is_some_and(|m| m < 1) {
            return err("La limite de téléchargements doit être d'au moins 1.");
        }
        Ok((ids, can_download as i64, maxd))
    }

    /// Crée un partage depuis le JSON de l'app (kind, items, label, message, can_download,
    /// download_quality, expires_days, password, max_downloads).
    pub fn create(&self, b: &Json) -> Result<Json, ShareError> {
        let kind = b["kind"].as_str().unwrap_or("");
        let quality = b["download_quality"].as_str().unwrap_or("original");
        let (ids, dl, maxd) = Self::clean(kind, &b["items"], b["can_download"].as_bool().unwrap_or(false), quality, &b["max_downloads"])?;
        let pw = b["password"].as_str().unwrap_or("");
        let (salt, hash) = if pw.is_empty() {
            (None, None)
        } else {
            if pw.chars().count() < 4 {
                return err("Mot de passe trop court (4 caractères au moins).");
            }
            let s = hex(&random(16));
            let h = hash_pw(pw, &s);
            (Some(s), Some(h))
        };
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random(16));
        let exp = expiry(b.get("expires_days").unwrap_or(&json!(7)))?;
        let label: String = b["label"].as_str().unwrap_or("").trim().chars().take(120).collect();
        let message: String = b["message"].as_str().unwrap_or("").trim().chars().take(1000).collect();
        let id = {
            let c = self.con();
            c.execute(
                "INSERT INTO shares (token, label, message, kind, items, can_download, download_quality, expires_at, pw_salt, pw_hash, max_downloads, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![token, label, message, kind, serde_json::to_string(&ids).unwrap_or_default(), dl, quality, exp, salt, hash, maxd, now_iso()],
            )
            .map_err(|e| ShareError(e.to_string()))?;
            c.last_insert_rowid()
        };
        self.get(id).ok_or(ShareError("Partage introuvable.".into()))
    }

    /// Champs modifiables : label, message, can_download, download_quality, expires_days,
    /// password ("" = l'enlever), max_downloads, revoked.
    pub fn update(&self, id: i64, f: &Json) -> Result<Option<Json>, ShareError> {
        let Some(cur) = self.row(id) else { return Ok(None) };
        let mut sets: Vec<(&str, rusqlite::types::Value)> = Vec::new();
        use rusqlite::types::Value as V;
        let has = |k: &str| f.get(k).is_some();
        if has("label") {
            sets.push(("label", V::Text(f["label"].as_str().unwrap_or("").trim().chars().take(120).collect())));
        }
        if has("message") {
            sets.push(("message", V::Text(f["message"].as_str().unwrap_or("").trim().chars().take(1000).collect())));
        }
        if has("can_download") || has("download_quality") || has("max_downloads") {
            let q = f.get("download_quality").and_then(|q| q.as_str()).unwrap_or(&cur.download_quality).to_string();
            let cd = f.get("can_download").and_then(|v| v.as_bool()).unwrap_or(cur.can_download);
            let md = f.get("max_downloads").cloned().unwrap_or(cur.max_downloads.map(|m| json!(m)).unwrap_or(Json::Null));
            let items = if cur.items.is_empty() { json!([0]) } else { json!(cur.items) };
            let (_, dl, maxd) = Self::clean(&cur.kind, &items, cd, &q, &md)?;
            sets.push(("can_download", V::Integer(dl)));
            sets.push(("download_quality", V::Text(q)));
            sets.push(("max_downloads", maxd.map(V::Integer).unwrap_or(V::Null)));
        }
        if has("expires_days") {
            sets.push(("expires_at", expiry(&f["expires_days"])?.map(V::Text).unwrap_or(V::Null)));
        }
        if has("password") {
            let pw = f["password"].as_str().unwrap_or("");
            if !pw.is_empty() && pw.chars().count() < 4 {
                return err("Mot de passe trop court (4 caractères au moins).");
            }
            if pw.is_empty() {
                sets.push(("pw_salt", V::Null));
                sets.push(("pw_hash", V::Null));
            } else {
                let s = hex(&random(16));
                sets.push(("pw_hash", V::Text(hash_pw(pw, &s))));
                sets.push(("pw_salt", V::Text(s)));
            }
        }
        if has("revoked") {
            sets.push(("revoked", V::Integer(f["revoked"].as_bool().unwrap_or(false) as i64)));
        }
        if !sets.is_empty() {
            let sql = format!("UPDATE shares SET {} WHERE id = ?", sets.iter().map(|(k, _)| format!("{k} = ?")).collect::<Vec<_>>().join(", "));
            let mut vals: Vec<V> = sets.into_iter().map(|(_, v)| v).collect();
            vals.push(V::Integer(id));
            self.con().execute(&sql, rusqlite::params_from_iter(vals.iter())).map_err(|e| ShareError(e.to_string()))?;
        }
        Ok(self.get(id))
    }

    pub fn delete(&self, id: i64) -> bool {
        let c = self.con();
        let n = c.execute("DELETE FROM shares WHERE id = ?1", [id]).unwrap_or(0);
        let _ = c.execute("DELETE FROM share_events WHERE share_id = ?1", [id]);
        n > 0
    }

    /// Repartir de zéro : une copie est gardée, puis plus aucun lien ne marche (ils visaient les morceaux de l'ancienne base).
    pub fn wipe(&self, copy: &Path) -> Result<(), String> {
        let c = self.con();
        let _ = std::fs::remove_file(copy);
        c.execute("VACUUM INTO ?1", [copy.to_string_lossy()]).map_err(|e| e.to_string())?;
        c.execute_batch("DELETE FROM shares; DELETE FROM share_events;").map_err(|e| e.to_string())
    }

    /// event : view, play, download.
    pub fn record(&self, id: i64, event: &str, track: Option<i64>) {
        let c = self.con();
        let now = now_iso();
        if let Some(col) = match event {
            "view" => Some("views"),
            "play" => Some("plays"),
            "download" => Some("downloads"),
            _ => None,
        } {
            let _ = c.execute(&format!("UPDATE shares SET {col} = {col} + 1, last_access = ?1 WHERE id = ?2"), params![now, id]);
        }
        let _ = c.execute("INSERT INTO share_events (share_id, at, event, track_id) VALUES (?1, ?2, ?3, ?4)", params![id, now, event, track]);
        let _ = c.execute(
            "DELETE FROM share_events WHERE share_id = ?1 AND id NOT IN (SELECT id FROM share_events WHERE share_id = ?1 ORDER BY id DESC LIMIT 500)",
            [id],
        );
    }

    fn row(&self, id: i64) -> Option<Share> {
        self.con().query_row("SELECT * FROM shares WHERE id = ?1", [id], row).ok()
    }

    pub fn by_token(&self, token: &str) -> Option<Share> {
        if token.is_empty() || token.len() > 64 {
            return None;
        }
        self.con().query_row("SELECT * FROM shares WHERE token = ?1", [token], row).ok()
    }

    pub fn get(&self, id: i64) -> Option<Json> {
        self.row(id).map(|s| Self::public_view(&s))
    }

    pub fn list(&self) -> Vec<Json> {
        let c = self.con();
        let Ok(mut st) = c.prepare("SELECT * FROM shares ORDER BY revoked, created_at DESC, id DESC") else { return Vec::new() };
        st.query_map([], row).map(|r| r.flatten().map(|s| Self::public_view(&s)).collect()).unwrap_or_default()
    }

    pub fn all(&self) -> Vec<Share> {
        let c = self.con();
        let Ok(mut st) = c.prepare("SELECT * FROM shares") else { return Vec::new() };
        st.query_map([], row).map(|r| r.flatten().collect()).unwrap_or_default()
    }

    pub fn events(&self, id: i64) -> Vec<Json> {
        let c = self.con();
        let Ok(mut st) = c.prepare("SELECT at, event, track_id FROM share_events WHERE share_id = ?1 ORDER BY id DESC LIMIT 50") else { return Vec::new() };
        st.query_map([id], |r| Ok(json!({ "at": r.get::<_, String>(0)?, "event": r.get::<_, String>(1)?, "track_id": r.get::<_, Option<i64>>(2)? })))
            .map(|r| r.flatten().collect())
            .unwrap_or_default()
    }

    /// active, revoked, expired.
    pub fn state(s: &Share) -> &'static str {
        if s.revoked {
            return "revoked";
        }
        match &s.expires_at {
            Some(e) if e.as_str() <= now_iso().as_str() => "expired",
            _ => "active",
        }
    }

    /// Pour la page de gestion (réseau privé) : tout sauf le sel et l'empreinte du mot de passe.
    pub fn public_view(s: &Share) -> Json {
        json!({
            "id": s.id, "token": s.token, "label": s.label, "message": s.message, "kind": s.kind, "items": s.items,
            "can_download": s.can_download, "download_quality": s.download_quality, "expires_at": s.expires_at,
            "max_downloads": s.max_downloads, "downloads": s.downloads, "views": s.views, "plays": s.plays,
            "last_access": s.last_access, "created_at": s.created_at, "revoked": s.revoked,
            "has_password": s.pw_hash.is_some(), "state": Self::state(s),
            "downloads_left": s.max_downloads.map(|m| (m - s.downloads).max(0)),
        })
    }

    pub fn check_password(&self, s: &Share, password: &str) -> bool {
        match (&s.pw_hash, &s.pw_salt) {
            (None, _) => true,
            (Some(h), Some(salt)) => same(&hash_pw(password, salt), h),
            _ => false,
        }
    }

    /// Preuve que le mot de passe a été donné ; change si le mot de passe change.
    pub fn cookie_value(&self, s: &Share) -> String {
        let key = self.secret.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("clé HMAC");
        mac.update(format!("{}:{}:{}", s.id, s.token, s.pw_hash.as_deref().unwrap_or("")).as_bytes());
        hex(&mac.finalize().into_bytes())
    }

    pub fn cookie_ok(&self, s: &Share, value: Option<&str>) -> bool {
        s.pw_hash.is_none() || value.is_some_and(|v| same(v, &self.cookie_value(s)))
    }

    pub fn can_download(s: &Share) -> bool {
        s.can_download && s.max_downloads.is_none_or(|m| s.downloads < m)
    }

    /// Importe les partages d'une base Studio Vault : jetons gardés (les liens déjà envoyés
    /// continuent de marcher), numéros gardés s'ils sont libres, historique des écoutes recopié,
    /// identifiants de morceaux et de sorties traduits. Si aucun partage n'existait encore, le
    /// secret des cookies est repris aussi : les visiteurs qui ont donné un mot de passe n'ont
    /// pas à le redonner.
    pub fn import(&self, src: &Path, tracks: &std::collections::HashMap<i64, i64>, releases: &std::collections::HashMap<i64, i64>) -> Result<usize, String> {
        let s = Connection::open_with_flags(src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
        let mut st = s.prepare("SELECT * FROM shares ORDER BY id").map_err(|e| e.to_string())?;
        let list: Vec<Share> = st.query_map([], row).map_err(|e| e.to_string())?.flatten().collect();
        let mut n = 0;
        let mut c = self.con();
        let empty = c.query_row("SELECT COUNT(*) FROM shares", [], |r| r.get::<_, i64>(0)).unwrap_or(1) == 0;
        let secret: Option<String> = s.query_row("SELECT v FROM secrets WHERE k = 'cookie'", [], |r| r.get(0)).ok();
        let tx = c.transaction().map_err(|e| e.to_string())?;
        for sh in list {
            if tx.query_row("SELECT 1 FROM shares WHERE token = ?1", [&sh.token], |_| Ok(())).is_ok() {
                continue;
            }
            let map = if sh.kind == "releases" { releases } else { tracks };
            let items: Vec<i64> = sh.items.iter().map(|i| *map.get(i).unwrap_or(i)).collect();
            let free = tx.query_row("SELECT 1 FROM shares WHERE id = ?1", [sh.id], |_| Ok(())).is_err();
            tx.execute(
                "INSERT INTO shares (id, token, label, message, kind, items, can_download, download_quality, expires_at, pw_salt, pw_hash, max_downloads,
                     downloads, views, plays, last_access, created_at, revoked)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                params![
                    free.then_some(sh.id),
                    sh.token,
                    sh.label,
                    sh.message,
                    sh.kind,
                    serde_json::to_string(&items).unwrap_or_default(),
                    sh.can_download as i64,
                    sh.download_quality,
                    sh.expires_at,
                    sh.pw_salt,
                    sh.pw_hash,
                    sh.max_downloads,
                    sh.downloads,
                    sh.views,
                    sh.plays,
                    sh.last_access,
                    sh.created_at,
                    sh.revoked as i64
                ],
            )
            .map_err(|e| e.to_string())?;
            let new_id = tx.last_insert_rowid();
            let mut ev = s.prepare("SELECT at, event, track_id FROM share_events WHERE share_id = ?1 ORDER BY id").map_err(|e| e.to_string())?;
            let events: Vec<(String, String, Option<i64>)> = ev.query_map([sh.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(|e| e.to_string())?.flatten().collect();
            for (at, event, track) in events {
                let track = track.map(|t| *tracks.get(&t).unwrap_or(&t));
                tx.execute("INSERT INTO share_events (share_id, at, event, track_id) VALUES (?1, ?2, ?3, ?4)", params![new_id, at, event, track]).map_err(|e| e.to_string())?;
            }
            n += 1;
        }
        let adopt = secret.filter(|_| empty && n > 0);
        if let Some(sec) = &adopt {
            tx.execute("UPDATE secrets SET v = ?1 WHERE k = 'cookie'", [sec]).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        if let Some(sec) = adopt {
            *self.secret.lock().unwrap_or_else(|e| e.into_inner()) = unhex(&sec);
        }
        Ok(n)
    }
}

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len() / 2).filter_map(|i| u8::from_str_radix(h.get(2 * i..2 * i + 2)?, 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_live_and_lock() {
        let p = std::env::temp_dir().join(format!("otoko-partages-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let s = ShareStore::open(&p).unwrap();
        assert!(s.create(&json!({ "kind": "tracks", "items": [] })).is_err(), "il faut au moins un morceau");
        let v = s.create(&json!({ "kind": "tracks", "items": [3, 1, 3], "label": "Démo", "password": "secret", "can_download": true, "max_downloads": 2 })).unwrap();
        assert_eq!((v["items"].clone(), v["has_password"].clone(), v["state"].clone()), (json!([1, 3]), json!(true), json!("active")));
        let sh = s.by_token(v["token"].as_str().unwrap()).unwrap();
        assert!(s.check_password(&sh, "secret") && !s.check_password(&sh, "faux"));
        let ck = s.cookie_value(&sh);
        assert!(s.cookie_ok(&sh, Some(&ck)) && !s.cookie_ok(&sh, Some("x")));
        s.record(sh.id, "download", Some(1));
        s.record(sh.id, "download", Some(1));
        assert!(!ShareStore::can_download(&s.by_token(&sh.token).unwrap()), "limite atteinte");
        let u = s.update(sh.id, &json!({ "revoked": true })).unwrap().unwrap();
        assert_eq!(u["state"], "revoked");
        assert_eq!(s.events(sh.id).len(), 2);
        assert!(s.delete(sh.id));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_strange_duration_is_refused() {
        for bad in [json!("NaN"), json!("inf"), json!(-1), json!(4000), json!("abc"), json!(true)] {
            assert!(expiry(&bad).is_err(), "{bad}");
        }
        assert!(expiry(&json!(7)).unwrap().is_some());
        assert!(expiry(&json!(0)).unwrap().is_none());
    }

    #[test]
    fn dates_and_hashes_match_studio_vault() {
        assert_eq!(iso(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_798_761_600)), "2027-01-01T00:00:00Z");
        // PBKDF2-HMAC-SHA256, 240 000 tours (valeur calculée par le Python de Studio Vault).
        assert_eq!(hash_pw("secret", "00112233445566778899aabbccddeeff"), "8c820185156465dc1cd247fadcf25364dba5a1441abaea9e6dd46665b23e86e1");
    }
}
