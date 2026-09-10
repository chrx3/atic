//! Exportar el tablero: una celda = una página (imagen, PDF, Word o PPT).

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Deserialize;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlipExportFoto {
    pub image_base64: String,
    #[serde(default)]
    pub mime: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlipExportTexto {
    pub body: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlipExportItem {
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlipExportLista {
    pub items: Vec<FlipExportItem>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlipExportPage {
    #[serde(default)]
    pub preview_base64: String,
    #[serde(default)]
    pub preview_mime: String,
    #[serde(default)]
    pub fotos: Vec<FlipExportFoto>,
    #[serde(default)]
    pub textos: Vec<FlipExportTexto>,
    #[serde(default)]
    pub checks: Vec<FlipExportLista>,
    #[serde(default)]
    pub ink_base64: String,
}

fn decode_image(raw: &str) -> Result<Vec<u8>, String> {
    let payload = match raw.split_once("base64,") {
        Some((_, rest)) => rest,
        None => raw,
    };
    if payload.trim().is_empty() {
        return Err("imagen vacía".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|err| format!("imagen inválida: {err}"))
}

fn ext_de(mime: &str) -> &'static str {
    if mime.contains("jpeg") || mime.contains("jpg") {
        "jpeg"
    } else if mime.contains("gif") {
        "gif"
    } else if mime.contains("webp") {
        "webp"
    } else {
        "png"
    }
}

fn es_jpeg(bytes: &[u8], mime: &str) -> bool {
    mime.contains("jpeg")
        || mime.contains("jpg")
        || (bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF)
}

fn es_webp(bytes: &[u8], mime: &str) -> bool {
    mime.contains("webp")
        || (bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP")
}

/// SOF0..SOF3 (y los progresivos) traen el tamaño real. Lo leemos a mano para
/// mandar el JPEG por DCTDecode sin decodificar píxeles — y sin mentir 1600×1600.
fn jpeg_dims(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 8 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        i += 2;
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if marker == 0xDA {
            break;
        }
        if i + 1 >= bytes.len() {
            break;
        }
        let len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
        if len < 2 {
            break;
        }
        let sof = matches!(
            marker,
            0xC0 | 0xC1
                | 0xC2
                | 0xC3
                | 0xC5
                | 0xC6
                | 0xC7
                | 0xC9
                | 0xCA
                | 0xCB
                | 0xCD
                | 0xCE
                | 0xCF
        );
        if sof && i + 6 < bytes.len() {
            let height = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            if width > 0 && height > 0 {
                return Some((width, height));
            }
        }
        i += len;
    }
    None
}

/// Word/PPT no abren WebP: se pasa a PNG. El resto se deja como vino.
fn bytes_oficina(raw: &[u8], mime: &str) -> Result<(Vec<u8>, &'static str), String> {
    if es_webp(raw, mime) {
        let img = image::load_from_memory(raw).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        return Ok((out, "png"));
    }
    Ok((raw.to_vec(), ext_de(mime)))
}

fn escape_xml(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c == '\t' || c == '\n' || c >= ' ' => out.push(c),
            _ => {}
        }
    }
    out
}

fn pdf_escape(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for character in value.chars() {
        let byte = match character {
            '\\' | '(' | ')' => {
                out.push(b'\\');
                character as u8
            }
            character if (character as u32) <= 0xFF => character as u8,
            _ => b'?',
        };
        out.push(byte);
    }
    out
}

fn zip_start(
    archive: &mut ZipWriter<std::fs::File>,
    name: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive
        .start_file(name, options)
        .map_err(|e| e.to_string())?;
    archive.write_all(bytes).map_err(|e| e.to_string())
}

fn preview_de(page: &FlipExportPage) -> Result<(Vec<u8>, String), String> {
    if page.preview_base64.is_empty() {
        return Err("no hay vista previa".into());
    }
    Ok((
        decode_image(&page.preview_base64)?,
        page.preview_mime.clone(),
    ))
}

fn write_zip_images(path: &Path, pages: &[FlipExportPage], ext: &str) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut archive = ZipWriter::new(file);
    for (i, page) in pages.iter().enumerate() {
        let (bytes, _) = preview_de(page)?;
        zip_start(&mut archive, &format!("pagina-{:02}.{ext}", i + 1), &bytes)?;
    }
    archive.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn write_single_image(path: &Path, page: &FlipExportPage) -> Result<(), String> {
    let (bytes, _) = preview_de(page)?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

fn rgba_planos(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>, Option<Vec<u8>>), String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    let mut alpha = Vec::with_capacity((width * height) as usize);
    let mut hay_alfa = false;
    for pixel in rgba.pixels() {
        rgb.extend_from_slice(&pixel.0[..3]);
        alpha.push(pixel[3]);
        if pixel[3] != 255 {
            hay_alfa = true;
        }
    }
    Ok((width, height, rgb, hay_alfa.then_some(alpha)))
}

const FONT_PT: f64 = 11.0;
const LINE_PT: f64 = 16.0;

/// Tamaño lógico de una celda del tablero, en píxeles de papel.
///
/// De acá salen la escala del PDF y la del PPT. Antes eran constantes sueltas
/// (800 px y 612 pt): al agrandar la pizarra el export habría seguido midiendo
/// contra una celda que ya no existe.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct FlipExportPageSize {
    pub w: f64,
    pub h: f64,
}

impl Default for FlipExportPageSize {
    fn default() -> Self {
        Self { w: 800.0, h: 800.0 }
    }
}

impl FlipExportPageSize {
    /// Ancho de la página del PDF, en puntos.
    const ANCHO_PT: f64 = 612.0;
    /// Ancho del lienzo del PPT, en EMU (7.5 pulgadas).
    const ANCHO_EMU: i64 = 6_858_000;

    /// Píxeles de papel → puntos del PDF. Uniforme en los dos ejes.
    fn pt(&self, px: f64) -> f64 {
        if self.w <= 0.0 {
            return px;
        }
        px * Self::ANCHO_PT / self.w
    }

    /// Puntos del PDF → píxeles de papel.
    fn px(&self, pt: f64) -> f64 {
        if self.w <= 0.0 {
            return pt;
        }
        pt * self.w / Self::ANCHO_PT
    }

    fn ancho_pt(&self) -> f64 {
        self.pt(self.w)
    }

    fn alto_pt(&self) -> f64 {
        self.pt(self.h)
    }

    /// Píxeles de papel → EMU, con la misma escala en los dos ejes.
    fn emu(&self, px: f64) -> i64 {
        if self.w <= 0.0 {
            return px.round() as i64;
        }
        (px * Self::ANCHO_EMU as f64 / self.w).round() as i64
    }

    fn ancho_emu(&self) -> i64 {
        Self::ANCHO_EMU
    }

    fn alto_emu(&self) -> i64 {
        self.emu(self.h)
    }
}

fn fmt_n(v: f64) -> String {
    format!("{:.2}", v)
}

struct PdfBuilder {
    objects: Vec<Vec<u8>>,
}

impl PdfBuilder {
    fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    fn add(&mut self, body: Vec<u8>) -> usize {
        self.objects.push(body);
        self.objects.len()
    }

    fn finish(self) -> Vec<u8> {
        let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(self.objects.len());
        for (index, object) in self.objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", self.objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                self.objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }
}

fn image_xobject(
    width: u32,
    height: u32,
    filter: &str,
    payload: &[u8],
    smask: Option<usize>,
) -> Vec<u8> {
    let smask_part = smask
        .map(|id| format!(" /SMask {id} 0 R"))
        .unwrap_or_default();
    let mut obj = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8{filter}{smask_part} /Length {} >>\nstream\n",
        payload.len()
    )
    .into_bytes();
    obj.extend_from_slice(payload);
    obj.extend_from_slice(b"\nendstream");
    obj
}

fn gray_xobject(width: u32, height: u32, payload: &[u8]) -> Vec<u8> {
    let mut obj = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
        payload.len()
    )
    .into_bytes();
    obj.extend_from_slice(payload);
    obj.extend_from_slice(b"\nendstream");
    obj
}

