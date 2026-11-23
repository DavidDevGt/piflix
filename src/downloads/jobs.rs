//! Modelo de las descargas: trabajos, cola y petición.

use super::files::clean_dir;
use crate::prelude::*;

pub(crate) const QUEUE: usize = 16;

pub(crate) const KEEP_JOBS: usize = 30;

// ---------------------------------------------------------------- modelo

/// Estado de una descarga. Se serializa en snake_case ("cola", "descargando", "enlace_caducado"…).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobState {
    Cola,
    Descargando,
    Listo,
    Error,
    Cancelado,
    EnlaceCaducado,
    Fallo,
}

impl JobState {
    /// En cola o descargando: el destino está ocupado.
    pub(crate) fn is_active(self) -> bool {
        matches!(self, JobState::Cola | JobState::Descargando)
    }
}

#[derive(Clone, Serialize)]
pub struct Job {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) kind: Kind,
    pub(crate) host: String,
    /// cola | descargando | listo | error | cancelado | enlace_caducado | fallo
    pub(crate) state: JobState,
    pub(crate) bytes: u64,
    /// total_bytes: de Content-Range (206) o Content-Length (200), nunca del Content-Length de un 206
    pub(crate) total: Option<u64>,
    pub(crate) error: Option<String>,
    pub(crate) can_resume: bool,
    /// tamaño del .part al empezar este intento
    pub(crate) bytes_base: u64,
    pub(crate) intentos: u32,
    #[serde(skip)]
    pub(crate) url: String,
    #[serde(skip)]
    pub(crate) resolve: String,
    #[serde(skip)]
    pub(crate) referer: String,
    #[serde(skip)]
    pub(crate) dest: PathBuf,
    #[serde(skip)]
    pub(crate) validator: Option<String>,
    /// petición original (URL escrita por el usuario, título, temporada…)
    #[serde(skip)]
    pub(crate) req: NewJob,
    #[serde(skip)]
    pub(crate) cancel: Arc<Notify>,
    #[serde(skip)]
    pub(crate) cancelled: Arc<AtomicBool>,
    #[serde(skip)]
    pub(crate) discard: Arc<AtomicBool>,
}

pub struct Downloads {
    pub(crate) jobs: Mutex<Vec<Job>>,
    pub(crate) next: AtomicU64,
    pub(crate) tx: mpsc::Sender<u64>,
}

impl Downloads {
    pub fn new() -> (Self, mpsc::Receiver<u64>) {
        let (tx, rx) = mpsc::channel(QUEUE);
        (Downloads { jobs: Mutex::new(Vec::new()), next: AtomicU64::new(1), tx }, rx)
    }
    pub(crate) fn get(&self, id: u64) -> Option<Job> {
        self.jobs.lock().unwrap().iter().find(|j| j.id == id).cloned()
    }
    pub(crate) fn find_by_dest(&self, dest: &Path) -> Option<Job> {
        self.jobs.lock().unwrap().iter().rev().find(|j| j.dest == dest && !j.state.is_active()).cloned()
    }
    pub(crate) fn is_active(&self, dest: &Path) -> bool {
        self.jobs.lock().unwrap().iter().any(|j| j.dest == dest && j.state.is_active())
    }
    /// mayor total entre las descargas que aún pueden necesitar espacio
    pub(crate) fn pending_max_total(&self) -> u64 {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .filter(|j| j.state != JobState::Listo)
            .filter_map(|j| j.total)
            .max()
            .unwrap_or(0)
    }
    pub(crate) fn set(&self, id: u64, f: impl FnOnce(&mut Job)) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
            f(j);
        }
    }
}

#[derive(Clone, Deserialize)]
pub struct NewJob {
    pub(crate) url: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) year: Option<u16>,
    pub(crate) season: Option<u32>,
    pub(crate) episode: Option<u32>,
    /// Página de origen del enlace (opcional); por defecto, el origen del propio enlace.
    pub(crate) referer: Option<String>,
    /// id de una serie ya registrada: el episodio va a SU carpeta (aunque el título se haya cambiado)
    pub(crate) show: Option<String>,
}

/// Si la petición apunta a una serie existente, el título pasa a ser el nombre real de su carpeta,
/// de modo que el episodio nuevo se agrupa con los demás. El cliente nunca elige la ruta.
pub(crate) async fn apply_show(s: &Shared, req: &mut NewJob) -> Result<(), String> {
    let Some(id) = req.show.as_deref().filter(|_| req.kind == "serie") else { return Ok(()) };
    let folder = {
        let items = s.items.read().await;
        let it = items.iter().find(|i| i.id == id && i.kind == Kind::Serie).ok_or("Esa serie ya no existe")?;
        it.episodes
            .first()
            .filter(|e| e.file.contains('/'))
            .and_then(|e| e.file.split('/').next())
            .map(String::from)
            .ok_or("Esa serie no está en una carpeta propia: escribe el nombre en vez de elegirla")?
    };
    if clean_dir(&folder) != folder {
        return Err(
            "El nombre de la carpeta de esa serie no es compatible: escribe el nombre en vez de elegirla".into()
        );
    }
    req.title = folder;
    Ok(())
}
