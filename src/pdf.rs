//! Malý PDF "canvas": text ve vložených (subsetovaných) fontech, obdélníky, obrázky, QR kód.
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Filter, Finish, Name, Pdf, Rect, Ref, Str};
use std::collections::BTreeMap;
use std::fmt::Write;
use subsetter::GlyphRemapper;

pub const PW: f32 = 595.28;
pub const PH: f32 = 841.89;
pub type Rgb = (f32, f32, f32);

pub fn hex(s: &str) -> Rgb {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() { return (0.24, 0.43, 0.95); }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
    (p(0), p(2), p(4))
}
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb { (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t) }
pub fn luma(c: Rgb) -> f32 { 0.299 * c.0 + 0.587 * c.1 + 0.114 * c.2 }

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Font { Regular = 0, Bold = 1, Light = 2 }

static FONT_DATA: [(&str, &[u8]); 5] = [
    ("Lato-Regular", include_bytes!("../assets/fonts/Lato-Regular.ttf")),
    ("Lato-Bold", include_bytes!("../assets/fonts/Lato-Bold.ttf")),
    ("Lato-Light", include_bytes!("../assets/fonts/Lato-Light.ttf")),
    ("DejaVuSans", include_bytes!("../assets/fonts/DejaVuSans.ttf")),
    ("DejaVuSans-Bold", include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf")),
];
pub fn font_file(name: &str) -> Option<&'static [u8]> {
    FONT_DATA.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}

struct Face {
    face: ttf_parser::Face<'static>,
    remap: GlyphRemapper,
    /// nový glyf -> (původní glyf, znak)
    used: BTreeMap<u16, (u16, char)>,
}

pub struct Image { pub w: u32, pub h: u32, rgb: Vec<u8>, alpha: Option<Vec<u8>> }

impl Image {
    /// Načte obrázek z data: URL (PNG nebo JPEG).
    pub fn from_data_url(url: &str) -> Option<Image> {
        use base64::Engine;
        let b64 = url.split_once("base64,")?.1;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
        let mut img = image::load_from_memory(&bytes).ok()?;
        if img.width() > 1400 || img.height() > 1400 { img = img.thumbnail(1400, 1400); }
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        let mut alpha = Vec::with_capacity((w * h) as usize);
        for p in rgba.pixels() { rgb.extend_from_slice(&p.0[..3]); alpha.push(p.0[3]); }
        let has_alpha = alpha.iter().any(|a| *a < 255);
        let z = |d: &[u8]| miniz_oxide::deflate::compress_to_vec_zlib(d, 7);
        Some(Image { w, h, rgb: z(&rgb), alpha: has_alpha.then(|| z(&alpha)) })
    }
}

pub struct Canvas {
    faces: Vec<Face>,
    pages: Vec<String>,
    images: Vec<Image>,
}

impl Canvas {
    pub fn new() -> Canvas {
        let faces = FONT_DATA.iter().map(|(_, d)| Face {
            face: ttf_parser::Face::parse(d, 0).expect("font"),
            remap: GlyphRemapper::new(),
            used: BTreeMap::new(),
        }).collect();
        Canvas { faces, pages: vec![String::new()], images: Vec::new() }
    }
    pub fn new_page(&mut self) { self.pages.push(String::new()); }
    pub fn page_count(&self) -> usize { self.pages.len() }
    /// Přepne kreslení na danou stranu (0 = první) – pro patičky s číslem strany.
    pub fn with_page<F: FnOnce(&mut Canvas)>(&mut self, idx: usize, f: F) {
        let last = self.pages.len() - 1;
        self.pages.swap(idx, last);
        f(self);
        self.pages.swap(idx, last);
    }
    fn out(&mut self) -> &mut String { self.pages.last_mut().unwrap() }

    fn resolve(&self, font: Font, c: char) -> (usize, u16) {
        let f = font as usize;
        if let Some(g) = self.faces[f].face.glyph_index(c) { return (f, g.0); }
        let fb = if font == Font::Bold { 4 } else { 3 };
        if let Some(g) = self.faces[fb].face.glyph_index(c) { return (fb, g.0); }
        (f, self.faces[f].face.glyph_index('?').map(|g| g.0).unwrap_or(0))
    }
    fn advance(&self, fi: usize, gid: u16) -> f32 {
        let f = &self.faces[fi].face;
        f.glyph_hor_advance(ttf_parser::GlyphId(gid)).unwrap_or(0) as f32 / f.units_per_em() as f32
    }
    pub fn width(&self, s: &str, font: Font, size: f32) -> f32 {
        s.chars().map(|c| { let (fi, g) = self.resolve(font, c); self.advance(fi, g) }).sum::<f32>() * size
    }

