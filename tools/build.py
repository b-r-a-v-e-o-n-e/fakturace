import re, sys, json, glob, os, subprocess, base64, uuid, collections
SRC, PARSED, OUT, LOGO, STAMP = sys.argv[1:6]
P = {r["number"]: r for r in json.load(open(PARSED, encoding="utf-8"))}
def num(s): return float(s.replace(",", "").replace("Kč", "").strip())
QTY = re.compile(r"^(-?\d[\d.,]*) (\S+)$")
def items_bbox(pdf):
    html = subprocess.run(["pdftotext", "-bbox-layout", pdf, "-"], capture_output=True, text=True).stdout
    lines = []
    for m in re.finditer(r'<line xMin="([\d.]+)" yMin="([\d.]+)" xMax="([\d.]+)" yMax="([\d.]+)">(.*?)</line>', html, re.S):
        w = " ".join(re.findall(r">([^<]+)</word>", m.group(5)))
        w = w.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", '"').replace("&apos;", "'")
        lines.append((float(m.group(2)), float(m.group(1)), w))
    y_head = next(y for y, x, w in lines if w.startswith("POLOŽKA"))
    y_end = min(y for y, x, w in lines if "PODPIS" in w or "SOUČET" in w)
    body = [(y, x, w) for y, x, w in lines if y_head + 5 < y < y_end - 2]
    x_qty = next(x for y, x, w in lines if w == "POČET" and abs(y - y_head) < 3)
    desc = sorted((y, w) for y, x, w in body if x < x_qty - 60)
    rows = sorted(y for y, x, w in body if abs(x - x_qty) < 25 and QTY.match(w))
    clusters = []
    for y, w in desc:
        if clusters and y - clusters[-1][-1][0] <= 10.5: clusters[-1].append((y, w))
        else: clusters.append([(y, w)])
    def join(ls):
        out = ""
        for w in ls:
            out = w if not out else out + ("\n" if w.startswith("(") else " ") + w
        return out
    row_desc, notes, flags = {}, [], []
    for c in clusters:
        hit = [i for i, ry in enumerate(rows) if c[0][0] - 5 <= ry <= c[-1][0] + 5]
        if len(hit) == 1 and hit[0] not in row_desc: row_desc[hit[0]] = join([w for _, w in c])
        elif not hit: notes.append(join([w for _, w in c]))
        else: flags.append("popis nelze jednoznacne priradit: " + " / ".join(w for _, w in c))
    return rows, row_desc, notes, flags

def hal(x): return int(round(x * 100 + (1e-7 if x >= 0 else -1e-7)))
def rnd(x): return int(x + (0.5 if x >= 0 else -0.5))
SUP = dict(name="WIDELOGIC s.r.o.", street="Světova 523/1", city="Libeň, Praha 8", zip="180 00", country="Česká republika", ico="27870367", dic="CZ27870367",
           vat_payer=True, register="", phone="", email="", web="", bank_account="2702507745/2010", bank_name="", iban="CZ8820100000002702507745", swift="", issued_by="")
