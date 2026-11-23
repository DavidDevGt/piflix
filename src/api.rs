//! Endpoints de lectura: catálogo, streaming, imágenes y salud, y las respuestas JSON compartidas.

use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{cache::CacheKind, library::refresh_library, model::Kind, prelude::*};

// ---------------------------------------------------------------- respuestas compartidas

pub(crate) fn err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

pub(crate) fn ok() -> Response {
    Json(json!({ "ok": true })).into_response()
}

pub(crate) fn disk_unavailable() -> Response {
    err(StatusCode::SERVICE_UNAVAILABLE, "El disco de almacenamiento no está disponible")
}

/// Sirve un archivo con Range/If-Range (206 Partial Content) delegando en tower-http.
async fn serve_file(path: PathBuf, req: Request) -> Option<Response> {
    ServeFile::new(path).oneshot(req).await.ok().map(IntoResponse::into_response)
}

// ---------------------------------------------------------------- catálogo

#[derive(Deserialize)]
pub(crate) struct MediaQuery {
    pub(crate) q: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) year: Option<u16>,
}

pub(crate) async fn api_media(State(s): State<Shared>, Query(q): Query<MediaQuery>) -> Response {
    if !s.cfg.storage_ok() {
        return disk_unavailable();
    }
    let needle = q.q.as_deref().map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty());
    let items = s.items.read().await;
    let list: Vec<&Item> = items
        .iter()
        .filter(|i| needle.as_ref().is_none_or(|n| i.search_title.contains(n.as_str())))
        .filter(|i| q.kind.as_deref().filter(|k| !k.is_empty()).is_none_or(|k| i.kind.as_str() == k))
        .filter(|i| q.year.is_none_or(|y| i.year == Some(y)))
        .collect();
    let mut years: Vec<u16> = items.iter().filter_map(|i| i.year).collect();
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();
    Json(json!({ "total": list.len(), "years": years, "items": list })).into_response()
}

pub(crate) async fn api_random(State(s): State<Shared>) -> Response {
    let items = s.items.read().await;
    if items.is_empty() {
        return err(StatusCode::NOT_FOUND, "Biblioteca vacía");
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    Json(&items[nanos % items.len()]).into_response()
}

pub(crate) async fn api_reload(State(s): State<Shared>) -> Response {
    match refresh_library(&s).await {
        Ok(n) => Json(json!({ "total": n })).into_response(),
        Err(e) => err(StatusCode::SERVICE_UNAVAILABLE, &e),
    }
}

// ---------------------------------------------------------------- streaming e imágenes

/// El cliente nunca manda rutas: solo un id (película o episodio) que se resuelve contra el catálogo.
pub(crate) async fn find_path(s: &Shared, id: &str) -> Option<PathBuf> {
    let items = s.items.read().await;
    for i in items.iter() {
        if i.kind == Kind::Pelicula && i.id == id {
            return Some(i.path.clone());
        }
        if let Some(e) = i.episodes.iter().find(|e| e.id == id) {
            return Some(e.path.clone());
        }
    }
    None
}

pub(crate) async fn api_stream(State(s): State<Shared>, AxPath(id): AxPath<String>, req: Request) -> Response {
    let Some(path) = find_path(&s, &id).await else {
        return err(StatusCode::NOT_FOUND, "No existe");
    };
    serve_file(path, req).await.unwrap_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "Error leyendo el archivo"))
}

pub(crate) async fn api_image(
    State(s): State<Shared>,
    AxPath((id, kind)): AxPath<(String, String)>,
    req: Request,
) -> Response {
    let kind = match kind.as_str() {
        "poster" => CacheKind::Poster,
        "backdrop" => CacheKind::Backdrop,
        _ => return err(StatusCode::NOT_FOUND, "No existe"),
    };
    if !s.has_item(&id).await {
        return err(StatusCode::NOT_FOUND, "No existe");
    }
    let Some(mut r) = serve_file(s.cfg.cache_file(&id, kind), req).await else {
        return err(StatusCode::NOT_FOUND, "Sin imagen");
    };
    if r.status().is_success() || r.status() == StatusCode::NOT_MODIFIED {
        r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=604800"));
    }
    r
}

pub(crate) async fn api_health(State(s): State<Shared>) -> Json<Value> {
    let temp = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()
        .and_then(|t| t.trim().parse::<f64>().ok())
        .map(|t| t / 1000.0);
    Json(json!({
        "ok": true,
        "storage": s.cfg.storage_ok(),
        "items": s.items.read().await.len(),
        "tmdb": s.cfg.tmdb_key.is_some(),
        "downloads": s.cfg.admin_token.is_some(),
        "enriching": s.enriching.load(Ordering::SeqCst),
        "temp_c": temp,
        "uptime_s": s.started.elapsed().as_secs(),
    }))
}
