//! Turns a dropped PDF, Word, Markdown or text file into chunks the analyst can retrieve.
//! The extracted text is stored; the original file is not kept.

use std::path::Path;

use crate::error::{AppError, AppResult};

const MAX_BYTES: u64 = 20 * 1024 * 1024;
const CHUNK_CHARS: usize = 900;
// Enough for a full MAX_TEXT_CHARS document (200k / 900 chars is about 225 chunks).
const MAX_CHUNKS: usize = 300;
const MAX_TEXT_CHARS: usize = 200_000;

pub fn read_knowledge(path: &Path) -> AppResult<Vec<String>> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(AppError::invalid("Choose a file, not a folder"));
    }
    if meta.len() > MAX_BYTES {
        return Err(AppError::invalid("Knowledge files are limited to 20 MB"));
    }
    let bytes = std::fs::read(path)?;
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
    let text = match extension.as_str() {
        "txt" | "md" => String::from_utf8_lossy(&bytes).into_owned(),
        "pdf" => pdf_extract::extract_text_from_mem(&bytes)
            .map_err(|error| AppError::invalid(format!("Could not read this PDF: {error}")))?,
        "docx" => docx_text(&bytes)?,
        _ => return Err(AppError::invalid("Knowledge files are PDF, Word (.docx), Markdown or text")),
    };
    let text = clip_chars(&text, MAX_TEXT_CHARS);
    let chunks = chunk(&text);
    if chunks.is_empty() {
        return Err(AppError::invalid("That file has no text to learn from"));
    }
    Ok(chunks)
}

fn docx_text(bytes: &[u8]) -> AppResult<String> {
    let docx = docx_rs::read_docx(bytes).map_err(|error| AppError::invalid(format!("Could not read this Word file: {error}")))?;
    let mut out = String::new();
    for child in &docx.document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(paragraph) => {
                push_paragraph(&mut out, paragraph);
                out.push_str("\n\n");
            }
            docx_rs::DocumentChild::Table(table) => {
                for row in &table.rows {
                    let docx_rs::TableChild::TableRow(row) = row;
                    for cell in &row.cells {
                        let docx_rs::TableRowChild::TableCell(cell) = cell;
                        for child in &cell.children {
                            if let docx_rs::TableCellContent::Paragraph(paragraph) = child {
                                push_paragraph(&mut out, paragraph);
                                out.push(' ');
                            }
                        }
                    }
                    out.push_str("\n\n");
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn push_paragraph(out: &mut String, paragraph: &docx_rs::Paragraph) {
    for child in &paragraph.children {
        if let docx_rs::ParagraphChild::Run(run) = child {
            for child in &run.children {
                if let docx_rs::RunChild::Text(text) = child {
                    out.push_str(&text.text);
                }
            }
        }
    }
}

fn chunk(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
        if paragraph.is_empty() {
            continue;
        }
        if !current.is_empty() && current.chars().count() + paragraph.chars().count() + 2 > CHUNK_CHARS {
            push_chunk(&mut chunks, &mut current);
            if chunks.len() == MAX_CHUNKS {
                return chunks;
            }
        }
        if paragraph.chars().count() > CHUNK_CHARS {
            if !current.is_empty() {
                push_chunk(&mut chunks, &mut current);
            }
            let chars: Vec<char> = paragraph.chars().collect();
            let mut start = 0;
            while start < chars.len() {
                if chunks.len() == MAX_CHUNKS {
                    return chunks;
                }
                let mut end = (start + CHUNK_CHARS).min(chars.len());
                if end < chars.len() {
                    // Cut at the last space in the second half of the window so words stay whole.
                    if let Some(space) = chars[start + CHUNK_CHARS / 2..end].iter().rposition(|c| *c == ' ') {
                        end = start + CHUNK_CHARS / 2 + space;
                    }
                }
                let piece: String = chars[start..end].iter().collect();
                if !piece.trim().is_empty() {
                    chunks.push(piece.trim().to_owned());
                }
                start = end;
            }
        } else {
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(&paragraph);
        }
    }
    if !current.trim().is_empty() && chunks.len() < MAX_CHUNKS {
        chunks.push(current);
    }
    chunks
}

fn push_chunk(chunks: &mut Vec<String>, current: &mut String) {
    if !current.trim().is_empty() {
        chunks.push(std::mem::take(current));
    }
}

fn clip_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars { text.to_owned() } else { text.chars().take(max_chars).collect() }
}
