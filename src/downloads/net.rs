//! Red de las descargas: validación de enlaces (SSRF), sondeo y curl.

use super::files::pick_validator;
use crate::prelude::*;

/// Referer válido para la configuración de curl: http(s), sin comillas, barras invertidas ni control.
pub(crate) fn clean_referer(r: &str) -> Result<String, String> {
    let r = r.trim();
    if r.is_empty() {
        return Ok(String::new());
    }
    let ok = (r.starts_with("http://") || r.starts_with("https://"))
        && r.len() <= 500
        && !r.chars().any(|c| c.is_control() || c == '"' || c == '\\' || c == ' ');
    if ok {
        Ok(r.to_string())
    } else {
        Err("El Referer debe ser una dirección http(s) válida".into())
    }
}

// ---------------------------------------------------------------- validación de enlaces

pub(crate) fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_broadcast()
                || v.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64)) // CGNAT 100.64/10
        }
        IpAddr::V6(v) => {
            if let Some(m) = v.to_ipv4_mapped() {
                return is_public(IpAddr::V4(m));
            }
            let s0 = v.segments()[0];
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (s0 & 0xfe00) == 0xfc00 // fc00::/7 (ULA)
                || (s0 & 0xffc0) == 0xfe80) // fe80::/10
        }
    }
}

/// Valida el enlace y devuelve el argumento `--resolve host:puerto:ip` que fija la IP comprobada.
pub(crate) async fn check_url(u: &str) -> Result<String, String> {
    if u.len() > 2048 || u.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '\'' | '\\')) {
        return Err("Enlace inválido".into());
    }
    let (rest, port0) = if let Some(r) = u.strip_prefix("https://") {
        (r, 443)
    } else if let Some(r) = u.strip_prefix("http://") {
        (r, 80)
    } else {
        return Err("El enlace debe empezar con http:// o https://".into());
    };
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    if auth.is_empty() || auth.contains('@') {
        return Err("Enlace inválido".into());
    }
    if auth.starts_with('[') {
        return Err("No se admiten direcciones IPv6 escritas directamente".into());
    }
    let (host, port) = match auth.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| "Puerto inválido".to_string())?),
        None => (auth, port0),
    };
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "No se pudo resolver el dominio del enlace".to_string())?
        .map(|a| a.ip())
        .collect();
    if addrs.is_empty() {
        return Err("No se pudo resolver el dominio del enlace".into());
    }
    if addrs.iter().any(|a| !(is_public(*a) || (cfg!(test) && a.is_loopback()))) {
        return Err("Ese enlace apunta a una dirección interna y no está permitido".into());
    }
    let pick = addrs.iter().find(|a| a.is_ipv4()).unwrap_or(&addrs[0]);
    let ip = match pick {
        IpAddr::V4(v) => v.to_string(),
        IpAddr::V6(v) => format!("[{v}]"),
    };
    Ok(format!("{host}:{port}:{ip}"))
}

pub(crate) struct Probe {
    pub(crate) url: String,
    pub(crate) resolve: String,
    pub(crate) total: Option<u64>,
    pub(crate) ctype: String,
    pub(crate) can_resume: bool,
    pub(crate) validator: Option<String>,
}

/// Range: bytes=0-0 debe dar 206 con Content-Range. Si el servidor ignora el rango y va a mandar
/// el archivo entero, --max-filesize corta al instante: curl sale con 63 y curl_out devuelve None.
/// Eso es lo ESPERADO ("sin rangos"), no un error. Nunca hace fallar el create.
pub(crate) async fn range_ok(url: &str, referer: &str, resolve: &str) -> bool {
    let args = ["-r", "0-0", "--max-filesize", "2", "-o", "/dev/null", "-w", "%{http_code}|%header{content-range}"];
    match curl_out(url, referer, resolve, &args).await {
        Some(o) => {
            let t = String::from_utf8_lossy(&o).to_string();
            t.starts_with("206|") && t.to_ascii_lowercase().contains("bytes")
        }
        None => false,
    }
}

