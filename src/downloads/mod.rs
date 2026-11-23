//! Descarga de videos por enlace directo hacia Peliculas/ o Series/.
//!
//! Defensas: requiere clave de administrador, solo http(s), rechaza destinos internos
//! (SSRF) validando cada redirección y fijando la IP resuelta, exige que el contenido
//! sea video (cabeceras + firma de archivo), escribe a `.part` y renombra al terminar,
//! no sobrescribe, comprueba espacio libre y ejecuta una descarga a la vez.

pub(crate) mod files;
pub(crate) mod jobs;
mod net;
mod sweep;
mod worker;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod unit_tests;

pub(crate) use jobs::Downloads;
pub(crate) use sweep::sweeper;
pub(crate) use worker::worker;

use self::{files::*, jobs::*, net::*};
use crate::{auth::Admin, prelude::*};

// ---------------------------------------------------------------- handlers

pub async fn list(State(s): State<Shared>, _: Admin) -> Response {
    let jobs = s.dl.jobs.lock().unwrap().clone();
    Json(json!({ "jobs": jobs })).into_response()
}

pub async fn create(State(s): State<Shared>, _: Admin, Json(req): Json<NewJob>) -> Response {
    create_inner(&s, req).await
}

pub(crate) async fn create_inner(s: &Shared, mut req: NewJob) -> Response {
    if let Err(e) = apply_show(s, &mut req).await {
        return err(StatusCode::BAD_REQUEST, &e);
    }
    let Some(kind) = Kind::parse(&req.kind) else {
        return err(StatusCode::BAD_REQUEST, "Categoría inválida");
    };
    if !s.cfg.storage_ok() {
        return disk_unavailable();
    }
    // Validación barata primero (nombre/ruta), red después
    if let Err(e) = build_dest(&s.cfg.media_root, &req, kind, "mp4") {
        return err(StatusCode::BAD_REQUEST, &e);
    }
    let referer = match clean_referer(req.referer.as_deref().unwrap_or("")) {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_REQUEST, &e),
    };

    // ¿Hay un .part de este mismo título? Un destino con descarga activa nunca se toca.
    let existing = find_part(&s.cfg.media_root, &req, kind).await;
    if let Some((d, _)) = &existing {
        if s.dl.is_active(d) {
            return err(StatusCode::CONFLICT, "Esa descarga ya está en curso");
        }
    }
    let prior = existing.as_ref().and_then(|(d, _)| s.dl.find_by_dest(d));
    let stored: Option<(Option<String>, Option<u64>)> = match (&prior, &existing) {
        (Some(j), _) => Some((j.validator.clone(), j.total)),
        (None, Some((d, _))) => read_meta(d).await.map(|m| (m.validator, m.total)),
        _ => None,
    };
    // Registro vivo + misma URL escrita por el usuario + enlace no caducado: se salta la sonda.
    // (La IP fijada puede estar vieja: run_job la refresca si el primer intento sale con 6/7.)
    let reuse = prior
        .as_ref()
        .filter(|j| j.state != JobState::EnlaceCaducado && j.can_resume && j.req.url.trim() == req.url.trim());
    let pr = match reuse {
        Some(j) => Probe {
            url: j.url.clone(),
            resolve: j.resolve.clone(),
            total: j.total,
            ctype: String::new(),
            can_resume: true,
            validator: j.validator.clone(),
        },
        None => match probe(req.url.trim(), &referer).await {
            Ok(p) => p,
            Err(e) => return err(StatusCode::BAD_REQUEST, &e),
        },
    };
    if reuse.is_none()
        && (pr.ctype.starts_with("text/")
            || pr.ctype.contains("html")
            || pr.ctype.contains("json")
            || pr.ctype.contains("xml"))
    {
        return err(
            StatusCode::BAD_REQUEST,
            "El enlace devuelve una página web, no un archivo de video. Usa el enlace directo al .mp4",
        );
    }
    // Sin registro vivo (reinicio, enlace nuevo) solo se reanuda si el validator coincide EXACTO.
    // Host sin ETag ni Last-Modified: un enlace nuevo no puede verificarse y se empieza de cero.
    let keep = match (&existing, &stored) {
        (Some(_), Some((v, t))) => {
            pr.can_resume && *t == pr.total && (reuse.is_some() || (v.is_some() && *v == pr.validator))
        }
        _ => false,
    };
    let part_len = if keep { existing.as_ref().map_or(0, |e| e.1) } else { 0 };
    // Destinos con .part obsoleto: se borran dentro de la sección crítica, nunca antes.
    let stale: Vec<PathBuf> =
        if existing.is_some() && !keep { candidate_dests(&s.cfg.media_root, &req, kind) } else { Vec::new() };
    let ext: String = match (&existing, keep) {
        (Some((d, _)), true) => d.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default(),
        _ => pick_ext(&pr.url, &pr.ctype).to_string(),
    };
    let (dest, name) = match build_dest(&s.cfg.media_root, &req, kind, &ext) {
        Ok(v) => v,
        Err(e) => return err(StatusCode::BAD_REQUEST, &e),
    };
    if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
        return err(StatusCode::CONFLICT, "Ya existe un archivo con ese nombre en la biblioteca");
    }
    if let Some(free) = free_bytes(&s.cfg.media_root).await {
        let need = pr.total.map_or(MIN_FREE_UNKNOWN, |t| t.saturating_sub(part_len) + MIN_FREE_MARGIN);
        if free < need {
            return err(StatusCode::INSUFFICIENT_STORAGE, "No hay espacio suficiente en el disco para esta descarga");
        }
    }

    let id = s.dl.next.fetch_add(1, Ordering::SeqCst);
    let host = pr.url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("").to_string();
    let can_resume = pr.can_resume;
    let meta = Meta { validator: pr.validator.clone(), total: pr.total };
    let job = Job {
        id,
        name,
        kind,
        host,
        state: JobState::Cola,
        bytes: part_len,
        total: pr.total,
        error: None,
        can_resume,
        bytes_base: part_len,
        intentos: 0,
        url: pr.url,
        resolve: pr.resolve,
        validator: pr.validator,
        req,
        referer,
        dest: dest.clone(),
        cancel: Arc::new(Notify::new()),
        cancelled: Arc::new(AtomicBool::new(false)),
        discard: Arc::new(AtomicBool::new(false)),
    };
    {
        // Comprobar duplicado e insertar es UNA sola sección crítica (sin await): dos clics seguidos
        // en Reanudar no pueden crear dos trabajos sobre el mismo destino.
        let mut jobs = s.dl.jobs.lock().unwrap();
        if jobs.iter().any(|j| j.dest == dest && j.state.is_active()) {
            return err(StatusCode::CONFLICT, "Esa descarga ya está en curso");
        }
        for d in &stale {
            if !jobs.iter().any(|j| j.dest == *d && j.state.is_active()) {
                drop_part_sync(d);
            }
        }
        jobs.push(job);
        while jobs.len() > KEEP_JOBS {
            match jobs.iter().position(|j| !j.state.is_active()) {
                Some(p) => {
                    jobs.remove(p);
                }
                None => break,
            }
        }
    }
    if s.dl.tx.try_send(id).is_err() {
        s.dl.jobs.lock().unwrap().retain(|j| j.id != id);
        return err(StatusCode::TOO_MANY_REQUESTS, "Hay demasiadas descargas en cola");
    }
    // Recién ahora (ya encolado) se retira el registro viejo del mismo destino.
    s.dl.jobs.lock().unwrap().retain(|j| !(j.dest == dest && j.id != id && !j.state.is_active()));
    if can_resume {
        write_meta(&dest, &meta).await;
    }
    (StatusCode::ACCEPTED, Json(json!({ "id": id }))).into_response()
}

