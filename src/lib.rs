//! Fakturace – malý webový server pro vystavování faktur, zálohových faktur a paragonů.
//! Knihovna: sdílí ji linuxový program (src/main.rs) i aplikace pro Android (JNI).
mod isdoc;
mod model;
mod pdf;
mod templates;

use model::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response, Server};

static INDEX: &str = include_str!("../assets/index.html");
const MAX_BODY: u64 = 24 * 1024 * 1024;

struct Resp { code: u16, ctype: &'static str, body: Vec<u8>, extra: Vec<(String, String)> }
type R = Result<Resp, (u16, String)>;

fn ok_json(v: Value) -> R { Ok(Resp { code: 200, ctype: "application/json; charset=utf-8", body: v.to_string().into_bytes(), extra: vec![] }) }
fn bad<T>(msg: impl Into<String>) -> Result<T, (u16, String)> { Err((400, msg.into())) }
fn file(ctype: &'static str, body: Vec<u8>, name: &str, download: bool) -> R {
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    let disp = format!("{}; filename=\"{}\"", if download { "attachment" } else { "inline" }, safe);
    Ok(Resp { code: 200, ctype, body, extra: vec![("Content-Disposition".into(), disp)] })
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 1) && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; continue; }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&').filter(|p| !p.is_empty()).map(|p| {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        (pct_decode(k), pct_decode(v))
    }).collect()
}

fn clean(s: &mut String) { *s = s.trim().to_string(); }

/// Doplní a očistí doklad před uložením nebo náhledem.
fn prepare(inv: &mut Invoice, set: &Settings, existing: Option<&Invoice>) {
    for s in [&mut inv.customer.name, &mut inv.customer.street, &mut inv.customer.city, &mut inv.customer.zip,
              &mut inv.customer.ico, &mut inv.customer.dic, &mut inv.note, &mut inv.order_no, &mut inv.deposit_ref] { clean(s); }
    if !KINDS.contains(&inv.kind.as_str()) { inv.kind = "faktura".into(); }
    if !matches!(inv.payment.as_str(), "prevod" | "hotove" | "karta" | "dobirka" | "zapocet") { inv.payment = "prevod".into(); }
    inv.items.retain(|i| !(i.desc.trim().is_empty() && i.price == 0.0));
    for it in &mut inv.items {
        clean(&mut it.desc);
        clean(&mut it.unit);
        if !it.qty.is_finite() { it.qty = 0.0; }
        if !it.price.is_finite() { it.price = 0.0; }
        if !it.vat.is_finite() || it.vat < 0.0 { it.vat = 0.0; }
    }
    if !inv.deposit.is_finite() { inv.deposit = 0.0; }
    match existing {
        Some(e) => {
            inv.id = e.id; inv.number = e.number.clone(); inv.year = e.year; inv.seq = e.seq;
            inv.uuid = e.uuid.clone(); inv.created = e.created.clone(); inv.kind = e.kind.clone();
            inv.canceled = e.canceled;
            inv.supplier = if inv.refresh_supplier { set.company.clone() } else { e.supplier.clone() };
        }
        None => inv.supplier = set.company.clone(),
    }
    if !inv.supplier.vat_payer { inv.reverse_charge = false; inv.prices_incl_vat = false; }
    if inv.kind == "paragon" {
        if inv.payment == "prevod" { inv.payment = "hotove".into(); }
        inv.due_date = inv.issue_date.clone();
        if inv.paid_date.is_empty() { inv.paid_date = inv.issue_date.clone(); }
    }
    if inv.kind == "ddpp" && inv.paid_date.is_empty() { inv.paid_date = inv.tax_date.clone(); }
    if inv.tax_date.is_empty() { inv.tax_date = inv.issue_date.clone(); }
    if inv.due_date.is_empty() { inv.due_date = inv.issue_date.clone(); }
    if inv.vs.trim().is_empty() || existing.is_none() && inv.vs.chars().any(|c| !c.is_ascii_digit()) {
        inv.vs = inv.number.chars().filter(|c| c.is_ascii_digit()).collect();
    }
    if inv.vs.len() > 10 { inv.vs = inv.vs[inv.vs.len() - 10..].to_string(); }
}

