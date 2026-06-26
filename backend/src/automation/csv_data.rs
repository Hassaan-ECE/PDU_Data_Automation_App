use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    #[error("CSV file is still changing and is not ready to process: {0}")]
    Unstable(String),
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CsvFileSnapshot {
    len: u64,
    modified_ms: Option<u128>,
}

pub fn wait_for_stable_csv(path: &Path, stable_for: Duration, max_wait: Duration) -> CsvResult<()> {
    if stable_for.is_zero() {
        csv_file_snapshot(path)?;
        return Ok(());
    }

    let started = Instant::now();
    let mut previous = csv_file_snapshot(path)?;
    let initial_age = csv_snapshot_age(&previous);
    if initial_age.is_some_and(|age| age >= stable_for) {
        return Ok(());
    }

    let mut stable_since = initial_age
        .and_then(|age| Instant::now().checked_sub(age))
        .unwrap_or_else(Instant::now);
    let poll_interval = Duration::from_millis(100).min(stable_for);

    loop {
        if stable_since.elapsed() >= stable_for {
            return Ok(());
        }

        if started.elapsed() >= max_wait {
            return Err(CsvDataError::Unstable(path.display().to_string()));
        }

        thread::sleep(poll_interval);

        let current = csv_file_snapshot(path)?;
        if current == previous {
            continue;
        }

        previous = current;
        stable_since = Instant::now();
    }
}

pub fn csv_fingerprint(path: &Path) -> CsvResult<String> {
    let metadata = fs::metadata(path)?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(system_time_millis)
        .unwrap_or_default();
    let bytes = fs::read(path)?;
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;

    for byte in &bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    Ok(format!(
        "fnv1a64:{hash:016x}:size:{}:mtime_ms:{modified_ms}",
        metadata.len()
    ))
}

pub fn csv_metadata_matches_fingerprint(path: &Path, fingerprint: &str) -> CsvResult<bool> {
    let Some((expected_len, expected_modified_ms)) = fingerprint_metadata(fingerprint) else {
        return Ok(false);
    };

    let snapshot = csv_file_snapshot(path)?;

    Ok(snapshot.len == expected_len
        && snapshot.modified_ms.unwrap_or_default() == expected_modified_ms)
}