/// HEAD con redirecciones manuales: cada salto se vuelve a validar.
pub(crate) async fn probe(url: &str, referer: &str) -> Result<Probe, String> {
    let mut cur = url.to_string();
    for _ in 0..6 {
        let resolve = check_url(&cur).await?;
        // etag va el último: si trae un '|' no corre los demás campos
        let fmt = "%{http_code}|%{redirect_url}|%{content_type}|%header{content-length}|%header{content-range}|%header{accept-ranges}|%header{last-modified}|%header{etag}";
        let mut got = (0u16, String::new(), String::new(), None, false, None);
        for head in [true, false] {
            let mode: &[&str] = if head { &["-I"] } else { &["-r", "0-0"] };
            let tail: &[&str] = &["-o", "/dev/null", "-w", fmt];
            let args = [mode, tail].concat();
            let out = curl_out(&cur, referer, &resolve, &args)
                .await
                .ok_or("No se pudo contactar con el servidor del enlace")?;
            let out = String::from_utf8_lossy(&out).to_string();
            let mut it = out.splitn(8, '|');
            let code: u16 = it.next().unwrap_or("").trim().parse().unwrap_or(0);
            let redirect = it.next().unwrap_or("").trim().to_string();
            let ctype = it.next().unwrap_or("").trim().to_ascii_lowercase();
            let clen = it.next().and_then(|x| x.trim().parse::<u64>().ok());
            let crange = it.next().and_then(|x| x.rsplit('/').next().and_then(|n| n.trim().parse::<u64>().ok()));
            // En un 206 el Content-Length son los bytes restantes: el total solo sale de Content-Range.
            let total = if code == 206 { crange } else { clen };
            let ar =
                it.next().unwrap_or("").to_ascii_lowercase().contains("bytes") || (code == 206 && crange.is_some());
            let lm = it.next().unwrap_or("").trim().to_string();
            let etag = it.next().unwrap_or("").trim().to_string();
            got = (code, redirect, ctype, total, ar, pick_validator(&etag, &lm));
            // Algunos servidores rechazan HEAD: reintentar con un GET de 1 byte.
            if !(head && matches!(code, 403 | 405 | 501)) {
                break;
            }
        }
        let (code, redirect, ctype, total, ar, validator) = got;
        if (300..400).contains(&code) && !redirect.is_empty() {
            cur = redirect;
            continue;
        }
        if !(200..300).contains(&code) {
            return Err(format!("El servidor del enlace respondió con el código {code}"));
        }
        // Reanudar exige poder verificar el tamaño exacto, así que sin total no hay reanudación.
        let can_resume = total.is_some() && (ar || range_ok(&cur, referer, &resolve).await);
        return Ok(Probe { url: cur, resolve, total, ctype, can_resume, validator });
    }
    Err("El enlace tiene demasiadas redirecciones".into())
}

pub(crate) const UA: &str =
    "Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

/// Referer = origen del enlace (algunos servidores lo exigen contra hotlinking).
pub(crate) fn origin(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, tail)) => format!("{scheme}://{}/", tail.split(['/', '?', '#']).next().unwrap_or("")),
        None => String::new(),
    }
}

/// Configuración de curl por stdin (URL y Referer no aparecen en `ps`).
pub(crate) fn curl_cfg(url: &str, referer: &str, if_range: Option<&str>) -> String {
    let r = if referer.is_empty() { origin(url) } else { referer.to_string() };
    // globoff: los corchetes/llaves de la URL (p. ej. "[latino]") no son patrones.
    let mut c = format!("globoff\nurl = \"{url}\"\nreferer = \"{r}\"\n");
    if let Some(v) = if_range {
        c.push_str(&format!("header = \"If-Range: {}\"\n", v.replace('"', "\\\"")));
    }
    c
}

/// curl sin shell.
pub(crate) async fn curl_out(url: &str, referer: &str, resolve: &str, extra: &[&str]) -> Option<Vec<u8>> {
    let base =
        ["-sS", "--max-time", "20", "--max-redirs", "0", "--proto", "=http,https", "--resolve", resolve, "-A", UA];
    let args = [&base[..], extra].concat();
    crate::curl::run(&args, &curl_cfg(url, referer, None)).await
}
