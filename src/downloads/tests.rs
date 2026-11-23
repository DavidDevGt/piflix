use super::*;
use crate::{auth::Admin, config::Config, model::AppState};
use axum::extract::FromRequestParts;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::RwLock,
};

const MB: usize = 1024 * 1024;

fn video(len: usize, seed: u8) -> Vec<u8> {
    let mut v: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect();
    v[4..8].copy_from_slice(b"ftyp");
    v
}

struct Srv {
    data: Mutex<Arc<Vec<u8>>>,
    etag: Mutex<String>,
    advertise: bool,
    honor_range: bool,
    cut_after: Mutex<Option<usize>>,
    stall_after: Mutex<Option<usize>>,
    body_gets: AtomicUsize,
    ranges: Mutex<Vec<String>>,
}

impl Srv {
    fn new(data: Vec<u8>, etag: &str) -> Srv {
        Srv {
            data: Mutex::new(Arc::new(data)),
            etag: Mutex::new(etag.to_string()),
            advertise: true,
            honor_range: true,
            cut_after: Mutex::new(None),
            stall_after: Mutex::new(None),
            body_gets: AtomicUsize::new(0),
            ranges: Mutex::new(Vec::new()),
        }
    }
    fn swap(&self, data: Vec<u8>, etag: &str) {
        *self.data.lock().unwrap() = Arc::new(data);
        *self.etag.lock().unwrap() = etag.to_string();
    }
    fn gets(&self) -> usize {
        self.body_gets.load(Ordering::SeqCst)
    }
}

async fn serve(srv: Arc<Srv>, mut sock: TcpStream) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = sock.read(&mut tmp).await.unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let method = head.lines().next().unwrap_or("").split(' ').next().unwrap_or("").to_string();
    let hdr = |name: &str| -> Option<String> {
        head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    let range = hdr("range");
    let if_range = hdr("if-range");
    let data = srv.data.lock().unwrap().clone();
    let etag = srv.etag.lock().unwrap().clone();
    let total = data.len();
    let mut common = format!("ETag: {etag}\r\nContent-Type: video/mp4\r\nConnection: close\r\n");
    if srv.advertise {
        common.push_str("Accept-Ranges: bytes\r\n");
    }
    if method == "HEAD" {
        let _ = sock.write_all(format!("HTTP/1.1 200 OK\r\n{common}Content-Length: {total}\r\n\r\n").as_bytes()).await;
        return;
    }
    let is_probe = range.as_deref() == Some("bytes=0-0");
    let (mut start, mut end, mut status) = (0usize, total - 1, 200);
    if let Some(r) = &range {
        let ok_validator = if_range.as_ref().is_none_or(|v| *v == etag);
        if srv.honor_range && ok_validator {
            let spec = r.trim_start_matches("bytes=");
            if let Some((a, b)) = spec.split_once('-') {
                start = a.parse().unwrap_or(0);
                end = b.parse().unwrap_or(total - 1).min(total - 1);
                status = 206;
            }
        }
    }
    if !is_probe {
        srv.body_gets.fetch_add(1, Ordering::SeqCst);
        srv.ranges.lock().unwrap().push(range.clone().unwrap_or_default());
    }
    let body = &data[start..=end];
    let mut h = format!("HTTP/1.1 {} X\r\n{common}", status);
    if status == 206 {
        h.push_str(&format!("Content-Range: bytes {start}-{end}/{total}\r\n"));
    }
    h.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let _ = sock.write_all(h.as_bytes()).await;
    let cut = if is_probe { None } else { srv.cut_after.lock().unwrap().take() };
    let stall = if is_probe { None } else { srv.stall_after.lock().unwrap().take() };
    if let Some(n) = cut {
        let _ = sock.write_all(&body[..n.min(body.len())]).await;
        let _ = sock.flush().await;
        return; // cierra a mitad: curl sale con 18
    }
    if let Some(n) = stall {
        let _ = sock.write_all(&body[..n.min(body.len())]).await;
        let _ = sock.flush().await;
        tokio::time::sleep(Duration::from_secs(600)).await;
        return;
    }
    let _ = sock.write_all(body).await;
}

struct Env {
    s: Shared,
    root: PathBuf,
    srv: Arc<Srv>,
    req: NewJob,
    dest: PathBuf,
}

