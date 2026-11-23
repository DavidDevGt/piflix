//! Biblioteca: escaneo del disco, reconocimiento de nombres, series y vigilancia de cambios.

use std::collections::BTreeMap;

use crate::prelude::*;

// ---------------------------------------------------------------- utilidades

/// FNV-1a de 64 bits: estable entre versiones de Rust (sirve de nombre de caché).
pub(crate) fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

pub(crate) fn capitalize(w: &str, first: bool) -> String {
    const SMALL: &[&str] = &["de", "del", "la", "el", "los", "las", "y", "en", "a", "of", "the", "and"];
    let lw = w.to_lowercase();
    if !first && SMALL.contains(&lw.as_str()) {
        return lw;
    }
    let mut c = lw.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub(crate) fn prettify(words: &[&str]) -> String {
    words.iter().enumerate().map(|(i, w)| capitalize(w, i == 0)).collect::<Vec<_>>().join(" ")
}

/// "el-hijo-de-batman-2014" -> ("El Hijo de Batman", Some(2014))
/// "2014", "(2014)" o "[2014]" -> Some(2014) si es un año plausible.
fn year_token(t: &str) -> Option<u16> {
    let t = t.trim_matches(|c| matches!(c, '(' | ')' | '[' | ']'));
    let y: u16 = t.parse().ok().filter(|_| t.len() == 4)?;
    (1900..=2100).contains(&y).then_some(y)
}

pub(crate) fn parse_name(stem: &str) -> (String, Option<u16>) {
    let cleaned: String = stem.chars().map(|c| if matches!(c, '-' | '_' | '.') { ' ' } else { c }).collect();
    let mut words: Vec<&str> = cleaned.split_whitespace().collect();
    let mut year = None;
    if let Some(y) = words.last().and_then(|last| year_token(last)) {
        year = Some(y);
        words.pop();
    }
    (prettify(&words), year)
}

/// Nombre de carpeta/serie: si ya viene con espacios se respeta; si es un slug se embellece.
pub(crate) fn display_name(raw: &str) -> (String, Option<u16>) {
    let raw = raw.trim();
    if raw.contains(char::is_whitespace) {
        if let Some((head, tail)) = raw.rsplit_once(' ') {
            if let Some(y) = year_token(tail) {
                return (head.trim().to_string(), Some(y));
            }
        }
        (raw.to_string(), None)
    } else {
        parse_name(raw)
    }
}

pub(crate) fn is_hidden(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('$') || name == "System Volume Information"
}

// ---------------------------------------------------------------- escaneo

pub(crate) struct RawFile {
    pub(crate) path: PathBuf,
    pub(crate) rel: String,
    pub(crate) size: u64,
}

pub(crate) fn walk(root: &Path) -> Vec<RawFile> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if is_hidden(&name) {
                continue;
            }
            let path = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(path);
                continue;
            }
            let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !VIDEO_EXT.contains(&ext.as_str()) {
                continue;
            }
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(RawFile { path, rel, size });
        }
    }
    out
}

/// Aplica carátulas y metadatos ya guardados en la caché.
pub(crate) fn apply_cache(item: &mut Item, cache: &Path) {
    let file = |kind| cache_file(cache, &item.id, kind);
    let (poster, backdrop, meta, title) =
        (file(CacheKind::Poster), file(CacheKind::Backdrop), file(CacheKind::Meta), file(CacheKind::Title));
    item.has_poster = poster.exists();
    item.has_backdrop = backdrop.exists();
    item.poster_v = std::fs::metadata(&poster)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    if let Some(v) = std::fs::read_to_string(meta).ok().and_then(|txt| serde_json::from_str::<Value>(&txt).ok()) {
        if let Some(t) = v["title"].as_str().filter(|t| !t.is_empty()) {
            item.set_title(t);
        }
        item.overview = v["overview"].as_str().filter(|t| !t.is_empty()).map(String::from);
        if item.year.is_none() {
            item.year = v["year"].as_u64().map(|y| y as u16);
        }
    }
    // el título puesto a mano gana sobre el de TMDB
    if let Ok(t) = std::fs::read_to_string(title) {
        let t = t.trim();
        if !t.is_empty() {
            item.set_title(t);
        }
    }
}

