use std::collections::HashSet;
use std::path::{Path, PathBuf};

use calamine::{open_workbook_auto, Data, Reader};
use serde::{Deserialize, Serialize};

use super::{ConnectorError, ConnectorKind, DuckDbSource, Locator, SourceTable};
use crate::engine::duck::{sql_string, Analytics};

const MAX_FOLDER_DEPTH: usize = 8;
const MAX_FOLDER_FILES: usize = 2_000;

/// Persisted in `connections.config_json` for file and folder connections.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PathConfig {
    pub path: PathBuf,
    #[serde(default)]
    pub recursive: bool,
}

pub fn source_for(kind: ConnectorKind, config: &PathConfig) -> Result<Box<dyn DuckDbSource>, ConnectorError> {
    match kind {
        ConnectorKind::File => Ok(Box::new(FileSource::new(config.path.clone()))),
        ConnectorKind::Folder => Ok(Box::new(FolderSource::new(config.path.clone(), config.recursive))),
        other => Err(ConnectorError::Unsupported(format!("{} is not a file source", other.as_str()))),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormat {
    Csv,
    Tsv,
    Parquet,
    Json,
    Xlsx,
}

impl FileFormat {
    pub const EXTENSIONS: [&'static str; 6] = ["csv", "tsv", "parquet", "json", "jsonl", "xlsx"];

    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_lowercase();
        match extension.as_str() {
            "csv" => Some(Self::Csv),
            "tsv" => Some(Self::Tsv),
            "parquet" => Some(Self::Parquet),
            "json" | "jsonl" => Some(Self::Json),
            "xlsx" => Some(Self::Xlsx),
            _ => None,
        }
    }
}

/// A single data file the user dropped or chose.
pub struct FileSource {
    path: PathBuf,
}

impl FileSource {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl DuckDbSource for FileSource {
    fn discover(&self) -> Result<Vec<SourceTable>, ConnectorError> {
        discover_file(&self.path)
    }

    fn register(&self, duck: &Analytics, table: &SourceTable, relation: &str) -> Result<(), ConnectorError> {
        register(duck, table, relation)
    }
}

/// A watched folder: every supported file inside becomes one or more tables.
pub struct FolderSource {
    path: PathBuf,
    recursive: bool,
}

impl FolderSource {
    pub fn new(path: PathBuf, recursive: bool) -> Self {
        Self { path, recursive }
    }
}

impl DuckDbSource for FolderSource {
    fn discover(&self) -> Result<Vec<SourceTable>, ConnectorError> {
        if !self.path.is_dir() {
            return Err(ConnectorError::Unavailable(format!("Folder is missing: {}", self.path.display())));
        }
        let mut files = Vec::new();
        collect_files(&self.path, self.recursive, 0, &mut files)?;
        files.sort();
        let mut tables = Vec::new();
        for file in files {
            match discover_file(&file) {
                Ok(found) => tables.extend(found),
                Err(error) => tracing::warn!("skipping {}: {error}", file.display()),
            }
        }
        Ok(tables)
    }

    fn register(&self, duck: &Analytics, table: &SourceTable, relation: &str) -> Result<(), ConnectorError> {
        register(duck, table, relation)
    }
}

fn collect_files(dir: &Path, recursive: bool, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), ConnectorError> {
    let entries = std::fs::read_dir(dir).map_err(|error| ConnectorError::Unavailable(format!("{}: {error}", dir.display())))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name.starts_with("~$") {
            continue;
        }
        if path.is_dir() {
            if recursive && depth < MAX_FOLDER_DEPTH {
                collect_files(&path, recursive, depth + 1, out)?;
            }
        } else if FileFormat::from_path(&path).is_some() {
            if out.len() >= MAX_FOLDER_FILES {
                return Err(ConnectorError::Unsupported(format!(
                    "Folder has more than {MAX_FOLDER_FILES} data files; choose a narrower folder"
                )));
            }
            out.push(path);
        }
    }
    Ok(())
}

pub fn discover_file(path: &Path) -> Result<Vec<SourceTable>, ConnectorError> {
    if !path.is_file() {
        return Err(ConnectorError::Unavailable(format!("File is missing: {}", path.display())));
    }
    let format = FileFormat::from_path(path)
        .ok_or_else(|| ConnectorError::Unsupported(format!("Unsupported file type: {}", path.display())))?;
    let source = path.to_string_lossy().into_owned();
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("table").to_owned();
    if format != FileFormat::Xlsx {
        return Ok(vec![SourceTable {
            source_ref: source,
            suggested_name: stem,
            locator: Locator::File { path: path.to_path_buf(), format },
        }]);
    }
    let mut workbook = open_workbook_auto(path).map_err(|error| ConnectorError::Failed(error.to_string()))?;
    let sheets = workbook.sheet_names();
    let populated: Vec<String> = sheets
        .into_iter()
        .filter(|sheet| workbook.worksheet_range(sheet).is_ok_and(|range| !range.is_empty()))
        .collect();
    if populated.is_empty() {
        return Err(ConnectorError::Failed("The workbook has no populated worksheets".into()));
    }
    let single = populated.len() == 1;
    Ok(populated
        .into_iter()
        .map(|sheet| SourceTable {
            source_ref: format!("{source}#{sheet}"),
            suggested_name: if single { stem.clone() } else { format!("{stem}_{sheet}") },
            locator: Locator::Sheet { path: path.to_path_buf(), sheet },
        })
        .collect())
}

fn register(duck: &Analytics, table: &SourceTable, relation: &str) -> Result<(), ConnectorError> {
    match &table.locator {
        Locator::File { path, format } => {
            let literal = sql_string(path.to_str().ok_or_else(|| ConnectorError::Unsupported("Path is not valid UTF-8".into()))?);
            let reader = match format {
                FileFormat::Csv => format!("read_csv_auto({literal})"),
                FileFormat::Tsv => format!("read_csv_auto({literal}, delim = '\t')"),
                FileFormat::Parquet => format!("read_parquet({literal})"),
                FileFormat::Json => format!("read_json_auto({literal})"),
                FileFormat::Xlsx => return Err(ConnectorError::Unsupported("XLSX files register per sheet".into())),
            };
            duck.create_view(relation, &reader)?;
        }
        Locator::Sheet { path, sheet } => materialize_sheet(duck, path, sheet, relation)?,
    }
    Ok(())
}

fn materialize_sheet(duck: &Analytics, path: &Path, sheet: &str, relation: &str) -> Result<(), ConnectorError> {
    let mut workbook = open_workbook_auto(path).map_err(|error| ConnectorError::Failed(error.to_string()))?;
    let range = workbook
        .worksheet_range(sheet)
        .map_err(|error| ConnectorError::Failed(format!("Sheet {sheet}: {error}")))?;
    let mut rows = range.rows();
    let header = rows.next().ok_or_else(|| ConnectorError::Failed(format!("Sheet {sheet} needs a header row")))?;
    let names = header_names(header).map_err(|message| ConnectorError::Failed(format!("Sheet {sheet}: {message}")))?;
    let width = names.len();
    duck.replace_text_table(relation, &names, rows.map(|row| (0..width).map(|index| row.get(index).and_then(cell_text)).collect()))?;
    Ok(())
}

fn cell_text(cell: &Data) -> Option<String> {
    let text = match cell {
        Data::Empty | Data::Error(_) => return None,
        Data::String(value) => value.trim().to_owned(),
        Data::Int(value) => value.to_string(),
        Data::Float(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        Data::DateTime(value) => excel_serial_text(value.as_f64())?,
        Data::DateTimeIso(value) | Data::DurationIso(value) => value.clone(),
    };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Excel stores dates as serial days since 1899-12-30 (1900 system, with its phantom leap day).
/// Values under one day are times of day. Renders ISO text so DuckDB can parse the column later.
fn excel_serial_text(serial: f64) -> Option<String> {
    if !serial.is_finite() {
        return None;
    }
    if (0.0..1.0).contains(&serial) {
        let seconds = (serial * 86_400.0).round() as u32;
        return Some(format!("{:02}:{:02}:{:02}", seconds / 3600 % 24, seconds / 60 % 60, seconds % 60));
    }
    let days = if serial >= 60.0 { serial } else { serial + 1.0 };
    let millis = (days * 86_400_000.0).round() as i64;
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30)?.and_hms_opt(0, 0, 0)?;
    let moment = epoch.checked_add_signed(chrono::Duration::milliseconds(millis))?;
    Some(if moment.time() == chrono::NaiveTime::MIN {
        moment.format("%Y-%m-%d").to_string()
    } else {
        moment.format("%Y-%m-%d %H:%M:%S").to_string()
    })
}

fn header_names(row: &[Data]) -> Result<Vec<String>, String> {
    let mut names = Vec::with_capacity(row.len());
    let mut seen = HashSet::new();
    let mut labeled = false;
    for (index, cell) in row.iter().enumerate() {
        let base = match cell_text(cell) {
            Some(value) => {
                labeled = true;
                value
            }
            None => format!("column_{}", index + 1),
        };
        let mut name = base.clone();
        let mut suffix = 2u32;
        while !seen.insert(name.clone()) {
            name = format!("{base}_{suffix}");
            suffix += 1;
        }
        names.push(name);
    }
    if !labeled {
        return Err("a header row is required".into());
    }
    Ok(names)
}
