//! Discographie exportable (comme `services/discography.py`) : une page web autonome (pochettes
//! intégrées) ou un PDF A4, avec les sorties, leurs pochettes, dates, titres, durées et liens.
//! Les pochettes arrivent déjà en JPEG (l'application les prépare) ; le PDF est écrit ici, avec
//! les polices Helvetica de base (aucune police à intégrer).

use rusqlite::Connection;

use super::release_label;
use super::releases::fr_date;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiscTrack {
    pub title: String,
    /// L'artiste, quand ce n'est pas soi (un featuring, un remix).
    pub feat: String,
    pub duration: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disc {
    pub id: i64,
    pub title: String,
    pub meta: String,
    pub link: String,
    pub credits: String,
    pub cover_path: String,
    pub tracks: Vec<DiscTrack>,
}

/// Une pochette prête : (largeur, hauteur, JPEG en RVB).
pub type Cover = (u32, u32, Vec<u8>);

fn duration(s: Option<f64>) -> String {
    match s {
        Some(s) if s > 0.0 => {
            let s = s.round() as i64;
            format!("{}:{:02}", s / 60, s % 60)
        }
        _ => String::new(),
    }
}

/// Les sorties, de la plus récente à la plus ancienne (les sans date d'abord, comme Studio Vault).
pub fn collect(conn: &Connection, artist: &str, only_released: bool) -> Vec<Disc> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let rels: Vec<(i64, String, String, String, String, String, String)> = conn
        .prepare(
            "SELECT id, title, IFNULL(rtype, ''), IFNULL(release_date, ''), IFNULL(sc_link, ''), IFNULL(credits, ''), IFNULL(cover_path, '')
             FROM releases ORDER BY (IFNULL(release_date, '') = ''), release_date DESC, title",
        )
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?.collect())
        .unwrap_or_default();
    let me = artist.trim().to_lowercase();
    rels.into_iter()
        .filter(|r| !only_released || (!r.3.is_empty() && r.3 <= today))
        .map(|(id, title, rtype, date, link, credits, cover_path)| {
            let rows: Vec<(String, String, Option<f64>, String, String)> = conn
                .prepare(
                    "SELECT IFNULL(t.title, ''), t.filename, t.duration_s, IFNULL(t.artist, ''), IFNULL(t.sc_url, '') FROM release_tracks rt
                     JOIN tracks t ON t.id = rt.track_id WHERE rt.release_id = ?1 ORDER BY rt.position",
                )
                .and_then(|mut s| s.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect())
                .unwrap_or_default();
            let total: f64 = rows.iter().filter_map(|r| r.2).sum();
            let when = if date.is_empty() {
                "date à venir".to_string()
            } else if date > today {
                format!("à paraître le {}", fr_date(&date))
            } else {
                fr_date(&date)
            };
            let meta = [
                release_label(&rtype),
                when,
                if rows.is_empty() { String::new() } else { format!("{} titre(s)", rows.len()) },
                if total > 0.0 { duration(Some(total)) } else { String::new() },
            ]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
            let tracks = rows
                .into_iter()
                .map(|(t, f, d, a, u)| DiscTrack {
                    title: if t.is_empty() { std::path::Path::new(&f).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(f) } else { t },
                    feat: if !a.trim().is_empty() && !me.is_empty() && a.trim().to_lowercase() != me { a.trim().to_string() } else { String::new() },
                    duration: duration(d),
                    url: u,
                })
                .collect();
            Disc { id, title, meta, link, credits, cover_path, tracks }
        })
        .collect()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// La page web autonome (pochettes intégrées).