fn embed_image(pdf: &mut PdfBuilder, bytes: &[u8], mime: &str) -> Result<usize, String> {
    if es_jpeg(bytes, mime) {
        let (w, h) = jpeg_dims(bytes).ok_or_else(|| "JPEG sin tamaño".to_string())?;
        return Ok(pdf.add(image_xobject(w, h, " /Filter /DCTDecode", bytes, None)));
    }
    let (w, h, rgb, alpha) = rgba_planos(bytes)?;
    let mask = alpha.map(|a| pdf.add(gray_xobject(w, h, &a)));
    Ok(pdf.add(image_xobject(w, h, "", &rgb, mask)))
}

fn wrap_lineas(texto: &str, max_w: f64) -> Vec<String> {
    let max_chars = ((max_w / (FONT_PT * 0.5)).floor() as usize).max(1);
    let mut out = Vec::new();
    for cruda in texto.split('\n') {
        if cruda.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut actual = String::new();
        for word in cruda.split_whitespace() {
            let prueba = if actual.is_empty() {
                word.to_string()
            } else {
                format!("{actual} {word}")
            };
            if prueba.chars().count() <= max_chars {
                actual = prueba;
            } else {
                if !actual.is_empty() {
                    out.push(actual);
                }
                actual = word.to_string();
            }
        }
        out.push(actual);
    }
    out
}

