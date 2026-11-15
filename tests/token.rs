//! Ověřuje ochranu tokenem, kterou používá aplikace pro Android.
use std::io::{Read, Write};

fn get(port: u16, cookie: Option<&str>) -> String {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let c = cookie.map(|c| format!("Cookie: {c}\r\n")).unwrap_or_default();
    write!(s, "GET /api/state HTTP/1.1\r\nHost: 127.0.0.1\r\n{c}Connection: close\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.lines().next().unwrap_or("").to_string()
}

#[test]
fn token_is_required() {
    let dir = std::env::temp_dir().join(format!("fakturace-test-{}", std::process::id()));
    let (port, _workers) = fakturace::start(fakturace::Config {
        ip: "127.0.0.1".parse().unwrap(), port: 0, dir: dir.clone(), password: None, token: Some("tajne123".into()),
    }).unwrap();
    assert!(port > 0);
    assert!(get(port, None).contains("403"));
    assert!(get(port, Some("fakturace_token=spatne")).contains("403"));
    assert!(get(port, Some("jine=1; fakturace_token=tajne123")).contains("200"));
    let _ = std::fs::remove_dir_all(dir);
}
