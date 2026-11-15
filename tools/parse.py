import re, sys, json, glob, os
TXT = sys.argv[1]
def num(s): return float(s.replace(",", "").replace("Kč", "").strip())
def date(s):
    m, d, y = s.split("/"); return f"{int(y):04d}-{int(m):02d}-{int(d):02d}"
SUP = [r"WIDELOGIC s\.r\.o\.", r"Světova 523/1", r"180 00\s+Libeň, Praha 8", r"IČ:\s+27870367", r"DIČ:\s+CZ27870367"]
RIGHT = re.compile(r"\s+(DATUM VYSTAVENÍ|DATUM USKUTEČNĚNÍ PLNĚNÍ|\d{1,2}/\d{1,2}/\d{4}|Forma úhrady:.*)\s*$")
ROW = re.compile(r"^(?P<d>.*?)\s+(?P<q>-?\d[\d.,]*)\s+(?P<u>\S+)\s+(?P<p>-?[\d,.]+) Kč\s+(?P<b>-?[\d,.]+) Kč\s+(?P<r>\d+(?:\.\d+)?)%\s+(?P<t>-?[\d,.]+) Kč\s+(?P<g>-?[\d,.]+) Kč\s*$")
out = []
for f in sorted(glob.glob(os.path.join(TXT, "FV_*.txt"))):
    L = open(f, encoding="utf-8").read().split("\n")
    r = {"file": os.path.basename(f), "flags": []}
    i_vs = next(i for i, l in enumerate(L) if "VS / ČÍSLO DOKLADU" in l)
    hdr = next(l for l in L[i_vs + 1:] if l.strip()).split()
    r["due"], r["account"], r["number"] = date(hdr[0]), hdr[1], hdr[2]
    if len(hdr) != 3: r["flags"].append("hlavicka:" + " ".join(hdr))
    i_d = next(i for i, l in enumerate(L) if "DODAVATEL" in l and "ODBĚRATEL" in l)
    i_p = next(i for i, l in enumerate(L) if l.strip().startswith("POLOŽKA"))
    dates = re.findall(r"\d{1,2}/\d{1,2}/\d{4}", "\n".join(L[i_d:i_p]))
    r["issue"], r["tax"] = date(dates[0]), date(dates[1])
    r["pay_raw"] = re.search(r"Forma úhrady:\s*(.*)", "\n".join(L[i_d:i_p])).group(1).strip()
    cust, supseen = [], 0
    for l in L[i_d + 1:i_p]:
        l2 = RIGHT.sub("", l)
        l2 = RIGHT.sub("", l2)
        for s in SUP:
            m = re.match(r"^\s*" + s, l2)
            if m: l2 = l2[m.end():]; supseen += 1; break
        l2 = re.sub(r"\s+", " ", l2).strip()
        if l2: cust.append(l2)
    if supseen != 5: r["flags"].append(f"dodavatel jiny ({supseen}/5)")
    c = {"name": "", "street": "", "zip": "", "city": "", "ico": "", "dic": "", "extra": []}
    for l in cust:
        if m := re.match(r"^DIČ:\s*(.*)$", l): c["dic"] = m.group(1).replace(" ", "")
        elif m := re.match(r"^IČ:\s*(.*)$", l): c["ico"] = m.group(1).replace(" ", "")
        elif (m := re.match(r"^(\d{3} ?\d{2})\s+(.+)$", l)) and c["name"]: c["zip"], c["city"] = m.group(1), m.group(2)
        elif not c["name"]: c["name"] = l
        elif not c["street"] and not c["zip"]: c["street"] = l
        else: c["extra"].append(l)
    r["customer"] = c; r["cust_raw"] = cust
    i_e = next(i for i, l in enumerate(L) if "PODPIS / RAZÍTKO" in l)
    blocks, cur = [], []
    for l in L[i_p + 1:i_e]:
        if l.strip(): cur.append(l)
        elif cur: blocks.append(cur); cur = []
    if cur: blocks.append(cur)
    items = []
    for b in blocks:
        rows = [(k, ROW.match(l)) for k, l in enumerate(b)]
        nrows = [k for k, m in rows if m]
        if not nrows: r["flags"].append("blok bez cisel: " + " | ".join(x.strip() for x in b)); continue
        extra = len(b) - len(nrows)
        if extra and len(nrows) > 1: r["flags"].append("NEJASNE viceradkove: " + " | ".join(x.strip() for x in b))
        for idx, k in enumerate(nrows):
            m = rows[k][1]
            lo = 0 if idx == 0 else nrows[idx - 1] + 1
            hi = len(b) if idx == len(nrows) - 1 else k + 1
            parts = [b[j].strip() if j != k else m.group("d").strip() for j in range(lo, hi)]
            desc = "\n".join(p for p in parts if p)
            items.append({"desc": desc, "qty": num(m.group("q")), "unit": m.group("u"), "p_disp": num(m.group("p")), "b_disp": num(m.group("b")),
                          "rate": float(m.group("r")), "t_disp": num(m.group("t")), "gross": num(m.group("g"))})
    r["items"] = items
    tail = "\n".join(L[i_e:])
    r["sum_base"] = num(re.search(r"SOUČET POLOŽEK\s+(-?[\d,.]+) Kč", tail).group(1))
    r["sum_tax"] = num(re.search(r"\n\s*DPH\s+(-?[\d,.]+) Kč", tail).group(1))
    r["sum_total"] = num(re.search(r"CELKEM S DPH\s+(-?[\d,.]+)(?: Kč)?\s*(?:\n|$)", tail).group(1))
    r["footer"] = [l.strip() for l in L[i_e:] if l.strip() and not re.search(r"PODPIS|SOUČET|^\s*DPH|CELKEM S DPH", l)]
    out.append(r)
json.dump(out, open(sys.argv[2], "w"), ensure_ascii=False, indent=1)
for r in out:
    c = r["customer"]
    print(f'{r["number"]} {r["issue"]} {r["tax"]} due {r["due"]} {r["pay_raw"]:9} | {c["name"]} | {c["street"]} | {c["zip"]} {c["city"]} | {c["ico"]} {c["dic"]} {c["extra"] or ""} | n={len(r["items"])} {r["sum_base"]}+{r["sum_tax"]}={r["sum_total"]} {r["account"] if r["account"]!="2702507745/2010" else ""} {r["footer"] if r["footer"]!=["Jsem plátce DPH."] else ""}')
    for fl in r["flags"]: print("     !!", fl)
