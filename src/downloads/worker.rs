//! Trabajador: una descarga a la vez, con reanudación, reintentos y verificación.

use super::{files::*, jobs::*, net::*};
use crate::prelude::*;

pub(crate) const MAX_RETRIES: u32 = 3;

// ---------------------------------------------------------------- trabajador (una descarga a la vez)

pub async fn worker(s: Shared, mut rx: mpsc::Receiver<u64>) {
    while let Some(id) = rx.recv().await {
        run_job(&s, id).await;
    }
}

pub(crate) fn fail(s: &Shared, id: u64, msg: &str) {
    let m = msg.to_string();
    s.dl.set(id, move |j| {
        j.state = JobState::Error;
        j.error = Some(m);
    });
}

/// El enlace murió: se conserva el .part y se pide uno nuevo (POST /api/downloads/{id}/resume).
pub(crate) fn expire(s: &Shared, job: &Job) {
    let msg = if job.validator.is_some() {
        "El enlace caducó. Pega uno nuevo para continuar desde donde quedó."
    } else {
        "El enlace caducó. Este servidor no permite verificar el archivo, así que con un enlace nuevo se empezará de cero."
    };
    s.dl.set(job.id, move |j| {
        j.state = JobState::EnlaceCaducado;
        j.error = Some(msg.to_string());
    });
}

/// Vuelve a sondear la URL ORIGINAL (la final puede estar caducada) y actualiza url, IP fijada,
/// total y validator. Devuelve si el archivo cambió respecto a lo que había.
pub(crate) async fn refresh(s: &Shared, job: &Job) -> Result<bool, String> {
    let p = probe(job.req.url.trim(), &job.referer).await?;
    let changed = p.total != job.total || p.validator != job.validator;
    let meta = Meta { validator: p.validator.clone(), total: p.total };
    s.dl.set(job.id, move |j| {
        j.url = p.url;
        j.resolve = p.resolve;
        j.total = p.total;
        j.validator = p.validator;
        j.can_resume = p.can_resume;
    });
    write_meta(&job.dest, &meta).await;
    Ok(changed)
}

pub(crate) struct CurlRun {
    pub(crate) cancelled: bool,
    pub(crate) spawn_err: bool,
    pub(crate) exit: Option<i32>,
    pub(crate) http: u16,
}

pub(crate) async fn run_curl(s: &Shared, job: &Job, part: &Path, resume: bool) -> CurlRun {
    let part_arg = part.to_string_lossy();
    let mut args = vec![
        "-sS",
        "-f",
        "--max-redirs",
        "0",
        "--proto",
        "=http,https",
        "--connect-timeout",
        "20",
        "--speed-limit",
        "1024",
        "--speed-time",
        "60",
        "--resolve",
        job.resolve.as_str(),
        "-A",
        UA,
        "-w",
        "%{http_code}",
        "-o",
        part_arg.as_ref(),
    ];
    if resume {
        args.extend(["-C", "-"]);
    }
    let spawned = crate::curl::spawn(&args);
    let mut child = match spawned {
        Ok(c) => c,
        Err(_) => return CurlRun { cancelled: false, spawn_err: true, exit: None, http: 0 },
    };
    if let Some(mut stdin) = child.stdin.take() {
        // If-Range: si el archivo cambió, el servidor manda 200 y curl sale con 33 (nunca mezcla versiones)
        let ifr = if resume { job.validator.as_deref() } else { None };
        let _ = stdin.write_all(curl_cfg(&job.url, &job.referer, ifr).as_bytes()).await;
    }

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut cancelled = false;
    let status = loop {
        tokio::select! {
            r = child.wait() => break r.ok(),
            _ = job.cancel.notified() => {
                cancelled = true;
                let _ = child.kill().await;
                break None;
            }
            _ = tick.tick() => {
                // el .part ya incluye los bytes previos: medirlo sigue siendo correcto al reanudar
                if let Ok(m) = tokio::fs::metadata(part).await {
                    let n = m.len();
                    s.dl.set(job.id, move |j| j.bytes = n);
                }
            }
        }
    };
    let mut out = String::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_string(&mut out).await;
    }
    let http = out.trim().parse().unwrap_or(0);
    CurlRun { cancelled, spawn_err: false, exit: status.and_then(|st| st.code()), http }
}

/// Verificación: no vacío, tamaño exacto igual al total y firma de video.
pub(crate) async fn verify(job: &Job, part: &Path) -> Result<u64, &'static str> {
    let size = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
    if size == 0 || job.total.is_some_and(|t| t != size) {
        return Err("La descarga quedó incompleta");
    }
    let mut head = [0u8; 12];
    let n = match tokio::fs::File::open(part).await {
        Ok(mut f) => f.read(&mut head).await.unwrap_or(0),
        Err(_) => 0,
    };
    let ext = job.dest.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    if !magic_ok(&ext, &head[..n]) {
        return Err("El archivo descargado no parece un video válido");
    }
    Ok(size)
}

/// Renombrado atómico: la biblioteca nunca ve un archivo a medias.
pub(crate) async fn finish(s: &Shared, job: &Job, part: &Path, size: u64) {
    if tokio::fs::try_exists(&job.dest).await.unwrap_or(false) {
        drop_part(&job.dest).await;
        return fail(s, job.id, "Ya existe un archivo con ese nombre en la biblioteca");
    }
    if tokio::fs::rename(part, &job.dest).await.is_err() {
        drop_part(&job.dest).await;
        return fail(s, job.id, "No se pudo guardar el archivo en la biblioteca");
    }
    let _ = tokio::fs::remove_file(meta_path(&job.dest)).await;
    s.dl.set(job.id, move |j| {
        j.state = JobState::Listo;
        j.bytes = size;
        j.error = None;
    });
    let _ = refresh_library(s).await;
}