pub(crate) fn movies(files: Vec<RawFile>, cache: &Path) -> Vec<Item> {
    files
        .into_iter()
        .map(|f| {
            let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let (title, year) = parse_name(&stem);
            let mut item = Item {
                year,
                file: f.rel.clone(),
                size: f.size,
                path: f.path,
                ..Item::new(Kind::Pelicula, fnv(&format!("pelicula/{}", f.rel)), title)
            };
            apply_cache(&mut item, cache);
            item
        })
        .collect()
}

pub(crate) fn num(b: &[u8], mut j: usize, max: usize) -> Option<(u32, usize)> {
    let start = j;
    while j < b.len() && j - start < max && b[j].is_ascii_digit() {
        j += 1;
    }
    if j == start {
        return None;
    }
    std::str::from_utf8(&b[start..j]).ok()?.parse().ok().map(|n| (n, j))
}

/// Busca "S01E02" o "1x02" en el nombre. Devuelve (temporada, episodio, inicio, fin).
pub(crate) fn find_marker(stem: &str) -> Option<(u32, u32, usize, usize)> {
    let b = stem.as_bytes();
    for i in 0..b.len() {
        if i > 0 && b[i - 1].is_ascii_alphanumeric() {
            continue;
        }
        if b[i].eq_ignore_ascii_case(&b's') {
            if let Some((s, j)) = num(b, i + 1, 2) {
                if j < b.len() && b[j].eq_ignore_ascii_case(&b'e') {
                    if let Some((e, k)) = num(b, j + 1, 3) {
                        if k >= b.len() || !b[k].is_ascii_alphabetic() {
                            return Some((s, e, i, k));
                        }
                    }
                }
            }
        }
        if b[i].is_ascii_digit() {
            if let Some((s, j)) = num(b, i, 2) {
                if j < b.len() && b[j].eq_ignore_ascii_case(&b'x') {
                    if let Some((e, k)) = num(b, j + 1, 3) {
                        if k - (j + 1) >= 2 && (k >= b.len() || !b[k].is_ascii_alphabetic()) {
                            return Some((s, e, i, k));
                        }
                    }
                }
            }
        }
    }
    None
}

/// "Temporada 2" / "Season 2" en las carpetas intermedias (nunca en la carpeta de la serie).
pub(crate) fn season_from_dirs(dirs: &[&str]) -> Option<u32> {
    for d in dirs.iter().skip(1).rev() {
        let l = d.to_lowercase();
        for pre in ["temporada", "season", "temp"] {
            if let Some(rest) = l.strip_prefix(pre) {
                let digits = rest.trim_matches(|c: char| !c.is_ascii_digit());
                if let Ok(n) = digits.parse() {
                    return Some(n);
                }
            }
        }
    }
    None
}

pub(crate) struct ShowAcc {
    pub(crate) display: String,
    pub(crate) year: Option<u16>,
    pub(crate) episodes: Vec<Episode>,
}

