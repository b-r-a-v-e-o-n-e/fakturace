//! Export do formátu ISDOC 6.0.2 (http://isdoc.cz/namespace/2013).
use crate::model::*;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"), '<' => o.push_str("&lt;"), '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}
fn tag(out: &mut String, ind: usize, name: &str, val: &str) {
    out.push_str(&format!("{}<{name}>{}</{name}>\n", "  ".repeat(ind), esc(val)));
}
/// "Světova 523/1" -> ("Světova", "523/1")
fn split_street(s: &str) -> (String, String) {
    let s = s.trim();
    if let Some(i) = s.rfind(' ') {
        let tail = &s[i + 1..];
        if tail.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return (s[..i].trim().to_string(), tail.to_string());
        }
    }
    (s.to_string(), String::new())
}
fn country_code(c: &str) -> (&'static str, String) {
    let l = c.trim().to_lowercase();
    match l.as_str() {
        "" | "cz" | "česká republika" | "česko" | "ceska republika" | "czech republic" => ("CZ", "Česká republika".into()),
        "sk" | "slovensko" | "slovenská republika" => ("SK", "Slovenská republika".into()),
        "de" | "německo" | "deutschland" => ("DE", "Německo".into()),
        "at" | "rakousko" => ("AT", "Rakousko".into()),
        "pl" | "polsko" => ("PL", "Polsko".into()),
        "bg" | "bulharsko" => ("BG", "Bulharsko".into()),
        _ if c.trim().len() == 2 => ("", c.trim().to_uppercase()),
        _ => ("", c.trim().to_string()),
    }
}
#[allow(clippy::too_many_arguments)]
fn party(out: &mut String, ind: usize, name: &str, street: &str, city: &str, zip: &str, country: &str, ico: &str, dic: &str, vat: bool, register: &str, contact: Option<(&str, &str, &str)>) {
    let p = "  ".repeat(ind);
    out.push_str(&format!("{p}<Party>\n{p}  <PartyIdentification>\n"));
    tag(out, ind + 2, "ID", ico.trim());
    out.push_str(&format!("{p}  </PartyIdentification>\n{p}  <PartyName>\n"));
    tag(out, ind + 2, "Name", name.trim());
    out.push_str(&format!("{p}  </PartyName>\n{p}  <PostalAddress>\n"));
    let (st, no) = split_street(street);
    tag(out, ind + 2, "StreetName", &st);
    tag(out, ind + 2, "BuildingNumber", &no);
    tag(out, ind + 2, "CityName", city.trim());
    tag(out, ind + 2, "PostalZone", zip.trim());
    let (code, cname) = country_code(country);
    out.push_str(&format!("{p}    <Country>\n"));
    tag(out, ind + 3, "IdentificationCode", if code.is_empty() && cname.len() == 2 { &cname } else { code });
    tag(out, ind + 3, "Name", &cname);
    out.push_str(&format!("{p}    </Country>\n{p}  </PostalAddress>\n"));
    if !dic.trim().is_empty() {
        out.push_str(&format!("{p}  <PartyTaxScheme>\n"));
        tag(out, ind + 2, "CompanyID", &dic.replace(' ', ""));
        tag(out, ind + 2, "TaxScheme", if vat { "VAT" } else { "TIN" });
        out.push_str(&format!("{p}  </PartyTaxScheme>\n"));
    }
    if !register.trim().is_empty() {
        out.push_str(&format!("{p}  <RegisterIdentification>\n"));
        tag(out, ind + 2, "Preformatted", register.trim());
        out.push_str(&format!("{p}  </RegisterIdentification>\n"));
    }
    if let Some((n, tel, mail)) = contact {
        if !(n.is_empty() && tel.is_empty() && mail.is_empty()) {
            out.push_str(&format!("{p}  <Contact>\n"));
            if !n.is_empty() { tag(out, ind + 2, "Name", n); }
            if !tel.is_empty() { tag(out, ind + 2, "Telephone", tel); }
            if !mail.is_empty() { tag(out, ind + 2, "ElectronicMail", mail); }
            out.push_str(&format!("{p}  </Contact>\n"));
        }
    }
    out.push_str(&format!("{p}</Party>\n"));
}

