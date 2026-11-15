//! Fakturace pro Linux – spouští webový server z příkazové řádky.
use fakturace::{start, Config, VERSION};
use std::path::PathBuf;

fn main() {
    let mut host = "127.0.0.1".to_string();
    let mut port = 8080u16;
    let mut data: Option<PathBuf> = None;
    let mut password = std::env::var("FAKTURACE_HESLO").ok().filter(|s| !s.is_empty());
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = |n: &str| args.next().unwrap_or_else(|| { eprintln!("Chybí hodnota pro {n}"); std::process::exit(2) });
        match a.as_str() {
            "--host" => host = val("--host"),
            "--port" | "-p" => port = val("--port").parse().unwrap_or_else(|_| { eprintln!("Neplatný port"); std::process::exit(2) }),
            "--data" | "-d" => data = Some(PathBuf::from(val("--data"))),
            "--heslo" | "--password" => password = Some(val("--heslo")),
            "--version" | "-V" => { println!("fakturace {VERSION}"); return; }
            _ => {
                println!("Fakturace {VERSION} – faktury, zálohové faktury a paragony v prohlížeči\n\n\
Použití: fakturace [volby]\n\n  \
--port, -p <číslo>   port webového serveru (výchozí 8080)\n  \
--host <adresa>      adresa, na které server poslouchá (výchozí 127.0.0.1;\n                       0.0.0.0 zpřístupní aplikaci v síti – nastavte pak heslo)\n  \
--data, -d <složka>  složka s daty (výchozí ~/.local/share/fakturace)\n  \
--heslo <heslo>      vyžadovat heslo (HTTP Basic, uživatel libovolný);\n                       lze i proměnnou prostředí FAKTURACE_HESLO\n  \
--version, -V        vypíše verzi");
                return;
            }
        }
    }
    let dir = data.unwrap_or_else(|| {
        if let Ok(x) = std::env::var("XDG_DATA_HOME") { if !x.is_empty() { return PathBuf::from(x).join("fakturace"); } }
        match std::env::var("HOME") { Ok(h) if !h.is_empty() => PathBuf::from(h).join(".local/share/fakturace"), _ => PathBuf::from("fakturace-data") }
    });
    let ip: std::net::IpAddr = host.parse().unwrap_or_else(|_| { eprintln!("Neplatná adresa --host (zadejte IP adresu, např. 127.0.0.1)"); std::process::exit(2) });
    let lan = !ip.is_loopback() && password.is_none();
    let (port, workers) = match start(Config { ip, port, dir: dir.clone(), password, token: None }) {
        Ok(x) => x,
        Err(e) => { eprintln!("Chyba: {e}"); std::process::exit(1) }
    };
    println!("Fakturace {VERSION} běží na http://{}:{}", if ip.is_unspecified() { "localhost".to_string() } else { host }, port);
    println!("Data: {}", dir.display());
    if lan { println!("POZOR: aplikace je dostupná ze sítě bez hesla. Spusťte ji s --heslo <heslo>."); }
    println!("Ukončení: Ctrl+C");
    for w in workers { let _ = w.join(); }
}