pub(crate) async fn run_job(s: &Shared, id: u64) {
    let Some(job) = s.dl.get(id) else { return };
    if job.state != JobState::Cola || job.cancelled.load(Ordering::SeqCst) {
        return; // cancelada o descartada mientras esperaba
    }
    s.dl.set(id, |j| j.state = JobState::Descargando);

    let part = part_path(&job.dest);
    if let Some(dir) = job.dest.parent() {
        if tokio::fs::create_dir_all(dir).await.is_err() {
            return fail(s, id, "No se pudo crear la carpeta de destino (¿disco en solo lectura?)");
        }
    }

    let (mut retries, mut restarted, mut conn_refreshed) = (0u32, false, false);
    loop {
        // Se relee el trabajo en cada vuelta: url, IP fijada, total y validator pueden haberse refrescado.
        let Some(job) = s.dl.get(id) else { return };

        // 1. Normalizar el .part. Orden: mayor que el total -> descartar; igual -> solo verificar;
        //    menor de 1 MB -> reiniciar (evita rarezas de rangos en archivos pequeños).
        let mut base = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
        if base > 0
            && (job.total.is_some_and(|t| base > t)
                || !job.can_resume
                || (base < MIN_RESUME && job.total != Some(base)))
        {
            let _ = tokio::fs::remove_file(&part).await;
            base = 0;
        }
        s.dl.set(id, move |j| {
            j.bytes_base = base;
            j.bytes = base; // el progreso no vuelve a cero al reanudar
            j.intentos = retries;
        });

        // 2. curl, salvo que el .part ya esté completo.
        let run = if base > 0 && job.total == Some(base) {
            CurlRun { cancelled: false, spawn_err: false, exit: Some(0), http: 0 }
        } else {
            run_curl(s, &job, &part, base > 0).await
        };

        if run.cancelled || job.cancelled.load(Ordering::SeqCst) {
            if job.discard.load(Ordering::SeqCst) {
                drop_part(&job.dest).await; // descartar borra; cancelar a secas conserva el .part
            }
            s.dl.set(id, |j| j.state = JobState::Cancelado);
            return;
        }
        if run.spawn_err {
            return fail(s, id, "No se pudo iniciar la descarga (¿curl instalado?)");
        }

        match (run.exit, run.http) {
            (Some(0), _) => match verify(&job, &part).await {
                Ok(size) => return finish(s, &job, &part, size).await,
                // Reanudado que no cuadra: se descarta y se reintenta UNA vez desde cero,
                // con el total y el validator refrescados (si no, el tamaño exacto fallaría siempre).
                Err(_) if base > 0 && !restarted => {
                    restarted = true;
                    if refresh(s, &job).await.is_err() {
                        return expire(s, &job);
                    }
                    let _ = tokio::fs::remove_file(&part).await;
                    s.dl.set(id, |j| j.bytes = 0);
                    continue;
                }
                Err(m) => {
                    drop_part(&job.dest).await;
                    return fail(s, id, m);
                }
            },
            // 33: servidor sin rangos o If-Range distinto (el archivo cambió). 416: rango imposible.
            // Truncar, quitar -C - y reintentar una sola vez desde cero; no se propaga como fallo.
            (Some(33), _) | (Some(22), 416) if !restarted => {
                restarted = true;
                if refresh(s, &job).await.is_err() {
                    return expire(s, &job); // la URL original también murió: .part conservado
                }
                let _ = tokio::fs::remove_file(&part).await;
                s.dl.set(id, |j| j.bytes = 0);
                continue;
            }
            // 403/410 reanudando: el enlace firmado murió. No se reintenta con la misma URL.
            (Some(22), 403 | 410) if base > 0 => return expire(s, &job),
            // Fallos transitorios: hasta MAX_RETRIES con espera exponencial; luego `fallo` y .part conservado.
            (Some(c), _) if matches!(c, 6 | 7 | 18 | 28 | 52 | 55 | 56) => {
                if matches!(c, 6 | 7) && !conn_refreshed {
                    conn_refreshed = true; // la IP fijada pudo caducar: re-sondear antes de contar el reintento
                    if let Ok(changed) = refresh(s, &job).await {
                        if changed {
                            let _ = tokio::fs::remove_file(&part).await;
                        }
                        continue;
                    }
                }
                retries += 1;
                if retries > MAX_RETRIES {
                    s.dl.set(id, |j| {
                        j.state = JobState::Fallo;
                        j.error =
                            Some("Se agotaron los reintentos; el avance se conservó para intentarlo de nuevo.".into());
                    });
                    return;
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2u64 << retries)) => {} // 4, 8, 16 s
                    _ = job.cancel.notified() => {
                        if job.discard.load(Ordering::SeqCst) {
                            drop_part(&job.dest).await;
                        }
                        s.dl.set(id, |j| j.state = JobState::Cancelado);
                        return;
                    }
                }
            }
            (code, _) => {
                if !job.can_resume {
                    let _ = tokio::fs::remove_file(&part).await; // nada que reanudar: no dejar basura
                }
                let msg = match code {
                    Some(22) => "El servidor del enlace rechazó la descarga",
                    Some(28) => "La descarga se detuvo por falta de velocidad o tiempo",
                    Some(6) | Some(7) => "No se pudo conectar con el servidor del enlace",
                    Some(23) => "No se pudo escribir en el disco",
                    _ => "Falló la descarga",
                };
                return fail(s, id, msg);
            }
        }
    }
}
