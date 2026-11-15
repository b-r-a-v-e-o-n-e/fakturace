//! Datový model, výpočty DPH, číselné řady a ukládání.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const KINDS: [&str; 4] = ["faktura", "zalohova", "paragon", "ddpp"];
/// § 30 odst. 1 ZDPH – zjednodušený daňový doklad jen do 10 000 Kč vč. daně.
pub const SIMPLIFIED_LIMIT: i64 = 10_000_00;
/// Zákon č. 254/2004 Sb. – limit plateb v hotovosti 270 000 Kč.
pub const CASH_LIMIT: i64 = 270_000_00;

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Party {
    pub name: String,
    pub street: String,
    pub city: String,
    pub zip: String,
    pub country: String,
    pub ico: String,
    pub dic: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Company {
    pub name: String,
    pub street: String,
    pub city: String,
    pub zip: String,
    pub country: String,
    pub ico: String,
    pub dic: String,
    pub vat_payer: bool,
    /// Údaj o zápisu v obchodním / živnostenském rejstříku (§ 435 OZ).
    pub register: String,
    pub phone: String,
    pub email: String,
    pub web: String,
    pub bank_account: String,
    pub bank_name: String,
    pub iban: String,
    pub swift: String,
    pub issued_by: String,
}
impl Default for Company {
    fn default() -> Self {
        Company {
            name: String::new(), street: String::new(), city: String::new(), zip: String::new(),
            country: "Česká republika".into(), ico: String::new(), dic: String::new(),
            vat_payer: true, register: String::new(), phone: String::new(), email: String::new(),
            web: String::new(), bank_account: String::new(), bank_name: String::new(),
            iban: String::new(), swift: String::new(), issued_by: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Series {
    pub pattern: String,
    pub next: u32,
}
impl Default for Series {
    fn default() -> Self { Series { pattern: "{RRRR}{NNNNNN}".into(), next: 1 } }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub company: Company,
    /// Aktivní účetní rok číselných řad.
    pub year: i32,
    pub series: BTreeMap<String, Series>,
    pub due_days: u32,
    pub default_vat: f64,
    pub default_payment: String,
    pub template: String,
    pub accent: String,
    pub round_cash: bool,
    pub show_qr: bool,
    pub footer_note: String,
    /// data: URL (PNG/JPEG)
    pub logo: String,
    pub stamp: String,
}
impl Default for Settings {
    fn default() -> Self {
        let mut series = BTreeMap::new();
        for (k, p) in [("faktura", "{RRRR}{NNNNNN}"), ("zalohova", "ZF{RRRR}{NNNN}"),
                       ("paragon", "PD{RRRR}{NNNN}"), ("ddpp", "DD{RRRR}{NNNN}")] {
            series.insert(k.to_string(), Series { pattern: p.into(), next: 1 });
        }
        Settings {
            company: Company::default(), year: 0, series, due_days: 14, default_vat: 21.0,
            default_payment: "prevod".into(), template: "klasik".into(), accent: "#3d6df2".into(),
            round_cash: true, show_qr: true, footer_note: String::new(),
            logo: String::new(), stamp: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Contact {
    pub id: u64,
    pub name: String,
    pub street: String,
    pub city: String,
    pub zip: String,
    pub country: String,
    pub ico: String,
    pub dic: String,
    pub email: String,
    pub phone: String,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Item {
    pub desc: String,
    pub qty: f64,
    pub unit: String,
    pub price: f64,
    pub vat: f64,
}
impl Default for Item {
    fn default() -> Self { Item { desc: String::new(), qty: 1.0, unit: "ks".into(), price: 0.0, vat: 21.0 } }
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Invoice {
    pub id: u64,
    pub kind: String,
    pub number: String,
    pub vs: String,
    pub year: i32,
    pub seq: u32,
    pub uuid: String,
    pub issue_date: String,
    pub tax_date: String,
    pub due_date: String,
    pub payment: String,
    pub supplier: Company,
    pub customer: Party,
    pub items: Vec<Item>,
    pub prices_incl_vat: bool,
    pub reverse_charge: bool,
    /// Odečtená (již uhrazená) záloha v Kč.
    pub deposit: f64,
    pub deposit_ref: String,
    /// Zaokrouhlení celkové částky na celé koruny: None = podle nastavení (jen hotovost).
    pub round_total: Option<bool>,
    pub order_no: String,
    pub note: String,
    pub template: String,
    pub paid_date: String,
    pub canceled: bool,
    pub created: String,
    /// Pouze při ukládání: převzít aktuální údaje dodavatele z nastavení.
    #[serde(skip_serializing)]
    pub refresh_supplier: bool,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Db {
    pub settings: Settings,
    pub contacts: Vec<Contact>,
    pub invoices: Vec<Invoice>,
    pub next_id: u64,
}

// ---------- výpočty ----------

#[derive(Serialize, Clone, Default, Debug)]
pub struct LineTotal { pub base: i64, pub tax: i64, pub total: i64 }
#[derive(Serialize, Clone, Default, Debug)]
pub struct RateTotal { pub rate: f64, pub base: i64, pub tax: i64, pub total: i64 }
#[derive(Serialize, Clone, Default, Debug)]
pub struct Totals {
    /// Doklad pracuje s DPH (dodavatel je plátce).
    pub vat: bool,
    pub lines: Vec<LineTotal>,
    pub rates: Vec<RateTotal>,
    pub base: i64,
    pub tax: i64,
    pub total: i64,
    pub deposit: i64,
    pub rounding: i64,
    pub payable: i64,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

fn rnd(x: f64) -> i64 { (x + if x < 0.0 { -0.5 } else { 0.5 }) as i64 }
pub fn to_hal(x: f64) -> i64 { rnd(x * 100.0 + if x < 0.0 { -1e-7 } else { 1e-7 }) }

pub fn compute(inv: &Invoice, round_cash: bool) -> Totals {
    let payer = inv.supplier.vat_payer;
    let mut t = Totals { vat: payer, ..Default::default() };
    // součty po sazbách: (sazba v setinách %, součet řádků)
    let mut groups: BTreeMap<i64, i64> = BTreeMap::new();
    for it in &inv.items {
        let amount = to_hal(it.qty * it.price);
        let rate = if payer { it.vat } else { 0.0 };
        let eff = if inv.reverse_charge { 0.0 } else { rate };
        let lt = if inv.prices_incl_vat && eff > 0.0 {
            let tax = rnd(amount as f64 * eff / (100.0 + eff));
            LineTotal { base: amount - tax, tax, total: amount }
        } else {
            let tax = rnd(amount as f64 * eff / 100.0);
            LineTotal { base: amount, tax, total: amount + tax }
        };
        t.lines.push(lt);
        *groups.entry(rnd(rate * 100.0)).or_insert(0) += amount;
    }
    // Daň se počítá ze součtu základů v dané sazbě a zaokrouhluje na haléře.
    for (r, sum) in groups.iter().rev() {
        let rate = *r as f64 / 100.0;
        let eff = if inv.reverse_charge { 0.0 } else { rate };
        let (base, tax) = if inv.prices_incl_vat && eff > 0.0 {
            let tax = rnd(*sum as f64 * eff / (100.0 + eff));
            (*sum - tax, tax)
        } else {
            (*sum, rnd(*sum as f64 * eff / 100.0))
        };
        t.rates.push(RateTotal { rate, base, tax, total: base + tax });
        t.base += base;
        t.tax += tax;
    }
    t.total = t.base + t.tax;
    t.deposit = to_hal(inv.deposit);
    let due = t.total - t.deposit;
    // Zaokrouhlení na celé koruny (výchozí u hotovosti) (rozdíl nevstupuje do základu daně, § 36 odst. 5 ZDPH).
    let do_round = inv.round_total.unwrap_or(round_cash && inv.payment == "hotove");
    t.payable = if do_round { rnd(due as f64 / 100.0) * 100 } else { due };
    t.rounding = t.payable - due;
    validate(inv, &mut t);
    t
}

fn validate(inv: &Invoice, t: &mut Totals) {
    let s = &inv.supplier;
    let payer = s.vat_payer;
    if !KINDS.contains(&inv.kind.as_str()) { t.errors.push("Neznámý typ dokladu.".into()); }
    if parse_date(&inv.issue_date).is_none() { t.errors.push("Vyplňte datum vystavení.".into()); }
    if inv.items.is_empty() || inv.items.iter().all(|i| i.desc.trim().is_empty()) {
        t.errors.push("Doklad musí mít alespoň jednu položku s popisem.".into());
    }
    if inv.kind != "paragon" && inv.customer.name.trim().is_empty() {
        t.errors.push("Vyplňte odběratele.".into());
    }
    if inv.kind == "paragon" && payer && t.total > SIMPLIFIED_LIMIT {
        t.errors.push("Zjednodušený daňový doklad (paragon) lze vystavit jen do 10 000 Kč včetně DPH (§ 30 ZDPH). Vystavte běžnou fakturu.".into());
    }
    if inv.kind == "paragon" && inv.reverse_charge {
        t.errors.push("Paragon nelze použít v režimu přenesení daňové povinnosti (§ 30 odst. 2 ZDPH).".into());
    }
    if inv.reverse_charge && inv.customer.dic.trim().is_empty() {
        t.errors.push("Přenesená daňová povinnost vyžaduje DIČ odběratele (plátce DPH).".into());
    }
    if s.name.trim().is_empty() || s.ico.trim().is_empty() {
        t.warnings.push("V Nastavení chybí název nebo IČO dodavatele – doklad nebude mít zákonné náležitosti.".into());
    }
    if s.street.trim().is_empty() || s.city.trim().is_empty() {
        t.warnings.push("V Nastavení chybí sídlo dodavatele.".into());
    }
    if payer && s.dic.trim().is_empty() {
        t.warnings.push("Plátce DPH musí na dokladu uvést své DIČ (§ 29 ZDPH).".into());
    }
    if s.register.trim().is_empty() {
        t.warnings.push("Doplňte v Nastavení údaj o zápisu v obchodním či živnostenském rejstříku (§ 435 občanského zákoníku).".into());
    }
    if payer && matches!(inv.kind.as_str(), "faktura" | "ddpp") {
        if !inv.customer.ico.trim().is_empty() && inv.customer.dic.trim().is_empty() {
            t.warnings.push("Odběratel má IČO, ale chybí DIČ. Je-li plátcem DPH, DIČ je povinná náležitost daňového dokladu.".into());
        }
        if let (Some(i), Some(d)) = (parse_date(&inv.issue_date), parse_date(&inv.tax_date)) {
            if i - d > 15 {
                t.warnings.push("Daňový doklad má být vystaven do 15 dnů od uskutečnění plnění nebo přijetí platby (§ 28 ZDPH).".into());
            }
        }
    }
    if inv.kind != "paragon" && inv.payment == "prevod" && s.bank_account.trim().is_empty() && s.iban.trim().is_empty() {
        t.warnings.push("Platba převodem, ale v Nastavení není číslo účtu.".into());
    }
    if inv.payment == "hotove" && t.payable > CASH_LIMIT {
        t.warnings.push("Platba v hotovosti přesahuje zákonný limit 270 000 Kč (zákon č. 254/2004 Sb.).".into());
    }
    if t.deposit > t.total && t.total > 0 {
        t.warnings.push("Odečtená záloha je vyšší než celková částka dokladu.".into());
    }
    if let (Some(i), Some(d)) = (parse_date(&inv.issue_date), parse_date(&inv.due_date)) {
        if d < i { t.warnings.push("Datum splatnosti je dříve než datum vystavení.".into()); }
    }
}

// ---------- data, formátování ----------

/// Počet dnů od 1970-01-01 pro datum ve tvaru RRRR-MM-DD.
pub fn parse_date(s: &str) -> Option<i64> {
    let p: Vec<&str> = s.trim().split('-').collect();
    if p.len() != 3 { return None; }
    let (y, m, d): (i64, i64, i64) = (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1900..=2200).contains(&y) { return None; }
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}
pub fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil((secs / 86400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}
/// 2026-11-14 -> 14.11.2026
pub fn cz_date(s: &str) -> String {
    match parse_date(s) {
        Some(z) => { let (y, m, d) = civil(z); format!("{d}.{m}.{y}") }
        None => String::new(),
    }
}
/// Haléře -> "9 210,00"
pub fn cz_num(h: i64) -> String {
    let neg = h < 0;
    let a = h.abs();
    let whole = (a / 100).to_string();
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 { out.push('\u{a0}'); }
        out.push(c);
    }
    format!("{}{},{:02}", if neg { "-" } else { "" }, out, a % 100)
}
pub fn cz_money(h: i64) -> String { format!("{}\u{a0}Kč", cz_num(h)) }
pub fn cz_qty(q: f64) -> String {
    let s = format!("{:.3}", q);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    s.replace('.', ",")
}
pub fn cz_rate(r: f64) -> String { format!("{}\u{a0}%", cz_qty(r)) }
pub fn dec(h: i64) -> String {
    format!("{}{}.{:02}", if h < 0 { "-" } else { "" }, h.abs() / 100, h.abs() % 100)
}

pub fn payment_label(p: &str) -> &'static str {
    match p { "hotove" => "Hotově", "karta" => "Platební kartou", "dobirka" => "Dobírkou", "zapocet" => "Zápočtem", _ => "Bankovním převodem" }
}
pub fn kind_label(k: &str) -> &'static str {
    match k { "zalohova" => "Zálohová faktura", "paragon" => "Paragon", "ddpp" => "Daňový doklad k přijaté platbě", _ => "Faktura" }
}

/// Český účet "předčíslí-číslo/kód" -> IBAN.
pub fn cz_iban(account: &str) -> Option<String> {
    let a: String = account.chars().filter(|c| !c.is_whitespace()).collect();
    let (num, bank) = a.split_once('/')?;
    let (prefix, number) = match num.split_once('-') { Some((p, n)) => (p, n), None => ("", num) };
    if bank.len() != 4 || !bank.chars().all(|c| c.is_ascii_digit()) { return None; }
    if prefix.len() > 6 || number.is_empty() || number.len() > 10 { return None; }
    if !prefix.chars().chain(number.chars()).all(|c| c.is_ascii_digit()) { return None; }
    let bban = format!("{bank}{prefix:0>6}{number:0>10}");
    let check_src = format!("{bban}123500"); // "CZ00" -> 12 35 00
    let mut rem: u32 = 0;
    for c in check_src.chars() { rem = (rem * 10 + c.to_digit(10)?) % 97; }
    Some(format!("CZ{:02}{bban}", 98 - rem))
}
pub fn company_iban(c: &Company) -> Option<String> {
    let i: String = c.iban.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    if i.len() >= 15 { Some(i) } else { cz_iban(&c.bank_account) }
}

/// Řetězec platby pro QR Platbu (Short Payment Descriptor, standard ČBA).
pub fn spayd(inv: &Invoice, payable: i64) -> Option<String> {
    if inv.payment != "prevod" || payable <= 0 || !inv.paid_date.is_empty() { return None; }
    let iban = company_iban(&inv.supplier)?;
    let mut s = format!("SPD*1.0*ACC:{iban}*AM:{}*CC:CZK", dec(payable));
    let vs: String = inv.vs.chars().filter(|c| c.is_ascii_digit()).take(10).collect();
    if !vs.is_empty() { s.push_str(&format!("*X-VS:{vs}")); }
    if let Some(z) = parse_date(&inv.due_date) {
        let (y, m, d) = civil(z);
        s.push_str(&format!("*DT:{y:04}{m:02}{d:02}"));
    }
    let msg: String = format!("{} {}", kind_label(&inv.kind), inv.number).to_uppercase()
        .chars().map(strip_diacritics).filter(|c| c.is_ascii_alphanumeric() || *c == ' ').take(60).collect();
    s.push_str(&format!("*MSG:{msg}"));
    Some(s)
}
fn strip_diacritics(c: char) -> char {
    const FROM: &str = "ÁČĎÉĚÍŇÓŘŠŤÚŮÝŽ";
    const TO: &str = "ACDEEINORSTUUYZ";
    match FROM.chars().position(|x| x == c) { Some(i) => TO.chars().nth(i).unwrap(), None => c }
}

// ---------- číselné řady ----------

pub fn format_number(pattern: &str, year: i32, seq: u32) -> String {
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(a) = rest.find('{') {
        out.push_str(&rest[..a]);
        let Some(b) = rest[a..].find('}') else { rest = &rest[a..]; break };
        let tok = &rest[a + 1..a + b];
        if tok == "RRRR" { out.push_str(&format!("{year:04}")); }
        else if tok == "RR" { out.push_str(&format!("{:02}", year.rem_euclid(100))); }
        else if !tok.is_empty() && tok.chars().all(|c| c == 'N') { out.push_str(&format!("{:0w$}", seq, w = tok.len())); }
        else { out.push_str(&rest[a..a + b + 1]); }
        rest = &rest[a + b + 1..];
    }
    out.push_str(rest);
    out
}
pub fn pattern_ok(p: &str) -> bool {
    p.contains("{N") && format_number(p, 2026, 1) != format_number(p, 2026, 2)
}

pub fn uuid4() -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
        b.copy_from_slice(&(n.wrapping_mul(0x9E3779B97F4A7C15F39CC0605CEDC835)).to_le_bytes());
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

// ---------- úložiště ----------

pub struct Store { pub db: Db, path: PathBuf }

impl Store {
    pub fn open(dir: &Path) -> Result<Store, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("Nelze vytvořit adresář {}: {e}", dir.display()))?;
        let path = dir.join("data.json");
        let mut db: Db = if path.exists() {
            let txt = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            serde_json::from_str(&txt).map_err(|e| format!("Poškozený soubor {}: {e}", path.display()))?
        } else { Db::default() };
        if db.settings.year == 0 { db.settings.year = today()[..4].parse().unwrap_or(2026); }
        if db.next_id == 0 { db.next_id = 1; }
        for k in KINDS { db.settings.series.entry(k.to_string()).or_default(); }
        Ok(Store { db, path })
    }
    pub fn save(&self) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        let txt = serde_json::to_string_pretty(&self.db).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, txt).map_err(|e| format!("Zápis dat selhal: {e}"))?;
        if self.path.exists() { let _ = std::fs::copy(&self.path, self.path.with_extension("json.bak")); }
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("Zápis dat selhal: {e}"))
    }
    pub fn new_id(&mut self) -> u64 { let i = self.db.next_id; self.db.next_id += 1; i }
}
