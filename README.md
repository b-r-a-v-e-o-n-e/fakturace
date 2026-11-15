# Fakturace

Malá aplikace pro Linux: webový server v Rustu, který se ovládá z prohlížeče.
Vystavuje faktury, zálohové faktury, paragony a daňové doklady k přijaté platbě,
vede adresář odběratelů a knihu vydaných dokladů, exportuje PDF a ISDOC 6.0.2.

## Spuštění

    ./fakturace                      # http://127.0.0.1:8080
    ./fakturace --port 9000 --data /cesta/k/datum
    ./fakturace --host 0.0.0.0 --heslo tajne   # přístup ze sítě, chráněný heslem

Data se ukládají do `~/.local/share/fakturace/data.json` (při každém zápisu vzniká
`data.json.bak`). Zálohu lze stáhnout a obnovit v Nastavení.

Server neumí HTTPS. Pro přístup z internetu ho dejte za reverzní proxy (nginx, Caddy).

### Služba systemd (volitelně)

    [Unit]
    Description=Fakturace
    After=network.target

    [Service]
    ExecStart=/opt/fakturace/fakturace --port 8080
    Restart=on-failure
    User=fakturace

    [Install]
    WantedBy=multi-user.target

## Android

Aplikace pro Android je tentýž server (knihovna `libfakturace.so`) spuštěný uvnitř aplikace a zobrazený ve WebView.
Data zůstávají jen v telefonu; server poslouchá na 127.0.0.1 a pustí jen vlastní aplikaci (tajný token v cookie).

### Sestavení na GitHubu

1. Nahrajte celý projekt do repozitáře na GitHubu (větev `main`).
2. Záložka **Actions** → workflow **Sestavení** se spustí sám po každém `push`, nebo ručně tlačítkem **Run workflow**.
3. Po doběhnutí stáhněte v detailu běhu artefakt **fakturace-android** (ZIP s `fakturace-android.apk`)
   a nainstalujte APK do telefonu (je potřeba povolit instalaci z neznámých zdrojů).

Tentýž workflow sestaví i linuxový program (artefakt **fakturace-linux-x86_64**).

### Vlastní podpisový klíč (doporučeno před prvním ostrým použitím)

Bez vlastního klíče se každé sestavení podepíše jiným dočasným klíčem. Novou verzi pak nejde nainstalovat
přes starou – musela by se odinstalovat, a tím by se smazala data. Klíč vytvořte jednou a uložte ho bezpečně:

    keytool -genkeypair -v -keystore fakturace.jks -alias fakturace -keyalg RSA -keysize 2048 -validity 10000
    base64 -w0 fakturace.jks > fakturace.jks.base64

V repozitáři pak v **Settings → Secrets and variables → Actions** přidejte:

| Secret | Hodnota |
|---|---|
| `KEYSTORE_BASE64` | obsah souboru `fakturace.jks.base64` |
| `KEYSTORE_PASSWORD` | heslo ke keystore |
| `KEY_ALIAS` | `fakturace` |
| `KEY_PASSWORD` | heslo ke klíči |

Soubor `fakturace.jks` nenahrávejte do repozitáře.

### Přenos dat mezi počítačem a telefonem

Nastavení → **Stáhnout zálohu** v jednom zařízení, **Obnovit ze zálohy** v druhém. Data se nesynchronizují;
doklady vystavujte vždy jen v jednom zařízení, jinak se rozejdou číselné řady.

### Rozdíly proti verzi pro Linux

- náhled PDF se zobrazuje jako obrázky (WebView neumí PDF)
- tlačítko PDF otevře doklad v prohlížeči PDF v telefonu; odtud jde sdílet a tisknout
- ISDOC, CSV a záloha se ukládají do složky Stažené
- Android 8.0 a novější

## Sestavení ze zdrojů

    cargo build --release --bin fakturace
    # statický binární soubor bez závislostí:
    RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --bin fakturace --target x86_64-unknown-linux-gnu

## Struktura

- `src/lib.rs` – HTTP server (tiny_http), API, pravidla pro číselné řady, vstupní bod pro Android (JNI)
- `src/main.rs` – spuštění z příkazové řádky v Linuxu
- `android/` – projekt aplikace pro Android (Java, WebView)
- `.github/workflows/build.yml` – sestavení APK a linuxového programu na GitHubu
- `tools/` – skripty pro import starých faktur z PDF
- `src/model.rs` – datový model, výpočet DPH, kontroly náležitostí, QR Platba, IBAN
- `src/pdf.rs` – generátor PDF (pdf-writer, vložené a subsetované fonty)
- `src/templates.rs` – čtyři šablony dokladů
- `src/isdoc.rs` – export ISDOC 6.0.2
- `assets/index.html` – celé webové rozhraní (česky, světlý a tmavý režim)

## Co aplikace hlídá

- náležitosti daňového dokladu podle § 29 zákona o DPH (IČO, DIČ, DUZP, základ, sazba, daň)
- údaj o zápisu v rejstříku (§ 435 občanského zákoníku)
- paragon (zjednodušený daňový doklad) jen do 10 000 Kč včetně DPH
- upozornění na limit hotovostních plateb 270 000 Kč
- vystavení daňového dokladu do 15 dnů od plnění
- souvislou číselnou řadu: smazat lze jen poslední doklad, ostatní se stornují
- zaokrouhlení hotovostních plateb na celé koruny mimo základ daně

## Omezení

- pouze měna CZK
- odečet zálohy snižuje částku k úhradě; nerozepisuje DPH již zdaněné zálohy po sazbách
- neobsahuje EET 2.0 (evidence tržeb od 1. 1. 2027) ani opravné daňové doklady (dobropisy)
- ISDOC se exportuje bez elektronického podpisu

## Licence fontů

Lato (SIL Open Font License 1.1, `assets/fonts/Lato-OFL.txt`), DejaVu Sans (Bitstream Vera / DejaVu license).
