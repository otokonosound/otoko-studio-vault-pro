//! Les liens par morceau : le lien d'une sortie sert à retrouver celui de chacun de ses titres. La page d'un set
//! SoundCloud ne détaille que ses cinq premiers titres ; le flux public du compte les donne tous, avec leur lien. Ce
//! module ne fait que lire ce qu'on lui donne : le réseau reste à l'application.

use std::sync::OnceLock;

use regex::Regex;
use rusqlite::{Connection, params};

use super::{projects, releases};

pub fn is_soundcloud(url: &str) -> bool {
    let u = url.trim().to_lowercase();
    u.starts_with("https://soundcloud.com/") || u.starts_with("https://www.soundcloud.com/") || u.starts_with("https://on.soundcloud.com/")
}

/// Le compte SoundCloud nommé dans une page.
pub fn user_id(html: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#""urn":"soundcloud:users:(\d+)""#).expect("expression")).captures(html).map(|c| c[1].to_string())
}

/// L'adresse du flux public d'un compte.
pub fn feed_url(user: &str) -> String {
    format!("https://feeds.soundcloud.com/users/soundcloud:users:{user}/sounds.rss")
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'")
}

/// Une page du flux : (titre, lien) de chaque morceau, et l'adresse de la page suivante.
pub fn parse_feed(xml: &str) -> (Vec<(String, String)>, Option<String>) {
    static ITEM: OnceLock<Regex> = OnceLock::new();
    static NEXT: OnceLock<Regex> = OnceLock::new();
    let item = ITEM.get_or_init(|| Regex::new(r"(?s)<item>.*?<title>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</title>.*?<link>(.*?)</link>.*?</item>").expect("expression"));
    let next = NEXT.get_or_init(|| Regex::new(r#"<atom:link href="([^"]+)" rel="next""#).expect("expression"));
    let list = item.captures_iter(xml).map(|c| (unescape(c[1].trim()), unescape(c[2].trim()))).collect();
    (list, next.captures(xml).map(|c| unescape(&c[1])))
}

/// Un titre comparable : sans ce qui est entre parenthèses, sans invité, sans accents ni ponctuation.
pub fn norm_title(t: &str) -> String {
    static PAREN: OnceLock<Regex> = OnceLock::new();
    static FEAT: OnceLock<Regex> = OnceLock::new();
    let t = PAREN.get_or_init(|| Regex::new(r"[(\[][^)\]]*[)\]]").expect("expression")).replace_all(t, " ");
    let t = FEAT.get_or_init(|| Regex::new(r"(?i)\s(feat\.?|ft\.?|featuring)\s.*$").expect("expression")).replace(&t, "");
    let folded: String = t
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// « Chap. 1 - Unknown World », « Circuit 14 : Kreyòl Horizon » : le titre sans ce qui le précède sur SoundCloud.
fn after_prefix(t: &str) -> Option<String> {
    [" - ", " – ", " : "].iter().filter_map(|sep| t.rfind(sep).map(|i| i + sep.len())).max().map(|i| norm_title(&t[i..]))
}

/// Le lien d'un titre : le même titre d'abord, sinon le seul qui finit par ce titre.
fn find_in<'a>(feed: &'a [(String, String)], want: &str) -> Option<&'a str> {
    if want.is_empty() {
        return None;
    }
    if let Some(f) = feed.iter().find(|(t, _)| norm_title(t) == want) {
        return Some(&f.1);
    }
    let mut hits = feed.iter().filter(|(t, _)| after_prefix(t).as_deref() == Some(want));
    match (hits.next(), hits.next()) {
        (Some(f), None) => Some(&f.1),
        _ => None,
    }
}

/// Ce que la liaison a fait pour une sortie.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub linked: usize,
    pub already: usize,
    /// Les titres que le flux ne connaît pas : privés, renommés, ou plus anciens que ses 150 derniers morceaux.
    pub unknown: Vec<String>,
}

/// Les titres d'une sortie, reliés à leur lien. Un single prend le lien de la sortie ; un lien déjà rempli n'est pas
/// touché.
pub fn link_release(conn: &Connection, release: i64, feed: &[(String, String)]) -> rusqlite::Result<Report> {
    let mut r = Report::default();
    let Some(s) = releases::load(conn, release) else { return Ok(r) };
    let is_set = s.sc_link.contains("/sets/");
    for id in &s.tracks {
        let (title, file, url): (String, String, String) =
            conn.query_row("SELECT IFNULL(title, ''), IFNULL(filename, ''), IFNULL(sc_url, '') FROM tracks WHERE id = ?1", [id], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)))?;
        if !url.trim().is_empty() {
            r.already += 1;
            continue;
        }
        let clean = projects::clean_title(&title, &file);
        let want = norm_title(&clean);
        let found = if !is_set && s.tracks.len() == 1 && is_soundcloud(&s.sc_link) {
            Some(s.sc_link.trim().to_string())
        } else {
            find_in(feed, &want).map(str::to_string)
        };
        match found {
            Some(link) => {
                conn.execute("UPDATE tracks SET sc_url = ?2, modified_at = datetime('now') WHERE id = ?1", params![id, link])?;
                r.linked += 1;
            }
            None => r.unknown.push(clean),
        }
    }
    Ok(r)
}

