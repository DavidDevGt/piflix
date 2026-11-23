//! Autenticación de administrador (cabecera `x-admin-token`).

use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{api::err, prelude::*};

// ---------------------------------------------------------------- autenticación

pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut d = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        d |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    d == 0
}

static FAILED_ATTEMPTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn authorized(s: &Shared, h: &HeaderMap) -> Result<(), Box<Response>> {
    let Some(tok) = s.cfg.admin_token.as_deref() else {
        return Err(Box::new(err(
            StatusCode::FORBIDDEN,
            "Las descargas están desactivadas: falta ADMIN_TOKEN (mínimo 12 caracteres) en el servidor",
        )));
    };
    let given = h.get("x-admin-token").and_then(|v| v.to_str().ok()).unwrap_or("");
    if ct_eq(given.as_bytes(), tok.as_bytes()) {
        Ok(())
    } else {
        // Los fallos se atienden de uno en uno: con peticiones en paralelo la espera no se esquivaría.
        let _turn = FAILED_ATTEMPTS.lock().await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        Err(Box::new(err(StatusCode::UNAUTHORIZED, "Clave de administrador incorrecta")))
    }
}

/// Extractor: la petición debe traer la clave de administrador. Como va antes del cuerpo en la firma
/// del handler, se comprueba ANTES de leer una sola línea del cuerpo (subidas incluidas).
pub(crate) struct Admin;

#[axum::async_trait]
impl FromRequestParts<Shared> for Admin {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, s: &Shared) -> Result<Self, Response> {
        authorized(s, &parts.headers).await.map(|_| Admin).map_err(|r| *r)
    }
}