    pub fn text(&mut self, x: f32, y: f32, s: &str, font: Font, size: f32, col: Rgb) {
        if s.is_empty() { return; }
        let mut ops = format!("BT {:.3} {:.3} {:.3} rg {:.2} {:.2} Td ", col.0, col.1, col.2, x, PH - y);
        let mut cur: Option<usize> = None;
        for c in s.chars() {
            let c = if c == '\t' { ' ' } else { c };
            let (fi, gid) = self.resolve(font, c);
            let new = self.faces[fi].remap.remap(gid);
            self.faces[fi].used.entry(new).or_insert((gid, c));
            if cur != Some(fi) {
                if cur.is_some() { ops.push_str("> Tj "); }
                let _ = write!(ops, "/F{} {:.2} Tf <", fi, size);
                cur = Some(fi);
            }
            let _ = write!(ops, "{:04X}", new);
        }
        ops.push_str("> Tj ET\n");
        self.out().push_str(&ops);
    }
    pub fn text_r(&mut self, x: f32, y: f32, s: &str, font: Font, size: f32, col: Rgb) {
        let w = self.width(s, font, size);
        self.text(x - w, y, s, font, size, col);
    }
    pub fn text_c(&mut self, x: f32, y: f32, s: &str, font: Font, size: f32, col: Rgb) {
        let w = self.width(s, font, size);
        self.text(x - w / 2.0, y, s, font, size, col);
    }
    /// Text zkrácený výpustkou na danou šířku.
    pub fn fit(&self, s: &str, font: Font, size: f32, maxw: f32) -> String {
        if self.width(s, font, size) <= maxw { return s.to_string(); }
        let mut out: String = s.to_string();
        while !out.is_empty() && self.width(&format!("{out}…"), font, size) > maxw { out.pop(); }
        format!("{}…", out.trim_end())
    }
    pub fn wrap(&self, s: &str, font: Font, size: f32, maxw: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in s.replace('\r', "").split('\n') {
            let mut line = String::new();
            for word in para.split_whitespace() {
                let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if self.width(&cand, font, size) <= maxw { line = cand; continue; }
                if !line.is_empty() { lines.push(std::mem::take(&mut line)); }
                // slovo delší než řádek se láme po znacích
                let mut w = String::new();
                for ch in word.chars() {
                    w.push(ch);
                    if self.width(&w, font, size) > maxw && w.chars().count() > 1 {
                        w.pop();
                        lines.push(std::mem::take(&mut w));
                        w.push(ch);
                    }
                }
                line = w;
            }
            lines.push(line);
        }
        while lines.len() > 1 && lines.last().map(|l| l.is_empty()).unwrap_or(false) { lines.pop(); }
        lines
    }
    /// Vícerádkový text, vrací y za posledním řádkem.
    pub fn para(&mut self, x: f32, y: f32, s: &str, font: Font, size: f32, col: Rgb, maxw: f32, lead: f32) -> f32 {
        let mut yy = y;
        for l in self.wrap(s, font, size, maxw) { self.text(x, yy, &l, font, size, col); yy += lead; }
        yy
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, col: Rgb) {
        let _ = write!(self.out(), "{:.3} {:.3} {:.3} rg {:.2} {:.2} {:.2} {:.2} re f\n", col.0, col.1, col.2, x, PH - y - h, w, h);
    }
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, col: Rgb) {
        let p = self.rrect_path(x, y, w, h, r);
        let _ = write!(self.out(), "{:.3} {:.3} {:.3} rg {p} f\n", col.0, col.1, col.2);
    }
    pub fn rrect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, lw: f32, col: Rgb) {
        let p = self.rrect_path(x, y, w, h, r);
        let _ = write!(self.out(), "{:.3} {:.3} {:.3} RG {:.2} w {p} S\n", col.0, col.1, col.2, lw);
    }
    fn rrect_path(&self, x: f32, y: f32, w: f32, h: f32, r: f32) -> String {
        let r = r.min(w / 2.0).min(h / 2.0);
        let (x0, y0, x1, y1) = (x, PH - y - h, x + w, PH - y);
        let k = r * 0.5523;
        format!("{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} l {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} l {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} l {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c h",
            x0 + r, y0, x1 - r, y0, x1 - r + k, y0, x1, y0 + r - k, x1, y0 + r,
            x1, y1 - r, x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1,
            x0 + r, y1, x0 + r - k, y1, x0, y1 - r + k, x0, y1 - r,
            x0, y0 + r, x0, y0 + r - k, x0 + r - k, y0, x0 + r, y0)
    }
    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, lw: f32, col: Rgb) {
        let _ = write!(self.out(), "{:.3} {:.3} {:.3} RG {:.2} w {:.2} {:.2} m {:.2} {:.2} l S\n", col.0, col.1, col.2, lw, x1, PH - y1, x2, PH - y2);
    }

    pub fn add_image(&mut self, img: Image) -> usize { self.images.push(img); self.images.len() - 1 }
    /// Vykreslí obrázek do rámečku se zachováním poměru stran. `ax`, `ay` = zarovnání 0..1.
    pub fn image(&mut self, idx: usize, x: f32, y: f32, w: f32, h: f32, ax: f32, ay: f32) {
        let (iw, ih) = (self.images[idx].w as f32, self.images[idx].h as f32);
        let s = (w / iw).min(h / ih);
        let (dw, dh) = (iw * s, ih * s);
        let (dx, dy) = (x + (w - dw) * ax, y + (h - dh) * ay);
        let _ = write!(self.out(), "q {:.3} 0 0 {:.3} {:.2} {:.2} cm /Im{} Do Q\n", dw, dh, dx, PH - dy - dh, idx);
    }

    pub fn qr(&mut self, data: &str, x: f32, y: f32, size: f32, dark: Rgb) -> bool {
        let Ok(code) = qrcode::QrCode::with_error_correction_level(data.as_bytes(), qrcode::EcLevel::M) else { return false };
        let n = code.width();
        let cells = code.to_colors();
        let m = size / n as f32;
        let mut ops = format!("{:.3} {:.3} {:.3} rg\n", dark.0, dark.1, dark.2);
        for r in 0..n {
            let mut c = 0;
            while c < n {
                if cells[r * n + c] == qrcode::Color::Dark {
                    let start = c;
                    while c < n && cells[r * n + c] == qrcode::Color::Dark { c += 1; }
                    let _ = write!(ops, "{:.3} {:.3} {:.3} {:.3} re\n", x + start as f32 * m, PH - y - (r + 1) as f32 * m, (c - start) as f32 * m + 0.02, m + 0.02);
                } else { c += 1; }
            }
        }
        ops.push_str("f\n");
        self.out().push_str(&ops);
        true
    }

    pub fn finish(self, title: &str, author: &str) -> Vec<u8> {
        let mut pdf = Pdf::new();
        let mut next = 1;
        let mut alloc = || { let r = Ref::new(next); next += 1; r };
        let catalog = alloc();
        let tree = alloc();
        let info = alloc();
        let page_ids: Vec<Ref> = self.pages.iter().map(|_| alloc()).collect();
        let content_ids: Vec<Ref> = self.pages.iter().map(|_| alloc()).collect();
        let font_ids: Vec<Option<Ref>> = self.faces.iter().map(|f| (!f.used.is_empty()).then(|| alloc())).collect();
        let img_ids: Vec<(Ref, Ref)> = self.images.iter().map(|_| (alloc(), alloc())).collect();

        pdf.catalog(catalog).pages(tree);
        pdf.pages(tree).kids(page_ids.iter().copied()).count(page_ids.len() as i32);
        pdf.document_info(info)
            .title(pdf_writer::TextStr(title))
            .author(pdf_writer::TextStr(author))
            .creator(pdf_writer::TextStr("Fakturace"));

        for (i, body) in self.pages.iter().enumerate() {
            let mut page = pdf.page(page_ids[i]);
            page.parent(tree).media_box(Rect::new(0.0, 0.0, PW, PH)).contents(content_ids[i]);
            let mut res = page.resources();
            {
                let mut fonts = res.fonts();
                for (fi, id) in font_ids.iter().enumerate() {
                    if let Some(id) = id { fonts.pair(Name(format!("F{fi}").as_bytes()), *id); }
                }
            }
            if !img_ids.is_empty() {
                let mut xo = res.x_objects();
                for (ii, (id, _)) in img_ids.iter().enumerate() { xo.pair(Name(format!("Im{ii}").as_bytes()), *id); }
            }
            res.finish();
            page.finish();
            let z = miniz_oxide::deflate::compress_to_vec_zlib(body.as_bytes(), 7);
            pdf.stream(content_ids[i], &z).filter(Filter::FlateDecode);
        }

        for (ii, img) in self.images.iter().enumerate() {
            let (id, mask_id) = img_ids[ii];
            let mut x = pdf.image_xobject(id, &img.rgb);
            x.filter(Filter::FlateDecode);
            x.width(img.w as i32).height(img.h as i32).bits_per_component(8);
            x.color_space().device_rgb();
            if img.alpha.is_some() { x.s_mask(mask_id); }
            x.finish();
            if let Some(a) = &img.alpha {
                let mut m = pdf.image_xobject(mask_id, a);
                m.filter(Filter::FlateDecode);
                m.width(img.w as i32).height(img.h as i32).bits_per_component(8);
                m.color_space().device_gray();
            }
        }

        for (fi, f) in self.faces.iter().enumerate() {
            let Some(type0) = font_ids[fi] else { continue };
            let (cid, desc, file, cmap_id) = (alloc(), alloc(), alloc(), alloc());
            let name = format!("{}+{}", ["FKTAAA", "FKTAAB", "FKTAAC", "FKTAAD", "FKTAAE"][fi], FONT_DATA[fi].0);
            let sys = SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 };
            let scale = 1000.0 / f.face.units_per_em() as f32;

            pdf.type0_font(type0)
                .base_font(Name(name.as_bytes()))
                .encoding_predefined(Name(b"Identity-H"))
                .descendant_font(cid)
                .to_unicode(cmap_id);

            let n = f.remap.num_gids();
            let widths: Vec<f32> = (0..n).map(|g| f.used.get(&g)
                .map(|(old, _)| f.face.glyph_hor_advance(ttf_parser::GlyphId(*old)).unwrap_or(0) as f32 * scale)
                .unwrap_or(0.0)).collect();
            let mut c = pdf.cid_font(cid);
            c.subtype(CidFontType::Type2).base_font(Name(name.as_bytes())).system_info(sys)
                .font_descriptor(desc).default_width(0.0).cid_to_gid_map_predefined(Name(b"Identity"));
            c.widths().consecutive(0, widths);
            c.finish();

            let bb = f.face.global_bounding_box();
            pdf.font_descriptor(desc)
                .name(Name(name.as_bytes()))
                .flags(FontFlags::SYMBOLIC)
                .bbox(Rect::new(bb.x_min as f32 * scale, bb.y_min as f32 * scale, bb.x_max as f32 * scale, bb.y_max as f32 * scale))
                .italic_angle(0.0)
                .ascent(f.face.ascender() as f32 * scale)
                .descent(f.face.descender() as f32 * scale)
                .cap_height(f.face.capital_height().unwrap_or(f.face.ascender()) as f32 * scale)
                .stem_v(if fi == 1 || fi == 4 { 140.0 } else { 80.0 })
                .font_file2(file);

            let mut cm = UnicodeCmap::new(Name(b"Custom"), sys);
            for (new, (_, ch)) in &f.used { cm.pair(*new, *ch); }
            let cm = cm.finish();
            pdf.cmap(cmap_id, cm.as_slice());

            let sub = subsetter::subset(FONT_DATA[fi].1, 0, &f.remap).unwrap_or_else(|_| FONT_DATA[fi].1.to_vec());
            let z = miniz_oxide::deflate::compress_to_vec_zlib(&sub, 7);
            let mut st = pdf.stream(file, &z);
            st.filter(Filter::FlateDecode);
            st.pair(Name(b"Length1"), sub.len() as i32);
        }
        pdf.finish()
    }
}