pub(crate) fn shows(files: Vec<RawFile>, cache: &Path) -> Vec<Item> {
    let mut map: BTreeMap<String, ShowAcc> = BTreeMap::new();
    for f in files {
        let parts: Vec<&str> = f.rel.split('/').collect();
        let (dirs, _) = parts.split_at(parts.len() - 1);
        let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let marker = find_marker(&stem);

        let raw_show = if let Some(d) = dirs.first() {
            d.to_string()
        } else if let Some((_, _, start, _)) = marker.filter(|m| m.2 > 0) {
            stem[..start].to_string()
        } else {
            stem.clone()
        };
        let (display, year) = display_name(&raw_show);
        if display.is_empty() {
            continue;
        }

        let (season, episode) = match marker {
            Some((s, e, _, _)) => (s, e),
            None => (season_from_dirs(dirs).unwrap_or(0), 0),
        };
        let title = match marker {
            Some((_, _, _, end)) => {
                let tail: String =
                    stem[end..].chars().map(|c| if matches!(c, '-' | '_' | '.') { ' ' } else { c }).collect();
                let words: Vec<&str> = tail.split_whitespace().collect();
                prettify(&words)
            }
            None => parse_name(&stem).0,
        };
        let ep = Episode {
            id: fnv(&format!("serie/{}", f.rel)),
            season,
            episode,
            title,
            file: f.rel.clone(),
            size: f.size,
            path: f.path,
        };
        map.entry(display.to_lowercase())
            .or_insert_with(|| ShowAcc { display, year, episodes: Vec::new() })
            .episodes
            .push(ep);
    }

    map.into_iter()
        .map(|(key, mut acc)| {
            acc.episodes.sort_by(|a, b| (a.season, a.episode, &a.file).cmp(&(b.season, b.episode, &b.file)));
            let mut item = Item {
                year: acc.year,
                size: acc.episodes.iter().map(|e| e.size).sum(),
                episodes: acc.episodes,
                file: key.clone(),
                ..Item::new(Kind::Serie, fnv(&format!("show/{key}")), acc.display)
            };
            apply_cache(&mut item, cache);
            item
        })
        .collect()
}

pub(crate) async fn rescan(s: &Shared) -> Result<usize, String> {
    if !s.cfg.storage_ok() {
        return Err(format!("almacenamiento no disponible: {}", s.cfg.media_root.display()));
    }
    let root = s.cfg.media_root.clone();
    let cache = s.cfg.cache_dir.clone();
    let _ = tokio::fs::create_dir_all(&cache).await;
    let mut items = tokio::task::spawn_blocking(move || {
        let mut v = movies(walk(&root.join("Peliculas")), &cache);
        v.extend(shows(walk(&root.join("Series")), &cache));
        v
    })
    .await
    .map_err(|e| e.to_string())?;
    items.sort_by(|a, b| a.search_title.cmp(&b.search_title));
    let n = items.len();
    *s.items.write().await = items;
    Ok(n)
}

/// Re-escanea el disco y completa en segundo plano las carátulas que falten.
pub(crate) async fn refresh_library(s: &Shared) -> Result<usize, String> {
    let n = rescan(s).await?;
    tokio::spawn(enrich_all(s.clone()));
    Ok(n)
}

/// Firma barata de la biblioteca: (ruta, tamaño) de cada video.
pub(crate) fn library_signature(root: &Path) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> =
        walk(&root.join("Peliculas")).into_iter().chain(walk(&root.join("Series"))).map(|f| (f.rel, f.size)).collect();
    v.sort();
    v
}