fn inv_json(inv: &Invoice, set: &Settings) -> Value {
    let mut v = serde_json::to_value(inv).unwrap_or(json!({}));
    v["totals"] = serde_json::to_value(compute(inv, set.round_cash)).unwrap_or(json!({}));
    v
}

fn sample_invoice(set: &Settings) -> Invoice {
    let mut c = set.company.clone();
    if c.name.is_empty() {
        c = Company {
            name: "Vzorová firma s.r.o.".into(), street: "Dlouhá 12/4".into(), city: "Praha 1".into(), zip: "110 00".into(),
            ico: "12345678".into(), dic: "CZ12345678".into(), vat_payer: true,
            register: "Společnost je zapsána v obchodním rejstříku vedeném Městským soudem v Praze, oddíl C, vložka 12345.".into(),
            bank_account: "2702507745/2010".into(), email: "info@vzorova-firma.cz".into(), phone: "+420 777 123 456".into(),
            issued_by: "Jana Nováková".into(), ..Company::default()
        };
    }
    let t = today();
    let due = parse_date(&t).map(|d| { let (y, m, dd) = civil(d + 14); format!("{y:04}-{m:02}-{dd:02}") }).unwrap_or_default();
    Invoice {
        kind: "faktura".into(), number: format_number("{RRRR}{NNNNNN}", set.year, 42), vs: format_number("{RRRR}{NNNNNN}", set.year, 42),
        issue_date: t.clone(), tax_date: t, due_date: due, payment: "prevod".into(), supplier: c,
        customer: Party { name: "Odběratel a syn, spol. s r.o.".into(), street: "Náměstí Míru 8".into(), city: "Brno".into(), zip: "602 00".into(), country: String::new(), ico: "87654321".into(), dic: "CZ87654321".into() },
        items: vec![
            Item { desc: "Návrh a realizace kamerového systému (provozovna Praha 10)".into(), qty: 1.0, unit: "ks".into(), price: 9210.0, vat: 21.0 },
            Item { desc: "Servisní práce technika".into(), qty: 3.5, unit: "hod".into(), price: 850.0, vat: 21.0 },
            Item { desc: "Odborná publikace – příručka uživatele".into(), qty: 2.0, unit: "ks".into(), price: 320.0, vat: 12.0 },
        ],
        note: "Děkujeme za spolupráci.".into(), uuid: uuid4(), ..Invoice::default()
    }
}

fn csv_export(db: &Db, year: Option<i32>) -> Vec<u8> {
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut out = String::from("\u{feff}Číslo;Typ;Odběratel;IČO;DIČ;Datum vystavení;DUZP;Splatnost;Uhrazeno;Forma úhrady;Základ;DPH;Celkem;K úhradě;Stav\r\n");
    let mut list: Vec<&Invoice> = db.invoices.iter().filter(|i| year.map(|y| i.year == y).unwrap_or(true)).collect();
    list.sort_by(|a, b| (a.year, &a.kind, a.seq).cmp(&(b.year, &b.kind, b.seq)));
    for i in list {
        let t = compute(i, db.settings.round_cash);
        let n = |h: i64| cz_num(h).replace('\u{a0}', "");
        let state = if i.canceled { "stornováno" } else if !i.paid_date.is_empty() { "uhrazeno" } else { "neuhrazeno" };
        let row = [q(&i.number), q(kind_label(&i.kind)), q(&i.customer.name), q(&i.customer.ico), q(&i.customer.dic),
            cz_date(&i.issue_date), cz_date(&i.tax_date), cz_date(&i.due_date), cz_date(&i.paid_date),
            q(payment_label(&i.payment)), n(t.base), n(t.tax), n(t.total), n(t.payable), state.to_string()];
        out.push_str(&row.join(";"));
        out.push_str("\r\n");
    }
    out.into_bytes()
}

fn body_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, (u16, String)> {
    serde_json::from_slice(body).map_err(|e| (400, format!("Neplatná data: {e}")))
}

