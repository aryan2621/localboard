//! Branded PDF for a report: narrative, figures, tables and a SQL appendix.

use serde_json::Value;

use super::{BRAND_NAME, BRAND_HEX};
use crate::engine::query::QueryResult;
use crate::error::{AppError, AppResult};
use crate::storage::artifacts::ReportRecord;

const PAGE_W: f32 = 612.0;
const PAGE_H: f32 = 792.0;
const MARGIN: f32 = 48.0;
const BOTTOM: f32 = 56.0;

pub fn write(report: &ReportRecord) -> AppResult<Vec<u8>> {
    let mut doc = Doc::new(brand_rgb(&report.theme), brand_name(&report.theme));
    doc.header(&report.name);
    doc.heading(&report.name, 22.0);
    for block in &report.blocks {
        if block.kind == "narrative" {
            if !block.title.trim().is_empty() {
                doc.heading(&block.title, 14.0);
            }
            doc.paragraph(&block.body);
            continue;
        }
        if !block.title.trim().is_empty() {
            doc.heading(&block.title, 14.0);
        }
        if !block.body.trim().is_empty() {
            doc.paragraph(&block.body);
        }
        if let Some(result) = &block.result {
            if block.kind == "table" || block.chart.as_ref().is_some_and(|chart| matches!(chart.kind, crate::engine::analyst::chart::ChartKind::Table)) {
                doc.table(result);
            } else {
                doc.figure(result);
                doc.table(result);
            }
        }
    }
    let sqls: Vec<(&str, &str)> = report
        .blocks
        .iter()
        .filter_map(|block| block.sql.as_deref().map(|sql| (block.title.as_str(), sql)))
        .filter(|(_, sql)| !sql.trim().is_empty())
        .collect();
    if !sqls.is_empty() {
        doc.heading("SQL appendix", 14.0);
        for (title, sql) in sqls {
            doc.heading(title, 11.0);
            doc.pre(sql);
        }
    }
    doc.finish().map_err(|error| AppError::Internal(error))
}

fn brand_name(theme: &Value) -> String {
    theme.get("name").and_then(Value::as_str).filter(|name| !name.trim().is_empty()).unwrap_or(BRAND_NAME).to_owned()
}

fn brand_rgb(theme: &Value) -> (f32, f32, f32) {
    let hex = theme.get("brand").and_then(Value::as_str).unwrap_or(BRAND_HEX);
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (u8::from_str_radix(&hex[0..2], 16), u8::from_str_radix(&hex[2..4], 16), u8::from_str_radix(&hex[4..6], 16)) {
            return (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
        }
    }
    (0.788, 0.384, 0.251)
}

struct Doc {
    brand: (f32, f32, f32),
    brand_name: String,
    pages: Vec<String>,
    current: String,
    y: f32,
}

impl Doc {
    fn new(brand: (f32, f32, f32), brand_name: String) -> Self {
        Self { brand, brand_name, pages: Vec::new(), current: String::new(), y: PAGE_H - MARGIN }
    }

    fn header(&mut self, title: &str) {
        let (r, g, b) = self.brand;
        self.current.push_str(&format!(
            "q {r:.3} {g:.3} {b:.3} rg 0 {top} {PAGE_W} 36 re f Q\n",
            top = PAGE_H - 36.0
        ));
        let label = self.brand_name.clone();
        self.text(MARGIN, PAGE_H - 24.0, 11.0, true, 1.0, 1.0, 1.0, &label);
        self.text(PAGE_W - MARGIN - 180.0, PAGE_H - 24.0, 9.0, false, 1.0, 1.0, 1.0, &clip(title, 40));
        self.y = PAGE_H - 56.0;
    }

    fn ensure(&mut self, need: f32) {
        if self.y - need < BOTTOM {
            self.new_page();
        }
    }

    fn new_page(&mut self) {
        self.pages.push(std::mem::take(&mut self.current));
        self.y = PAGE_H - MARGIN;
        self.header("");
    }

    fn heading(&mut self, text: &str, size: f32) {
        self.ensure(size + 16.0);
        self.y -= size + 10.0;
        self.text(MARGIN, self.y, size, true, 0.11, 0.10, 0.09, text);
        self.y -= 8.0;
    }

    fn paragraph(&mut self, text: &str) {
        for line in wrap(text, 92) {
            self.ensure(16.0);
            self.y -= 14.0;
            self.text(MARGIN, self.y, 10.0, false, 0.28, 0.25, 0.22, &line);
        }
        self.y -= 6.0;
    }

    fn pre(&mut self, text: &str) {
        for line in wrap(text, 96) {
            self.ensure(12.0);
            self.y -= 11.0;
            self.text(MARGIN, self.y, 8.0, false, 0.25, 0.24, 0.22, &line);
        }
        self.y -= 8.0;
    }

    fn table(&mut self, result: &QueryResult) {
        let cols = result.columns.len().min(6);
        if cols == 0 {
            return;
        }
        let width = (PAGE_W - MARGIN * 2.0) / cols as f32;
        self.ensure(22.0);
        self.y -= 16.0;
        for (index, column) in result.columns.iter().take(cols).enumerate() {
            self.text(MARGIN + width * index as f32, self.y, 8.0, true, 0.47, 0.44, 0.42, &clip(&column.name, 18));
        }
        for row in result.rows.iter().take(24) {
            self.ensure(14.0);
            self.y -= 12.0;
            for (index, value) in row.iter().take(cols).enumerate() {
                self.text(MARGIN + width * index as f32, self.y, 8.0, false, 0.11, 0.10, 0.09, &clip(&cell(value), 18));
            }
        }
        if result.rows.len() > 24 || result.truncated {
            self.y -= 12.0;
            self.text(MARGIN, self.y, 8.0, false, 0.47, 0.44, 0.42, "Showing the first rows.");
        }
        self.y -= 10.0;
    }

