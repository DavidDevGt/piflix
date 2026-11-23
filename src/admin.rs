//! Edición manual de carátula y título.

use crate::{auth::Admin, prelude::*};

// ---------------------------------------------------------------- carátula puesta a mano

/// POST /api/cover/{id}: cuerpo = JPEG (el navegador lo recorta y reduce antes de enviarlo).
/// Reemplaza la carátula de la caché; el enriquecedor de TMDB nunca pisa una imagen existente.
pub async fn set_cover(State(s): State<Shared>, _: Admin, AxPath(id): AxPath<String>, body: Bytes) -> Response {
    // El id sale de la biblioteca (nunca del cliente hacia el sistema de archivos sin comprobar)
    if !s.has_item(&id).await {
        return err(StatusCode::NOT_FOUND, "No existe ese título");
    }
    if !body.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return err(StatusCode::BAD_REQUEST, "La imagen debe ser un JPEG");
    }
    let _ = tokio::fs::create_dir_all(&s.cfg.cache_dir).await;
    if write_atomic(&s.cfg.cache_file(&id, CacheKind::Poster), &body).await.is_err() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "No se pudo guardar la carátula");
    }
    let _ = rescan(&s).await;
    ok()
}

// ---------------------------------------------------------------- título puesto a mano

#[derive(Deserialize)]
pub struct TitleReq {
    pub(crate) title: String,
}

/// POST /api/title/{id}  {"title": "..."}  — título vacío = volver al original.
/// Se guarda en la caché (`{id}.title.txt`), no se renombra ninguna carpeta: el id, las
/// posiciones guardadas y la carátula siguen siendo los mismos.
pub async fn set_title(
    State(s): State<Shared>,
    _: Admin,
    AxPath(id): AxPath<String>,
    Json(req): Json<TitleReq>,
) -> Response {
    if !s.has_item(&id).await {
        return err(StatusCode::NOT_FOUND, "No existe ese título");
    }
    let t: String = req.title.trim().chars().filter(|c| !c.is_control()).collect();
    if t.chars().count() > 80 {
        return err(StatusCode::BAD_REQUEST, "El título es demasiado largo (máximo 80 caracteres)");
    }
    let _ = tokio::fs::create_dir_all(&s.cfg.cache_dir).await;
    let file = s.cfg.cache_file(&id, CacheKind::Title);
    if t.is_empty() {
        let _ = tokio::fs::remove_file(&file).await;
    } else if write_atomic(&file, t.as_bytes()).await.is_err() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "No se pudo guardar el título");
    }
    let _ = rescan(&s).await;

    // Sin carátula: se vuelve a consultar TMDB con el título nuevo (si hay API configurada).
    // enrich_one nunca pisa una imagen que ya exista.
    let tmdb = s.cfg.tmdb_key.is_some();
    let mut tmdb_error = false;
    if tmdb {
        let item = s.items.read().await.iter().find(|i| i.id == id).cloned();
        if let Some(it) = item.filter(|i| !i.has_poster) {
            tmdb_error = matches!(enrich_one(&s, &it).await, Fetch::Retry);
            let _ = rescan(&s).await;
        }
    }
    let has_poster = s.items.read().await.iter().any(|i| i.id == id && i.has_poster);
    Json(json!({ "ok": true, "tmdb": tmdb, "tmdb_error": tmdb_error, "has_poster": has_poster })).into_response()
}