/// Vigila las carpetas y re-escanea solo cuando hay cambios ya estables
/// (dos lecturas iguales seguidas), para no leer un archivo a medio copiar.
pub(crate) async fn watch_library(s: Shared) {
    let root = s.cfg.media_root.clone();
    let sig = |r: PathBuf| async move {
        tokio::task::spawn_blocking(move || library_signature(&r)).await.unwrap_or_default()
    };
    let mut applied = sig(root.clone()).await;
    let mut prev = applied.clone();
    loop {
        tokio::time::sleep(Duration::from_secs(15)).await;
        if !s.cfg.storage_ok() {
            continue;
        }
        let cur = sig(root.clone()).await;
        if cur != applied && cur == prev {
            if let Ok(n) = refresh_library(&s).await {
                eprintln!("piflix: biblioteca actualizada, {n} títulos");
                applied = cur.clone();
            }
        }
        prev = cur;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmdb::url_encode;

    fn raw(rel: &str, size: u64) -> RawFile {
        RawFile { path: PathBuf::from("/x").join(rel), rel: rel.to_string(), size }
    }
    fn no_cache() -> PathBuf {
        std::env::temp_dir().join("piflix-test-no-cache-dir")
    }

    #[test]
    fn fnv_estable() {
        assert_eq!(fnv(""), "cbf29ce484222325");
        assert_eq!(fnv("a"), fnv("a"));
        assert_ne!(fnv("pelicula/a.mp4"), fnv("serie/a.mp4"));
    }

    #[test]
    fn nombres_de_pelicula() {
        assert_eq!(parse_name("el-hijo-de-batman-2014"), ("El Hijo de Batman".into(), Some(2014)));
        assert_eq!(parse_name("superman_red_son_2020"), ("Superman Red Son".into(), Some(2020)));
        assert_eq!(parse_name("sin-anio"), ("Sin Anio".into(), None));
        assert_eq!(parse_name("pelicula-3000"), ("Pelicula 3000".into(), None));
        // año fuera de rango
    }

    #[test]
    fn nombres_de_carpeta() {
        assert_eq!(display_name("Avatar Seven Havens"), ("Avatar Seven Havens".into(), None));
        assert_eq!(display_name("Avatar Seven Havens (2024)"), ("Avatar Seven Havens".into(), Some(2024)));
        assert_eq!(display_name("avatar-seven-havens"), ("Avatar Seven Havens".into(), None));
    }

    #[test]
    fn marcadores_de_episodio() {
        assert_eq!(find_marker("show-s01e02-titulo"), Some((1, 2, 5, 11)));
        assert_eq!(find_marker("1x05"), Some((1, 5, 0, 4)));
        assert_eq!(find_marker("Episodio 12"), None);
        assert_eq!(find_marker("s1e2x"), None);
    }

    #[test]
    fn temporada_desde_carpetas() {
        assert_eq!(season_from_dirs(&["Show", "Temporada 2"]), Some(2));
        assert_eq!(season_from_dirs(&["Show", "Season 3"]), Some(3));
        assert_eq!(season_from_dirs(&["Season 3"]), None); // la carpeta de la serie nunca cuenta
    }

    #[test]
    fn peliculas_y_series_se_agrupan() {
        let m = movies(vec![raw("el-hijo-de-batman-2014.mp4", 10)], &no_cache());
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].title.as_str(), m[0].year, m[0].kind), ("El Hijo de Batman", Some(2014), Kind::Pelicula));
        assert_eq!(m[0].id, fnv("pelicula/el-hijo-de-batman-2014.mp4"));

        let s = shows(
            vec![
                raw("Avatar Seven Havens/Temporada 1/avatar-seven-havens-s01e02.mp4", 5),
                raw("Avatar Seven Havens/Temporada 1/avatar-seven-havens-s01e01.mp4", 7),
                raw("Otra Serie/otra-serie-1x01.mp4", 1),
            ],
            &no_cache(),
        );
        assert_eq!(s.len(), 2);
        let a = s.iter().find(|i| i.title == "Avatar Seven Havens").unwrap();
        assert_eq!((a.kind, a.size, a.id.clone()), (Kind::Serie, 12, fnv("show/avatar seven havens")));
        let eps: Vec<(u32, u32)> = a.episodes.iter().map(|e| (e.season, e.episode)).collect();
        assert_eq!(eps, vec![(1, 1), (1, 2)]);
    }

    #[test]
    fn la_cache_pone_titulo_y_caratula() {
        let dir = std::env::temp_dir().join(format!("piflix-test-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut it = movies(vec![raw("a-2020.mp4", 1)], &dir).remove(0);
        assert!(!it.has_poster && it.title == "A");
        std::fs::write(cache_file(&dir, &it.id, CacheKind::Poster), b"x").unwrap();
        std::fs::write(
            cache_file(&dir, &it.id, CacheKind::Meta),
            r#"{"title":"Desde TMDB","overview":"sinopsis","year":1999}"#,
        )
        .unwrap();
        std::fs::write(cache_file(&dir, &it.id, CacheKind::Title), " Mi título \n").unwrap();
        apply_cache(&mut it, &dir);
        assert!(it.has_poster && it.poster_v > 0);
        assert_eq!(it.title, "Mi título"); // el título manual gana sobre TMDB
        assert_eq!(it.search_title, "mi título");
        assert_eq!(it.overview.as_deref(), Some("sinopsis"));
        assert_eq!(it.year, Some(2020)); // el año del nombre no lo pisa TMDB
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn url_encode_basico() {
        assert_eq!(url_encode("Batman & Robin"), "Batman%20%26%20Robin");
        assert_eq!(url_encode("a-b_c.d~"), "a-b_c.d~");
    }
}