fn pdf_tj(stream: &mut Vec<u8>, x: f64, y: f64, line: &str) {
    stream.extend_from_slice(format!("1 0 0 1 {} {} Tm (", fmt_n(x), fmt_n(y)).as_bytes());
    stream.extend(pdf_escape(line));
    stream.extend_from_slice(b") Tj\n");
}

fn pdf_bytes(pages: &[FlipExportPage], pagina: FlipExportPageSize) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("no hay páginas".into());
    }
    let n = pages.len();
    let mut pdf = PdfBuilder::new();
    let catalog_id = pdf.add(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    debug_assert_eq!(catalog_id, 1);
    let _pages_slot = pdf.add(Vec::new());
    let font_id = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );

    let mut page_ids = Vec::with_capacity(n);
    for page in pages {
        struct XObj {
            name: String,
            id: usize,
            x: f64,
            y: f64,
            w: f64,
            h: f64,
            ink: bool,
        }
        let mut xobjs: Vec<XObj> = Vec::new();
        for (i, foto) in page.fotos.iter().enumerate() {
            let raw = decode_image(&foto.image_base64)?;
            let id = embed_image(&mut pdf, &raw, &foto.mime)?;
            xobjs.push(XObj {
                name: format!("Im{i}"),
                id,
                x: pagina.pt(foto.x),
                y: pagina.alto_pt() - pagina.pt(foto.y + foto.h),
                w: pagina.pt(foto.w).max(1.0),
                h: pagina.pt(foto.h).max(1.0),
                ink: false,
            });
        }
        if !page.ink_base64.is_empty() {
            let raw = decode_image(&page.ink_base64)?;
            let id = embed_image(&mut pdf, &raw, "image/png")?;
            xobjs.push(XObj {
                name: "Ink".into(),
                id,
                x: 0.0,
                y: 0.0,
                w: pagina.ancho_pt(),
                h: pagina.alto_pt(),
                ink: true,
            });
        }

        let mut stream = Vec::new();
        for o in xobjs.iter().filter(|o| !o.ink) {
            stream.extend_from_slice(
                format!(
                    "q {} 0 0 {} {} {} cm /{} Do Q\n",
                    fmt_n(o.w),
                    fmt_n(o.h),
                    fmt_n(o.x),
                    fmt_n(o.y),
                    o.name
                )
                .as_bytes(),
            );
        }
        stream.extend_from_slice(format!("BT /F1 {} Tf\n", fmt_n(FONT_PT)).as_bytes());
        let pad = pagina.pt(8.0);
        for texto in &page.textos {
            let max_w = pagina.pt(texto.w) - pad * 2.0;
            let fondo = pagina.alto_pt() - pagina.pt(texto.y + texto.h);
            let mut y_board = texto.y;
            for line in wrap_lineas(&texto.body, max_w.max(12.0)) {
                let baseline = pagina.alto_pt() - pagina.pt(y_board + 13.0);
                if baseline < fondo {
                    break;
                }
                pdf_tj(&mut stream, pagina.pt(texto.x) + pad, baseline, &line);
                y_board += pagina.px(LINE_PT);
            }
        }
        for lista in &page.checks {
            let fondo = pagina.alto_pt() - pagina.pt(lista.y + lista.h);
            let mut y_board = lista.y;
            for item in &lista.items {
                let baseline = pagina.alto_pt() - pagina.pt(y_board + 13.0);
                if baseline < fondo {
                    break;
                }
                let marca = if item.done { "[x] " } else { "[ ] " };
                pdf_tj(
                    &mut stream,
                    pagina.pt(lista.x) + pad,
                    baseline,
                    &format!("{marca}{}", item.text),
                );
                y_board += pagina.px(LINE_PT);
            }
        }
        stream.extend_from_slice(b"ET\n");
        for o in xobjs.iter().filter(|o| o.ink) {
            stream.extend_from_slice(
                format!(
                    "q {} 0 0 {} {} {} cm /{} Do Q\n",
                    fmt_n(o.w),
                    fmt_n(o.h),
                    fmt_n(o.x),
                    fmt_n(o.y),
                    o.name
                )
                .as_bytes(),
            );
        }

        let mut content = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        content.extend_from_slice(&stream);
        content.extend_from_slice(b"\nendstream");
        let content_id = pdf.add(content);

        let mut xobj_dict = String::new();
        for o in &xobjs {
            xobj_dict.push_str(&format!("/{} {} 0 R ", o.name, o.id));
        }
        let page_id = pdf.add(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 {font_id} 0 R >> /XObject << {xobj_dict}>> >> /Contents {content_id} 0 R >>",
                fmt_n(pagina.ancho_pt()),
                fmt_n(pagina.alto_pt())
            )
            .into_bytes(),
        );
        page_ids.push(page_id);
    }

    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    pdf.objects[1] = format!("<< /Type /Pages /Kids [{kids}] /Count {n} >>").into_bytes();
    Ok(pdf.finish())
}