pub fn export(inv: &Invoice, round_cash: bool) -> String {
    let t = compute(inv, round_cash);
    let s = &inv.supplier;
    let payer = s.vat_payer;
    let vat_doc = payer && inv.kind != "zalohova";
    let doc_type = match inv.kind.as_str() {
        "zalohova" => 4,
        "ddpp" => 5,
        "paragon" if payer => 7,
        _ => 1,
    };
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Invoice xmlns=\"http://isdoc.cz/namespace/2013\" version=\"6.0.2\">\n");
    tag(&mut o, 1, "DocumentType", &doc_type.to_string());
    tag(&mut o, 1, "ID", &inv.number);
    tag(&mut o, 1, "UUID", &inv.uuid);
    tag(&mut o, 1, "IssuingSystem", concat!("Fakturace ", env!("CARGO_PKG_VERSION")));
    tag(&mut o, 1, "IssueDate", &inv.issue_date);
    if vat_doc && parse_date(&inv.tax_date).is_some() { tag(&mut o, 1, "TaxPointDate", &inv.tax_date); }
    tag(&mut o, 1, "VATApplicable", if payer { "true" } else { "false" });
    o.push_str("  <ElectronicPossibilityAgreementReference/>\n");
    let mut note = inv.note.trim().to_string();
    if inv.reverse_charge { note = format!("Daň odvede zákazník. {note}").trim().to_string(); }
    if !note.is_empty() { tag(&mut o, 1, "Note", &note); }
    tag(&mut o, 1, "LocalCurrencyCode", "CZK");
    tag(&mut o, 1, "CurrRate", "1");
    tag(&mut o, 1, "RefCurrRate", "1");

    o.push_str("  <AccountingSupplierParty>\n");
    party(&mut o, 2, &s.name, &s.street, &s.city, &s.zip, &s.country, &s.ico, &s.dic, payer, &s.register,
          Some((s.issued_by.trim(), s.phone.trim(), s.email.trim())));
    o.push_str("  </AccountingSupplierParty>\n");
    let c = &inv.customer;
    if c.name.trim().is_empty() {
        o.push_str("  <AnonymousCustomerParty>\n");
        tag(&mut o, 2, "ID", "anonym");
        tag(&mut o, 2, "IDScheme", "Fakturace");
        o.push_str("  </AnonymousCustomerParty>\n");
    } else {
        o.push_str("  <AccountingCustomerParty>\n");
        party(&mut o, 2, &c.name, &c.street, &c.city, &c.zip, &c.country, &c.ico, &c.dic, true, "", None);
        o.push_str("  </AccountingCustomerParty>\n");
    }
    if !inv.order_no.trim().is_empty() {
        o.push_str("  <OrderReferences>\n    <OrderReference id=\"obj1\">\n");
        tag(&mut o, 3, "SalesOrderID", "");
        tag(&mut o, 3, "ExternalOrderID", inv.order_no.trim());
        o.push_str("    </OrderReference>\n  </OrderReferences>\n");
    }

    o.push_str("  <InvoiceLines>\n");
    for (i, it) in inv.items.iter().enumerate() {
        let lt = &t.lines[i];
        let q = if it.qty == 0.0 { 1.0 } else { it.qty };
        let rate = if payer { it.vat } else { 0.0 };
        o.push_str("    <InvoiceLine>\n");
        tag(&mut o, 3, "ID", &(i + 1).to_string());
        o.push_str(&format!("      <InvoicedQuantity unitCode=\"{}\">{}</InvoicedQuantity>\n", esc(&it.unit), it.qty));
        tag(&mut o, 3, "LineExtensionAmount", &dec(lt.base));
        tag(&mut o, 3, "LineExtensionAmountTaxInclusive", &dec(lt.total));
        tag(&mut o, 3, "LineExtensionTaxAmount", &dec(lt.tax));
        tag(&mut o, 3, "UnitPrice", &format!("{:.4}", lt.base as f64 / 100.0 / q));
        tag(&mut o, 3, "UnitPriceTaxInclusive", &format!("{:.4}", lt.total as f64 / 100.0 / q));
        o.push_str("      <ClassifiedTaxCategory>\n");
        tag(&mut o, 4, "Percent", &format!("{rate}"));
        tag(&mut o, 4, "VATCalculationMethod", if inv.prices_incl_vat { "1" } else { "0" });
        tag(&mut o, 4, "VATApplicable", if payer { "true" } else { "false" });
        o.push_str("      </ClassifiedTaxCategory>\n      <Item>\n");
        tag(&mut o, 4, "Description", it.desc.trim());
        o.push_str("      </Item>\n    </InvoiceLine>\n");
    }
    o.push_str("  </InvoiceLines>\n");

    if t.deposit != 0 {
        o.push_str("  <NonTaxedDeposits>\n    <NonTaxedDeposit>\n");
        let id = if inv.deposit_ref.trim().is_empty() { "záloha" } else { inv.deposit_ref.trim() };
        tag(&mut o, 3, "ID", id);
        tag(&mut o, 3, "VariableSymbol", &id.chars().filter(|c| c.is_ascii_digit()).collect::<String>());
        tag(&mut o, 3, "DepositAmount", &dec(t.deposit));
        o.push_str("    </NonTaxedDeposit>\n  </NonTaxedDeposits>\n");
    }

    o.push_str("  <TaxTotal>\n");
    for r in &t.rates {
        o.push_str("    <TaxSubTotal>\n");
        tag(&mut o, 3, "TaxableAmount", &dec(r.base));
        tag(&mut o, 3, "TaxAmount", &dec(r.tax));
        tag(&mut o, 3, "TaxInclusiveAmount", &dec(r.total));
        tag(&mut o, 3, "AlreadyClaimedTaxableAmount", "0");
        tag(&mut o, 3, "AlreadyClaimedTaxAmount", "0");
        tag(&mut o, 3, "AlreadyClaimedTaxInclusiveAmount", "0");
        tag(&mut o, 3, "DifferenceTaxableAmount", &dec(r.base));
        tag(&mut o, 3, "DifferenceTaxAmount", &dec(r.tax));
        tag(&mut o, 3, "DifferenceTaxInclusiveAmount", &dec(r.total));
        o.push_str("      <TaxCategory>\n");
        tag(&mut o, 4, "Percent", &format!("{}", r.rate));
        tag(&mut o, 4, "VATApplicable", if payer { "true" } else { "false" });
        if inv.reverse_charge { tag(&mut o, 4, "LocalReverseChargeFlag", "true"); }
        o.push_str("      </TaxCategory>\n    </TaxSubTotal>\n");
    }
    tag(&mut o, 2, "TaxAmount", &dec(t.tax));
    o.push_str("  </TaxTotal>\n  <LegalMonetaryTotal>\n");
    tag(&mut o, 2, "TaxExclusiveAmount", &dec(t.base));
    tag(&mut o, 2, "TaxInclusiveAmount", &dec(t.total));
    tag(&mut o, 2, "AlreadyClaimedTaxExclusiveAmount", "0");
    tag(&mut o, 2, "AlreadyClaimedTaxInclusiveAmount", "0");
    tag(&mut o, 2, "DifferenceTaxExclusiveAmount", &dec(t.base));
    tag(&mut o, 2, "DifferenceTaxInclusiveAmount", &dec(t.total));
    tag(&mut o, 2, "PayableRoundingAmount", &dec(t.rounding));
    tag(&mut o, 2, "PaidDepositsAmount", &dec(t.deposit));
    tag(&mut o, 2, "PayableAmount", &dec(t.payable));
    o.push_str("  </LegalMonetaryTotal>\n");

    let code = match inv.payment.as_str() { "hotove" => 10, "karta" => 48, "dobirka" => 50, "zapocet" => 97, _ => 42 };
    o.push_str("  <PaymentMeans>\n    <Payment>\n");
    tag(&mut o, 3, "PaidAmount", &dec(t.payable));
    tag(&mut o, 3, "PaymentMeansCode", &code.to_string());
    if code == 10 {
        o.push_str("      <Details>\n");
        tag(&mut o, 4, "DocumentID", &inv.number);
        tag(&mut o, 4, "IssueDate", if parse_date(&inv.paid_date).is_some() { &inv.paid_date } else { &inv.issue_date });
        o.push_str("      </Details>\n");
    } else if code == 42 {
        let acc: String = s.bank_account.chars().filter(|c| !c.is_whitespace()).collect();
        let (num, bank) = acc.split_once('/').unwrap_or((acc.as_str(), ""));
        let due = if parse_date(&inv.due_date).is_some() { &inv.due_date } else { &inv.issue_date };
        o.push_str("      <Details>\n");
        tag(&mut o, 4, "PaymentDueDate", due);
        tag(&mut o, 4, "ID", num);
        tag(&mut o, 4, "BankCode", bank);
        tag(&mut o, 4, "Name", s.bank_name.trim());
        tag(&mut o, 4, "IBAN", &company_iban(s).unwrap_or_default());
        tag(&mut o, 4, "BIC", s.swift.trim());
        let vs: String = inv.vs.chars().filter(|c| c.is_ascii_digit()).collect();
        if !vs.is_empty() { tag(&mut o, 4, "VariableSymbol", &vs); }
        o.push_str("      </Details>\n");
    }
    o.push_str("    </Payment>\n  </PaymentMeans>\n</Invoice>\n");
    o
}
