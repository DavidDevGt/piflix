//! Archivos de la caché por título (`{id}.poster.jpg`, `{id}.json`…): el único sitio que conoce sus nombres.

use crate::prelude::*;

#[derive(Clone, Copy)]
pub(crate) enum CacheKind {
    Poster,
    Backdrop,
    /// metadatos de TMDB
    Meta,
    /// título puesto a mano
    Title,
}

impl CacheKind {
    fn suffix(self) -> &'static str {
        match self {
            CacheKind::Poster => "poster.jpg",
            CacheKind::Backdrop => "backdrop.jpg",
            CacheKind::Meta => "json",
            CacheKind::Title => "title.txt",
        }
    }
}

pub(crate) fn cache_file(dir: &Path, id: &str, kind: CacheKind) -> PathBuf {
    dir.join(format!("{id}.{}", kind.suffix()))
}

/// Escritura atómica: temporal oculto + renombrado, así nadie lee un archivo a medias.
pub(crate) async fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    let result = async {
        tokio::fs::write(&tmp, data).await?;
        tokio::fs::rename(&tmp, path).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_de_la_cache() {
        let d = Path::new("/c");
        assert_eq!(cache_file(d, "abc", CacheKind::Poster), PathBuf::from("/c/abc.poster.jpg"));
        assert_eq!(cache_file(d, "abc", CacheKind::Backdrop), PathBuf::from("/c/abc.backdrop.jpg"));
        assert_eq!(cache_file(d, "abc", CacheKind::Meta), PathBuf::from("/c/abc.json"));
        assert_eq!(cache_file(d, "abc", CacheKind::Title), PathBuf::from("/c/abc.title.txt"));
    }

    #[tokio::test]
    async fn escritura_atomica_reemplaza_y_no_deja_temporales() {
        let dir = std::env::temp_dir().join(format!("piflix-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.txt");
        write_atomic(&f, b"uno").await.unwrap();
        write_atomic(&f, b"dos").await.unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"dos");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no debe quedar el temporal");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
