use super::*;
use crate::auth::ct_eq;

fn req(kind: &str, title: &str) -> NewJob {
    NewJob {
        url: String::new(),
        kind: kind.into(),
        title: title.into(),
        year: None,
        season: None,
        episode: None,
        referer: None,
        show: None,
    }
}

#[test]
fn slug_y_carpetas() {
    assert_eq!(slug("El Niño Ágil!"), "el-nino-agil");
    assert_eq!(slug("!!!"), "");
    assert_eq!(clean_dir("a/b:c."), "abc");
    assert_eq!(clean_dir("  ..Serie  "), "Serie");
}

#[test]
fn destinos() {
    let root = Path::new("/r");
    let mut m = req("pelicula", "El Hijo de Batman");
    m.year = Some(2014);
    let (p, name) = build_dest(root, &m, Kind::Pelicula, "mp4").unwrap();
    assert_eq!(
        (p, name.as_str()),
        (PathBuf::from("/r/Peliculas/el-hijo-de-batman-2014.mp4"), "El Hijo de Batman (2014)")
    );
    m.year = Some(1800);
    assert!(build_dest(root, &m, Kind::Pelicula, "mp4").is_err());

    let mut s = req("serie", "Avatar Seven Havens");
    assert!(build_dest(root, &s, Kind::Serie, "mkv").is_err()); // falta temporada
    s.season = Some(1);
    s.episode = Some(2);
    let (p, name) = build_dest(root, &s, Kind::Serie, "mkv").unwrap();
    assert_eq!(p, PathBuf::from("/r/Series/Avatar Seven Havens/Temporada 1/avatar-seven-havens-s01e02.mkv"));
    assert_eq!(name, "Avatar Seven Havens · T1 E2");
    s.episode = Some(0);
    assert!(build_dest(root, &s, Kind::Serie, "mkv").is_err());
    assert!(build_dest(root, &req("pelicula", "***"), Kind::Pelicula, "mp4").is_err());
}

#[test]
fn firmas_de_video() {
    assert!(magic_ok("mp4", b"\0\0\0\x18ftypisom"));
    assert!(!magic_ok("mp4", b"<html>......"));
    assert!(magic_ok("mkv", &[0x1A, 0x45, 0xDF, 0xA3, 0]));
    assert!(magic_ok("avi", b"RIFF1234AVI "));
    assert!(!magic_ok("avi", b"MZ"));
}

#[test]
fn ips_publicas_e_internas() {
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    assert!(is_public(ip("8.8.8.8")));
    for internal in [
        "10.0.0.1",
        "192.168.1.9",
        "127.0.0.1",
        "169.254.1.1",
        "100.64.0.1",
        "0.0.0.0",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:10.0.0.1",
    ] {
        assert!(!is_public(ip(internal)), "{internal}");
    }
}

#[test]
fn comparacion_de_clave() {
    assert!(ct_eq(b"secreto-largo", b"secreto-largo"));
    assert!(!ct_eq(b"secreto-largo", b"secreto-larga"));
    assert!(!ct_eq(b"corta", b"corta-pero-mas"));
}

#[test]
fn nombres_de_part() {
    let d = Path::new("/r/Peliculas/a.mp4");
    assert_eq!(part_path(d), PathBuf::from("/r/Peliculas/.a.mp4.part"));
    assert_eq!(meta_path(d), PathBuf::from("/r/Peliculas/.a.mp4.part.json"));
}

#[test]
fn validadores() {
    assert_eq!(pick_validator("\"abc\"", "Tue"), Some("\"abc\"".into()));
    assert_eq!(pick_validator("W/\"abc\"", "Tue"), Some("Tue".into())); // débil: no sirve en If-Range
    assert_eq!(pick_validator("", ""), None);
}

#[test]
fn extension_segun_url_o_tipo() {
    assert_eq!(pick_ext("https://h/x.MKV?t=1", ""), "mkv");
    assert_eq!(pick_ext("https://h/x", "video/webm"), "webm");
    assert_eq!(pick_ext("https://h/x", ""), "mp4");
}
