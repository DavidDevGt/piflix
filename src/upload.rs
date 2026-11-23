//! Subida de videos desde el navegador (cuerpo en crudo, una a la vez).

use crate::{
    auth::Admin,
    downloads::{files::*, jobs::*},
    prelude::*,
};

// ---------------------------------------------------------------- subida desde el navegador

/// Una subida a la vez: la Pi tiene poco disco/CPU y así no hay dos escritores sobre el mismo destino.
pub(crate) static UPLOAD_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

pub(crate) const MAX_UPLOAD: u64 = 50 * 1024 * 1024 * 1024;

#[derive(Deserialize)]
pub struct UploadQ {
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) year: Option<u16>,
    pub(crate) season: Option<u32>,
    pub(crate) episode: Option<u32>,
    /// extensión del archivo original (mp4, mkv…)
    pub(crate) ext: Option<String>,
    /// id de una serie existente (añadir episodios a su carpeta)
    pub(crate) show: Option<String>,
}

/// Borra el .part si la subida se corta (el cliente cierra la pestaña: se descarta el futuro).
pub(crate) struct PartGuard(Option<PathBuf>);

impl Drop for PartGuard {
    fn drop(&mut self) {
        if let Some(d) = self.0.take() {
            drop_part_sync(&d);
        }
    }
}

/// POST /api/upload?kind=&title=&year=&season=&episode=&ext=  — cuerpo: el video en crudo.
/// Clave de administrador en `x-admin-token` (se comprueba ANTES de leer el cuerpo).
pub async fn upload(State(s): State<Shared>, _: Admin, h: HeaderMap, Query(q): Query<UploadQ>, body: Body) -> Response {
    use http_body_util::BodyExt;
    let Some(kind) = Kind::parse(&q.kind) else {
        return err(StatusCode::BAD_REQUEST, "Categoría inválida");
    };
    let ext = q.ext.as_deref().unwrap_or("mp4").to_ascii_lowercase();
    let Some(ext) = VIDEO_EXT.iter().find(|x| **x == ext.as_str()).copied() else {
        return err(StatusCode::BAD_REQUEST, "Formato no admitido: usa mp4, mkv, webm, avi o similares");
    };
    // Sondeo previo (x-upload-check): valida todo SIN leer el cuerpo, así el navegador recibe un error
    // legible en vez de un corte de conexión cuando rechazamos un archivo enorme.
    let check = h.contains_key("x-upload-check");
    let len_hdr = if check { "x-upload-size" } else { "content-length" };
    let len: u64 = h.get(len_hdr).and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
    if len == 0 {
        return err(StatusCode::LENGTH_REQUIRED, "Falta el tamaño del archivo");
    }
    if len > MAX_UPLOAD {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "El archivo es demasiado grande");
    }
    if !s.cfg.storage_ok() {
        return disk_unavailable();
    }
    let mut req = NewJob {
        url: String::new(),
        kind: q.kind.clone(),
        title: q.title,
        year: q.year,
        season: q.season,
        episode: q.episode,
        referer: None,
        show: q.show,
    };
    if let Err(e) = apply_show(&s, &mut req).await {
        return err(StatusCode::BAD_REQUEST, &e);
    }
    let (dest, name) = match build_dest(&s.cfg.media_root, &req, kind, ext) {
        Ok(v) => v,
        Err(e) => return err(StatusCode::BAD_REQUEST, &e),
    };
    // El destino puede existir con otra extensión: se comprueba como en las descargas.
    for d in candidate_dests(&s.cfg.media_root, &req, kind) {
        if tokio::fs::try_exists(&d).await.unwrap_or(false) {
            return err(StatusCode::CONFLICT, "Ya existe un archivo con ese nombre en la biblioteca");
        }
    }
    if s.dl.is_active(&dest) {
        return err(StatusCode::CONFLICT, "Hay una descarga en curso con ese nombre");
    }
    if let Some(free) = free_bytes(&s.cfg.media_root).await {
        if free < len + MIN_FREE_MARGIN {
            return err(StatusCode::INSUFFICIENT_STORAGE, "No hay espacio suficiente en el disco para este archivo");
        }
    }
    if check {
        return if UPLOAD_SLOT.available_permits() == 0 {
            err(StatusCode::TOO_MANY_REQUESTS, "Ya hay otra subida en curso. Espera a que termine")
        } else {
            ok()
        };
    }
    let Ok(_slot) = UPLOAD_SLOT.try_acquire() else {
        return err(StatusCode::TOO_MANY_REQUESTS, "Ya hay otra subida en curso. Espera a que termine");
    };

    if let Some(dir) = dest.parent() {
        if tokio::fs::create_dir_all(dir).await.is_err() {
            return err(StatusCode::INTERNAL_SERVER_ERROR, "No se pudo crear la carpeta de destino");
        }
    }
    let part = part_path(&dest);
    let mut guard = PartGuard(Some(dest.clone()));
    let Ok(mut file) = tokio::fs::File::create(&part).await else {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "No se pudo escribir en el disco");
    };

    let (mut got, mut head) = (0u64, Vec::with_capacity(12));
    let mut body = body;
    while let Some(frame) = body.frame().await {
        let Ok(frame) = frame else {
            return err(StatusCode::BAD_REQUEST, "La subida se interrumpió");
        };
        let Ok(data) = frame.into_data() else { continue };
        got += data.len() as u64;
        if got > len {
            return err(StatusCode::BAD_REQUEST, "El archivo es más grande de lo declarado");
        }
        if head.len() < 12 {
            let n = (12 - head.len()).min(data.len());
            head.extend_from_slice(&data[..n]);
        }
        if file.write_all(&data).await.is_err() {
            return err(StatusCode::INSUFFICIENT_STORAGE, "Error al escribir en el disco (¿está lleno?)");
        }
    }
    if got != len || file.flush().await.is_err() {
        return err(StatusCode::BAD_REQUEST, "La subida quedó incompleta");
    }
    drop(file);
    if !magic_ok(ext, &head) {
        return err(StatusCode::BAD_REQUEST, "El archivo no parece un video válido");
    }
    if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
        return err(StatusCode::CONFLICT, "Ya existe un archivo con ese nombre en la biblioteca");
    }
    if tokio::fs::rename(&part, &dest).await.is_err() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "No se pudo guardar el archivo en la biblioteca");
    }
    guard.0 = None; // ya es parte de la biblioteca
    let _ = refresh_library(&s).await;
    (StatusCode::CREATED, Json(json!({ "ok": true, "name": name }))).into_response()
}