/// Cancela una descarga en cola o en curso; si ya terminó, la quita de la lista.
pub async fn cancel(State(s): State<Shared>, _: Admin, AxPath(id): AxPath<u64>) -> Response {
    let Some(job) = s.dl.get(id) else {
        return err(StatusCode::NOT_FOUND, "No existe esa descarga");
    };
    match job.state {
        JobState::Cola => s.dl.set(id, |j| j.state = JobState::Cancelado),
        JobState::Descargando => {
            job.cancelled.store(true, Ordering::SeqCst);
            job.cancel.notify_one();
        }
        _ => s.dl.jobs.lock().unwrap().retain(|j| j.id != id),
    }
    ok()
}

/// Descarta: borra el .part y su .json. En curso, mata curl y run_job los borra.
pub async fn discard(State(s): State<Shared>, _: Admin, AxPath(id): AxPath<u64>) -> Response {
    let Some(job) = s.dl.get(id) else {
        return err(StatusCode::NOT_FOUND, "No existe esa descarga");
    };
    job.discard.store(true, Ordering::SeqCst);
    job.cancelled.store(true, Ordering::SeqCst);
    if job.state == JobState::Descargando {
        job.cancel.notify_one();
    } else {
        // En cola el id sigue en el canal: run_job lo ignora (registro ausente o cancelled).
        drop_part(&job.dest).await;
        s.dl.jobs.lock().unwrap().retain(|j| j.id != id);
    }
    ok()
}

#[derive(Deserialize)]
pub(crate) struct ResumeReq {
    pub(crate) url: Option<String>,
}

/// Reanuda con la petición guardada; `{"url": "..."}` opcional para pegar un enlace nuevo.
/// El cuerpo se lee como bytes para distinguir «sin cuerpo» de «cuerpo inválido».
pub async fn resume(State(s): State<Shared>, _: Admin, AxPath(id): AxPath<u64>, body: Bytes) -> Response {
    let Some(job) = s.dl.get(id) else {
        return err(StatusCode::NOT_FOUND, "No existe esa descarga");
    };
    if job.state.is_active() || job.state == JobState::Listo {
        return err(StatusCode::CONFLICT, "Esa descarga no se puede reanudar");
    }
    let mut req = job.req.clone();
    if !body.iter().all(|b| b.is_ascii_whitespace()) {
        match serde_json::from_slice::<ResumeReq>(&body) {
            Ok(ResumeReq { url: Some(u) }) => req.url = u,
            Ok(_) => {}
            Err(_) => return err(StatusCode::BAD_REQUEST, "Cuerpo inválido"),
        }
    }
    create_inner(&s, req).await
}
