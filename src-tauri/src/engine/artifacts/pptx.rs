//! Minimal PPTX writer: editable titles and bullets, charts as PNG.

use std::io::{Cursor, Write};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::engine::query::{ColumnKind, QueryResult};
use crate::error::{AppError, AppResult};
use crate::storage::artifacts::DeckRecord;

const SLIDE_W: i64 = 12_192_000;
const SLIDE_H: i64 = 6_858_000;

pub fn write(deck: &DeckRecord) -> AppResult<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let slides = &deck.slides;
        zip.start_file("[Content_Types].xml", opts).map_err(zip_err)?;
        zip.write_all(content_types(slides.len()).as_bytes()).map_err(io_err)?;
        zip.start_file("_rels/.rels", opts).map_err(zip_err)?;
        zip.write_all(RELS.as_bytes()).map_err(io_err)?;
        zip.start_file("docProps/core.xml", opts).map_err(zip_err)?;
        zip.write_all(core(&deck.name).as_bytes()).map_err(io_err)?;
        zip.start_file("docProps/app.xml", opts).map_err(zip_err)?;
        zip.write_all(app(slides.len()).as_bytes()).map_err(io_err)?;
        zip.start_file("ppt/presentation.xml", opts).map_err(zip_err)?;
        zip.write_all(presentation(slides.len()).as_bytes()).map_err(io_err)?;
        zip.start_file("ppt/_rels/presentation.xml.rels", opts).map_err(zip_err)?;
        zip.write_all(presentation_rels(slides.len()).as_bytes()).map_err(io_err)?;
        zip.start_file("ppt/slideMasters/slideMaster1.xml", opts).map_err(zip_err)?;
        zip.write_all(MASTER.as_bytes()).map_err(io_err)?;
        zip.start_file("ppt/slideMasters/_rels/slideMaster1.xml.rels", opts).map_err(zip_err)?;
        zip.write_all(MASTER_RELS.as_bytes()).map_err(io_err)?;
        zip.start_file("ppt/slideLayouts/slideLayout1.xml", opts).map_err(zip_err)?;
        zip.write_all(LAYOUT.as_bytes()).map_err(io_err)?;
        zip.start_file("ppt/slideLayouts/_rels/slideLayout1.xml.rels", opts).map_err(zip_err)?;
        zip.write_all(LAYOUT_RELS.as_bytes()).map_err(io_err)?;
        zip.start_file("ppt/theme/theme1.xml", opts).map_err(zip_err)?;
        zip.write_all(THEME.as_bytes()).map_err(io_err)?;
        for (index, slide) in slides.iter().enumerate() {
            let n = index + 1;
            let png = slide.result.as_ref().and_then(chart_png);
            zip.start_file(format!("ppt/slides/slide{n}.xml"), opts).map_err(zip_err)?;
            zip.write_all(slide_xml(&slide.title, &slide.body, png.is_some()).as_bytes()).map_err(io_err)?;
            zip.start_file(format!("ppt/slides/_rels/slide{n}.xml.rels"), opts).map_err(zip_err)?;
            zip.write_all(slide_rels(n, png.is_some()).as_bytes()).map_err(io_err)?;
            if let Some(bytes) = png {
                zip.start_file(format!("ppt/media/image{n}.png"), opts).map_err(zip_err)?;
                zip.write_all(&bytes).map_err(io_err)?;
            }
        }
        zip.finish().map_err(zip_err)?;
    }
    Ok(cursor.into_inner())
}

fn zip_err(error: zip::result::ZipError) -> AppError {
    AppError::Internal(format!("Could not write the deck: {error}"))
}

fn io_err(error: std::io::Error) -> AppError {
    AppError::Internal(format!("Could not write the deck: {error}"))
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn content_types(count: usize) -> String {
    let mut overrides = String::new();
    for n in 1..=count {
        overrides.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Default Extension=\"png\" ContentType=\"image/png\"/>\
         <Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\
         <Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>\
         <Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>\
         <Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
         <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
         <Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>\
         {overrides}</Types>"
    )
}

fn presentation(count: usize) -> String {
    let sld = (1..=count).map(|n| format!("<p:sldId id=\"{}\" r:id=\"rId{n}\"/>", 255 + n)).collect::<Vec<_>>().join("");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
         <p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rIdMaster\"/></p:sldMasterIdLst>\
         <p:sldIdLst>{sld}</p:sldIdLst>\
         <p:sldSz cx=\"{SLIDE_W}\" cy=\"{SLIDE_H}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>"
    )
}

fn presentation_rels(count: usize) -> String {
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rIdMaster\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>",
    );
    for n in 1..=count {
        rels.push_str(&format!(
            "<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{n}.xml\"/>"
        ));
    }
    rels.push_str("</Relationships>");
    rels
}

fn core(title: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
         <dc:title>{}</dc:title><dc:creator>LocalBoard</dc:creator></cp:coreProperties>",
        esc(title)
    )
}

fn app(count: usize) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\">\
         <Application>LocalBoard</Application><Slides>{count}</Slides></Properties>"
    )
}

fn slide_rels(n: usize, image: bool) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>",
    );
    if image {
        xml.push_str(&format!(
            "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/image{n}.png\"/>"
        ));
    }
    xml.push_str("</Relationships>");
    xml
}