/// PDF nativo: fotos y texto en las coords del tablero, tinta encima con alfa.
/// No usa el pantallazo (`preview_*`); un tablero sin preview igual exporta.
fn write_pdf(
    path: &Path,
    pages: &[FlipExportPage],
    pagina: FlipExportPageSize,
) -> Result<(), String> {
    std::fs::write(path, pdf_bytes(pages, pagina)?).map_err(|e| e.to_string())
}

fn word_drawing(rid: u32, name: &str, cx: i64, cy: i64) -> String {
    format!(
        r#"<w:p><w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="{rid}" name="{name}"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{rid}" name="{name}"/><pic:cNvPicPr><a:picLocks noChangeAspect="1"/></pic:cNvPicPr></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
    )
}

fn word_parrafo(texto: &str) -> String {
    format!(
        r#"<w:p><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
        escape_xml(texto)
    )
}

fn write_docx(
    path: &Path,
    pages: &[FlipExportPage],
    pagina: FlipExportPageSize,
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut archive = ZipWriter::new(file);

    zip_start(
        &mut archive,
        "[Content_Types].xml",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="png" ContentType="image/png"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#,
    )?;
    zip_start(
        &mut archive,
        "_rels/.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#,
    )?;
    zip_start(
        &mut archive,
        "docProps/core.xml",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>Tablero Atic</dc:title><dc:creator>Atic</dc:creator></cp:coreProperties>"#,
    )?;
    zip_start(
        &mut archive,
        "docProps/app.xml",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Atic</Application></Properties>"#,
    )?;

    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    let mut document = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>"#,
    );

    let mut rid = 0u32;
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            document.push_str(r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#);
        }
        document.push_str(&word_parrafo(&format!("Página {}", i + 1)));

        if !page.ink_base64.is_empty() {
            rid += 1;
            let bytes = decode_image(&page.ink_base64)?;
            zip_start(&mut archive, &format!("word/media/tinta{rid}.png"), &bytes)?;
            rels.push_str(&format!(
                r#"<Relationship Id="rId{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/tinta{rid}.png"/>"#
            ));
            document.push_str(&word_drawing(
                rid,
                &format!("tinta{rid}"),
                pagina.ancho_emu() / 2,
                pagina.alto_emu() / 2,
            ));
        }

        for foto in &page.fotos {
            rid += 1;
            let raw = decode_image(&foto.image_base64)?;
            let (bytes, ext) = bytes_oficina(&raw, &foto.mime)?;
            zip_start(&mut archive, &format!("word/media/foto{rid}.{ext}"), &bytes)?;
            rels.push_str(&format!(
                r#"<Relationship Id="rId{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/foto{rid}.{ext}"/>"#
            ));
            document.push_str(&word_drawing(
                rid,
                &format!("foto{rid}"),
                pagina.emu(foto.w).max(91440),
                pagina.emu(foto.h).max(91440),
            ));
        }

        for texto in &page.textos {
            for line in texto.body.lines() {
                document.push_str(&word_parrafo(line));
            }
        }
        for lista in &page.checks {
            for item in &lista.items {
                let marca = if item.done { "☑ " } else { "☐ " };
                document.push_str(&word_parrafo(&format!("{marca}{}", item.text)));
            }
        }
    }
    rels.push_str("</Relationships>");
    document.push_str(
        r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720"/></w:sectPr></w:body></w:document>"#,
    );

    zip_start(
        &mut archive,
        "word/_rels/document.xml.rels",
        rels.as_bytes(),
    )?;
    zip_start(&mut archive, "word/document.xml", document.as_bytes())?;
    archive.finish().map_err(|e| e.to_string())?;
    Ok(())
}

