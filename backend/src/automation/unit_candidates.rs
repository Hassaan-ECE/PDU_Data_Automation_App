use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::Serialize;

use crate::config::{load_layout_profile, ReportLayoutProfile};

#[derive(Debug, Clone, Serialize)]
pub struct LatestUnitCandidateResult {
    pub candidate: Option<UnitCandidate>,
    pub searched_roots: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnitCandidate {
    pub serial_number: String,
    pub serial_label: String,
    pub unit_folder: String,
    pub detection_reason: String,
    pub detection_source: String,
    pub timestamp_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct SerialDetectionRules {
    folder_pattern: Option<String>,
    metadata_files: Vec<String>,
}

#[derive(Debug, Clone)]
struct CandidateMatch {
    serial_number: String,
    detection_reason: String,
    detection_source: String,
}

#[derive(Debug, Clone)]
struct CandidateWithTime {
    candidate: UnitCandidate,
    sort_time: SystemTime,
}

pub fn latest_unit_candidate() -> LatestUnitCandidateResult {
    let profile = match load_layout_profile() {
        Ok(profile) => profile,
        Err(error) => {
            return LatestUnitCandidateResult {
                candidate: None,
                searched_roots: Vec::new(),
                warnings: vec![format!(
                    "layout profile could not be loaded for unit suggestion: {error}"
                )],
            };
        }
    };
    let roots = candidate_roots(&profile);
    let rules = SerialDetectionRules::from_profile(Some(&profile));

    latest_unit_candidate_in_roots(&roots, &rules, Vec::new())
}

fn latest_unit_candidate_in_roots(
    roots: &[PathBuf],
    rules: &SerialDetectionRules,
    mut warnings: Vec<String>,
) -> LatestUnitCandidateResult {
    let searched_roots = roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>();
    let mut best = None::<CandidateWithTime>;

    for root in roots {
        if !root.is_dir() {
            warnings.push(format!(
                "unit suggestion root not found: {}",
                root.display()
            ));
            continue;
        }

        let Ok(entries) = fs::read_dir(root) else {
            warnings.push(format!(
                "unit suggestion root could not be read: {}",
                root.display()
            ));
            continue;
        };

        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();

            if !path.is_dir() || is_ignored_folder(&path) {
                continue;
            }

            let Some(candidate_match) = detect_unit_serial(&path, rules) else {
                continue;
            };
            let Ok(metadata) = path.metadata() else {
                continue;
            };
            let sort_time = folder_sort_time(&metadata).unwrap_or(SystemTime::UNIX_EPOCH);
            let timestamp_ms = system_time_millis(sort_time);
            let serial_number = candidate_match.serial_number;
            let candidate = UnitCandidate {
                serial_label: format!("SN {serial_number}"),
                serial_number,
                unit_folder: path.display().to_string(),
                detection_reason: candidate_match.detection_reason,
                detection_source: candidate_match.detection_source,
                timestamp_ms,
            };

            if best
                .as_ref()
                .is_none_or(|current| sort_time > current.sort_time)
            {
                best = Some(CandidateWithTime {
                    candidate,
                    sort_time,
                });
            }
        }
    }

    LatestUnitCandidateResult {
        candidate: best.map(|best| best.candidate),
        searched_roots,
        warnings,
    }
}

fn candidate_roots(profile: &ReportLayoutProfile) -> Vec<PathBuf> {
    let mut roots = Vec::<PathBuf>::new();

    if let Some(root) = Path::new(&profile.templates.default_template_root).parent() {
        roots.push(root.to_path_buf());
    }

    dedupe_paths(roots)
}

impl SerialDetectionRules {
    fn from_profile(profile: Option<&ReportLayoutProfile>) -> Self {
        let fallback = Self::fallback();
        let Some(profile) = profile else {
            return fallback;
        };
        let Some(serial_number) = profile.serial_number.as_ref() else {
            return fallback;
        };

        Self {
            folder_pattern: Some(serial_number.folder_pattern.clone()),
            metadata_files: if serial_number.metadata_files.is_empty() {
                fallback.metadata_files
            } else {
                serial_number.metadata_files.clone()
            },
        }
    }

