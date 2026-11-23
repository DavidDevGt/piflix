# GordoFlix (PiFlix)

Servidor casero de biblioteca y streaming en Rust, pensado para una Raspberry Pi con poca RAM
(corre con `MemoryMax=180M`). Sirve películas y series desde un disco, con carátulas de TMDB,
reproductor web, subidas desde el navegador y descargas por enlace directo.

## Características

- **Catálogo automático**: escanea `Peliculas/` y `Series/`, deduce título, año, temporada y episodio
  (`S01E02`, `1x02`, carpetas `Temporada N`) y recarga solo al detectar cambios.
- **Streaming con Range**: seek y reanudación en el navegador (tower-http `ServeFile`).
- **Carátulas y sinopsis** desde TMDB (opcional), con edición manual de título y carátula.
- **Subidas** desde el navegador (una a la vez, hasta 50 GB, validación de firma de video).
- **Descargas por URL** con defensas SSRF (valida cada redirección y fija la IP resuelta), archivos `.part`,
  reanudación con `If-Range`, reintentos con espera exponencial y purga de `.part` viejos.
- Todo lo que escribe exige la clave de administrador (`x-admin-token`).

## Estructura

```
src/            servidor (axum + tokio); downloads/ contiene la cola de descargas
static/         frontend en HTML/CSS/JS plano, sin build
deploy/         unidad systemd y ejemplo de variables de entorno
scripts/        utilidades de operación
```

## Requisitos

- Rust estable y `curl` (TLS y descargas se hacen con el `curl` del sistema).
- Un directorio de medios con `Peliculas/` y `Series/`.

## Ejecutar

```sh
cp deploy/piflix.env.example .env   # edítalo
set -a; . ./.env; set +a
STATIC_DIR=static cargo run --release
```

Variables: `PORT` (8080), `MEDIA_ROOT` (`/mnt/storage`), `CACHE_DIR` (`/mnt/storage/.piflix-cache`),
`STATIC_DIR` (`static`), `TMDB_API_KEY`, `TMDB_LANG` (`es-MX`), `ADMIN_TOKEN` (mín. 12 caracteres).
Sin `ADMIN_TOKEN` las funciones de escritura quedan desactivadas.

## Instalar en la Pi (systemd)

```sh
cargo build --release          # en una Pi de 512 MB: CARGO_BUILD_JOBS=1
sudo install -d /opt/piflix && sudo cp -r target/release/piflix static /opt/piflix/
sudo cp deploy/piflix.service /etc/systemd/system/
sudo install -m 600 deploy/piflix.env.example /etc/piflix.env   # y edítalo
sudo systemctl enable --now piflix
```

`scripts/clave-admin.sh` muestra o rota la clave de administrador.

## Pruebas

```sh
cargo test
```

Incluyen un servidor HTTP falso que corta conexiones, ignora rangos y cambia de versión para probar la reanudación.

## Seguridad

- No publiques tu `/etc/piflix.env`. Pon el servicio detrás de HTTPS (p. ej. un túnel de Cloudflare):
  la clave viaja en una cabecera.
- Las rutas nunca vienen del cliente: se resuelven por ID contra el catálogo.

## Licencia

MIT
