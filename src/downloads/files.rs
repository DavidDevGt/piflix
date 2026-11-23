//! Rutas, nombres y archivos `.part` de las descargas y subidas.

use super::jobs::NewJob;
use crate::prelude::*;

pub(crate) const MIN_FREE_MARGIN: u64 = 512 * 1024 * 1024;

pub(crate) const MIN_FREE_UNKNOWN: u64 = 2 * 1024 * 1024 * 1024;

pub(crate) const MIN_RESUME: u64 = 1024 * 1024; // un .part menor se trunca y se reinicia

// ---------------------------------------------------------------- .part, sidecar y reanudación

/// `.nombre.mp4.part`: oculto, así que el vigilante de la biblioteca nunca lo ve.
pub(crate) fn part_path(dest: &Path) -> PathBuf {
    let f = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    dest.with_file_name(format!(".{f}.part"))
}

/// `.nombre.mp4.part.json`: solo validator y total (nunca URLs firmadas).
pub(crate) fn meta_path(dest: &Path) -> PathBuf {
    let mut s = part_path(dest).into_os_string();
    s.push(".json");
    PathBuf::from(s)
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Meta {
    pub(crate) validator: Option<String>,
    pub(crate) total: Option<u64>,
}

/// Corrupto o inexistente => None => se empieza de cero (el lado seguro).
pub(crate) async fn read_meta(dest: &Path) -> Option<Meta> {
    serde_json::from_slice(&tokio::fs::read(meta_path(dest)).await.ok()?).ok()
}

pub(crate) async fn write_meta(dest: &Path, m: &Meta) {
    if let Ok(b) = serde_json::to_vec(m) {
        let _ = tokio::fs::write(meta_path(dest), b).await;
    }
}

pub(crate) async fn drop_part(dest: &Path) {
    let dest = dest.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || drop_part_sync(&dest)).await;
}

pub(crate) fn drop_part_sync(dest: &Path) {
    let _ = std::fs::remove_file(part_path(dest));
    let _ = std::fs::remove_file(meta_path(dest));
}

/// ETag fuerte si existe; si no, Last-Modified. Un ETag débil (W/) no sirve en If-Range.
pub(crate) fn pick_validator(etag: &str, lm: &str) -> Option<String> {
    let ok = |v: &str| !v.is_empty() && v.len() <= 200 && !v.chars().any(|c| c.is_control() || c == '\\');
    if ok(etag) && !etag.starts_with("W/") {
        Some(etag.to_string())
    } else if ok(lm) {
        Some(lm.to_string())
    } else {
        None
    }
}

/// ¿Hay un .part de este título? La extensión sale de la sonda, así que se prueban todas.
pub(crate) async fn find_part(root: &Path, req: &NewJob, kind: Kind) -> Option<(PathBuf, u64)> {
    for d in candidate_dests(root, req, kind) {
        if let Ok(m) = tokio::fs::metadata(part_path(&d)).await {
            return Some((d, m.len()));
        }
    }
    None
}

/// Todos los destinos posibles de una petición: uno por extensión de video (la real sale de la sonda).
pub(crate) fn candidate_dests(root: &Path, req: &NewJob, kind: Kind) -> Vec<PathBuf> {
    VIDEO_EXT.iter().filter_map(|e| build_dest(root, req, kind, e).ok().map(|(d, _)| d)).collect()
}

// ---------------------------------------------------------------- nombres y rutas

pub(crate) fn fold(c: char) -> char {
    match c.to_lowercase().next().unwrap_or(c) {
        'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        x => x,
    }
}

pub(crate) fn slug(s: &str) -> String {
    let mut o = String::new();
    let mut dash = false;
    for c in s.chars().map(fold) {
        if c.is_ascii_alphanumeric() {
            o.push(c);
            dash = false;
        } else if !dash && !o.is_empty() {
            o.push('-');
            dash = true;
        }
    }
    let t = o.trim_matches('-');
    t.chars().take(80).collect()
}

pub(crate) fn clean_dir(s: &str) -> String {
    let t: String = s.chars().filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c)).collect();
    t.trim().trim_matches('.').trim().chars().take(80).collect::<String>().trim().to_string()
}

pub(crate) fn pick_ext(url: &str, ctype: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or("");
    if let Some(ext) = path.rsplit('.').next() {
        let ext = ext.to_ascii_lowercase();
        if let Some(v) = VIDEO_EXT.iter().find(|x| **x == ext.as_str()) {
            return v;
        }
    }
    if ctype.contains("webm") {
        "webm"
    } else if ctype.contains("matroska") {
        "mkv"
    } else {
        "mp4"
    }
}

/// Devuelve (ruta destino, nombre para mostrar). Nunca incluye separadores del cliente.
pub(crate) fn build_dest(root: &Path, req: &NewJob, kind: Kind, ext: &str) -> Result<(PathBuf, String), String> {
    let title = req.title.trim();
    let sl = slug(title);
    if sl.is_empty() {
        return Err("Escribe un título válido (letras o números)".into());
    }
    if kind == Kind::Pelicula {
        let (file, name) = match req.year {
            Some(y) if (1900..=2100).contains(&y) => (format!("{sl}-{y}.{ext}"), format!("{title} ({y})")),
            Some(_) => return Err("El año debe estar entre 1900 y 2100".into()),
            None => (format!("{sl}.{ext}"), title.to_string()),
        };
        Ok((root.join("Peliculas").join(file), name))
    } else {
        let season = req.season.ok_or("Indica la temporada")?;
        let episode = req.episode.ok_or("Indica el episodio")?;
        if season > 99 || !(1..=999).contains(&episode) {
            return Err("Temporada (0-99) o episodio (1-999) fuera de rango".into());
        }
        let dir = clean_dir(title);
        if dir.is_empty() {
            return Err("Escribe un nombre de serie válido".into());
        }
        let file = format!("{sl}-s{season:02}e{episode:02}.{ext}");
        let path = root.join("Series").join(dir).join(format!("Temporada {season}")).join(file);
        Ok((path, format!("{title} · T{season} E{episode}")))
    }
}

pub(crate) async fn free_bytes(dir: &Path) -> Option<u64> {
    let out = Command::new("df").arg("-Pk").arg(dir).output().await.ok()?;
    let txt = String::from_utf8_lossy(&out.stdout).to_string();
    let kb: u64 = txt.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()?;
    Some(kb * 1024)
}

pub(crate) fn magic_ok(ext: &str, head: &[u8]) -> bool {
    match ext {
        "webm" | "mkv" => head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]),
        "avi" => head.starts_with(b"RIFF"),
        _ => head.len() >= 8 && matches!(&head[4..8], b"ftyp" | b"moov" | b"mdat" | b"free" | b"wide" | b"skip"),
    }
}
