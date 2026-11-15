//! Šablony dokladů: klasik (podle původní faktury), moderní, minimal, výrazný.
use crate::model::*;
use crate::pdf::{hex, luma, mix, Canvas, Font, Image, Rgb, PH, PW};
use Font::{Bold, Light, Regular};

pub const TEMPLATES: [(&str, &str, &str); 4] = [
    ("klasik", "Klasik", "Váš současný vzhled: černé pruhy, modrá linka, šedý panel s adresami a pruhovaná tabulka."),
    ("moderni", "Moderní", "Vzdušná šablona s platební kartou v barvě firmy a QR Platbou hned nahoře."),
    ("minimal", "Minimal", "Černobílá typografická šablona bez výplní, číslo dokladu jako hlavní prvek."),
    ("vyrazny", "Výrazný", "Barevné záhlaví přes celou šířku strany a tmavý box s částkou k úhradě."),
];

const INK: Rgb = (0.09, 0.10, 0.12);
const GREY: Rgb = (0.42, 0.44, 0.48);
const PALE: Rgb = (0.962, 0.962, 0.966);
const HAIR: Rgb = (0.86, 0.87, 0.89);
const WHITE: Rgb = (1.0, 1.0, 1.0);
const RED: Rgb = (0.80, 0.16, 0.16);
const ML: f32 = 44.0;
const MR: f32 = 551.0;
const BOTTOM: f32 = 782.0;

struct Ctx<'a> {
    inv: &'a Invoice,
    t: &'a Totals,
    accent: Rgb,
    logo: Option<usize>,
    stamp: Option<usize>,
    title: String,
    subtitle: String,
    /// (popisek, hodnota): vystavení, plnění, splatnost, úhrada
    meta: Vec<(String, String)>,
    /// účet, IBAN, VS …
    pay: Vec<(String, String)>,
    due_label: String,
    due_value: String,
    notes: Vec<String>,
    footer: String,
    qr: Option<String>,
    rows: Vec<(String, String)>,
    final_label: String,
    /// 0 = s DPH, 1 = přenesená daňová povinnost, 2 = neplátce
    mode: u8,
}

fn build<'a>(inv: &'a Invoice, t: &'a Totals, set: &'a Settings, cv: &mut Canvas) -> Ctx<'a> {
    let s = &inv.supplier;
    let payer = s.vat_payer;
    let (title, subtitle) = match (inv.kind.as_str(), payer) {
        ("zalohova", _) => ("Zálohová faktura", "není daňovým dokladem"),
        ("paragon", true) => ("Paragon", "zjednodušený daňový doklad"),
        ("paragon", false) => ("Paragon", "doklad o zaplacení"),
        ("ddpp", _) => ("Daňový doklad", "k přijaté platbě"),
        (_, true) => ("Faktura", "daňový doklad"),
        _ => ("Faktura", ""),
    };
    let mut meta = vec![("Datum vystavení".to_string(), cz_date(&inv.issue_date))];
    if payer && inv.kind != "zalohova" && !inv.tax_date.is_empty() {
        let l = if inv.kind == "ddpp" { "Datum přijetí platby" } else { "Datum uskutečnění plnění" };
        meta.push((l.to_string(), cz_date(&inv.tax_date)));
    }
    let paid_now = matches!(inv.kind.as_str(), "paragon" | "ddpp");
    let (due_label, due_value) = if paid_now {
        ("Uhrazeno".to_string(), cz_date(if inv.paid_date.is_empty() { &inv.issue_date } else { &inv.paid_date }))
    } else {
        ("Datum splatnosti".to_string(), cz_date(&inv.due_date))
    };
    meta.push((due_label.clone(), due_value.clone()));
    meta.push(("Forma úhrady".to_string(), payment_label(&inv.payment).to_string()));

    let mut pay = Vec::new();
    if !paid_now && inv.payment == "prevod" {
        if !s.bank_account.is_empty() { pay.push(("Číslo účtu".to_string(), s.bank_account.clone())); }
        if let Some(i) = company_iban(s) {
            let grouped: Vec<String> = i.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).to_string()).collect();
            pay.push(("IBAN".to_string(), grouped.join(" ")));
        }
        if !s.swift.is_empty() { pay.push(("BIC / SWIFT".to_string(), s.swift.clone())); }
    }
    if !paid_now && !inv.vs.is_empty() { pay.push(("Variabilní symbol".to_string(), inv.vs.clone())); }
    if !inv.order_no.is_empty() { pay.push(("Objednávka".to_string(), inv.order_no.clone())); }

    let mode = if !payer { 2 } else if inv.reverse_charge { 1 } else { 0 };
    let mut rows: Vec<(String, String)> = Vec::new();
    match mode {
        0 => {
            if t.rates.len() <= 1 {
                rows.push(("Základ daně".into(), cz_money(t.base)));
                let r = t.rates.first().map(|r| r.rate).unwrap_or(0.0);
                rows.push((format!("DPH {}", cz_rate(r)), cz_money(t.tax)));
            } else {
                for r in &t.rates {
                    rows.push((format!("Základ daně {}", cz_rate(r.rate)), cz_money(r.base)));
                    rows.push((format!("DPH {}", cz_rate(r.rate)), cz_money(r.tax)));
                }
            }
        }
        1 => rows.push(("Celkem bez DPH".into(), cz_money(t.base))),
        _ => {}
    }
    let extras = t.deposit != 0 || t.rounding != 0;
    if extras && mode != 1 { rows.push((if mode == 0 { "Celkem s DPH" } else { "Celkem" }.into(), cz_money(t.total))); }
    if t.deposit != 0 {
        let l = if inv.deposit_ref.is_empty() { "Uhrazeno zálohou".to_string() } else { format!("Uhrazeno zálohou ({})", inv.deposit_ref) };
        rows.push((l, cz_money(-t.deposit)));
    }
    if t.rounding != 0 { rows.push(("Zaokrouhlení".into(), cz_money(t.rounding))); }
    let final_label = if paid_now && !extras { if mode == 0 { "Celkem s DPH" } else { "Celkem" } }
        else if extras || mode != 0 { "K úhradě" } else { "Celkem s DPH" }.to_string();

    let mut notes = Vec::new();
    if inv.reverse_charge { notes.push("Daň odvede zákazník (režim přenesení daňové povinnosti).".to_string()); }
    if inv.kind == "zalohova" {
        notes.push(if payer { "Toto není daňový doklad. Daňový doklad vystavíme po přijetí platby." } else { "Zálohová faktura – není účetním ani daňovým dokladem." }.to_string());
    }
    if inv.kind == "faktura" && !inv.paid_date.is_empty() {
        notes.push(format!("Uhrazeno dne {}. Neplaťte.", cz_date(&inv.paid_date)));
    }
    if !inv.note.trim().is_empty() { notes.push(inv.note.trim().to_string()); }

    let mut footer = if payer { "Jsem plátce DPH.".to_string() } else { "Nejsem plátce DPH.".to_string() };
    if !s.register.trim().is_empty() { footer.push_str(&format!(" {}", s.register.trim())); }
    if !s.issued_by.trim().is_empty() { footer.push_str(&format!("  Vystavil: {}.", s.issued_by.trim().trim_end_matches('.'))); }
    let contact: Vec<&str> = [s.phone.as_str(), s.email.as_str(), s.web.as_str()].into_iter().filter(|x| !x.trim().is_empty()).collect();
    if !contact.is_empty() { footer.push_str(&format!("\n{}", contact.join("   "))); }
    if !set.footer_note.trim().is_empty() { footer.push_str(&format!("\n{}", set.footer_note.trim())); }

    let logo = Image::from_data_url(&set.logo).map(|i| cv.add_image(i));
    let stamp = Image::from_data_url(&set.stamp).map(|i| cv.add_image(i));
    let qr = if set.show_qr && !paid_now { spayd(inv, t.payable) } else { None };
    Ctx {
        inv, t, accent: hex(&set.accent), logo, stamp, title: title.into(), subtitle: subtitle.into(),
        meta, pay, due_label, due_value, notes, footer, qr, rows, final_label, mode,
    }
}

