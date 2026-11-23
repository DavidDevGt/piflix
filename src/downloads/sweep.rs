//! Purgado de `.part` antiguos o que estorban por falta de espacio.

use super::files::*;
use crate::prelude::*;

// ---------------------------------------------------------------- purgado de .part

pub(crate) const PART_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

pub(crate) struct PartFile {
    pub(crate) dest: PathBuf,
    pub(crate) mtime: SystemTime,
    pub(crate) len: u64,
}

/// Busca `.nombre.part` bajo Peliculas/ y Series/ (los ocultos no los ve `walk`).
pub(crate) fn scan_parts(root: &Path) -> Vec<PartFile> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("Peliculas"), root.join("Series")];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !name.starts_with('.') {
                    stack.push(e.path());
                }
                continue;
            }
            if let Some(inner) = name.strip_prefix('.').and_then(|n| n.strip_suffix(".part")) {
                let Ok(m) = e.metadata() else { continue };
                out.push(PartFile {
                    dest: dir.join(inner),
                    mtime: m.modified().unwrap_or_else(|_| SystemTime::now()),
                    len: m.len(),
                });
            } else if name.starts_with('.') && name.ends_with(".part.json") {
                // sidecar huérfano (su .part ya no existe)
                let part = dir.join(name.trim_end_matches(".json"));
                if !part.exists() {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    out
}

/// Borra los .part de más de 7 días sin descarga activa y, si el espacio libre baja de dos veces
/// el mayor total pendiente, los más antiguos hasta recuperarlo. Los `fallo`/`enlace_caducado`
/// protegen su .part solo hasta este barrido.
pub(crate) async fn purge(s: &Shared) {
    let root = s.cfg.media_root.clone();
    if !root.is_dir() {
        return;
    }
    let parts = tokio::task::spawn_blocking(move || scan_parts(&root)).await.unwrap_or_default();
    let now = SystemTime::now();
    let mut purged: Vec<PathBuf> = Vec::new();
    let mut left: Vec<PartFile> = Vec::new();
    for p in parts {
        let old = now.duration_since(p.mtime).is_ok_and(|a| a > PART_TTL);
        if old && !s.dl.is_active(&p.dest) {
            drop_part(&p.dest).await;
            purged.push(p.dest);
        } else {
            left.push(p);
        }
    }
    let need = s.dl.pending_max_total().saturating_mul(2);
    if need > 0 {
        if let Some(mut free) = free_bytes(&s.cfg.media_root).await {
            left.sort_by_key(|p| p.mtime);
            for p in &left {
                if free >= need {
                    break;
                }
                if s.dl.is_active(&p.dest) {
                    continue;
                }
                drop_part(&p.dest).await;
                free += p.len;
                purged.push(p.dest.clone());
            }
        }
    }
    if !purged.is_empty() {
        eprintln!("piflix: purgados {} archivos .part antiguos", purged.len());
        s.dl.jobs.lock().unwrap().retain(|j| !(purged.contains(&j.dest) && !j.state.is_active()));
    }
}

/// Barrido al arrancar y una vez al día.
pub async fn sweeper(s: Shared) {
    loop {
        purge(&s).await;
        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
    }
}
