//! Configuración desde variables de entorno.

use crate::prelude::*;

// ---------------------------------------------------------------- config

pub(crate) struct Config {
    pub(crate) port: u16,
    pub(crate) media_root: PathBuf,
    pub(crate) cache_dir: PathBuf,
    pub(crate) static_dir: PathBuf,
    pub(crate) tmdb_key: Option<String>,
    pub(crate) admin_token: Option<String>,
    pub(crate) lang: String,
}

impl Config {
    pub(crate) fn from_env() -> Self {
        let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        Config {
            port: env("PORT", "8080").parse().unwrap_or(8080),
            media_root: PathBuf::from(env("MEDIA_ROOT", "/mnt/storage")),
            cache_dir: PathBuf::from(env("CACHE_DIR", "/mnt/storage/.piflix-cache")),
            static_dir: PathBuf::from(env("STATIC_DIR", "static")),
            tmdb_key: std::env::var("TMDB_API_KEY").ok().filter(|k| !k.trim().is_empty()),
            admin_token: std::env::var("ADMIN_TOKEN").ok().filter(|t| t.trim().len() >= 12),
            lang: env("TMDB_LANG", "es-MX"),
        }
    }
}

impl Config {
    /// ¿Está el almacenamiento montado y accesible?
    pub(crate) fn storage_ok(&self) -> bool {
        self.media_root.is_dir()
    }

    /// Archivo de caché de un título (carátula, metadatos, título manual).
    pub(crate) fn cache_file(&self, id: &str, kind: crate::cache::CacheKind) -> PathBuf {
        crate::cache::cache_file(&self.cache_dir, id, kind)
    }
}