fn route(method: &Method, path: &str, query: &HashMap<String, String>, body: &[u8], store: &Mutex<Store>) -> R {
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    let mut st = store.lock().unwrap_or_else(|e| e.into_inner());
    let tpl = query.get("template").map(|s| s.as_str());
    let dl = query.get("dl").map(|v| v == "1").unwrap_or(false);
    match (method, seg.as_slice()) {
        (Method::Get, [""]) => Ok(Resp { code: 200, ctype: "text/html; charset=utf-8", body: INDEX.as_bytes().to_vec(), extra: vec![("Cache-Control".into(), "no-cache".into())] }),
        (Method::Get, ["fonts", name]) => match pdf::font_file(name.trim_end_matches(".ttf")) {
            Some(d) => Ok(Resp { code: 200, ctype: "font/ttf", body: d.to_vec(), extra: vec![("Cache-Control".into(), "max-age=604800".into())] }),
            None => Err((404, "Nenalezeno".into())),
        },
        (Method::Get, ["api", "state"]) => {
            let set = &st.db.settings;
            let invs: Vec<Value> = st.db.invoices.iter().map(|i| inv_json(i, set)).collect();
            let preview: HashMap<&str, String> = KINDS.iter().map(|k| {
                let s = set.series.get(*k).cloned().unwrap_or_default();
                (*k, format_number(&s.pattern, set.year, s.next))
            }).collect();
            ok_json(json!({
                "settings": set, "contacts": st.db.contacts, "invoices": invs, "next_numbers": preview, "today": today(),
                "templates": templates::TEMPLATES.iter().map(|(id, n, d)| json!({"id": id, "name": n, "desc": d})).collect::<Vec<_>>(),
                "version": env!("CARGO_PKG_VERSION"),
            }))
        }
        (Method::Put, ["api", "settings"]) => {
            let mut s: Settings = body_json(body)?;
            for k in KINDS {
                let e = s.series.entry(k.to_string()).or_default();
                if !pattern_ok(&e.pattern) { return bad(format!("Číselná řada „{}“ musí obsahovat pořadové číslo, např. {{NNNN}}.", kind_label(k))); }
                if e.next == 0 { e.next = 1; }
                let used = st.db.invoices.iter().filter(|i| i.kind == k && i.year == s.year).map(|i| i.seq).max().unwrap_or(0);
                if e.next <= used { return bad(format!("Řada „{}“: další číslo musí být vyšší než poslední vystavené ({used}).", kind_label(k))); }
            }
            if !(2000..=2200).contains(&s.year) { return bad("Neplatný rok."); }
            if s.accent.len() != 7 || !s.accent.starts_with('#') { s.accent = "#3d6df2".into(); }
            for img in [&s.logo, &s.stamp] {
                if !img.is_empty() && pdf::Image::from_data_url(img).is_none() { return bad("Obrázek se nepodařilo načíst. Použijte PNG nebo JPEG."); }
            }
            if let Some(i) = cz_iban(&s.company.bank_account) { if s.company.iban.trim().is_empty() { s.company.iban = i; } }
            st.db.settings = s;
            st.save().map_err(|e| (500, e))?;
            ok_json(json!({"ok": true}))
        }
        (Method::Post, ["api", "year"]) => {
            let v: Value = body_json(body)?;
            let y = v["year"].as_i64().unwrap_or(0) as i32;
            if !(2000..=2200).contains(&y) { return bad("Zadejte platný rok."); }
            st.db.settings.year = y;
            let invs = &st.db.invoices;
            let used: HashMap<String, u32> = KINDS.iter().map(|k| (k.to_string(), invs.iter().filter(|i| i.kind == *k && i.year == y).map(|i| i.seq).max().unwrap_or(0))).collect();
            for (k, s) in st.db.settings.series.iter_mut() { s.next = used.get(k).copied().unwrap_or(0) + 1; }
            st.save().map_err(|e| (500, e))?;
            ok_json(json!({"ok": true}))
        }
        (Method::Post, ["api", "contacts"]) | (Method::Put, ["api", "contacts", _]) => {
            let mut c: Contact = body_json(body)?;
            if c.name.trim().is_empty() { return bad("Vyplňte název nebo jméno."); }
            if let [_, _, id] = seg.as_slice() {
                let id: u64 = id.parse().map_err(|_| (400, "Neplatné ID".to_string()))?;
                c.id = id;
                match st.db.contacts.iter_mut().find(|x| x.id == id) { Some(x) => *x = c.clone(), None => return Err((404, "Kontakt nenalezen.".into())) }
            } else {
                c.id = st.new_id();
                st.db.contacts.push(c.clone());
            }
            st.save().map_err(|e| (500, e))?;
            ok_json(json!(c))
        }
        (Method::Delete, ["api", "contacts", id]) => {
            let id: u64 = id.parse().map_err(|_| (400, "Neplatné ID".to_string()))?;
            st.db.contacts.retain(|c| c.id != id);
            st.save().map_err(|e| (500, e))?;
            ok_json(json!({"ok": true}))
        }
        (Method::Post, ["api", "calc"]) => {
            let mut inv: Invoice = body_json(body)?;
            let ex = st.db.invoices.iter().find(|i| i.id == inv.id && inv.id != 0).cloned();
            prepare(&mut inv, &st.db.settings, ex.as_ref());
            ok_json(serde_json::to_value(compute(&inv, st.db.settings.round_cash)).unwrap_or(json!({})))
        }
        (Method::Post, ["api", "preview"]) => {
            let mut inv: Invoice = body_json(body)?;
            let ex = st.db.invoices.iter().find(|i| i.id == inv.id && inv.id != 0).cloned();
            if ex.is_none() {
                let s = st.db.settings.series.get(&inv.kind).cloned().unwrap_or_default();
                inv.number = format_number(&s.pattern, st.db.settings.year, s.next);
            }
            prepare(&mut inv, &st.db.settings, ex.as_ref());
            file("application/pdf", templates::render(&inv, &st.db.settings, tpl), "nahled.pdf", false)
        }
        (Method::Get, ["api", "sample"]) => {
            let inv = sample_invoice(&st.db.settings);
            let mut set = st.db.settings.clone();
            if let Some(a) = query.get("accent") { if a.len() == 6 { set.accent = format!("#{a}"); } }
            file("application/pdf", templates::render(&inv, &set, tpl), "vzor.pdf", false)
        }
        (Method::Post, ["api", "invoices"]) => {
            let mut inv: Invoice = body_json(body)?;
            let set = st.db.settings.clone();
            let series = set.series.get(&inv.kind).cloned().unwrap_or_default();
            inv.id = 0;
            inv.number = format_number(&series.pattern, set.year, series.next);
            inv.year = set.year;
            inv.seq = series.next;
            inv.uuid = uuid4();
            inv.created = today();
            inv.canceled = false;
            if st.db.invoices.iter().any(|i| i.number == inv.number) {
                return bad(format!("Číslo dokladu {} už existuje. Upravte číselnou řadu v Nastavení.", inv.number));
            }
            prepare(&mut inv, &set, None);
            let t = compute(&inv, set.round_cash);
            if !t.errors.is_empty() { return bad(t.errors.join(" ")); }
            inv.id = st.new_id();
            if let Some(s) = st.db.settings.series.get_mut(&inv.kind) { s.next += 1; }
            st.db.invoices.push(inv.clone());
            st.save().map_err(|e| (500, e))?;
            ok_json(inv_json(&inv, &set))
        }
        (Method::Put, ["api", "invoices", id]) => {
            let id: u64 = id.parse().map_err(|_| (400, "Neplatné ID".to_string()))?;
            let mut inv: Invoice = body_json(body)?;
            let set = st.db.settings.clone();
            let Some(ex) = st.db.invoices.iter().find(|i| i.id == id).cloned() else { return Err((404, "Doklad nenalezen.".into())) };
            prepare(&mut inv, &set, Some(&ex));
            let t = compute(&inv, set.round_cash);
            if !t.errors.is_empty() { return bad(t.errors.join(" ")); }
            if let Some(slot) = st.db.invoices.iter_mut().find(|i| i.id == id) { *slot = inv.clone(); }
            st.save().map_err(|e| (500, e))?;
            ok_json(inv_json(&inv, &set))
        }
        (Method::Post, ["api", "invoices", id, action]) => {
            let id: u64 = id.parse().map_err(|_| (400, "Neplatné ID".to_string()))?;
            let v: Value = if body.is_empty() { json!({}) } else { body_json(body)? };
            let set = st.db.settings.clone();
            let Some(inv) = st.db.invoices.iter_mut().find(|i| i.id == id) else { return Err((404, "Doklad nenalezen.".into())) };
            match *action {
                "paid" => {
                    let d = v["date"].as_str().unwrap_or("");
                    if !d.is_empty() && parse_date(d).is_none() { return bad("Neplatné datum úhrady."); }
                    inv.paid_date = d.to_string();
                }
                "storno" => inv.canceled = v["canceled"].as_bool().unwrap_or(true),
                _ => return Err((404, "Neznámá akce.".into())),
            }
            let out = inv_json(inv, &set);
            st.save().map_err(|e| (500, e))?;
            ok_json(out)
        }
        (Method::Delete, ["api", "invoices", id]) => {
            let id: u64 = id.parse().map_err(|_| (400, "Neplatné ID".to_string()))?;
            let Some(inv) = st.db.invoices.iter().find(|i| i.id == id).cloned() else { return Err((404, "Doklad nenalezen.".into())) };
            let year = st.db.settings.year;
            let next = st.db.settings.series.get(&inv.kind).map(|s| s.next).unwrap_or(0);
            if inv.year != year || inv.seq + 1 != next {
                return bad("Smazat lze jen poslední doklad v řadě – číselná řada musí zůstat bez mezer. Starší doklad stornujte.");
            }
            st.db.invoices.retain(|i| i.id != id);
            if let Some(s) = st.db.settings.series.get_mut(&inv.kind) { s.next = inv.seq.max(1); }
            st.save().map_err(|e| (500, e))?;
            ok_json(json!({"ok": true}))
        }
        (Method::Get, ["api", "invoices", id, fmt]) => {
            let id: u64 = id.parse().map_err(|_| (400, "Neplatné ID".to_string()))?;
            let Some(inv) = st.db.invoices.iter().find(|i| i.id == id) else { return Err((404, "Doklad nenalezen.".into())) };
            let base = format!("{}_{}", match inv.kind.as_str() { "zalohova" => "ZF", "paragon" => "PD", "ddpp" => "DD", _ => "FV" }, inv.number);
            match *fmt {
                "pdf" => file("application/pdf", templates::render(inv, &st.db.settings, tpl), &format!("{base}.pdf"), dl),
                "isdoc" => file("application/x-isdoc+xml", isdoc::export(inv, st.db.settings.round_cash).into_bytes(), &format!("{base}.isdoc"), true),
                _ => Err((404, "Neznámý formát.".into())),
            }
        }
        (Method::Get, ["api", "export", "csv"]) => {
            let y = query.get("year").and_then(|y| y.parse().ok());
            file("text/csv; charset=utf-8", csv_export(&st.db, y), &format!("kniha-faktur-{}.csv", y.map(|y: i32| y.to_string()).unwrap_or("vse".into())), true)
        }
        (Method::Get, ["api", "backup"]) => {
            let body = serde_json::to_vec_pretty(&st.db).map_err(|e| (500, e.to_string()))?;
            file("application/json", body, &format!("fakturace-zaloha-{}.json", today()), true)
        }
        (Method::Post, ["api", "restore"]) => {
            let db: Db = body_json(body)?;
            if db.next_id == 0 { return bad("Soubor není záloha aplikace Fakturace."); }
            st.db = db;
            st.save().map_err(|e| (500, e))?;
            ok_json(json!({"ok": true}))
        }
        _ => Err((404, "Nenalezeno".into())),
    }
}