fn slide_xml(title: &str, body: &str, image: bool) -> String {
    let bullets = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let line = line.trim_start_matches(['-', '•', '*']).trim();
            format!(
                "<a:p><a:pPr marL=\"0\" indent=\"0\"/><a:r><a:rPr lang=\"en-US\" sz=\"1800\"/><a:t>{}</a:t></a:r></a:p>",
                esc(line)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let bullets = if bullets.is_empty() {
        "<a:p><a:r><a:rPr lang=\"en-US\" sz=\"1800\"/><a:t></a:t></a:r></a:p>".to_owned()
    } else {
        bullets
    };
    let picture = if image {
        format!(
            "<p:pic><p:nvPicPr><p:cNvPr id=\"4\" name=\"Chart\"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId2\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"6400800\" y=\"1371600\"/><a:ext cx=\"5181600\" cy=\"4114800\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>"
        )
    } else {
        String::new()
    };
    let body_w = if image { 5_791_200 } else { 11_277_600 };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
         <p:cSld><p:spTree>\
         <p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
         <p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>\
         <p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Title\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr>\
         <p:spPr><a:xfrm><a:off x=\"457200\" y=\"274320\"/><a:ext cx=\"{body_w}\" cy=\"914400\"/></a:xfrm></p:spPr>\
         <p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\" sz=\"3200\" b=\"1\"/><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>\
         <p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Body\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr><p:ph type=\"body\"/></p:nvPr></p:nvSpPr>\
         <p:spPr><a:xfrm><a:off x=\"457200\" y=\"1371600\"/><a:ext cx=\"{body_w}\" cy=\"4800600\"/></a:xfrm></p:spPr>\
         <p:txBody><a:bodyPr wrap=\"square\"/><a:lstStyle/>{bullets}</p:txBody></p:sp>\
         {picture}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
        esc(title)
    )
}

fn chart_png(result: &QueryResult) -> Option<Vec<u8>> {
    let measure = result.columns.iter().position(|column| column.kind == ColumnKind::Number)?;
    let values: Vec<f32> = result.rows.iter().take(16).filter_map(|row| row.get(measure).and_then(as_f32)).collect();
    if values.is_empty() {
        return None;
    }
    Some(bar_png(&values))
}

fn bar_png(values: &[f32]) -> Vec<u8> {
    const W: u32 = 640;
    const H: u32 = 360;
    let mut pixels = vec![255u8; (W * H * 3) as usize];
    let max = values.iter().map(|v| v.abs()).fold(1.0_f32, f32::max);
    let bar_w = ((W as usize - 40) / values.len().max(1)).min(36);
    for (index, value) in values.iter().enumerate() {
        let h = ((value.abs() / max) * (H as f32 - 40.0)).max(2.0) as u32;
        let x0 = 20 + index * (bar_w + 4);
        let y0 = H - 20 - h;
        for y in y0..H.saturating_sub(20) {
            for x in x0 as u32..(x0 as u32 + bar_w as u32).min(W) {
                let i = ((y * W + x) * 3) as usize;
                if i + 2 < pixels.len() {
                    pixels[i] = 201;
                    pixels[i + 1] = 98;
                    pixels[i + 2] = 64;
                }
            }
        }
    }
    png_rgb(W, H, &pixels)
}

fn png_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(((width * 3 + 1) * height) as usize);
    for y in 0..height {
        raw.push(0);
        let start = (y * width * 3) as usize;
        raw.extend_from_slice(&rgb[start..start + (width * 3) as usize]);
    }
    let mut deflated = Vec::new();
    {
        let mut encoder = ZlibEncoder::new(&mut deflated, Compression::fast());
        let _ = encoder.write_all(&raw);
        let _ = encoder.finish();
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    chunk(&mut out, b"IHDR", &{
        let mut hdr = Vec::new();
        hdr.extend_from_slice(&width.to_be_bytes());
        hdr.extend_from_slice(&height.to_be_bytes());
        hdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        hdr
    });
    chunk(&mut out, b"IDAT", &deflated);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc_data = Vec::with_capacity(4 + data.len());
    crc_data.extend_from_slice(kind);
    crc_data.extend_from_slice(data);
    out.extend_from_slice(&crc_data);
    out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn as_f32(value: &Value) -> Option<f32> {
    match value {
        Value::Number(number) => number.as_f64().map(|n| n as f32),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/>\
</Relationships>";

const MASTER_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"../theme/theme1.xml\"/>\
</Relationships>";

const LAYOUT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"../slideMasters/slideMaster1.xml\"/>\
</Relationships>";

const MASTER: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="FFFAF5"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>
<p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp>
</p:spTree></p:cSld>
<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>
</p:sldMaster>"#;

const LAYOUT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1">
<p:cSld><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
</p:spTree></p:cSld>
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#;

const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="LocalBoard">
<a:themeElements>
<a:clrScheme name="LocalBoard">
<a:dk1><a:srgbClr val="1C1917"/></a:dk1><a:lt1><a:srgbClr val="FFFAF5"/></a:lt1>
<a:dk2><a:srgbClr val="44403C"/></a:dk2><a:lt2><a:srgbClr val="E7E5E4"/></a:lt2>
<a:accent1><a:srgbClr val="C96240"/></a:accent1><a:accent2><a:srgbClr val="78716C"/></a:accent2>
<a:accent3><a:srgbClr val="A8A29E"/></a:accent3><a:accent4><a:srgbClr val="D6D3D1"/></a:accent4>
<a:accent5><a:srgbClr val="292524"/></a:accent5><a:accent6><a:srgbClr val="FAFAF9"/></a:accent6>
<a:hlink><a:srgbClr val="C96240"/></a:hlink><a:folHlink><a:srgbClr val="78716C"/></a:folHlink>
</a:clrScheme>
<a:fontScheme name="LocalBoard">
<a:majorFont><a:latin typeface="Georgia"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>
<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>
</a:fontScheme>
<a:fmtScheme name="LocalBoard">
<a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>
<a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>
<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst>
</a:fmtScheme>
</a:themeElements>
</a:theme>"#;