PAY = {"Prevodem": "prevod", "Hotove": "hotove"}
YEARFIX, report, invoices = [], [], []
pdfs = {os.path.basename(p)[3:-4]: p for p in glob.glob(os.path.join(SRC, "**", "FV_*.pdf"), recursive=True)}
assert sorted(pdfs) == sorted(P) and len(P) == 55
for n in sorted(P):
    r = P[n]; fl = []
    rows, row_desc, notes, f2 = items_bbox(pdfs[n]); fl += f2
    if len(rows) != len(r["items"]): fl.append(f"pocet radku {len(rows)} != {len(r['items'])}")
    items = []; groups = collections.OrderedDict()
    for i, it in enumerate(r["items"]):
        desc = row_desc.get(i)
        if desc is None: fl.append(f"radek {i+1} bez popisu"); desc = it["desc"].replace("\n", " ")
        k = 1 + it["rate"] / 100
        b, g = it["b_disp"], it["gross"]
        if abs(round(b * k, 2) - g) <= 0.005 or (g == int(g) and abs(b * k - g) < 0.51): base = b
        else: base = round(g / k, 2); fl.append(f"radek {i+1}: zaklad odvozen z ceny s DPH ({b} -> {base})")
        price = round(base / it["qty"], 2)
        if hal(price * it["qty"]) != hal(base): fl.append(f"radek {i+1}: cena/ks {price} x {it['qty']} != {base}")
        if abs(price - it["p_disp"]) > 0.5: fl.append(f"radek {i+1}: cena/ks v PDF {it['p_disp']} vs {price}")
        q = it["qty"]
        items.append(dict(desc=desc, qty=int(q) if q == int(q) else q, unit=it["unit"], price=price, vat=it["rate"]))
        groups[it["rate"]] = groups.get(it["rate"], 0) + hal(q * price)
    base = sum(groups.values()); tax = sum(rnd(v * rt / 100) for rt, v in groups.items()); total = base + tax
    pb, pt, ptot = hal(r["sum_base"]), hal(r["sum_tax"]), hal(r["sum_total"])
    if base != pb: fl.append(f"ZAKLAD nesedi: PDF {pb/100} vs {base/100}")
    taxnote = ""
    if tax != pt:
        if pt % 100 == 0 and abs(pt - tax) <= 50: taxnote = f"DPH v PDF {pt//100} Kč, přesně {tax/100:.2f} Kč"
        else: fl.append(f"DPH nesedi: PDF {pt/100} vs {tax/100}")
    if total == ptot: rt_flag = False
    elif ptot % 100 == 0 and rnd(total / 100) * 100 == ptot: rt_flag = True
    else: rt_flag = False; fl.append(f"CELKEM nesedi: PDF {ptot/100} vs {total/100}")
    d = {k: r[k] for k in ("issue", "tax", "due")}
    if d["issue"].startswith("2025-"):
        YEARFIX.append((n, r["issue"]))
        d = {k: "2026" + v[4:] for k, v in d.items()}
    c = r["customer"]
    if c["extra"]: fl.append("adresa navic: " + "; ".join(c["extra"]))
    if r["pay_raw"] not in PAY: fl.append("forma uhrady: " + r["pay_raw"])
    if r["account"] != "2702507745/2010": fl.append("jiny ucet " + r["account"])
    if any(x != "Jsem plátce DPH." and not x.startswith("ZAKROUHLENI") for x in r["footer"]): fl.append("paticka: " + " | ".join(r["footer"]))
    cust = dict(name=c["name"], street=c["street"].rstrip(","), city=c["city"], zip=c["zip"], country="", ico=c["ico"], dic=c["dic"])
    invoices.append(dict(id=0, kind="faktura", number=n, vs=n, year=2026, seq=int(n[4:]), uuid=str(uuid.uuid4()), issue_date=d["issue"], tax_date=d["tax"], due_date=d["due"],
        payment=PAY.get(r["pay_raw"], "prevod"), supplier=SUP, customer=cust, items=items, prices_incl_vat=False, reverse_charge=False, deposit=0.0, deposit_ref="",
        round_total=rt_flag, order_no="", note="\n".join(notes), template="", paid_date="", canceled=False, created=d["issue"]))
    report.append(dict(number=n, file=os.path.relpath(pdfs[n], SRC), pdf_total=ptot, calc_total=rnd(total/100)*100 if rt_flag else total, rounded=rt_flag, rounding=(rnd(total/100)*100-total) if rt_flag else 0,
                       taxnote=taxnote, notes=len(notes), flags=fl, issue=d["issue"], customer=c["name"], nitems=len(items)))