const THEME_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Atic"><a:themeElements><a:clrScheme name="Atic"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1C1917"/></a:dk2><a:lt2><a:srgbClr val="F4F1EA"/></a:lt2><a:accent1><a:srgbClr val="5B8DEF"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="4472C4"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="Atic"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Atic"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="35000"><a:schemeClr val="phClr"><a:tint val="37000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="15000"/><a:satMod val="350000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="1"/></a:gradFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:shade val="51000"/><a:satMod val="130000"/></a:schemeClr></a:gs><a:gs pos="80000"><a:schemeClr val="phClr"><a:shade val="93000"/><a:satMod val="130000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="94000"/><a:satMod val="135000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="0"/></a:gradFill></a:fillStyleLst><a:lnStyleLst><a:ln w="9525" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"><a:shade val="95000"/><a:satMod val="105000"/></a:schemeClr></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="25400" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="38100" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst><a:outerShdw blurRad="40000" dist="23000" dir="5400000" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="35000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="40000"/><a:satMod val="350000"/></a:schemeClr></a:gs><a:gs pos="40000"><a:schemeClr val="phClr"><a:tint val="45000"/><a:satMod val="350000"/><a:shade val="99000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="20000"/><a:satMod val="255000"/></a:schemeClr></a:gs></a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="-80000" r="50000" b="180000"/></a:path></a:gradFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="80000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="30000"/><a:satMod val="200000"/></a:schemeClr></a:gs></a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="50000" r="50000" b="50000"/></a:path></a:gradFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#;

fn tx_style_levels() -> String {
    let mut out = String::new();
    for lvl in 1..=9 {
        let sz = (2000 - (lvl as i32 - 1) * 200).max(1100);
        out.push_str(&format!(
            r#"<a:lvl{lvl}pPr marL="{mar}" indent="0" algn="l"><a:defRPr sz="{sz}" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:defRPr></a:lvl{lvl}pPr>"#,
            mar = (lvl - 1) * 457200
        ));
    }
    out
}

