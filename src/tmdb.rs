//! Metadatos y carátulas desde TMDB (vía curl: la Pi no lleva TLS en Rust).

use crate::prelude::*;

pub(crate) fn url_encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{:02X}", b)),
        }
    }
    o
}

// ---------------------------------------------------------------- TMDB (vía curl, sin TLS en Rust)

/// La URL (con la api key) se pasa por stdin para que no aparezca en `ps`.
pub(crate) async fn curl_get(url: &str) -> Option<Vec<u8>> {
    crate::curl::run(&["-sS", "-f", "-L", "--max-time", "25"], &format!("url = \"{url}\"\n")).await
}

pub(crate) enum Fetch {
    Done,
    Retry, // fallo de red / API: se reintenta en el próximo reload
}

pub(crate) async fn enrich_one(s: &Shared, item: &Item) -> Fetch {
    let Some(key) = s.cfg.tmdb_key.as_deref() else { return Fetch::Retry };
    let (ep, ykey) = if item.kind == Kind::Serie { ("tv", "first_air_date_year") } else { ("movie", "year") };
    let q = |with_year: bool| {
        let mut u = format!(
            "https://api.themoviedb.org/3/search/{ep}?api_key={key}&language={}&query={}",
            s.cfg.lang,
            url_encode(&item.title)
        );
        if let (true, Some(y)) = (with_year, item.year) {
            u.push_str(&format!("&{ykey}={y}"));
        }
        u
    };
    let mut best: Option<Value> = None;
    for with_year in [true, false] {
        let Some(body) = curl_get(&q(with_year)).await else { return Fetch::Retry };
        let Ok(v) = serde_json::from_slice::<Value>(&body) else { return Fetch::Retry };
        if let Some(first) = v["results"].as_array().and_then(|a| a.first()) {
            best = Some(first.clone());
            break;
        }
        if item.year.is_none() {
            break;
        }
    }

    let cache = &s.cfg.cache_dir;
    let mut meta = json!({ "title": "", "overview": "", "year": null });
    if let Some(r) = &best {
        let title = r["title"].as_str().or_else(|| r["name"].as_str()).unwrap_or("");
        let date = r["release_date"].as_str().or_else(|| r["first_air_date"].as_str()).unwrap_or("");
        meta = json!({
            "title": title,
            "overview": r["overview"].as_str().unwrap_or(""),
            "year": date.get(..4).and_then(|y| y.parse::<u64>().ok()),
            "tmdb_id": r["id"],
        });
        for (field, size, kind) in
            [("poster_path", "w342", CacheKind::Poster), ("backdrop_path", "w780", CacheKind::Backdrop)]
        {
            if cache_file(cache, &item.id, kind).exists() {
                continue; // carátula puesta a mano o ya descargada: no se pisa
            }
            if let Some(p) = r[field].as_str() {
                let url = format!("https://image.tmdb.org/t/p/{size}{p}");
                match curl_get(&url).await {
                    Some(bytes) if !bytes.is_empty() => {
                        let _ = write_atomic(&cache_file(cache, &item.id, kind), &bytes).await;
                    }
                    _ => return Fetch::Retry,
                }
            }
        }
    }
    let _ = write_atomic(&cache_file(cache, &item.id, CacheKind::Meta), meta.to_string().as_bytes()).await;
    Fetch::Done
}

/// Un solo trabajador secuencial: no satura CPU, RAM ni la red de la Pi.
pub(crate) async fn enrich_all(s: Shared) {
    if s.cfg.tmdb_key.is_none() || s.enriching.swap(true, Ordering::SeqCst) {
        return;
    }
    let pending: Vec<Item> =
        s.items.read().await.iter().filter(|i| !s.cfg.cache_file(&i.id, CacheKind::Meta).exists()).cloned().collect();
    for it in pending {
        if let Fetch::Retry = enrich_one(&s, &it).await {
            eprintln!("tmdb: sin datos para '{}' (se reintentará en el próximo reload)", it.title);
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    let _ = rescan(&s).await; // refresca items con los metadatos nuevos
    s.enriching.store(false, Ordering::SeqCst);
}