pub fn html(artist: &str, discs: &[Disc], cover: &dyn Fn(&Disc) -> Option<Cover>) -> String {
    let name = if artist.trim().is_empty() { "Discographie" } else { artist.trim() };
    let mut blocks = Vec::new();
    for r in discs {
        let img = cover(r).map(|(_, _, jpg)| format!("<img src=\"data:image/jpeg;base64,{}\" width=\"220\" height=\"220\" alt=\"\">", base64(&jpg))).unwrap_or_default();
        let rows: String = r
            .tracks
            .iter()
            .enumerate()
            .map(|(n, t)| {
                let mut title = esc(&t.title);
                if !t.url.is_empty() {
                    title = format!("<a href=\"{}\">{title}</a>", esc(&t.url));
                }
                let feat = if t.feat.is_empty() { String::new() } else { format!(" <span class=\"muted\">— {}</span>", esc(&t.feat)) };
                format!("<tr><td class=\"num\">{:02}</td><td>{title}{feat}</td><td class=\"dur\">{}</td></tr>", n + 1, t.duration)
            })
            .collect();
        let link = if r.link.is_empty() { String::new() } else { format!("<p><a href=\"{}\">Écouter sur SoundCloud</a></p>", esc(&r.link)) };
        let credits = if r.credits.is_empty() { String::new() } else { format!("<p class=\"muted small\">{}</p>", esc(&r.credits).replace('\n', "<br>")) };
        blocks.push(format!(
            "<section class=\"rel\"><div class=\"cov\">{img}</div><div class=\"body\"><h2>{}</h2><p class=\"muted\">{}</p>{link}<table class=\"tl\">{rows}</table>{credits}</div></section>",
            esc(&r.title),
            esc(&r.meta)
        ));
    }
    let body = if blocks.is_empty() { "<p>Aucune sortie.</p>".to_string() } else { blocks.join("\n") };
    format!(
        r#"<!doctype html>
<html lang="fr"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{t} — discographie</title>
<style>
body {{ font-family: "Segoe UI", Arial, sans-serif; color: #1d1d24; background: #ffffff; max-width: 900px; margin: 32px auto; padding: 0 16px; }}
h1 {{ font-size: 30px; margin: 0 0 4px 0; }}
h2 {{ font-size: 20px; margin: 0 0 4px 0; }}
.muted {{ color: #6b6b78; }}
.small {{ font-size: 12px; }}
.rel {{ display: flex; gap: 24px; margin: 0 0 28px 0; border-top: 1px solid #e3e3ea; padding-top: 18px; }}
.cov img {{ border-radius: 10px; display: block; }}
.body {{ flex: 1; min-width: 0; }}
.tl {{ width: 100%; border-collapse: collapse; }}
.tl td {{ border-bottom: 1px solid #f0f0f4; font-size: 14px; padding: 3px; }}
.num {{ color: #9a9aa6; width: 28px; }}
.dur {{ color: #6b6b78; text-align: right; width: 50px; }}
a {{ color: #c2410c; text-decoration: none; }}
@media (max-width: 600px) {{ .rel {{ flex-direction: column; }} }}
</style></head>
<body>
<h1>{t}</h1>
<p class="muted">Discographie — {n} sortie(s) · mise à jour le {d}</p>
{body}
</body></html>
"#,
        t = esc(name),
        n = discs.len(),
        d = chrono::Local::now().format("%d/%m/%Y"),
    )
}

// ---------------------------------------------------------------------- PDF

/// Largeurs des polices Helvetica et Helvetica-Bold (millièmes de corps), de l'espace au tilde.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722,
    722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556,
    556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722,
    722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611,
    611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// Le caractère dans l'encodage WinAnsi des polices de base (« ? » s'il n'y est pas).
fn win_ansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        '‰' => 0x89,
        'Š' => 0x8a,
        '‹' => 0x8b,
        'Œ' => 0x8c,
        'Ž' => 0x8e,
        '™' => 0x99,
        'š' => 0x9a,
        '›' => 0x9b,
        'ž' => 0x9e,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        'œ' => 0x9c,
        'Ÿ' => 0x9f,
        _ => b'?',
    }
}

/// La lettre de base d'un caractère accentué (pour sa largeur).
fn base_letter(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
        'À' | 'Á' | 'Â' | 'Ä' | 'Ã' | 'Å' => 'A',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'î' | 'ï' | 'í' | 'ì' => 'i',
        'Î' | 'Ï' | 'Í' | 'Ì' => 'I',
        'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
        'Ô' | 'Ö' | 'Ó' | 'Ò' | 'Õ' => 'O',
        'û' | 'ü' | 'ú' | 'ù' => 'u',
        'Û' | 'Ü' | 'Ú' | 'Ù' => 'U',
        'ç' => 'c',
        'Ç' => 'C',
        'ñ' => 'n',
        'Ñ' => 'N',
        'ÿ' => 'y',
        _ => c,
    }
}

fn text_width(s: &str, size: f64, bold: bool) -> f64 {
    let table = if bold { &HELV_BOLD } else { &HELV };
    plain(s)
        .chars()
        .map(|c| {
            let c = base_letter(c);
            match c {
                ' '..='~' => table[c as usize - 32] as f64,
                '–' => 556.0,
                '—' | '…' => 1000.0,
                '’' | '‘' => 222.0,
                _ => 556.0,
            }
        })
        .sum::<f64>()
        * size
        / 1000.0
}

/// Raccourcit le texte pour qu'il tienne dans `width` (avec « … »).
fn elide(s: &str, size: f64, bold: bool, width: f64) -> String {
    if text_width(s, size, bold) <= width {
        return s.to_string();
    }
    let mut out: String = s.to_string();
    while !out.is_empty() && text_width(&format!("{out}…"), size, bold) > width {
        out.pop();
    }
    format!("{}…", out.trim_end())
}

/// Coupe le texte en lignes qui tiennent dans `width`.
fn wrap(s: &str, size: f64, width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.lines() {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if text_width(&candidate, size, false) > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

/// Les signes absents de WinAnsi qu'on sait écrire autrement (℗ des crédits de disque…).
fn plain(s: &str) -> String {
    s.replace('℗', "(P)").replace('≈', "~").replace(['‐', '‑', '‒'], "-").replace('′', "'").replace('″', "\"")
}

fn pdf_string(s: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in plain(s).chars() {
        let b = win_ansi(c);
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

struct Page {
    content: Vec<u8>,
    /// (x1, y1, x2, y2, adresse) : les liens cliquables.
    links: Vec<(f64, f64, f64, f64, String)>,
    images: Vec<usize>,
}

impl Page {
    fn new() -> Page {
        Page { content: Vec::new(), links: Vec::new(), images: Vec::new() }
    }

    fn text(&mut self, x: f64, y: f64, size: f64, bold: bool, gray: f64, s: &str) {
        self.content.extend(format!("BT /{} {size:.1} Tf {gray:.3} {gray:.3} {gray:.3} rg {x:.2} {y:.2} Td ", if bold { "F2" } else { "F1" }).as_bytes());
        self.content.extend(pdf_string(s));
        self.content.extend(b" Tj ET\n");
    }

    fn colored(&mut self, x: f64, y: f64, size: f64, rgb: (f64, f64, f64), s: &str) {
        self.content.extend(format!("BT /F1 {size:.1} Tf {:.3} {:.3} {:.3} rg {x:.2} {y:.2} Td ", rgb.0, rgb.1, rgb.2).as_bytes());
        self.content.extend(pdf_string(s));
        self.content.extend(b" Tj ET\n");
    }

    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, gray: f64) {
        self.content.extend(format!("{gray:.3} {gray:.3} {gray:.3} RG 0.6 w {x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S\n").as_bytes());
    }

    fn image(&mut self, idx: usize, x: f64, y: f64, w: f64, h: f64) {
        self.content.extend(format!("q {w:.2} 0 0 {h:.2} {x:.2} {y:.2} cm /Im{idx} Do Q\n").as_bytes());
        self.images.push(idx);
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, rgb: (f64, f64, f64)) {
        self.content.extend(format!("{:.3} {:.3} {:.3} rg {x:.2} {y:.2} {w:.2} {h:.2} re f\n", rgb.0, rgb.1, rgb.2).as_bytes());
    }
}

/// Le PDF A4 de la discographie.
pub fn pdf(artist: &str, discs: &[Disc], cover: &dyn Fn(&Disc) -> Option<Cover>) -> Vec<u8> {
    const W: f64 = 595.28;
    const H: f64 = 841.89;
    const M: f64 = 42.0;
    const COV: f64 = 120.0;
    let name = if artist.trim().is_empty() { "Discographie" } else { artist.trim() };
    let mut images: Vec<Cover> = Vec::new();
    let mut pages: Vec<Page> = vec![Page::new()];
    let mut y = H - M;
    {
        let p = pages.last_mut().unwrap();
        p.text(M, y - 24.0, 24.0, true, 0.11, name);
        p.text(M, y - 42.0, 10.0, false, 0.42, &format!("Discographie — {} sortie(s) · mise à jour le {}", discs.len(), chrono::Local::now().format("%d/%m/%Y")));
    }
    y -= 62.0;
    let tx = M + COV + 18.0;
    let tw = W - M - tx;
    for r in discs {
        // Hauteur du bloc, pour savoir s'il tient sur la page.
        let credits = if r.credits.trim().is_empty() { Vec::new() } else { wrap(r.credits.trim(), 8.5, tw) };
        let text_h = 22.0 + 15.0 + if r.link.is_empty() { 0.0 } else { 15.0 } + r.tracks.len() as f64 * 15.0 + if credits.is_empty() { 0.0 } else { 8.0 + credits.len() as f64 * 11.0 };
        let block = text_h.max(COV) + 26.0;
        if y - block < M && y < H - M - 80.0 {
            pages.push(Page::new());
            y = H - M;
        }
        let p = pages.last_mut().unwrap();
        p.line(M, y, W - M, y, 0.89);
        let top = y - 16.0;
        match cover(r) {
            Some(img) => {
                images.push(img);
                p.image(images.len() - 1, M, top - COV, COV, COV);
            }
            None => {
                let hue = r.title.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32)) % 6;
                let rgb = [(0.33, 0.29, 0.72), (0.06, 0.43, 0.34), (0.6, 0.24, 0.11), (0.09, 0.37, 0.65), (0.6, 0.21, 0.34), (0.23, 0.43, 0.07)][hue as usize];
                p.rect(M, top - COV, COV, COV, rgb);
                let initials: String = r.title.split_whitespace().filter_map(|w| w.chars().next()).filter(|c| c.is_alphanumeric()).take(2).flat_map(|c| c.to_uppercase()).collect();
                p.text(M + 10.0, top - 36.0, 28.0, true, 1.0, &initials);
            }
        }
        let mut ty = top - 16.0;
        p.text(tx, ty, 16.0, true, 0.11, &elide(&r.title, 16.0, true, tw));
        ty -= 16.0;
        p.text(tx, ty, 10.0, false, 0.42, &elide(&r.meta, 10.0, false, tw));
        if !r.link.is_empty() {
            ty -= 15.0;
            let label = "Écouter sur SoundCloud";
            p.colored(tx, ty, 10.0, (0.76, 0.25, 0.05), label);
            p.links.push((tx, ty - 2.0, tx + text_width(label, 10.0, false), ty + 10.0, r.link.clone()));
        }
        ty -= 6.0;
        for (n, t) in r.tracks.iter().enumerate() {
            ty -= 15.0;
            p.text(tx, ty, 10.0, false, 0.6, &format!("{:02}", n + 1));
            let dur_w = text_width(&t.duration, 10.0, false);
            let room = tw - 26.0 - dur_w - 10.0;
            let title = elide(&t.title, 10.0, false, room);
            if t.url.is_empty() {
                p.text(tx + 22.0, ty, 10.0, false, 0.11, &title);
            } else {
                p.colored(tx + 22.0, ty, 10.0, (0.76, 0.25, 0.05), &title);
                p.links.push((tx + 22.0, ty - 2.0, tx + 22.0 + text_width(&title, 10.0, false), ty + 10.0, t.url.clone()));
            }
            if !t.feat.is_empty() {
                let x = tx + 22.0 + text_width(&title, 10.0, false);
                let feat = elide(&format!(" — {}", t.feat), 10.0, false, (room - text_width(&title, 10.0, false)).max(0.0));
                if feat.chars().count() > 3 {
                    p.text(x, ty, 10.0, false, 0.42, &feat);
                }
            }
            p.text(W - M - dur_w, ty, 10.0, false, 0.42, &t.duration);
            p.line(tx, ty - 4.0, W - M, ty - 4.0, 0.94);
        }
        if !credits.is_empty() {
            ty -= 8.0;
            for l in &credits {
                ty -= 11.0;
                p.text(tx, ty, 8.5, false, 0.42, l);
            }
        }
        y = (top - COV).min(ty) - 18.0;
    }
    if discs.is_empty() {
        pages[0].text(M, y, 11.0, false, 0.11, "Aucune sortie.");
    }
    // Objets : 1 catalogue, 2 pages, 3 et 4 polices, puis les images, puis chaque page (page,
    // contenu, liens).
    let mut objs: Vec<Vec<u8>> = Vec::new();
    let n_img = images.len();
    let first_page = 5 + n_img;
    let mut page_ids = Vec::new();
    let mut next = first_page;
    for p in &pages {
        page_ids.push(next);
        next += 2 + p.links.len();
    }
    objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objs.push(format!("<< /Type /Pages /Count {} /Kids [{}] >>", pages.len(), page_ids.iter().map(|i| format!("{i} 0 R")).collect::<Vec<_>>().join(" ")).into_bytes());
    objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
    objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec());
    for (w, h, jpg) in &images {
        let mut o = format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", jpg.len()).into_bytes();
        o.extend(jpg);
        o.extend(b"\nendstream");
        objs.push(o);
    }
    for (k, p) in pages.iter().enumerate() {
        let id = page_ids[k];
        let xobj: String = p.images.iter().map(|i| format!("/Im{i} {} 0 R ", 5 + i)).collect();
        let annots: String = (0..p.links.len()).map(|j| format!("{} 0 R ", id + 2 + j)).collect();
        objs.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W} {H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> /XObject << {xobj}>> >> /Contents {} 0 R{} >>",
                id + 1,
                if annots.is_empty() { String::new() } else { format!(" /Annots [{annots}]") }
            )
            .into_bytes(),
        );
        let mut c = format!("<< /Length {} >>\nstream\n", p.content.len()).into_bytes();
        c.extend(&p.content);
        c.extend(b"endstream");
        objs.push(c);
        for (x1, y1, x2, y2, url) in &p.links {
            let mut o = format!("<< /Type /Annot /Subtype /Link /Rect [{x1:.2} {y1:.2} {x2:.2} {y2:.2}] /Border [0 0 0] /A << /S /URI /URI ").into_bytes();
            o.extend(pdf_string(url));
            o.extend(b" >> >>");
            objs.push(o);
        }
    }
    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// Le nom proposé pour le fichier.
pub fn file_name(artist: &str, ext: &str) -> String {
    let base = if artist.trim().is_empty() { "Discographie".to_string() } else { format!("{} - discographie", artist.trim()) };
    format!("{}.{ext}", base.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_and_html() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (id, path, filename, title, duration_s, artist, sc_url) VALUES
               (1, 'a.wav', 'a.wav', 'Nuit (été)', 245, 'Otoko', 'https://soundcloud.com/otoko/nuit'),
               (2, 'b.wav', 'b.wav', '', 200, 'Daft', '');
             INSERT INTO releases (id, title, rtype, release_date, sc_link, credits) VALUES (1, 'Été', 'ep', '2020-07-08', 'https://soundcloud.com/otoko/sets/ete', 'Composé par Otoko'),
               (2, 'Bientôt', 'single', '2099-01-01', '', '');
             INSERT INTO release_tracks VALUES (1, 1, 1), (1, 2, 2);",
        )
        .unwrap();
        let discs = collect(&conn, "Otoko", false);
        assert_eq!(discs.len(), 2);
        assert_eq!(discs[0].meta, "Single · à paraître le 01/01/2099");
        assert_eq!(discs[1].meta, "EP · 08/07/2020 · 2 titre(s) · 7:25");
        assert_eq!((discs[1].tracks[1].title.as_str(), discs[1].tracks[1].feat.as_str()), ("b", "Daft"));
        assert_eq!(collect(&conn, "Otoko", true).len(), 1);
        let h = html("Otoko", &discs, &|_| None);
        assert!(h.contains("<h1>Otoko</h1>") && h.contains("href=\"https://soundcloud.com/otoko/nuit\">Nuit (été)</a>"));
        let p = pdf("Otoko", &discs, &|_| None);
        let text = String::from_utf8_lossy(&p);
        assert!(p.starts_with(b"%PDF-1.4") && text.ends_with("%%EOF\n"));
        assert!(text.contains("/URI (https://soundcloud.com/otoko/nuit)"));
        // L'accent passe en WinAnsi (é = 0xE9) ; ℗ s'écrit (P).
        assert!(p.windows(4).any(|w| w == b"(\xc9t\xe9"));
        assert_eq!(pdf_string("℗ 2025 (Otoko)"), b"(\\(P\\) 2025 \\(Otoko\\))".to_vec());
        assert_eq!(base64(b"OTOKO!"), "T1RPS08h");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(file_name("Otoko", "pdf"), "Otoko - discographie.pdf");
    }
}