fn party_lines(name: &str, street: &str, zip: &str, city: &str, country: &str) -> Vec<String> {
    let mut v = vec![name.to_string()];
    if !street.is_empty() { v.push(street.to_string()); }
    let zc = format!("{} {}", zip, city).trim().to_string();
    if !zc.is_empty() { v.push(zc); }
    if !country.is_empty() && !matches!(country.to_lowercase().as_str(), "česká republika" | "cz" | "česko" | "ceska republika") {
        v.push(country.to_string());
    }
    v
}
fn sup_lines(c: &Ctx) -> Vec<String> {
    let s = &c.inv.supplier;
    party_lines(&s.name, &s.street, &s.zip, &s.city, &s.country)
}
fn cust_lines(c: &Ctx) -> Vec<String> {
    let p = &c.inv.customer;
    if p.name.trim().is_empty() { return Vec::new(); }
    party_lines(&p.name, &p.street, &p.zip, &p.city, &p.country)
}

/// Blok adresy: název tučně, řádky, IČO/DIČ. Vrací y za blokem.
#[allow(clippy::too_many_arguments)]
fn party(cv: &mut Canvas, x: f32, y: f32, w: f32, lines: &[String], ico: &str, dic: &str, size: f32, col: Rgb, bold_name: bool) -> f32 {
    let lead = size * 1.38;
    let mut yy = y;
    for (i, l) in lines.iter().enumerate() {
        let f = if i == 0 && bold_name { Bold } else { Regular };
        yy = cv.para(x, yy, l, f, size, col, w, lead);
    }
    let lw = size * 3.2;
    if !ico.is_empty() {
        cv.text(x, yy, "IČO:", Regular, size, mix(col, GREY, 0.6));
        cv.text(x + lw, yy, ico, Regular, size, col);
        yy += lead;
    }
    if !dic.is_empty() {
        cv.text(x, yy, "DIČ:", Regular, size, mix(col, GREY, 0.6));
        cv.text(x + lw, yy, dic, Regular, size, col);
        yy += lead;
    }
    yy
}

// ---------- tabulka položek ----------