impl CsvDataError {
    pub fn is_transient_file_access(&self) -> bool {
        match self {
            CsvDataError::Io(source) => is_transient_file_access_error(source),
            CsvDataError::Unstable(_) => true,
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

    pub fn row_at(&self, one_based_index: u32) -> CsvResult<&[String]> {
        let Some(zero_based_index) = one_based_index.checked_sub(1) else {
            return Err(CsvDataError::Empty(self.path.display().to_string()));
        };
        let index = usize::try_from(zero_based_index)
            .map_err(|_| CsvDataError::Empty(self.path.display().to_string()))?;

        self.rows
            .get(index)
            .map(Vec::as_slice)
            .ok_or_else(|| CsvDataError::Empty(self.path.display().to_string()))
    }

    pub fn last_numeric_row_after_header(&self, column: &str) -> CsvResult<&[String]> {
        let index = excel_col_to_index(column)?;

        self.rows
            .iter()
            .skip(1)
            .rev()
            .find(|row| {
                row.get(index)
                    .map(|value| value.trim().replace(',', "").parse::<f64>().is_ok())
                    .unwrap_or(false)
            })
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

fn csv_file_snapshot(path: &Path) -> CsvResult<CsvFileSnapshot> {
    let metadata = fs::metadata(path)?;

    Ok(CsvFileSnapshot {
        len: metadata.len(),
        modified_ms: metadata.modified().ok().and_then(system_time_millis),
    })
}

fn csv_snapshot_age(snapshot: &CsvFileSnapshot) -> Option<Duration> {
    let modified_ms = snapshot.modified_ms?;
    let now_ms = system_time_millis(SystemTime::now())?;
    let age_ms = now_ms.checked_sub(modified_ms)?;
    let age_ms = u64::try_from(age_ms).unwrap_or(u64::MAX);

    Some(Duration::from_millis(age_ms))
}

fn fingerprint_metadata(fingerprint: &str) -> Option<(u64, u128)> {
    let mut parts = fingerprint.split(':');

    if parts.next()? != "fnv1a64" {
        return None;
    }

    parts.next()?;

    if parts.next()? != "size" {
        return None;
    }

    let len = parts.next()?.parse::<u64>().ok()?;

    if parts.next()? != "mtime_ms" {
        return None;
    }

    let modified_ms = parts.next()?.parse::<u128>().ok()?;

    if parts.next().is_some() {
        return None;
    }

    Some((len, modified_ms))
}

fn system_time_millis(time: SystemTime) -> Option<u128> {
    Some(time.duration_since(UNIX_EPOCH).ok()?.as_millis())
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
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

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

    #[test]
    fn stable_csv_wait_passes_after_unchanged_window() {
        let temp = tempfile::tempdir().expect("temp dir");
        let csv_path = temp.path().join("stable.csv");
        fs::write(&csv_path, "a,b\n1,2\n").expect("write csv");

        wait_for_stable_csv(
            &csv_path,
            Duration::from_millis(20),
            Duration::from_millis(200),
        )
        .expect("stable csv should pass");
    }

    #[test]
    fn stable_csv_wait_skips_sleep_for_already_stable_csv() {
        let temp = tempfile::tempdir().expect("temp dir");
        let csv_path = temp.path().join("stable.csv");
        fs::write(&csv_path, "a,b\n1,2\n").expect("write csv");
        thread::sleep(Duration::from_millis(160));

        let started = Instant::now();
        wait_for_stable_csv(
            &csv_path,
            Duration::from_millis(120),
            Duration::from_millis(500),
        )
        .expect("already stable csv should pass");

        assert!(
            started.elapsed() < Duration::from_millis(100),
            "already stable CSV should not wait for a fresh stability window"
        );
    }

    #[test]
    fn metadata_match_recognizes_existing_csv_fingerprint() {
        let temp = tempfile::tempdir().expect("temp dir");
        let csv_path = temp.path().join("fingerprint.csv");
        fs::write(&csv_path, "a,b\n1,2\n").expect("write csv");
        let fingerprint = csv_fingerprint(&csv_path).expect("fingerprint");

        assert!(csv_metadata_matches_fingerprint(&csv_path, &fingerprint)
            .expect("metadata check should work"));

        fs::write(&csv_path, "a,b\n1,2\n3,4\n").expect("rewrite csv");

        assert!(!csv_metadata_matches_fingerprint(&csv_path, &fingerprint)
            .expect("metadata check should work"));
    }

    #[test]
    fn changing_csv_wait_returns_unstable_transient_error() {
        let temp = tempfile::tempdir().expect("temp dir");
        let csv_path = temp.path().join("changing.csv");
        fs::write(&csv_path, "a,b\n1,2\n").expect("write csv");
        let writer_path = csv_path.clone();
        let (writer_started_tx, writer_started_rx) = mpsc::channel();

        let writer = thread::spawn(move || {
            fs::write(&writer_path, "a,b\n1,2\n").expect("initial rewrite");
            writer_started_tx.send(()).expect("signal writer started");

            for index in 1..20 {
                thread::sleep(Duration::from_millis(20));
                fs::write(&writer_path, format!("a,b\n{},2\n", "1".repeat(index + 1)))
                    .expect("rewrite csv");
            }
        });

        writer_started_rx.recv().expect("writer should start");
        let error = wait_for_stable_csv(
            &csv_path,
            Duration::from_millis(500),
            Duration::from_millis(180),
        )
        .expect_err("changing csv should not become stable before max wait");

        writer.join().expect("writer thread");
        assert!(error.is_transient_file_access());
    }
}