    fn figure(&mut self, result: &QueryResult) {
        let Some(measure) = result.columns.iter().position(|column| column.kind == crate::engine::query::ColumnKind::Number) else {
            return;
        };
        let values: Vec<f32> = result
            .rows
            .iter()
            .take(16)
            .filter_map(|row| row.get(measure).and_then(as_f32))
            .collect();
        if values.is_empty() {
            return;
        }
        let max = values.iter().map(|v| v.abs()).fold(1.0_f32, f32::max);
        self.ensure(90.0);
        self.y -= 80.0;
        let base = self.y;
        let (r, g, b) = self.brand;
        let bar_w = ((PAGE_W - MARGIN * 2.0) / values.len().max(1) as f32).min(22.0);
        self.current.push_str("q\n");
        for (index, value) in values.iter().enumerate() {
            let h = (value.abs() / max * 70.0).max(2.0);
            let x = MARGIN + index as f32 * (bar_w + 4.0);
            self.current.push_str(&format!("{r:.3} {g:.3} {b:.3} rg {x:.1} {base:.1} {bar_w:.1} {h:.1} re f\n"));
        }
        self.current.push_str("Q\n");
        self.y -= 8.0;
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, x: f32, y: f32, size: f32, bold: bool, r: f32, g: f32, b: f32, text: &str) {
        let font = if bold { "F2" } else { "F1" };
        self.current.push_str(&format!(
            "BT /{font} {size:.1} Tf {r:.3} {g:.3} {b:.3} rg 1 0 0 1 {x:.1} {y:.1} Tm ({}) Tj ET\n",
            pdf_str(text)
        ));
    }

    fn finish(mut self) -> Result<Vec<u8>, String> {
        if !self.current.is_empty() || self.pages.is_empty() {
            self.pages.push(self.current);
        }
        if self.pages.is_empty() {
            self.pages.push(String::new());
        }
        assemble(&self.pages)
    }
}

fn assemble(pages: &[String]) -> Result<Vec<u8>, String> {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    objects.push(Vec::new());
    let catalog_id = 1;
    let pages_id = 2;
    let font1 = 3;
    let font2 = 4;
    objects.resize(5, Vec::new());
    objects[font1] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec();
    objects[font2] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec();

    let mut page_ids = Vec::new();
    for content in pages {
        let content_id = objects.len();
        let stream = content.as_bytes();
        objects.push(format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes());
        objects[content_id].extend_from_slice(stream);
        objects[content_id].extend_from_slice(b"\nendstream");
        let page_id = objects.len();
        objects.push(
            format!(
                "<< /Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] \
                 /Contents {content_id} 0 R /Resources << /Font << /F1 {font1} 0 R /F2 {font2} 0 R >> >> >>"
            )
            .into_bytes(),
        );
        page_ids.push(page_id);
    }
    let kids = page_ids.iter().map(|id| format!("{id} 0 R")).collect::<Vec<_>>().join(" ");
    objects[pages_id] = format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", page_ids.len()).into_bytes();
    objects[catalog_id] = format!("<< /Type /Catalog /Pages {pages_id} 0 R >>").into_bytes();

    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = vec![0usize];
    for (index, object) in objects.iter().enumerate().skip(1) {
        offsets.push(out.len());
        out.extend_from_slice(format!("{index} 0 obj\n").as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len()).as_bytes());
    if out.len() < 8 {
        return Err("Could not write the PDF.".into());
    }
    Ok(out)
}

fn pdf_str(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '(' => "\\(".to_owned(),
            ')' => "\\)".to_owned(),
            '\\' => "\\\\".to_owned(),
            c if c.is_ascii() && !c.is_control() => c.to_string(),
            c => match winansi(c) {
                Some(code) => format!("\\{code:03o}"),
                None => "?".to_owned(),
            },
        })
        .collect()
}

/// Maps a character to its WinAnsi (Windows-1252) code so Latin-1 and common punctuation render.
fn winansi(c: char) -> Option<u8> {
    let code = u32::from(c);
    match code {
        0xA0..=0xFF => Some(code as u8),
        _ => Some(match c {
            '\u{20AC}' => 0x80,
            '\u{201A}' => 0x82,
            '\u{0192}' => 0x83,
            '\u{201E}' => 0x84,
            '\u{2026}' => 0x85,
            '\u{2020}' => 0x86,
            '\u{2021}' => 0x87,
            '\u{02C6}' => 0x88,
            '\u{2030}' => 0x89,
            '\u{0160}' => 0x8A,
            '\u{2039}' => 0x8B,
            '\u{0152}' => 0x8C,
            '\u{017D}' => 0x8E,
            '\u{2018}' => 0x91,
            '\u{2019}' => 0x92,
            '\u{201C}' => 0x93,
            '\u{201D}' => 0x94,
            '\u{2022}' => 0x95,
            '\u{2013}' => 0x96,
            '\u{2014}' => 0x97,
            '\u{02DC}' => 0x98,
            '\u{2122}' => 0x99,
            '\u{0161}' => 0x9A,
            '\u{203A}' => 0x9B,
            '\u{0153}' => 0x9C,
            '\u{017E}' => 0x9E,
            '\u{0178}' => 0x9F,
            _ => return None,
        }),
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current = word.to_owned();
            } else if current.len() + 1 + word.len() > width {
                lines.push(std::mem::take(&mut current));
                current = word.to_owned();
            } else {
                current.push(' ');
                current.push_str(word);
            }
        }
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

fn cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn as_f32(value: &Value) -> Option<f32> {
    match value {
        Value::Number(number) => number.as_f64().map(|n| n as f32),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}