/// Nastavení serveru.
pub struct Config {
    pub ip: std::net::IpAddr,
    /// 0 = libovolný volný port
    pub port: u16,
    pub dir: PathBuf,
    /// Heslo pro HTTP Basic (přístup ze sítě).
    pub password: Option<String>,
    /// Tajný token v cookie `fakturace_token` (Android: chrání před ostatními aplikacemi v zařízení).
    pub token: Option<String>,
}

struct Auth { password: Option<String>, token: Option<String> }

fn authorized(req: &Request, auth: &Auth) -> bool {
    use base64::Engine;
    if let Some(tok) = &auth.token {
        let want = format!("fakturace_token={tok}");
        let ok = req.headers().iter().filter(|h| h.field.equiv("Cookie"))
            .any(|h| h.value.as_str().split(';').any(|c| c.trim() == want));
        if !ok { return false; }
    }
    let Some(pw) = &auth.password else { return true };
    req.headers().iter().find(|h| h.field.equiv("Authorization")).and_then(|h| {
        let v = h.value.as_str();
        let b = base64::engine::general_purpose::STANDARD.decode(v.strip_prefix("Basic ")?.trim()).ok()?;
        let s = String::from_utf8(b).ok()?;
        Some(s.split_once(':')?.1 == pw)
    }).unwrap_or(false)
}

