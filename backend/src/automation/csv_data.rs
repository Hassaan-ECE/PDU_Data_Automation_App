use std::fs;
use std::path::{Path, PathBuf};

use csv::ReaderBuilder;
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Error)]
pub enum CsvDataError {
    #[error("CSV file could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error("CSV parser failed for {path}: {source}")]
    Csv {
        path: String,
        #[source]
        source: csv::Error,
    },
    #[error("CSV file has no data rows: {0}")]
    Empty(String),
    #[error("{label} source column {column} is missing from {file}")]
    MissingColumn {
        file: String,
        label: String,
        column: String,
    },
    #[error("{label} source column {column} is blank in {file}")]
    BlankValue {
        file: String,
        label: String,
        column: String,
    },
    #[error("{label} source column {column} value '{value}' is not numeric in {file}")]
    NonnumericValue {
        file: String,
        label: String,
        column: String,
        value: String,
    },
    #[error("invalid Excel column reference '{0}'")]
    InvalidColumn(String),
}

pub type CsvResult<T> = Result<T, CsvDataError>;

impl CsvDataError {
    pub fn is_transient_file_access(&self) -> bool {
        match self {
            CsvDataError::Io(source) => is_transient_file_access_error(source),
            CsvDataError::Csv { source, .. } => match source.kind() {
                csv::ErrorKind::Io(source) => is_transient_file_access_error(source),
                _ => false,
            },
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CsvTable {
    path: PathBuf,
    rows: Vec<Vec<String>>,
}

impl CsvTable {
    pub fn read(path: &Path) -> CsvResult<Self> {
        let delimiter = sniff_delimiter(path)?;
        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .delimiter(delimiter)
            .from_path(path)
            .map_err(|source| CsvDataError::Csv {
                path: path.display().to_string(),
                source,
            })?;

        let rows = reader
            .records()
            .map(|record| {
                record
                    .map(|record| record.iter().map(ToOwned::to_owned).collect::<Vec<_>>())
                    .map_err(|source| CsvDataError::Csv {
                        path: path.display().to_string(),
                        source,
                    })
            })
            .collect::<CsvResult<Vec<_>>>()?;

        Ok(Self {
            path: path.to_path_buf(),
            rows,
        })
    }

    pub fn first_data_row_after_header(&self) -> CsvResult<&[String]> {
        self.rows
            .iter()
            .skip(1)
            .find(|row| row.iter().any(|value| !value.trim().is_empty()))
            .map(Vec::as_slice)
            .ok_or_else(|| CsvDataError::Empty(self.path.display().to_string()))
    }

    pub fn last_data_row_after_header(&self) -> CsvResult<&[String]> {
        self.rows
            .iter()
            .skip(1)
            .rev()
            .find(|row| row.iter().any(|value| !value.trim().is_empty()))
            .map(Vec::as_slice)
            .ok_or_else(|| CsvDataError::Empty(self.path.display().to_string()))
    }

    pub fn last_data_row(&self) -> CsvResult<&[String]> {
        self.rows
            .iter()
            .rev()
            .find(|row| row.iter().any(|value| !value.trim().is_empty()))
            .map(Vec::as_slice)
            .ok_or_else(|| CsvDataError::Empty(self.path.display().to_string()))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn required_number(row: &[String], column: &str, file: &Path, label: &str) -> CsvResult<f64> {
    let index = excel_col_to_index(column)?;
    let Some(raw) = row.get(index) else {
        return Err(CsvDataError::MissingColumn {
            file: file.display().to_string(),
            label: label.to_string(),
            column: column.to_string(),
        });
    };

    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Err(CsvDataError::BlankValue {
            file: file.display().to_string(),
            label: label.to_string(),
            column: column.to_string(),
        });
    }

    let normalized = trimmed.replace(',', "");
    normalized
        .parse::<f64>()
        .map_err(|_| CsvDataError::NonnumericValue {
            file: file.display().to_string(),
            label: label.to_string(),
            column: column.to_string(),
            value: trimmed.to_string(),
        })
}

pub fn excel_col_to_index(column: &str) -> CsvResult<usize> {
    let trimmed = column.trim().to_ascii_uppercase();

    if trimmed.is_empty() || trimmed.len() > 3 {
        return Err(CsvDataError::InvalidColumn(column.to_string()));
    }

    let mut index = 0usize;

    for byte in trimmed.bytes() {
        if !byte.is_ascii_uppercase() {
            return Err(CsvDataError::InvalidColumn(column.to_string()));
        }

        index = index * 26 + usize::from(byte - b'A' + 1);
    }

    Ok(index - 1)
}

pub fn find_latest_csv(root: &Path, step: u16, required_fragments: &[String]) -> Option<PathBuf> {
    let step_tag = format!("_STEP{step}_");
    let required_fragments = required_fragments
        .iter()
        .map(|fragment| fragment.to_ascii_uppercase())
        .collect::<Vec<_>>();

    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
        })
        .filter(|path| {
            let Some(file_name) = path.file_name().and_then(|file_name| file_name.to_str()) else {
                return false;
            };

            let upper = file_name.to_ascii_uppercase();
            upper.contains(&step_tag)
                && required_fragments
                    .iter()
                    .all(|fragment| upper.contains(fragment))
        })
        .filter_map(|path| {
            let modified = path
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()?;
            Some((path, modified))
        })
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

pub fn detected_steps(root: &Path) -> Vec<(u16, PathBuf)> {
    let step_re = regex::Regex::new(r"_STEP(\d+)_").expect("step regex is valid");
    let mut found = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
        {
            continue;
        }

        let Some(captures) = step_re.captures(file_name) else {
            continue;
        };

        let Some(step) = captures
            .get(1)
            .and_then(|match_| match_.as_str().parse::<u16>().ok())
        else {
            continue;
        };

        found.push((step, path.to_path_buf()));
    }

    found
}

pub fn round_to(value: f64, digits: u32) -> f64 {
    let factor = 10_f64.powi(digits as i32);
    (value * factor).round() / factor
}

fn sniff_delimiter(path: &Path) -> CsvResult<u8> {
    let sample = fs::read_to_string(path)?;
    let sample = sample.chars().take(4096).collect::<String>();
    let candidates = [b',', b';', b'\t', b'|'];

    let delimiter = candidates
        .iter()
        .copied()
        .max_by_key(|candidate| sample.bytes().filter(|byte| byte == candidate).count())
        .unwrap_or(b',');

    if sample.bytes().filter(|byte| *byte == delimiter).count() == 0 {
        Ok(b',')
    } else {
        Ok(delimiter)
    }
}

fn is_transient_file_access_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33))
        || matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    #[test]
    fn excel_columns_are_zero_based() {
        assert_eq!(excel_col_to_index("A").unwrap(), 0);
        assert_eq!(excel_col_to_index("Z").unwrap(), 25);
        assert_eq!(excel_col_to_index("AA").unwrap(), 26);
        assert_eq!(excel_col_to_index("EG").unwrap(), 136);
    }

    #[test]
    fn round_to_uses_decimal_places() {
        assert_eq!(round_to(12.3456, 2), 12.35);
        assert_eq!(round_to(12.344, 2), 12.34);
    }

    #[test]
    fn sharing_violation_is_transient_file_access() {
        let error = CsvDataError::Io(std::io::Error::from_raw_os_error(32));

        assert!(error.is_transient_file_access());
    }

    #[test]
    fn would_block_is_transient_file_access() {
        let error = CsvDataError::Io(std::io::Error::from(ErrorKind::WouldBlock));

        assert!(error.is_transient_file_access());
    }

    #[test]
    fn malformed_values_are_not_transient_file_access() {
        let error = CsvDataError::NonnumericValue {
            file: "sample.csv".to_string(),
            label: "Current".to_string(),
            column: "A".to_string(),
            value: "bad".to_string(),
        };

        assert!(!error.is_transient_file_access());
    }
}