async fn env(name: &str, srv: Srv) -> Env {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("piflix-test-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Peliculas")).unwrap();
    let cfg = Config {
        port: 0,
        media_root: root.clone(),
        cache_dir: root.join(".cache"),
        static_dir: PathBuf::from("static"),
        tmdb_key: None,
        admin_token: Some("xxxxxxxxxxxx".to_string()),
        lang: "es-MX".to_string(),
    };
    let (dl, rx) = Downloads::new();
    let s: Shared = Arc::new(AppState {
        cfg,
        items: RwLock::new(Vec::new()),
        enriching: AtomicBool::new(false),
        started: Instant::now(),
        dl,
    });
    tokio::spawn(worker(s.clone(), rx));
    let srv = Arc::new(srv);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sv = srv.clone();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(serve(sv.clone(), sock));
        }
    });
    let req = NewJob {
        url: format!("http://127.0.0.1:{port}/video.mp4"),
        kind: "pelicula".into(),
        title: "Prueba Reanudar".into(),
        year: Some(2020),
        season: None,
        episode: None,
        referer: None,
        show: None,
    };
    let dest = build_dest(&root, &req, Kind::Pelicula, "mp4").unwrap().0;
    Env { s, root, srv, req, dest }
}

impl Env {
    async fn start(&self) -> u64 {
        let r = create_inner(&self.s, self.req.clone()).await;
        assert_eq!(r.status(), StatusCode::ACCEPTED, "create debía responder 202");
        self.s.dl.jobs.lock().unwrap().last().unwrap().id
    }
    async fn wait(&self, id: u64, secs: u64, f: impl Fn(&Job) -> bool) -> Job {
        let t0 = Instant::now();
        loop {
            if let Some(j) = self.s.dl.get(id) {
                if f(&j) {
                    return j;
                }
            }
            assert!(
                t0.elapsed().as_secs() < secs,
                "tiempo agotado; estado: {:?}",
                self.s.dl.get(id).map(|j| (j.state, j.error))
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn admin() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("x-admin-token", "xxxxxxxxxxxx".parse().unwrap());
    h
}

// 1. Servidor con Range: corte a mitad, reintento con -C -, el archivo final verifica.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t1_reanuda_con_range() {
    let data = video(3 * MB, 1);
    let e = env("t1", Srv::new(data.clone(), "\"v1\"")).await;
    *e.srv.cut_after.lock().unwrap() = Some(2 * MB);
    let id = e.start().await;
    let j = e.wait(id, 40, |j| j.state == JobState::Listo).await;
    assert_eq!(std::fs::read(&e.dest).unwrap(), data, "contenido distinto");
    assert_eq!(j.intentos, 1);
    assert_eq!(e.srv.gets(), 2);
    let r = e.srv.ranges.lock().unwrap().clone();
    assert_eq!(r[0], "");
    let from: usize = r[1].trim_start_matches("bytes=").trim_end_matches('-').parse().unwrap();
    assert!(from >= MB, "debía reanudar desde el avance previo, no de cero: {}", r[1]);
    assert!(!part_path(&e.dest).exists() && !meta_path(&e.dest).exists());
    e.cleanup();
}

// 2. Servidor que anuncia rangos pero responde 200: curl sale con 33, reinicio limpio y sin error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t2_ignora_range_reinicio_limpio() {
    let data = video(3 * MB, 2);
    let mut srv = Srv::new(data.clone(), "\"v1\"");
    srv.honor_range = false;
    let e = env("t2", srv).await;
    *e.srv.cut_after.lock().unwrap() = Some(2 * MB);
    let id = e.start().await;
    let j = e.wait(id, 40, |j| j.state == JobState::Listo).await;
    assert!(j.error.is_none());
    assert_eq!(std::fs::read(&e.dest).unwrap(), data);
    assert_eq!(e.srv.gets(), 3, "corte + intento con rango (200) + reinicio completo");
    e.cleanup();
}

// 3. El servidor cambia el archivo entre intentos: If-Range falla, se descarta y se reinicia con el total nuevo.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t3_cambia_el_archivo() {
    let old = video(3 * MB, 3);
    let new = video(3 * MB + 500_000, 9);
    let e = env("t3", Srv::new(old, "\"v1\"")).await;
    *e.srv.cut_after.lock().unwrap() = Some(2 * MB);
    let id = e.start().await;
    let t0 = Instant::now();
    while e.srv.gets() < 1 {
        assert!(t0.elapsed().as_secs() < 20, "el servidor no recibió la primera petición");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    e.srv.swap(new.clone(), "\"v2\""); // antes de que acabe la espera del reintento
    let j = e.wait(id, 40, |j| j.state == JobState::Listo || j.state == JobState::Error).await;
    assert_eq!(j.state, JobState::Listo, "error: {:?}", j.error);
    assert_eq!(std::fs::read(&e.dest).unwrap(), new, "debía quedar la versión nueva completa");
    assert_eq!(j.total, Some(new.len() as u64), "el total debía refrescarse");
    e.cleanup();
}

// 4. .part ya completo: no se lanza curl.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t4_part_completo_no_lanza_curl() {
    let data = video(3 * MB, 4);
    let e = env("t4", Srv::new(data.clone(), "\"v1\"")).await;
    std::fs::write(part_path(&e.dest), &data).unwrap();
    write_meta(&e.dest, &Meta { validator: Some("\"v1\"".into()), total: Some(data.len() as u64) }).await;
    let id = e.start().await;
    e.wait(id, 20, |j| j.state == JobState::Listo).await;
    assert_eq!(e.srv.gets(), 0, "no debía pedirse el cuerpo");
    assert_eq!(std::fs::read(&e.dest).unwrap(), data);
    e.cleanup();
}