fn handle(mut req: Request, store: &Mutex<Store>, auth: &Auth) {
    let hdr = |k: &str, v: &str| Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap();
    if !authorized(&req, auth) {
        let mut r = Response::from_string("Přístup odepřen.").with_status_code(if auth.password.is_some() { 401 } else { 403 })
            .with_header(hdr("Content-Type", "text/plain; charset=utf-8"));
        if auth.password.is_some() { r = r.with_header(hdr("WWW-Authenticate", "Basic realm=\"Fakturace\", charset=\"UTF-8\"")); }
        let _ = req.respond(r);
        return;
    }
    let url = req.url().to_string();
    let (path, q) = url.split_once('?').unwrap_or((&url, ""));
    let query = parse_query(q);
    let mut body = Vec::new();
    let res = if req.body_length().map(|l| l as u64 > MAX_BODY).unwrap_or(false) {
        Err((413, "Požadavek je příliš velký.".to_string()))
    } else if req.as_reader().take(MAX_BODY + 1).read_to_end(&mut body).is_err() {
        Err((400, "Chyba čtení požadavku.".to_string()))
    } else {
        // Změny přijímáme jen ze stejného původu (ochrana proti CSRF z cizích stránek).
        let cross = req.method() != &Method::Get && req.headers().iter().any(|h| {
            h.field.equiv("Sec-Fetch-Site") && !matches!(h.value.as_str(), "same-origin" | "none")
        });
        if cross { Err((403, "Požadavek z cizí stránky byl odmítnut.".to_string())) }
        else {
            let m = req.method().clone();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| route(&m, path, &query, &body, store)))
                .unwrap_or_else(|_| Err((500, "Vnitřní chyba při zpracování požadavku.".to_string())))
        }
    };
    let resp = res.unwrap_or_else(|(code, msg)| Resp { code, ctype: "application/json; charset=utf-8", body: json!({"error": msg}).to_string().into_bytes(), extra: vec![] });
    let mut r = Response::from_data(resp.body).with_status_code(resp.code)
        .with_header(hdr("Content-Type", resp.ctype))
        .with_header(hdr("X-Content-Type-Options", "nosniff"));
    for (k, v) in &resp.extra { r = r.with_header(hdr(k, v)); }
    let _ = req.respond(r);
}