fn write_pptx(
    path: &Path,
    pages: &[FlipExportPage],
    pagina: FlipExportPageSize,
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut archive = ZipWriter::new(file);
    let n = pages.len();
    let ew = pagina.ancho_emu();
    let eh = pagina.alto_emu();

    let mut types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="png" ContentType="image/png"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#,
    );
    for i in 1..=n {
        types.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        ));
    }
    types.push_str("</Types>");
    zip_start(&mut archive, "[Content_Types].xml", types.as_bytes())?;
    zip_start(
        &mut archive,
        "_rels/.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#,
    )?;
    zip_start(
        &mut archive,
        "docProps/core.xml",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Tablero Atic</dc:title><dc:creator>Atic</dc:creator></cp:coreProperties>"#,
    )?;
    zip_start(
        &mut archive,
        "docProps/app.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Atic</Application><Slides>{n}</Slides></Properties>"#
        )
        .as_bytes(),
    )?;

    let mut pres_rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>"#,
    );
    let mut sld_ids = String::new();
    for i in 1..=n {
        let rid = i + 1;
        pres_rels.push_str(&format!(
            r#"<Relationship Id="rId{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i}.xml"/>"#
        ));
        sld_ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{rid}"/>"#, 255 + i));
    }
    pres_rels.push_str("</Relationships>");

    let levels = tx_style_levels();
    let master = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{ew}" cy="{eh}"/><a:chOff x="0" y="0"/><a:chExt cx="{ew}" cy="{eh}"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle>{levels}</p:titleStyle><p:bodyStyle>{levels}</p:bodyStyle><p:otherStyle>{levels}</p:otherStyle></p:txStyles></p:sldMaster>"#
    );
    zip_start(
        &mut archive,
        "ppt/slideMasters/slideMaster1.xml",
        master.as_bytes(),
    )?;
    zip_start(
        &mut archive,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#,
    )?;
    let layout = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1"><p:cSld name="En blanco"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{ew}" cy="{eh}"/><a:chOff x="0" y="0"/><a:chExt cx="{ew}" cy="{eh}"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#
    );
    zip_start(
        &mut archive,
        "ppt/slideLayouts/slideLayout1.xml",
        layout.as_bytes(),
    )?;
    zip_start(
        &mut archive,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#,
    )?;
    zip_start(&mut archive, "ppt/theme/theme1.xml", THEME_XML.as_bytes())?;

    let presentation = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{sld_ids}</p:sldIdLst><p:sldSz cx="{ew}" cy="{eh}" type="custom"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
    );
    zip_start(
        &mut archive,
        "ppt/_rels/presentation.xml.rels",
        pres_rels.as_bytes(),
    )?;
    zip_start(
        &mut archive,
        "ppt/presentation.xml",
        presentation.as_bytes(),
    )?;

    for (i, page) in pages.iter().enumerate() {
        let nro = i + 1;
        let mut rels = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>"#,
        );
        let mut shapes = String::new();
        let mut shape_id = 2u32;
        let mut rel_id = 1u32;

        let poner_foto = |archive: &mut ZipWriter<std::fs::File>,
                          rels: &mut String,
                          shapes: &mut String,
                          shape_id: &mut u32,
                          rel_id: &mut u32,
                          bytes: &[u8],
                          ext: &str,
                          name: &str,
                          x: i64,
                          y: i64,
                          cx: i64,
                          cy: i64|
         -> Result<(), String> {
            *rel_id += 1;
            *shape_id += 1;
            let rid = *rel_id;
            let sid = *shape_id;
            zip_start(
                archive,
                &format!("ppt/media/{name}{nro}_{rid}.{ext}"),
                bytes,
            )?;
            rels.push_str(&format!(
                r#"<Relationship Id="rId{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/{name}{nro}_{rid}.{ext}"/>"#
            ));
            shapes.push_str(&format!(
                r#"<p:pic><p:nvPicPr><p:cNvPr id="{sid}" name="{name}{sid}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId{rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#
            ));
            Ok(())
        };

        if !page.ink_base64.is_empty() {
            let bytes = decode_image(&page.ink_base64)?;
            poner_foto(
                &mut archive,
                &mut rels,
                &mut shapes,
                &mut shape_id,
                &mut rel_id,
                &bytes,
                "png",
                "tinta",
                0,
                0,
                ew,
                eh,
            )?;
        }
        for foto in &page.fotos {
            let raw = decode_image(&foto.image_base64)?;
            let (bytes, ext) = bytes_oficina(&raw, &foto.mime)?;
            poner_foto(
                &mut archive,
                &mut rels,
                &mut shapes,
                &mut shape_id,
                &mut rel_id,
                &bytes,
                ext,
                "foto",
                pagina.emu(foto.x),
                pagina.emu(foto.y),
                pagina.emu(foto.w).max(91440),
                pagina.emu(foto.h).max(91440),
            )?;
        }

        let mut caja_texto = |shape_id: &mut u32,
                              x: i64,
                              y: i64,
                              cx: i64,
                              cy: i64,
                              paras: String| {
            *shape_id += 1;
            let sid = *shape_id;
            shapes.push_str(&format!(
                r#"<p:sp><p:nvSpPr><p:cNvPr id="{sid}" name="Texto {sid}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln></p:spPr><p:txBody><a:bodyPr wrap="square" lIns="91440" tIns="45720" rIns="91440" bIns="45720"/><a:lstStyle/>{paras}</p:txBody></p:sp>"#
            ));
        };

        for texto in &page.textos {
            let mut paras = String::new();
            let lineas = if texto.body.trim().is_empty() {
                vec![""]
            } else {
                texto.body.lines().collect()
            };
            for line in lineas {
                if line.is_empty() {
                    paras.push_str(r#"<a:p><a:endParaRPr lang="es-CL" sz="1400"/></a:p>"#);
                } else {
                    paras.push_str(&format!(
                        r#"<a:p><a:r><a:rPr lang="es-CL" sz="1400" dirty="0"><a:solidFill><a:srgbClr val="1C1917"/></a:solidFill><a:latin typeface="Calibri"/></a:rPr><a:t>{}</a:t></a:r></a:p>"#,
                        escape_xml(line)
                    ));
                }
            }
            caja_texto(
                &mut shape_id,
                pagina.emu(texto.x),
                pagina.emu(texto.y),
                pagina.emu(texto.w).max(182880),
                pagina.emu(texto.h).max(182880),
                paras,
            );
        }
        for lista in &page.checks {
            let mut paras = String::new();
            for item in &lista.items {
                let marca = if item.done { "☑ " } else { "☐ " };
                let color = if item.done { "78716C" } else { "1C1917" };
                paras.push_str(&format!(
                    r#"<a:p><a:r><a:rPr lang="es-CL" sz="1400" dirty="0"><a:solidFill><a:srgbClr val="{color}"/></a:solidFill><a:latin typeface="Calibri"/></a:rPr><a:t>{}</a:t></a:r></a:p>"#,
                    escape_xml(&format!("{marca}{}", item.text))
                ));
            }
            if paras.is_empty() {
                paras.push_str(r#"<a:p><a:endParaRPr lang="es-CL" sz="1400"/></a:p>"#);
            }
            caja_texto(
                &mut shape_id,
                pagina.emu(lista.x),
                pagina.emu(lista.y),
                pagina.emu(lista.w).max(182880),
                pagina.emu(lista.h).max(182880),
                paras,
            );
        }

        rels.push_str("</Relationships>");
        let slide = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{ew}" cy="{eh}"/><a:chOff x="0" y="0"/><a:chExt cx="{ew}" cy="{eh}"/></a:xfrm></p:grpSpPr>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
        );
        zip_start(
            &mut archive,
            &format!("ppt/slides/_rels/slide{nro}.xml.rels"),
            rels.as_bytes(),
        )?;
        zip_start(
            &mut archive,
            &format!("ppt/slides/slide{nro}.xml"),
            slide.as_bytes(),
        )?;
    }
    archive.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn window_flip_export(
    format: String,
    path: String,
    pages: Vec<FlipExportPage>,
    page_w: Option<f64>,
    page_h: Option<f64>,
) -> Result<String, String> {
    // El tamaño de la celda viaja desde el tablero: el backend no lo adivina.
    let pagina = FlipExportPageSize {
        w: page_w.filter(|v| *v > 0.0).unwrap_or(800.0),
        h: page_h.filter(|v| *v > 0.0).unwrap_or(800.0),
    };
    if pages.is_empty() {
        return Err(crate::ui_lang::msg(
            "El tablero está vacío.",
            "The board is empty.",
        ));
    }
    let destination = PathBuf::from(path.trim());
    if destination.as_os_str().is_empty() {
        return Err(crate::ui_lang::msg(
            "Elige un archivo de destino.",
            "Choose a destination file.",
        ));
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    match format.as_str() {
        "png" | "jpeg" => {
            if pages.len() == 1 {
                write_single_image(&destination, &pages[0])?;
            } else {
                write_zip_images(&destination, &pages, ext_de(&pages[0].preview_mime))?;
            }
        }
        "pdf" => write_pdf(&destination, &pages, pagina)?,
        "docx" => write_docx(&destination, &pages, pagina)?,
        "pptx" => write_pptx(&destination, &pages, pagina)?,
        _ => {
            return Err(crate::ui_lang::msg(
                "Formato de exportación inválido.",
                "Invalid export format.",
            ))
        }
    }
    Ok(destination.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use zip::ZipArchive;

    fn png_stub() -> String {
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==".into()
    }

    fn page_texto() -> FlipExportPage {
        FlipExportPage {
            preview_base64: png_stub(),
            preview_mime: "image/png".into(),
            fotos: vec![FlipExportFoto {
                image_base64: png_stub(),
                mime: "image/png".into(),
                x: 40.0,
                y: 40.0,
                w: 200.0,
                h: 120.0,
            }],
            textos: vec![FlipExportTexto {
                body: "Hola tablero".into(),
                x: 40.0,
                y: 200.0,
                w: 240.0,
                h: 80.0,
            }],
            checks: vec![FlipExportLista {
                items: vec![FlipExportItem {
                    text: "Mail".into(),
                    done: true,
                }],
                x: 40.0,
                y: 300.0,
                w: 240.0,
                h: 80.0,
            }],
            ink_base64: String::new(),
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("flip-export-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn zip_nombres(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut names = Vec::new();
        for i in 0..archive.len() {
            names.push(archive.by_index(i).unwrap().name().to_string());
        }
        names
    }

    fn zip_texto(path: &Path, inner: &str) -> String {
        let file = std::fs::File::open(path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut entry = archive.by_name(inner).unwrap();
        let mut buf = String::new();
        Read::read_to_string(&mut entry, &mut buf).unwrap();
        buf
    }

    #[test]
    fn zip_png_tiene_firma() {
        let zip = tmp("p.zip");
        write_zip_images(&zip, &[page_texto()], "png").unwrap();
        assert!(std::fs::read(&zip).unwrap().starts_with(b"PK"));
        let _ = std::fs::remove_dir_all(zip.parent().unwrap());
    }

    #[test]
    fn pptx_tiene_maestro_y_texto() {
        let pptx = tmp("t.pptx");
        write_pptx(&pptx, &[page_texto()], FlipExportPageSize::default()).unwrap();
        let names = zip_nombres(&pptx);
        assert!(names.iter().any(|n| n.contains("slideMaster1.xml")));
        assert!(names.iter().any(|n| n.contains("theme1.xml")));
        assert!(names.iter().any(|n| n.contains("slideLayout1.xml")));
        let slide = zip_texto(&pptx, "ppt/slides/slide1.xml");
        assert!(slide.contains("Hola tablero"));
        assert!(slide.contains("Mail"));
        assert!(slide.contains("p:txBody"));
        let rels = zip_texto(&pptx, "ppt/slides/_rels/slide1.xml.rels");
        assert!(rels.contains("slideLayout"));
        let _ = std::fs::remove_dir_all(pptx.parent().unwrap());
    }

    #[test]
    fn docx_texto_nativo() {
        let docx = tmp("t.docx");
        write_docx(&docx, &[page_texto()], FlipExportPageSize::default()).unwrap();
        let xml = zip_texto(&docx, "word/document.xml");
        assert!(xml.contains("Hola tablero"));
        assert!(xml.contains("Mail"));
        assert!(xml.contains("<w:t"));
        let _ = std::fs::remove_dir_all(docx.parent().unwrap());
    }

    #[test]
    fn jpeg_dims_desde_sof0() {
        let mut jpeg = vec![
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x10, 0x00, 0x20,
        ];
        jpeg.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]);
        assert_eq!(jpeg_dims(&jpeg), Some((32, 16)));
    }

    #[test]
    fn ext_de_respeta_webp() {
        assert_eq!(ext_de("image/webp"), "webp");
        assert_eq!(ext_de("image/jpeg"), "jpeg");
    }

    fn page_nativa() -> FlipExportPage {
        FlipExportPage {
            preview_base64: String::new(),
            preview_mime: String::new(),
            fotos: vec![FlipExportFoto {
                image_base64: png_stub(),
                mime: "image/png".into(),
                x: 40.0,
                y: 40.0,
                w: 200.0,
                h: 120.0,
            }],
            textos: vec![FlipExportTexto {
                body: "Hola tablero".into(),
                x: 40.0,
                y: 200.0,
                w: 240.0,
                h: 80.0,
            }],
            checks: vec![FlipExportLista {
                items: vec![FlipExportItem {
                    text: "Mail".into(),
                    done: true,
                }],
                x: 40.0,
                y: 300.0,
                w: 240.0,
                h: 80.0,
            }],
            ink_base64: png_stub(),
        }
    }

    #[test]
    fn pdf_texto_nativo_sin_preview() {
        let pdf = tmp("t.pdf");
        write_pdf(&pdf, &[page_nativa()], FlipExportPageSize::default()).unwrap();
        let bytes = std::fs::read(&pdf).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("Hola tablero"));
        assert!(s.contains("[x] Mail"));
        assert!(s.contains("/MediaBox [0 0 612.00 612.00]"));
        assert!(!s.contains("500 0 0 500"));
        let _ = std::fs::remove_dir_all(pdf.parent().unwrap());
    }

    #[test]
    fn pdf_deriva_la_pagina_del_tamano_del_tablero() {
        // 1200x900 (la celda de hoy): 612 pt de ancho y alto proporcional.
        let pagina = FlipExportPageSize {
            w: 1200.0,
            h: 900.0,
        };
        let bytes = pdf_bytes(&[page_nativa()], pagina).unwrap();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("/MediaBox [0 0 612.00 459.00]"), "mediabox 4:3");
        // Una foto de 200x120 px del tablero: 102 x 61.2 pt, en su esquina.
        assert!(s.contains("102.00 0 0 61.20"), "escala uniforme de la foto");
    }

    #[test]
    fn pdf_jpeg_usa_dims_reales() {
        let mut jpeg = vec![
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x10, 0x00, 0x20, 0x01, 0x01, 0x11,
            0x00,
        ];
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg);
        let page = FlipExportPage {
            preview_base64: String::new(),
            preview_mime: String::new(),
            fotos: vec![FlipExportFoto {
                image_base64: b64,
                mime: "image/jpeg".into(),
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
            }],
            textos: vec![],
            checks: vec![],
            ink_base64: String::new(),
        };
        let bytes = pdf_bytes(&[page], FlipExportPageSize::default()).unwrap();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("/Width 32"));
        assert!(s.contains("/Height 16"));
        assert!(s.contains("/DCTDecode"));
        assert!(!s.contains("/Width 1600"));
    }
}