/// Les sorties qui ont un lien SoundCloud et des titres sans lien.
pub fn to_link(conn: &Connection) -> Vec<(i64, String, String)> {
    let list: Vec<(i64, String, String)> = conn
        .prepare(
            "SELECT r.id, r.title, r.sc_link FROM releases r WHERE IFNULL(r.sc_link, '') <> ''
               AND EXISTS (SELECT 1 FROM release_tracks rt JOIN tracks t ON t.id = rt.track_id WHERE rt.release_id = r.id AND IFNULL(t.sc_url, '') = '')
             ORDER BY r.release_date DESC",
        )
        .and_then(|mut s| s.query_map([], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)))?.collect())
        .unwrap_or_default();
    list.into_iter().filter(|r| is_soundcloud(&r.2)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<rss><channel>
        <atom:link href="https://feeds.soundcloud.com/users/soundcloud:users:1/sounds.rss" rel="self" type="application/rss+xml"/>
        <atom:link href="https://feeds.soundcloud.com/users/soundcloud:users:1/sounds.rss?before=2&amp;x=1" rel="next" type="application/rss+xml"/>
        <title>Otoko No Sound</title><link>https://soundcloud.com/otokonosound</link>
        <item><guid>1</guid><title>Dôme Quichotte (Original Track - Château Perché Tribute)</title><pubDate>x</pubDate><link>https://soundcloud.com/o/dome-quichotte-5</link></item>
        <item><title>Rock &amp; Roll</title><link>https://soundcloud.com/o/rock</link></item>
    </channel></rss>"#;

    #[test]
    fn the_feed_reads_titles_and_next_page() {
        let (list, next) = parse_feed(FEED);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].1, "https://soundcloud.com/o/dome-quichotte-5");
        assert_eq!(list[1].0, "Rock & Roll");
        assert_eq!(next.as_deref(), Some("https://feeds.soundcloud.com/users/soundcloud:users:1/sounds.rss?before=2&x=1"));
        assert_eq!(user_id(r#"x"urn":"soundcloud:users:525752529"y"#).as_deref(), Some("525752529"));
    }

    #[test]
    fn titles_compare_without_their_decorations() {
        assert_eq!(norm_title("Dôme Quichotte (Original Track - Château Perché Tribute)"), "dome quichotte");
        assert_eq!(norm_title("Yakushima Shiratani feat. Casdu"), "yakushima shiratani");
        assert_eq!(norm_title("Can't believe it"), "can t believe it");
    }

    #[test]
    fn a_numbered_title_is_found_when_it_is_the_only_one() {
        let feed: Vec<(String, String)> = [("Chap. 1 - Unknown World", "a"), ("Circuit 14 : Kreyòl Horizon", "b"), ("Opening - Pendulum", "c"), ("Live - Echo", "d"), ("Mix - Echo", "e"), ("Welcome", "f"), ("Chap. 2 - Welcome", "g")].iter().map(|(t, l)| (t.to_string(), l.to_string())).collect();
        assert_eq!(find_in(&feed, "unknown world"), Some("a"));
        assert_eq!(find_in(&feed, &norm_title("Kreyòl Horizon")), Some("b"));
        assert_eq!(find_in(&feed, "welcome"), Some("f"), "le même titre passe d'abord");
        assert_eq!(find_in(&feed, "echo"), None, "deux candidats : on ne devine pas");
        assert_eq!(find_in(&feed, ""), None);
    }

    #[test]
    fn a_release_links_its_tracks() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, title, sc_url) VALUES
                (1, 'a', 'Dôme Quichotte.wav', 'Dôme Quichotte', ''),
                (2, 'b', 'Inédit.wav', 'Inédit', ''),
                (3, 'c', 'Rock.wav', 'Rock & Roll', 'https://deja'),
                (4, 'd', 'Solo.wav', 'Solo', '');
             INSERT INTO releases (id, title, rtype, sc_link) VALUES (1, 'Set', 'ep', 'https://soundcloud.com/o/sets/set'), (2, 'Solo', 'single', 'https://soundcloud.com/o/solo');
             INSERT INTO release_tracks (release_id, track_id, position) VALUES (1, 1, 1), (1, 2, 2), (1, 3, 3), (2, 4, 1);",
        )
        .unwrap();
        assert_eq!(to_link(&conn).len(), 2);
        let (feed, _) = parse_feed(FEED);
        let r = link_release(&conn, 1, &feed).unwrap();
        assert_eq!((r.linked, r.already, r.unknown), (1, 1, vec!["Inédit".to_string()]));
        let url: String = conn.query_row("SELECT sc_url FROM tracks WHERE id = 1", [], |x| x.get(0)).unwrap();
        assert_eq!(url, "https://soundcloud.com/o/dome-quichotte-5");
        assert_eq!(link_release(&conn, 2, &[]).unwrap().linked, 1, "un single prend le lien de sa sortie");
        assert!(!is_soundcloud("https://example.com/x"));
    }
}