/// Spustí server na pozadí. Vrací skutečný port a pracovní vlákna.
pub fn start(cfg: Config) -> Result<(u16, Vec<JoinHandle<()>>), String> {
    let store = Arc::new(Mutex::new(Store::open(&cfg.dir)?));
    let server = Server::http(std::net::SocketAddr::new(cfg.ip, cfg.port))
        .map_err(|e| format!("Server se nepodařilo spustit na {}:{}: {e}", cfg.ip, cfg.port))?;
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(cfg.port);
    let server = Arc::new(server);
    let auth = Arc::new(Auth { password: cfg.password, token: cfg.token });
    let workers = (0..4).map(|_| {
        let (server, store, auth) = (server.clone(), store.clone(), auth.clone());
        std::thread::spawn(move || {
            while let Ok(req) = server.recv() { handle(req, &store, &auth); }
        })
    }).collect();
    Ok((port, workers))
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Vstupní bod pro Android: `cz.fakturace.app.NativeServer.start(dataDir, port, token)`.
/// Vrací port, na kterém server běží, nebo 0 při chybě.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_cz_fakturace_app_NativeServer_start<'local>(
    mut env: jni::JNIEnv<'local>,
    _class: jni::objects::JClass<'local>,
    data_dir: jni::objects::JString<'local>,
    port: jni::sys::jint,
    token: jni::objects::JString<'local>,
) -> jni::sys::jint {
    let dir: String = match env.get_string(&data_dir) { Ok(s) => s.into(), Err(_) => return 0 };
    let token: String = match env.get_string(&token) { Ok(s) => s.into(), Err(_) => return 0 };
    let run = move |port: u16| start(Config {
        ip: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), port, dir: PathBuf::from(&dir),
        password: None, token: Some(token.clone()).filter(|t| !t.is_empty()),
    });
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Pevný port drží stejný původ stránky (a tím i uložené předvolby); je-li obsazený, vezme se volný.
        run(port.clamp(0, 65535) as u16).or_else(|_| run(0))
    }));
    match res { Ok(Ok((p, _))) => p as jni::sys::jint, _ => 0 }
}