# zálohová faktura (FakturaOnline) – hodnoty ověřeny proti textu PDF
zf_txt = subprocess.run(["pdftotext", "-layout", glob.glob(os.path.join(SRC, "**", "faktura_ZF*.pdf"), recursive=True)[0], "-"], capture_output=True, text=True).stdout
for needle in ("ZF2026000001", "Qasum s.r.o.", "Na příkopě 853/12, Nové Město", "11000 Praha", "IČO 07375522", "DIČ CZ07375522", "17. 02. 2026", "24. 02. 2026", "Variabilní symbol   2026000001",
               "Zaloha na material - vymena cctv system", "40 000,00 Kč", "Převodem", "2702507745/2010"):
    assert needle in zf_txt, needle
assert re.search(r"1 ks\s+Zaloha na material - vymena cctv system\s+40 000,00\s+0%\s+40 000,00\s+0,00\s+40 000,00", zf_txt)
invoices.append(dict(id=0, kind="zalohova", number="ZF2026000001", vs="2026000001", year=2026, seq=1, uuid=str(uuid.uuid4()), issue_date="2026-02-17", tax_date="2026-02-17", due_date="2026-02-24",
    payment="prevod", supplier=SUP, customer=dict(name="Qasum s.r.o.", street="Na příkopě 853/12, Nové Město", city="Praha", zip="110 00", country="", ico="07375522", dic="CZ07375522"),
    items=[dict(desc="Zaloha na material - vymena cctv system", qty=1, unit="ks", price=40000.0, vat=0.0)], prices_incl_vat=False, reverse_charge=False, deposit=0.0, deposit_ref="",
    round_total=False, order_no="", note="", template="", paid_date="", canceled=False, created="2026-02-17"))
invoices.sort(key=lambda i: (i["issue_date"], i["kind"] != "zalohova", i["number"]))
nid = 1
for i in invoices: i["id"] = nid; nid += 1
contacts = collections.OrderedDict()
for i in sorted((x for x in invoices if x["kind"] == "faktura"), key=lambda x: x["number"]):
    c = i["customer"]; contacts[c["ico"]] = dict(c, email="", phone="", note="")
variants = collections.defaultdict(set)
for i in invoices: variants[i["customer"]["ico"]].add((i["customer"]["name"], i["customer"]["street"], i["customer"]["zip"], i["customer"]["city"]))
clist = []
for c in sorted(contacts.values(), key=lambda c: c["name"].lower()):
    c["id"] = nid; nid += 1; clist.append(c)
durl = lambda p: "data:image/png;base64," + base64.b64encode(open(p, "rb").read()).decode()
settings = dict(company=SUP, year=2026, series=dict(faktura=dict(pattern="{RRRR}{NNNNNN}", next=56), zalohova=dict(pattern="ZF{RRRR}{NNNNNN}", next=2),
    paragon=dict(pattern="PD{RRRR}{NNNN}", next=1), ddpp=dict(pattern="DD{RRRR}{NNNN}", next=1)), due_days=10, default_vat=21.0, default_payment="prevod", template="klasik",
    accent="#3d6df2", round_cash=True, show_qr=True, footer_note="", logo=durl(LOGO), stamp=durl(STAMP))
json.dump(dict(settings=settings, contacts=clist, invoices=invoices, next_id=nid), open(os.path.join(OUT, "fakturace-import-2026.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
json.dump(dict(report=report, yearfix=YEARFIX, variants={k: sorted(v) for k, v in variants.items() if len(v) > 1}), open(os.path.join(OUT, "report.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
print("faktur", len(invoices), "kontaktu", len(clist), "oprava roku", YEARFIX)
print("zaokrouhleno:", [(r["number"][-2:], r["rounding"]/100) for r in report if r["rounded"]])
print("DPH pozn.:", [(r["number"][-2:], r["taxnote"]) for r in report if r["taxnote"]])
print("s poznamkou:", [r["number"][-2:] for r in report if r["notes"]])
for r in report:
    for f in r["flags"]: print("!!", r["number"], f)
print("varianty adres:", json.dumps({k: sorted(v) for k, v in variants.items() if len(v) > 1}, ensure_ascii=False))
seqs = sorted(i["seq"] for i in invoices if i["kind"] == "faktura"); print("rada souvisla:", seqs == list(range(1, 56)))