#[derive(Clone, Copy, PartialEq)]
enum Al { L, C, R }
struct Col { label: &'static str, w: f32, al: Al, kind: u8 }

fn columns(c: &Ctx, width: f32) -> Vec<Col> {
    let price = if c.inv.prices_incl_vat && c.mode == 0 { "Cena/ks s DPH" } else { "Cena/ks" };
    let mut v = match c.mode {
        0 => vec![
            Col { label: "Položka", w: 0.0, al: Al::L, kind: 0 }, Col { label: "Počet", w: 46.0, al: Al::C, kind: 1 },
            Col { label: price, w: 62.0, al: Al::R, kind: 2 }, Col { label: "Cena", w: 60.0, al: Al::R, kind: 3 },
            Col { label: "% DPH", w: 34.0, al: Al::C, kind: 4 }, Col { label: "DPH", w: 54.0, al: Al::R, kind: 5 },
            Col { label: "Celkem s DPH", w: 74.0, al: Al::R, kind: 6 },
        ],
        1 => vec![
            Col { label: "Položka", w: 0.0, al: Al::L, kind: 0 }, Col { label: "Počet", w: 52.0, al: Al::C, kind: 1 },
            Col { label: price, w: 78.0, al: Al::R, kind: 2 }, Col { label: "% DPH", w: 44.0, al: Al::C, kind: 4 },
            Col { label: "Celkem bez DPH", w: 90.0, al: Al::R, kind: 3 },
        ],
        _ => vec![
            Col { label: "Položka", w: 0.0, al: Al::L, kind: 0 }, Col { label: "Počet", w: 60.0, al: Al::C, kind: 1 },
            Col { label: price, w: 90.0, al: Al::R, kind: 2 }, Col { label: "Celkem", w: 100.0, al: Al::R, kind: 6 },
        ],
    };
    let fixed: f32 = v.iter().map(|c| c.w).sum();
    v[0].w = width - fixed;
    v
}

struct TStyle {
    caps: bool,
    head_fill: Option<Rgb>,
    head_text: Rgb,
    head_h: f32,
    head_size: f32,
    head_rule: Option<(f32, Rgb)>,
    zebra: Option<Rgb>,
    vlines: Option<Rgb>,
    hlines: Option<Rgb>,
    pitch: f32,
    size: f32,
    /// Klasik: pruhy tabulky vyplní stranu až k součtům.
    fill: bool,
    pad: f32,
}

fn cell(c: &Ctx, i: usize, kind: u8) -> String {
    let it = &c.inv.items[i];
    let lt = &c.t.lines[i];
    match kind {
        1 => format!("{} {}", cz_qty(it.qty), it.unit).trim().to_string(),
        2 => cz_money(to_hal(it.price)),
        3 => cz_money(lt.base),
        4 => cz_rate(it.vat),
        5 => cz_money(lt.tax),
        6 => cz_money(lt.total),
        _ => String::new(),
    }
}

/// Vykreslí tabulku se stránkováním; `reserve` = výška potřebná pod tabulkou na poslední straně.
/// Vrací y, kde tabulka na poslední straně končí.
fn table(cv: &mut Canvas, c: &Ctx, x0: f32, x1: f32, top: f32, reserve: f32, st: &TStyle) -> f32 {
    let cols = columns(c, x1 - x0);
    let lead = st.size * 1.22;
    let head = |cv: &mut Canvas, y: f32| -> f32 {
        if let Some(f) = st.head_fill { cv.rect(x0, y, x1 - x0, st.head_h, f); }
        let mut x = x0;
        let by = y + st.head_h / 2.0 + st.head_size * 0.36;
        for col in &cols {
            let l = if st.caps { col.label.to_uppercase() } else { col.label.to_string() };
            match col.al {
                Al::L => cv.text(x + st.pad, by, &l, Bold, st.head_size, st.head_text),
                Al::C => cv.text_c(x + col.w / 2.0, by, &l, Bold, st.head_size, st.head_text),
                Al::R => cv.text_r(x + col.w - st.pad, by, &l, Bold, st.head_size, st.head_text),
            }
            x += col.w;
        }
        if let Some((w, col)) = st.head_rule { cv.line(x0, y + st.head_h, x1, y + st.head_h, w, col); }
        y + st.head_h
    };
    let close = |cv: &mut Canvas, from: f32, used: f32, to: f32| {
        // pruhy a svislé linky od `from` do `to` (obsah končí v `used`)
        let end = if st.fill { to } else { used };
        if st.fill {
            if let Some(z) = st.zebra {
                let mut y = used;
                let mut k = ((used - from) / st.pitch).round() as i32;
                while y + st.pitch <= end + 0.5 { if k % 2 == 1 { cv.rect(x0, y, x1 - x0, st.pitch, z); } y += st.pitch; k += 1; }
            }
        }
        if let Some(vc) = st.vlines {
            let mut x = x0;
            for col in &cols[..cols.len() - 1] { x += col.w; cv.line(x, from, x, end, 0.5, vc); }
        }
        end
    };
    let cont = |cv: &mut Canvas| -> f32 {
        cv.new_page();
        cv.text(x0, 52.0, &format!("{} {} – pokračování", c.title, c.inv.number), Bold, 9.0, INK);
        cv.line(x0, 60.0, x1, 60.0, 0.6, HAIR);
        74.0
    };

    let mut body_top = head(cv, top);
    let mut y = body_top;
    let mut stripe = 0;
    for i in 0..c.inv.items.len() {
        let lines = cv.wrap(&c.inv.items[i].desc, Regular, st.size, cols[0].w - 2.0 * st.pad);
        let n = lines.len().max(1) as f32;
        let mut h = (n * lead + (st.pitch - lead)).max(st.pitch);
        if st.fill { h = (h / st.pitch).ceil() * st.pitch; }
        if y + h > BOTTOM && y > body_top {
            close(cv, body_top, y, y);
            let t = cont(cv);
            body_top = head(cv, t);
            y = body_top;
            stripe = 0;
        }
        if let Some(z) = st.zebra {
            let k = (h / st.pitch).round().max(1.0) as i32;
            for j in 0..k { if (stripe + j) % 2 == 1 { cv.rect(x0, y + j as f32 * st.pitch, x1 - x0, if st.fill { st.pitch } else { h / k as f32 }, z); } }
            stripe += k;
        }
        let by = y + (st.pitch - lead) / 2.0 + st.size * 0.92;
        let mut x = x0;
        for col in &cols {
            if col.kind == 0 {
                for (li, l) in lines.iter().enumerate() { cv.text(x + st.pad, by + li as f32 * lead, l, Regular, st.size, INK); }
            } else {
                let v = cell(c, i, col.kind);
                let (f, sz) = if col.kind == 6 || (col.kind == 3 && c.mode == 1) { (Bold, st.size + 0.6) } else { (Regular, st.size) };
                match col.al {
                    Al::C => cv.text_c(x + col.w / 2.0, by, &v, f, sz, INK),
                    _ => cv.text_r(x + col.w - st.pad, by, &v, f, sz, INK),
                }
            }
            x += col.w;
        }
        y += h;
        if let Some(hc) = st.hlines { cv.line(x0, y, x1, y, 0.5, hc); }
    }
    if y + reserve > BOTTOM {
        close(cv, body_top, y, if st.fill { BOTTOM } else { y });
        cont(cv)
    } else {
        close(cv, body_top, y, BOTTOM - reserve)
    }
}

fn notes_h(cv: &Canvas, c: &Ctx, w: f32, size: f32) -> f32 {
    c.notes.iter().map(|n| cv.wrap(n, Regular, size, w).len() as f32 * size * 1.35 + 3.0).sum()
}
fn notes(cv: &mut Canvas, c: &Ctx, x: f32, y: f32, w: f32, size: f32, col: Rgb) -> f32 {
    let mut yy = y;
    for n in &c.notes { yy = cv.para(x, yy, n, Regular, size, col, w, size * 1.35) + 3.0; }
    yy
}
fn qr_block(cv: &mut Canvas, c: &Ctx, x: f32, y: f32, size: f32, caption: bool) -> bool {
    let Some(q) = &c.qr else { return false };
    cv.rect(x - 3.0, y - 3.0, size + 6.0, size + 6.0, WHITE);
    if !cv.qr(q, x, y, size, INK) { return false; }
    if caption { cv.text_c(x + size / 2.0, y + size + 10.0, "QR Platba", Bold, 6.0, GREY); }
    true
}
fn footer(cv: &mut Canvas, c: &Ctx, page: usize, pages: usize) {
    let lines = cv.wrap(&c.footer, Regular, 6.6, MR - ML - 60.0);
    let mut y = 822.0 - (lines.len().saturating_sub(1)) as f32 * 8.6;
    for l in &lines { cv.text(ML, y, l, Regular, 6.6, GREY); y += 8.6; }
    if pages > 1 { cv.text_r(MR, 822.0, &format!("Strana {} / {}", page + 1, pages), Regular, 6.6, GREY); }
    if c.inv.canceled && page == 0 {
        cv.rrect_stroke(PW / 2.0 - 46.0, 12.0, 92.0, 19.0, 3.0, 1.2, RED);
        cv.text_c(PW / 2.0, 25.5, "STORNOVÁNO", Bold, 9.5, RED);
    }
}
fn logo_or_name(cv: &mut Canvas, c: &Ctx, x: f32, y: f32, w: f32, h: f32, ax: f32, col: Rgb) {
    if let Some(l) = c.logo { cv.image(l, x, y, w, h, ax, 0.5); return; }
    let nw = w.max(250.0);
    let name = cv.fit(&c.inv.supplier.name, Bold, 15.0, nw);
    let by = y + h / 2.0 + 5.5;
    if ax > 0.5 { cv.text_r(x + w, by, &name, Bold, 15.0, col); } else { cv.text(x, by, &name, Bold, 15.0, col); }
}
fn signature(cv: &mut Canvas, c: &Ctx, x: f32, y: f32, w: f32) {
    if let Some(s) = c.stamp { cv.image(s, x, y - 62.0, w, 60.0, 0.5, 1.0); }
    cv.line(x, y, x + w, y, 0.5, GREY);
    cv.text(x, y + 9.0, "Razítko a podpis", Regular, 6.6, GREY);
}

// ---------- KLASIK ----------

fn klasik(cv: &mut Canvas, c: &Ctx) {
    let dark: Rgb = (0.11, 0.11, 0.11);
    let lab: Rgb = (0.36, 0.36, 0.38);
    logo_or_name(cv, c, ML, 42.0, 160.0, 31.0, 0.0, INK);
    cv.text_r(MR, 69.0, &c.title.to_uppercase(), Bold, 21.0, dark);
    if !c.subtitle.is_empty() { cv.text_r(MR, 46.0, &c.subtitle.to_uppercase(), Bold, 5.8, lab); }
    cv.rect(ML, 78.0, MR - ML, 2.2, c.accent);

    // řádek: splatnost | účet | číslo dokladu
    cv.text(ML + 1.0, 94.0, &c.due_label.to_uppercase(), Bold, 5.8, lab);
    cv.rect(ML, 99.5, 125.0, 15.0, dark);
    cv.text(ML + 3.0, 111.3, &c.due_value, Bold, 12.0, WHITE);
    let bank = &c.inv.supplier.bank_account;
    let (ml, mv) = if !bank.is_empty() && c.inv.kind != "paragon" { ("Číslo účtu", bank.clone()) } else { ("Forma úhrady", payment_label(&c.inv.payment).to_string()) };
    cv.text_c(PW / 2.0, 94.0, &ml.to_uppercase(), Bold, 5.8, lab);
    cv.text_c(PW / 2.0, 111.3, &mv, Bold, 12.0, dark);
    let same = c.inv.vs == c.inv.number;
    cv.text_r(MR, 94.0, if same { "VS / ČÍSLO DOKLADU" } else { "ČÍSLO DOKLADU" }, Bold, 5.8, lab);
    cv.text_r(MR, 111.3, &c.inv.number, Bold, 12.0, dark);

    // šedý panel
    let sup = sup_lines(c);
    let cust = cust_lines(c);
    let h_of = |cv: &Canvas, lines: &[String], w: f32, extra: usize| -> f32 {
        lines.iter().map(|l| cv.wrap(l, Regular, 8.2, w).len()).sum::<usize>() as f32 * 11.3 + extra as f32 * 11.3
    };
    let s = &c.inv.supplier;
    let cu = &c.inv.customer;
    let cnt = |a: &str, b: &str| (!a.is_empty()) as usize + (!b.is_empty()) as usize;
    let right_rows = c.meta.len() - 1 + (!same && !c.inv.vs.is_empty()) as usize;
    let ph = (h_of(cv, &sup, 150.0, cnt(&s.ico, &s.dic)))
        .max(h_of(cv, &cust, 165.0, cnt(&cu.ico, &cu.dic)))
        .max(right_rows as f32 * 22.0) + 24.0;
    let py = 128.0;
    cv.rect(ML, py, MR - ML, ph, PALE);
    cv.text(ML + 5.0, py + 12.0, "DODAVATEL", Regular, 5.8, lab);
    party(cv, ML + 5.0, py + 23.0, 150.0, &sup, &s.ico, &s.dic, 8.2, dark, false);
    if !cust.is_empty() {
        cv.text(207.0, py + 12.0, "ODBĚRATEL", Bold, 5.8, lab);
        party(cv, 207.0, py + 23.0, 165.0, &cust, &cu.ico, &cu.dic, 8.2, dark, false);
    }
    let rx = MR - 30.0;
    let mut ry = py + 12.0;
    for (i, (l, v)) in c.meta.iter().enumerate() {
        if i == c.meta.len() - 2 { continue; } // splatnost je nahoře
        if i == c.meta.len() - 1 {
            let w = cv.width(v, Regular, 8.2);
            cv.text_r(rx, ry + 4.0, v, Regular, 8.2, dark);
            cv.text_r(rx - w - 3.0, ry + 4.0, &format!("{l}:"), Regular, 8.2, GREY);
            ry += 15.0;
        } else {
            cv.text_r(rx, ry, &l.to_uppercase(), Regular, 5.8, lab);
            cv.text_r(rx, ry + 11.0, v, Regular, 8.2, dark);
            ry += 22.0;
        }
    }
    if !same && !c.inv.vs.is_empty() {
        let w = cv.width(&c.inv.vs, Regular, 8.2);
        cv.text_r(rx, ry + 4.0, &c.inv.vs, Regular, 8.2, dark);
        cv.text_r(rx - w - 3.0, ry + 4.0, "Variabilní symbol:", Regular, 8.2, GREY);
    }

    // tabulka
    let nrows = c.rows.len() as f32;
    let tot_h = nrows * 14.0 + 24.0;
    let nh = notes_h(cv, c, 268.0, 7.4);
    let below = (tot_h + 14.0 + nh).max(if c.qr.is_some() { 156.0 } else { 78.0 });
    let st = TStyle {
        caps: true, head_fill: Some(dark), head_text: WHITE, head_h: 9.6, head_size: 5.8, head_rule: None,
        zebra: Some(PALE), vlines: Some((0.90, 0.90, 0.91)), hlines: None, pitch: 17.6, size: 7.4, fill: true, pad: 5.0,
    };
    let top = py + ph + 37.0;
    let end = table(cv, c, ML, MR, top, below + 22.0, &st);
    let y0 = end + 20.0;

    // podpis / razítko
    cv.rrect_stroke(ML, y0, 199.0, 49.0, 0.5, 0.5, (0.88, 0.88, 0.89));
    cv.text_c(ML + 99.5, y0 + 9.5, "PODPIS / RAZÍTKO", Regular, 5.8, lab);
    if let Some(sidx) = c.stamp { cv.image(sidx, ML + 8.0, y0 + 11.0, 183.0, 56.0, 0.5, 0.0); }
    if c.qr.is_some() { qr_block(cv, c, ML + 4.0, y0 + 86.0, 56.0, false); cv.text(ML + 68.0, y0 + 117.0, "QR Platba", Bold, 6.5, lab); }

    // součty
    let lx = 283.0;
    let vx = MR - 24.0;
    let mut y = y0 + 8.0;
    for (i, (l, v)) in c.rows.iter().enumerate() {
        cv.text(lx, y, &l.to_uppercase(), Regular, 5.8, lab);
        cv.text_r(vx, y + 0.5, v, if i == 0 { Bold } else { Regular }, 8.6, dark);
        y += 14.0;
    }
    let by = y - 8.0;
    cv.rect(416.0, by, MR - 416.0, 22.0, dark);
    cv.text_r(vx, by + 15.6, &cz_money(c.t.payable), Bold, 12.5, WHITE);
    cv.text(lx, by + 13.0, &c.final_label.to_uppercase(), Bold, 5.8, dark);
    notes(cv, c, lx, by + 40.0, 268.0, 7.4, dark);
}

// ---------- MODERNÍ ----------

fn totals_right(cv: &mut Canvas, c: &Ctx, x: f32, y: f32, size: f32, lead: f32) -> f32 {
    let mut yy = y;
    for (l, v) in &c.rows {
        cv.text(x, yy, l, Regular, size, GREY);
        cv.text_r(MR - 10.0, yy, v, Regular, size + 0.4, INK);
        yy += lead;
    }
    yy
}

fn moderni(cv: &mut Canvas, c: &Ctx) {
    let a = c.accent;
    let adark = mix(a, INK, 0.25);
    logo_or_name(cv, c, ML, 38.0, 190.0, 42.0, 0.0, INK);
    cv.text_r(MR, 62.0, &c.title, Light, 27.0, INK);
    cv.text_r(MR, 80.0, &c.inv.number, Bold, 11.0, adark);
    if !c.subtitle.is_empty() { cv.text_r(MR, 92.5, &c.subtitle, Regular, 7.2, GREY); }

    let y = 124.0;
    let s = &c.inv.supplier;
    let cu = &c.inv.customer;
    cv.text(ML, y, "Dodavatel", Bold, 7.2, adark);
    let e1 = party(cv, ML, y + 15.0, 220.0, &sup_lines(c), &s.ico, &s.dic, 8.6, INK, true);
    let cust = cust_lines(c);
    let mut e2 = y;
    if !cust.is_empty() {
        cv.text(310.0, y, "Odběratel", Bold, 7.2, adark);
        e2 = party(cv, 310.0, y + 15.0, 240.0, &cust, &cu.ico, &cu.dic, 8.6, INK, true);
    }
    let cy = e1.max(e2) + 8.0;

    // platební karta
    let has_qr = c.qr.is_some();
    let ch = 82.0;
    cv.rrect(ML, cy, MR - ML, ch, 7.0, mix(a, WHITE, 0.90));
    let inner = MR - ML - 28.0 - if has_qr { 78.0 } else { 0.0 };
    let n = c.meta.len().max(1) as f32;
    for (i, (l, v)) in c.meta.iter().enumerate() {
        let x = ML + 14.0 + inner * i as f32 / n;
        cv.text(x, cy + 18.0, l, Regular, 6.6, GREY);
        cv.text(x, cy + 30.5, &cv.fit(v, Bold, 9.2, inner / n - 6.0), Bold, 9.2, INK);
    }
    cv.line(ML + 14.0, cy + 41.0, ML + 14.0 + inner, cy + 41.0, 0.5, mix(a, WHITE, 0.65));
    let mut x = ML + 14.0;
    for (l, v) in c.pay.iter().filter(|p| p.0 != "BIC / SWIFT").take(3) {
        let sz = if l == "IBAN" { 7.6 } else { 9.2 };
        cv.text(x, cy + 55.0, l, Regular, 6.6, GREY);
        cv.text(x, cy + 67.5, v, Bold, sz, INK);
        x += cv.width(v, Bold, sz).max(52.0) + 18.0;
    }
    if has_qr { qr_block(cv, c, MR - 14.0 - 60.0, cy + 11.0, 60.0, false); }

    let st = TStyle {
        caps: false, head_fill: None, head_text: GREY, head_h: 20.0, head_size: 6.9, head_rule: Some((0.9, INK)),
        zebra: None, vlines: None, hlines: Some(HAIR), pitch: 21.0, size: 8.2, fill: false, pad: 4.0,
    };
    let tot_h = c.rows.len() as f32 * 14.5 + 44.0;
    let nh = notes_h(cv, c, 250.0, 7.6);
    let reserve = (tot_h).max(nh + 86.0) + 18.0;
    let end = table(cv, c, ML, MR, cy + ch + 18.0, reserve, &st);

    let ty = end + 22.0;
    let lx = 340.0;
    let yy = totals_right(cv, c, lx + 10.0, ty, 8.2, 14.5);
    cv.rrect(lx, yy - 4.0, MR - lx, 28.0, 6.0, a);
    let on = if luma(a) > 0.62 { INK } else { WHITE };
    cv.text(lx + 10.0, yy + 13.5, &c.final_label, Bold, 8.4, on);
    cv.text_r(MR - 10.0, yy + 14.8, &cz_money(c.t.payable), Bold, 13.0, on);
    let ny = notes(cv, c, ML, ty, 250.0, 7.6, INK);
    signature(cv, c, ML, (ny + 62.0).max(yy + 14.0), 150.0);
}

// ---------- MINIMAL ----------

fn minimal(cv: &mut Canvas, c: &Ctx) {
    let head = if c.subtitle.is_empty() { c.title.clone() } else { format!("{} – {}", c.title, c.subtitle) };
    cv.text(ML, 50.0, &head, Regular, 9.0, GREY);
    cv.text(ML - 1.5, 84.0, &c.inv.number, Light, 31.0, INK);
    logo_or_name(cv, c, MR - 150.0, 50.0, 150.0, 32.0, 1.0, INK);
    cv.line(ML, 102.0, MR, 102.0, 0.6, INK);

    let y = 122.0;
    let s = &c.inv.supplier;
    let cu = &c.inv.customer;
    cv.text(ML, y, "Dodavatel", Regular, 6.9, GREY);
    let e1 = party(cv, ML, y + 14.0, 155.0, &sup_lines(c), &s.ico, &s.dic, 8.3, INK, true);
    let cust = cust_lines(c);
    let mut e2 = y;
    if !cust.is_empty() {
        cv.text(215.0, y, "Odběratel", Regular, 6.9, GREY);
        e2 = party(cv, 215.0, y + 14.0, 155.0, &cust, &cu.ico, &cu.dic, 8.3, INK, true);
    }
    let mut ry = y;
    for (l, v) in c.meta.iter().chain(c.pay.iter().filter(|p| p.0 != "BIC / SWIFT")) {
        let sz = if l == "IBAN" { 7.2 } else { 8.3 };
        cv.text(386.0, ry + 0.3, l, Regular, 6.9, GREY);
        cv.text_r(MR, ry + 0.3, v, if l == "Datum splatnosti" || l == "Variabilní symbol" { Bold } else { Regular }, sz, INK);
        ry += 12.6;
    }
    let top = e1.max(e2).max(ry) + 20.0;
    let st = TStyle {
        caps: false, head_fill: None, head_text: GREY, head_h: 19.0, head_size: 6.9, head_rule: Some((0.5, INK)),
        zebra: None, vlines: None, hlines: Some((0.90, 0.90, 0.91)), pitch: 20.0, size: 8.2, fill: false, pad: 0.0,
    };
    cv.line(ML, top, MR, top, 0.5, INK);
    let tot_h = c.rows.len() as f32 * 14.0 + 50.0;
    let qh = if c.qr.is_some() { 84.0 } else { 0.0 };
    let nh = notes_h(cv, c, 240.0, 7.6);
    let reserve = tot_h.max(nh + qh + 20.0) + 80.0;
    let end = table(cv, c, ML, MR, top, reserve, &st);

    let ty = end + 24.0;
    let lx = 352.0;
    let mut yy = ty;
    for (l, v) in &c.rows {
        cv.text(lx, yy, l, Regular, 8.0, GREY);
        cv.text_r(MR, yy, v, Regular, 8.4, INK);
        yy += 14.0;
    }
    cv.line(lx, yy - 3.0, MR, yy - 3.0, 1.0, INK);
    cv.text(lx, yy + 20.0, &c.final_label, Regular, 9.0, INK);
    cv.text_r(MR, yy + 22.0, &cz_money(c.t.payable), Light, 21.0, INK);
    let mut ny = notes(cv, c, ML, ty, 240.0, 7.6, INK);
    if c.qr.is_some() { ny += 8.0; qr_block(cv, c, ML + 3.0, ny, 62.0, false); cv.text(ML + 76.0, ny + 34.0, "QR Platba", Regular, 6.9, GREY); ny += 70.0; }
    signature(cv, c, lx, (yy + 100.0).max(ny).min(BOTTOM - 4.0), MR - lx);
}

// ---------- VÝRAZNÝ ----------

fn vyrazny(cv: &mut Canvas, c: &Ctx) {
    let a = c.accent;
    let on = if luma(a) > 0.62 { INK } else { WHITE };
    let on2 = mix(on, a, 0.28);
    let bh = 152.0;
    cv.rect(0.0, 0.0, PW, bh, a);
    if let Some(l) = c.logo {
        cv.rrect(ML, 30.0, 168.0, 50.0, 6.0, WHITE);
        cv.image(l, ML + 10.0, 37.0, 148.0, 36.0, 0.5, 0.5);
    } else {
        cv.text(ML, 62.0, &cv.fit(&c.inv.supplier.name, Bold, 16.0, 250.0), Bold, 16.0, on);
    }
    cv.text_r(MR, 60.0, &c.title, Bold, 27.0, on);
    let numw = cv.width(&c.inv.number, Bold, 11.5);
    cv.text_r(MR, 79.0, &c.inv.number, Bold, 11.5, on);
    if !c.subtitle.is_empty() { cv.text_r(MR - numw - 8.0, 79.0, &c.subtitle, Regular, 8.0, on2); }
    cv.line(ML, 100.0, MR, 100.0, 0.5, on2);
    let n = c.meta.len().max(1) as f32;
    for (i, (l, v)) in c.meta.iter().enumerate() {
        let x = ML + (MR - ML) * i as f32 / n;
        cv.text(x, 117.0, l, Regular, 6.8, on2);
        cv.text(x, 130.5, v, Bold, 9.6, on);
    }

    let y = bh + 28.0;
    let s = &c.inv.supplier;
    let cu = &c.inv.customer;
    let adark = mix(a, INK, 0.35);
    cv.text(ML, y, "Dodavatel", Bold, 7.2, adark);
    let e1 = party(cv, ML, y + 15.0, 150.0, &sup_lines(c), &s.ico, &s.dic, 8.5, INK, true);
    let cust = cust_lines(c);
    let mut e2 = y;
    if !cust.is_empty() {
        cv.text(206.0, y, "Odběratel", Bold, 7.2, adark);
        e2 = party(cv, 206.0, y + 15.0, 150.0, &cust, &cu.ico, &cu.dic, 8.5, INK, true);
    }
    // tmavý box s částkou
    let bx = 372.0;
    let pays: Vec<&(String, String)> = c.pay.iter().filter(|p| p.0 != "BIC / SWIFT" && p.0 != "Objednávka").collect();
    let boxh = 52.0 + pays.len() as f32 * 12.0 + if pays.is_empty() { 0.0 } else { 6.0 };
    cv.rrect(bx, y - 12.0, MR - bx, boxh, 6.0, INK);
    cv.text(bx + 12.0, y + 5.0, &c.final_label, Regular, 7.2, (0.72, 0.74, 0.78));
    cv.text(bx + 12.0, y + 26.0, &cz_money(c.t.payable), Bold, 17.0, WHITE);
    let mut py = y + 46.0;
    for (l, v) in &pays {
        let sz = if l.as_str() == "IBAN" { 6.6 } else { 7.6 };
        let short = match l.as_str() { "Číslo účtu" => "Účet", "Variabilní symbol" => "VS", o => o };
        cv.text(bx + 12.0, py, short, Regular, 6.6, (0.72, 0.74, 0.78));
        cv.text_r(MR - 12.0, py, v, Bold, sz, WHITE);
        py += 12.0;
    }
    let top = e1.max(e2).max(y - 12.0 + boxh) + 22.0;
    let st = TStyle {
        caps: false, head_fill: Some(mix(a, WHITE, 0.86)), head_text: mix(a, INK, 0.55), head_h: 19.0, head_size: 7.0, head_rule: None,
        zebra: None, vlines: None, hlines: Some(HAIR), pitch: 21.0, size: 8.2, fill: false, pad: 6.0,
    };
    let tot_h = c.rows.len() as f32 * 14.5 + 40.0;
    let qh = if c.qr.is_some() { 80.0 } else { 0.0 };
    let nh = notes_h(cv, c, 250.0, 7.6);
    let reserve = (tot_h + 86.0).max(nh + qh + 10.0) + 20.0;
    let end = table(cv, c, ML, MR, top, reserve, &st);

    let ty = end + 22.0;
    let lx = 340.0;
    let yy = totals_right(cv, c, lx + 10.0, ty, 8.2, 14.5);
    cv.rect(lx, yy - 4.0, MR - lx, 27.0, a);
    cv.text(lx + 10.0, yy + 13.0, &c.final_label, Bold, 8.4, on);
    cv.text_r(MR - 10.0, yy + 14.2, &cz_money(c.t.payable), Bold, 13.0, on);
    signature(cv, c, lx, yy + 104.0, MR - lx);
    let ny = notes(cv, c, ML, ty, 250.0, 7.6, INK);
    if c.qr.is_some() { qr_block(cv, c, ML + 3.0, ny + 8.0, 62.0, false); cv.text(ML + 76.0, ny + 42.0, "QR Platba", Bold, 6.9, adark); }
}

pub fn render(inv: &Invoice, set: &Settings, template: Option<&str>) -> Vec<u8> {
    let t = compute(inv, set.round_cash);
    let mut cv = Canvas::new();
    let c = build(inv, &t, set, &mut cv);
    let tpl = template.filter(|t| !t.is_empty())
        .or(Some(inv.template.as_str()).filter(|t| !t.is_empty()))
        .unwrap_or(set.template.as_str());
    match tpl {
        "moderni" => moderni(&mut cv, &c),
        "minimal" => minimal(&mut cv, &c),
        "vyrazny" => vyrazny(&mut cv, &c),
        _ => klasik(&mut cv, &c),
    }
    let n = cv.page_count();
    for i in 0..n { cv.with_page(i, |cv| footer(cv, &c, i, n)); }
    let _ = PH;
    cv.finish(&format!("{} {}", kind_label(&inv.kind), inv.number), &inv.supplier.name)
}