    fn fallback() -> Self {
        Self {
            folder_pattern: Some(r"^(?<serial>\d{6,})$".to_string()),
            metadata_files: vec![
                "SN.txt".to_string(),
                "serial_number.txt".to_string(),
                "info.txt".to_string(),
                "metadata.txt".to_string(),
            ],
        }
    }
}

fn detect_unit_serial(path: &Path, rules: &SerialDetectionRules) -> Option<CandidateMatch> {
    detect_serial_from_folder_name(path, rules).or_else(|| detect_serial_from_metadata(path, rules))
}

fn detect_serial_from_folder_name(
    path: &Path,
    rules: &SerialDetectionRules,
) -> Option<CandidateMatch> {
    let folder_name = path.file_name()?.to_string_lossy();
    let pattern = rules.folder_pattern.as_deref()?;
    let regex = Regex::new(pattern).ok()?;
    let captures = regex.captures(&folder_name)?;
    let serial = captures
        .name("serial")
        .or_else(|| captures.get(1))
        .or_else(|| captures.get(0))?
        .as_str()
        .trim()
        .to_string();

    if serial.is_empty() {
        return None;
    }

    Some(CandidateMatch {
        serial_number: serial,
        detection_reason: "matched layout serial_number.folder_pattern".to_string(),
        detection_source: folder_name.to_string(),
    })
}

fn detect_serial_from_metadata(
    path: &Path,
    rules: &SerialDetectionRules,
) -> Option<CandidateMatch> {
    let metadata_re =
        Regex::new(r"(?i)(?:sn|serial\s*number)[:\s=]*(\d{6,})").expect("metadata regex is valid");

    for file_name in &rules.metadata_files {
        let metadata_path = path.join(file_name);

        if !metadata_path.is_file() {
            continue;
        }

        let Ok(content) = fs::read_to_string(&metadata_path) else {
            continue;
        };
        let Some(captures) = metadata_re.captures(&content) else {
            continue;
        };
        let Some(serial) = captures.get(1).map(|match_| match_.as_str().to_string()) else {
            continue;
        };

        return Some(CandidateMatch {
            serial_number: serial,
            detection_reason: "matched layout serial_number.metadata_files".to_string(),
            detection_source: metadata_path.display().to_string(),
        });
    }

    None
}

fn is_ignored_folder(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let upper = name.to_ascii_uppercase();

    upper.starts_with('.')
        || upper == "00_TEMPLATE"
        || upper.starts_with("00_")
        || matches!(
            upper.as_str(),
            "ARCHIVE" | "CONFIG" | "RELEASE-SUPPORT" | "RELEASE_SUPPORT" | "SHARED"
        )
}

fn folder_sort_time(metadata: &fs::Metadata) -> Option<SystemTime> {
    match (metadata.created().ok(), metadata.modified().ok()) {
        (Some(created), Some(modified)) => Some(created.max(modified)),
        (Some(created), None) => Some(created),
        (None, Some(modified)) => Some(modified),
        (None, None) => None,
    }
}

fn system_time_millis(time: SystemTime) -> Option<u64> {
    let millis = time.duration_since(UNIX_EPOCH).ok()?.as_millis();

    u64::try_from(millis).ok()
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::<String>::new();
    let mut deduped = Vec::new();

    for path in paths {
        let key = path.display().to_string().to_ascii_lowercase();

        if seen.insert(key) {
            deduped.push(path);
        }
    }

    deduped
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn latest_candidate_prefers_newest_valid_unit_folder() {
        let temp = TempDir::new().expect("temp dir");
        let older = temp.path().join("262343000071");
        let newer = temp.path().join("262343000072");
        fs::create_dir_all(&older).expect("older unit");
        thread::sleep(Duration::from_millis(25));
        fs::create_dir_all(&newer).expect("newer unit");
        fs::create_dir_all(temp.path().join("00_Template")).expect("template folder");
        let rules = SerialDetectionRules::fallback();

        let result = latest_unit_candidate_in_roots(&[temp.path().to_path_buf()], &rules, vec![]);
        let candidate = result.candidate.expect("candidate should be found");

        assert_eq!(candidate.serial_number, "262343000072");
        assert_eq!(candidate.unit_folder, newer.display().to_string());
        assert!(candidate
            .detection_reason
            .contains("serial_number.folder_pattern"));
    }

    #[test]
    fn latest_candidate_can_use_metadata_file_when_folder_name_does_not_match() {
        let temp = TempDir::new().expect("temp dir");
        let unit = temp.path().join("active-unit");
        fs::create_dir_all(&unit).expect("unit folder");
        fs::write(unit.join("SN.txt"), "Serial Number: 262343000073").expect("metadata");
        let rules = SerialDetectionRules::fallback();

        let result = latest_unit_candidate_in_roots(&[temp.path().to_path_buf()], &rules, vec![]);
        let candidate = result.candidate.expect("candidate should be found");

        assert_eq!(candidate.serial_number, "262343000073");
        assert!(candidate
            .detection_reason
            .contains("serial_number.metadata_files"));
    }

    #[test]
    fn latest_candidate_returns_none_without_valid_units() {
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir_all(temp.path().join("00_Template")).expect("template folder");
        let rules = SerialDetectionRules::fallback();

        let result = latest_unit_candidate_in_roots(&[temp.path().to_path_buf()], &rules, vec![]);

        assert!(result.candidate.is_none());
        assert_eq!(
            result.searched_roots,
            vec![temp.path().display().to_string()]
        );
    }

    #[test]
    fn candidate_roots_follow_profile_template_root_parent() {
        let temp = TempDir::new().expect("temp dir");
        let template_root = temp.path().join("Configured_Template");
        let profile = profile_with_template_root(&template_root);

        assert_eq!(candidate_roots(&profile), vec![temp.path().to_path_buf()]);
    }

    fn profile_with_template_root(template_root: &Path) -> ReportLayoutProfile {
        ReportLayoutProfile::from_json(&format!(
            r#"{{
  "schema_version": 1,
  "profile_id": "test",
  "display_name": "Test",
  "templates": {{
    "default_template_root": "{}",
    "main_report_template": "main.xlsx",
    "print_report_template": "print.xlsx"
  }},
  "workbooks": {{
    "main": {{ "file_pattern": "main*.xlsx" }},
    "print": {{ "file_pattern": "print*.xlsx" }}
  }}
}}"#,
            template_root.display().to_string().replace('\\', "\\\\")
        ))
        .expect("test profile")
    }
}