// 5. .part mayor que el total: se descarta y se reinicia.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t5_part_mayor_que_el_total() {
    let data = video(3 * MB, 5);
    let e = env("t5", Srv::new(data.clone(), "\"v1\"")).await;
    let mut big = data.clone();
    big.extend_from_slice(&[0u8; 100]);
    std::fs::write(part_path(&e.dest), &big).unwrap();
    write_meta(&e.dest, &Meta { validator: Some("\"v1\"".into()), total: Some(data.len() as u64) }).await;
    let id = e.start().await;
    e.wait(id, 30, |j| j.state == JobState::Listo).await;
    assert_eq!(e.srv.gets(), 1);
    assert_eq!(e.srv.ranges.lock().unwrap()[0], "", "debía pedir el archivo entero, sin Range");
    assert_eq!(std::fs::read(&e.dest).unwrap(), data);
    e.cleanup();
}

// 6. Cancelar conserva el .part (y su .json); descartar los borra.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t6_cancelar_conserva_descartar_borra() {
    let data = video(3 * MB, 6);
    let e = env("t6", Srv::new(data, "\"v1\"")).await;
    *e.srv.stall_after.lock().unwrap() = Some(3 * MB / 2);
    let id = e.start().await;
    let part = part_path(&e.dest);
    let t0 = Instant::now();
    while std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) < 1_200_000 {
        assert!(t0.elapsed().as_secs() < 30, "el .part no creció");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let r = cancel(State(e.s.clone()), Admin, AxPath(id)).await;
    assert_eq!(r.status(), StatusCode::OK);
    e.wait(id, 10, |j| j.state == JobState::Cancelado).await;
    assert!(part.exists(), "cancelar debía conservar el .part");
    assert!(meta_path(&e.dest).exists(), "y su .json");
    let r = discard(State(e.s.clone()), Admin, AxPath(id)).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(!part.exists() && !meta_path(&e.dest).exists(), "descartar debía borrarlos");
    assert!(e.s.dl.get(id).is_none());
    e.cleanup();
}

// 7. El extractor Admin: clave correcta pasa; incorrecta o ausente da 401 (antes de leer ningún cuerpo).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t7_admin_exige_la_clave() {
    let e = env("t7", Srv::new(video(2 * MB, 9), "\"v1\"")).await;
    let check = |h: HeaderMap| {
        let s = e.s.clone();
        async move {
            let mut parts = axum::http::Request::builder().body(()).unwrap().into_parts().0;
            parts.headers = h;
            Admin::from_request_parts(&mut parts, &s).await.map(|_| ())
        }
    };
    assert!(check(admin()).await.is_ok(), "la clave correcta debía pasar");
    let mut bad = HeaderMap::new();
    bad.insert("x-admin-token", "otra-clave-xx".parse().unwrap());
    assert_eq!(check(bad).await.unwrap_err().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(check(HeaderMap::new()).await.unwrap_err().status(), StatusCode::UNAUTHORIZED);
    e.cleanup();
}
