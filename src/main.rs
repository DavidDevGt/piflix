//! GordoFlix: servidor de biblioteca y streaming. Este archivo solo cablea módulos y rutas.

mod admin;
mod api;
mod auth;
mod cache;
mod config;
mod curl;
mod downloads;
mod library;
mod model;
mod tmdb;
mod upload;

/// Importaciones comunes: cada módulo empieza con `use crate::prelude::*;`.
#[allow(unused_imports)]
mod prelude {
    pub(crate) use std::{
        collections::BTreeMap,
        net::IpAddr,
        path::{Path, PathBuf},
        process::Stdio,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, Mutex,
        },
        time::{Duration, Instant, SystemTime},
    };

    pub(crate) use axum::{
        body::{Body, Bytes},
        extract::{Path as AxPath, Query, Request, State},
        http::{header, HeaderMap, HeaderValue, StatusCode},
        response::{IntoResponse, Response},
        Json,
    };
    pub(crate) use serde::{Deserialize, Serialize};
    pub(crate) use serde_json::{json, Value};
    pub(crate) use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        process::Command,
        sync::{mpsc, Notify, RwLock},
    };

    pub(crate) use crate::{
        api::{disk_unavailable, err, ok},
        cache::{cache_file, write_atomic, CacheKind},
        config::Config,
        library::{refresh_library, rescan},
        model::{AppState, Episode, Item, Kind, Shared, VIDEO_EXT},
        tmdb::{enrich_all, enrich_one, Fetch},
    };
}

use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::services::ServeDir;

use crate::{api::*, library::watch_library, prelude::*};

// ---------------------------------------------------------------- main

fn main() {
    // 2 hilos de trabajo bastan para un servidor de streaming en 4 núcleos lentos
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(run());
}

async fn run() {
    let cfg = Config::from_env();
    let port = cfg.port;
    let (dl, dl_rx) = downloads::Downloads::new();
    let state: Shared = Arc::new(AppState {
        cfg,
        items: RwLock::new(Vec::new()),
        enriching: AtomicBool::new(false),
        started: Instant::now(),
        dl,
    });

    match rescan(&state).await {
        Ok(n) => eprintln!("piflix: {n} títulos en la biblioteca"),
        Err(e) => eprintln!("piflix: aviso: {e}"),
    }
    if state.cfg.tmdb_key.is_none() {
        eprintln!("piflix: TMDB_API_KEY no configurada, sin carátulas");
    }
    if state.cfg.admin_token.is_none() {
        eprintln!("piflix: ADMIN_TOKEN no configurado (mínimo 12 caracteres): descargas desactivadas");
    }
    tokio::spawn(enrich_all(state.clone()));
    tokio::spawn(downloads::worker(state.clone(), dl_rx));
    tokio::spawn(downloads::sweeper(state.clone()));
    tokio::spawn(watch_library(state.clone()));

    let static_dir = state.cfg.static_dir.clone();
    let app = Router::new()
        .route("/api/media", get(api_media))
        .route("/api/media/random", get(api_random))
        .route("/api/media/:id/stream", get(api_stream))
        .route("/api/img/:id/:kind", get(api_image))
        .route("/api/reload", post(api_reload))
        .route("/api/health", get(api_health))
        .route("/api/downloads", get(downloads::list).post(downloads::create))
        .route("/api/downloads/:id", delete(downloads::cancel))
        .route("/api/upload", post(upload::upload))
        .route("/api/cover/:id", post(admin::set_cover))
        .route("/api/title/:id", post(admin::set_title))
        .route("/api/downloads/:id/discard", post(downloads::discard))
        .route("/api/downloads/:id/resume", post(downloads::resume))
        .fallback_service(ServeDir::new(static_dir).append_index_html_on_directories(true))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.expect("bind");
    eprintln!("piflix: escuchando en http://0.0.0.0:{port}");
    axum::serve(listener, app).await.expect("serve");
}
