//! Modelo del catálogo y estado compartido.

use crate::{downloads::Downloads, prelude::*};

pub(crate) const VIDEO_EXT: &[&str] = &["mp4", "m4v", "webm", "mkv", "mov", "avi"];

// ---------------------------------------------------------------- modelo

/// Película o serie. Se serializa como "pelicula" / "serie" (lo que ya espera el sitio).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Pelicula,
    Serie,
}

impl Kind {
    pub(crate) fn parse(s: &str) -> Option<Kind> {
        match s {
            "pelicula" => Some(Kind::Pelicula),
            "serie" => Some(Kind::Serie),
            _ => None,
        }
    }
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Kind::Pelicula => "pelicula",
            Kind::Serie => "serie",
        }
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct Episode {
    pub(crate) id: String,
    pub(crate) season: u32,
    pub(crate) episode: u32,
    pub(crate) title: String,
    pub(crate) file: String,
    pub(crate) size: u64,
    #[serde(skip)]
    pub(crate) path: PathBuf,
}

/// Una película (un archivo) o una serie (varios episodios agrupados).
#[derive(Clone, Serialize)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) year: Option<u16>,
    pub(crate) kind: Kind,
    pub(crate) file: String,
    pub(crate) size: u64,
    pub(crate) overview: Option<String>,
    pub(crate) has_poster: bool,
    pub(crate) has_backdrop: bool,
    /// mtime de la carátula (segundos): invalida la caché del navegador al cambiarla
    pub(crate) poster_v: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) episodes: Vec<Episode>,
    #[serde(skip)]
    pub(crate) path: PathBuf,
    #[serde(skip)]
    pub(crate) search_title: String,
}

pub(crate) struct AppState {
    pub(crate) cfg: Config,
    pub(crate) items: RwLock<Vec<Item>>,
    pub(crate) enriching: AtomicBool,
    pub(crate) started: Instant,
    pub(crate) dl: Downloads,
}

pub(crate) type Shared = Arc<AppState>;

impl Item {
    /// Elemento vacío: quien lo construye completa año, archivo, tamaño y ruta.
    pub(crate) fn new(kind: Kind, id: String, title: String) -> Item {
        Item {
            id,
            search_title: title.to_lowercase(),
            title,
            year: None,
            kind,
            file: String::new(),
            size: 0,
            overview: None,
            has_poster: false,
            has_backdrop: false,
            poster_v: 0,
            episodes: Vec::new(),
            path: PathBuf::new(),
        }
    }

    /// Cambia el título visible y el de búsqueda a la vez.
    pub(crate) fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
        self.search_title = title.to_lowercase();
    }
}

impl AppState {
    pub(crate) async fn has_item(&self, id: &str) -> bool {
        self.items.read().await.iter().any(|i| i.id == id)
    }
}
