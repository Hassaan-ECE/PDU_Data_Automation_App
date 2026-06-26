use std::collections::{HashMap, HashSet};
use std::fs;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::resource_paths;

const PROFILE_FILE_NAME: &str = "pdu500.rev02.layout.json";
const DEFAULT_PROFILE_JSON: &str =
    include_str!("../../../config/report-layouts/pdu500.rev02.layout.json");
#[cfg(test)]
const EXAMPLE_PROFILE_JSON: &str =
    include_str!("../../../config/report-layouts/pdu500.layout.example.json");

#[derive(Debug, Error)]
pub enum LayoutProfileError {
    #[error("layout profile could not be read from {path}: {source}")]
    ReadFailed {
        path: String,
        source: std::io::Error,
    },
    #[error("layout profile JSON is invalid in {path}: {source}")]
    InvalidJson {
        path: String,
        source: serde_json::Error,
    },
    #[error("layout profile is invalid in {path}: {details}")]
    InvalidProfile { path: String, details: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReportLayoutProfile {
    pub schema_version: u16,
    pub profile_id: String,
    pub display_name: String,
    #[serde(default)]
    pub status: Option<String>,
    pub templates: ReportTemplates,
    pub workbooks: HashMap<String, WorkbookDefinition>,
    #[serde(default)]
    pub serial_number: Option<SerialNumberRules>,
    #[serde(default)]
    pub task_groups: Vec<TaskGroup>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReportTemplates {
    pub default_template_root: String,
    pub main_report_template: String,
    pub print_report_template: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkbookDefinition {
    pub file_pattern: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SerialNumberRules {
    pub folder_pattern: String,
    #[serde(default)]
    pub metadata_files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskGroup {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub tasks: Vec<TaskDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskDefinition {
    pub id: String,
    pub label: String,
    pub step: StepNumber,
    #[serde(default)]
    pub detection_steps: Vec<u16>,
    #[serde(default)]
    pub processor: Option<String>,
    pub csv_pattern: String,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub mappings: Vec<MappingDefinition>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum StepNumber {
    Number(u16),
    Pending(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct MappingDefinition {
    pub label: String,
    #[serde(default)]
    pub source: Option<MappingSource>,
    #[serde(default)]
    pub transform: Option<TransformDefinition>,
    pub target: MappingTarget,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MappingSource {
    pub column: String,
    pub row: MappingRow,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum MappingRow {
    Index(u32),
    Selector(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransformDefinition {
    #[serde(default)]
    pub scale_by: Option<f64>,
    #[serde(default)]
    pub round: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MappingTarget {
    pub workbook: String,
    pub sheet: String,
    pub cell: String,
    #[serde(default)]
    pub number_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidationResult {
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProfileLoadSummary {
    pub profile_id: String,
    pub display_name: String,
    pub task_count: usize,
    pub validation: ValidationResult,
}

impl ValidationResult {
    fn new() -> Self {
        Self {
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

impl ReportLayoutProfile {
    pub fn from_json(json: &str) -> Result<Self, LayoutProfileError> {
        Self::from_json_with_path("inline layout profile".to_string(), json)
    }

    fn from_json_with_path(path: String, json: &str) -> Result<Self, LayoutProfileError> {
        serde_json::from_str(json)
            .map_err(|source| LayoutProfileError::InvalidJson { path, source })
    }

    pub fn validate(&self) -> ValidationResult {
        let mut result = ValidationResult::new();
        let example_only = self.status.as_deref() == Some("example_only");

        if self.schema_version != 1 {
            result.errors.push(format!(
                "profile '{}' uses unsupported schema_version {}",
                self.profile_id, self.schema_version
            ));
        }

        if self.profile_id.trim().is_empty() {
            result
                .errors
                .push("profile_id must not be empty".to_string());
        }

        if self.display_name.trim().is_empty() {
            result
                .errors
                .push("display_name must not be empty".to_string());
        }

        validate_templates(&self.templates, &mut result);
        validate_workbooks(&self.workbooks, &mut result);
        validate_serial_number_rules(self.serial_number.as_ref(), &mut result);
        validate_task_groups(self, example_only, &mut result);

        result
    }

    pub fn task_count(&self) -> usize {
        self.task_groups
            .iter()
            .map(|group| group.tasks.len())
            .sum::<usize>()
    }

    pub fn to_load_summary(&self) -> ProfileLoadSummary {
        ProfileLoadSummary {
            profile_id: self.profile_id.clone(),
            display_name: self.display_name.clone(),
            task_count: self.task_count(),
            validation: self.validate(),
        }
    }
}

pub fn load_layout_profile() -> Result<ReportLayoutProfile, LayoutProfileError> {
    if let Some(path) = configured_profile_path() {
        return load_from_path(path);
    }

    load_layout_profile_from_candidates(candidate_profile_paths())
}

fn load_layout_profile_from_candidates(
    candidate_paths: Vec<PathBuf>,
) -> Result<ReportLayoutProfile, LayoutProfileError> {
    for path in candidate_paths {
        if path.is_file() {
            return load_from_path(path);
        }
    }

    parse_and_validate_profile("built-in defaults".to_string(), DEFAULT_PROFILE_JSON)
}

fn configured_profile_path() -> Option<PathBuf> {
    std::env::var_os("PDU_LAYOUT_PROFILE_PATH").map(PathBuf::from)
}

fn candidate_profile_paths() -> Vec<PathBuf> {
    resource_paths::report_layout_candidate_paths(PROFILE_FILE_NAME)
}

#[cfg(test)]
fn load_layout_profile_with_resource_dir(
    resource_dir: &Path,
) -> Result<ReportLayoutProfile, LayoutProfileError> {
    load_layout_profile_from_candidates(resource_paths::report_layout_candidate_paths_for(
        PROFILE_FILE_NAME,
        Some(resource_dir),
    ))
}

fn load_from_path(path: PathBuf) -> Result<ReportLayoutProfile, LayoutProfileError> {
    let display_path = path.display().to_string();
    let json = fs::read_to_string(&path).map_err(|source| LayoutProfileError::ReadFailed {
        path: display_path.clone(),
        source,
    })?;

    parse_and_validate_profile(display_path, &json)
}

fn parse_and_validate_profile(
    path: String,
    json: &str,
) -> Result<ReportLayoutProfile, LayoutProfileError> {
    let profile = ReportLayoutProfile::from_json_with_path(path.clone(), json)?;
    let validation = profile.validate();

    if validation.is_valid() {
        return Ok(profile);
    }

    Err(LayoutProfileError::InvalidProfile {
        path,
        details: validation.errors.join("; "),
    })
}

#[cfg(test)]
fn parse_example_profile_fixture() -> Result<ReportLayoutProfile, LayoutProfileError> {
    ReportLayoutProfile::from_json_with_path("example profile".to_string(), EXAMPLE_PROFILE_JSON)
}

fn validate_templates(templates: &ReportTemplates, result: &mut ValidationResult) {
    if templates.default_template_root.trim().is_empty() {
        result
            .errors
            .push("templates.default_template_root must not be empty".to_string());
    }

    if templates.main_report_template.trim().is_empty() {
        result
            .errors
            .push("templates.main_report_template must not be empty".to_string());
    }

    if templates.print_report_template.trim().is_empty() {
        result
            .errors
            .push("templates.print_report_template must not be empty".to_string());
    }
}

fn validate_workbooks(
    workbooks: &HashMap<String, WorkbookDefinition>,
    result: &mut ValidationResult,
) {
    if workbooks.is_empty() {
        result
            .errors
            .push("workbooks must define at least one workbook".to_string());
    }

    for required in ["main", "print"] {
        if !workbooks.contains_key(required) {
            result
                .errors
                .push(format!("workbooks must define '{required}'"));
        }
    }

    for (key, workbook) in workbooks {
        if key.trim().is_empty() {
            result
                .errors
                .push("workbook keys must not be empty".to_string());
        }

        if workbook.file_pattern.trim().is_empty() {
            result
                .errors
                .push(format!("workbook '{}' must define file_pattern", key));
        }
    }
}

fn validate_serial_number_rules(
    serial_number: Option<&SerialNumberRules>,
    result: &mut ValidationResult,
) {
    let Some(serial_number) = serial_number else {
        result
            .warnings
            .push("serial_number rules are not defined".to_string());
        return;
    };

    if serial_number.folder_pattern.trim().is_empty() {
        result
            .errors
            .push("serial_number.folder_pattern must not be empty".to_string());
    }

    if serial_number
        .metadata_files
        .iter()
        .any(|file| file.trim().is_empty())
    {
        result
            .errors
            .push("serial_number.metadata_files must not contain empty entries".to_string());
    }
}

fn validate_task_groups(
    profile: &ReportLayoutProfile,
    example_only: bool,
    result: &mut ValidationResult,
) {
    if profile.task_groups.is_empty() {
        result
            .errors
            .push("task_groups must not be empty".to_string());
    }

    let mut group_ids = HashSet::new();
    let mut task_ids = HashSet::new();

    for group in &profile.task_groups {
        if group.id.trim().is_empty() {
            result
                .errors
                .push("task group id must not be empty".to_string());
        } else if !group_ids.insert(group.id.as_str()) {
            result
                .errors
                .push(format!("duplicate task group id '{}'", group.id));
        }

        if group.label.trim().is_empty() {
            result
                .errors
                .push(format!("task group '{}' label must not be empty", group.id));
        }

        if group.tasks.is_empty() {
            result
                .warnings
                .push(format!("task group '{}' has no tasks", group.id));
        }

        for task in &group.tasks {
            validate_task(task, profile, example_only, &mut task_ids, result);
        }
    }
}

fn validate_task<'a>(
    task: &'a TaskDefinition,
    profile: &ReportLayoutProfile,
    example_only: bool,
    task_ids: &mut HashSet<&'a str>,
    result: &mut ValidationResult,
) {
    if task.id.trim().is_empty() {
        result.errors.push("task id must not be empty".to_string());
    } else if !task_ids.insert(task.id.as_str()) {
        result
            .errors
            .push(format!("duplicate task id '{}'", task.id));
    }

    if task.label.trim().is_empty() {
        result
            .errors
            .push(format!("task '{}' label must not be empty", task.id));
    }

    match &task.step {
        StepNumber::Number(0) => result
            .errors
            .push(format!("task '{}' step must be greater than zero", task.id)),
        StepNumber::Number(_) => {}
        StepNumber::Pending(value) if example_only => result.warnings.push(format!(
            "task '{}' has unresolved example step '{}'",
            task.id, value
        )),
        StepNumber::Pending(value) => result.errors.push(format!(
            "task '{}' must use a numeric step before production use, found '{}'",
            task.id, value
        )),
    }

    if task.detection_steps.iter().any(|step| *step == 0) {
        result.errors.push(format!(
            "task '{}' detection_steps must be greater than zero",
            task.id
        ));
    }

    let has_processor = task
        .processor
        .as_deref()
        .map(str::trim)
        .is_some_and(|processor| !processor.is_empty());
    if task.processor.is_some() && !has_processor {
        result
            .errors
            .push(format!("task '{}' processor must not be empty", task.id));
    }

    if task.csv_pattern.trim().is_empty() {
        result
            .errors
            .push(format!("task '{}' csv_pattern must not be empty", task.id));
    }

    if task.mappings.is_empty() && !has_processor {
        result
            .warnings
            .push(format!("task '{}' has no report mappings", task.id));
    }

    for mapping in &task.mappings {
        validate_mapping(task, mapping, profile, result);
    }
}

fn validate_mapping(
    task: &TaskDefinition,
    mapping: &MappingDefinition,
    profile: &ReportLayoutProfile,
    result: &mut ValidationResult,
) {
    if mapping.label.trim().is_empty() {
        result.warnings.push(format!(
            "task '{}' contains a mapping with no label",
            task.id
        ));
    }

    match &mapping.source {
        Some(source) => validate_mapping_source(task, mapping, source, result),
        None => result.warnings.push(format!(
            "task '{}' mapping '{}' has no source rule",
            task.id, mapping.label
        )),
    }

    if let Some(transform) = &mapping.transform {
        if transform.scale_by == Some(0.0) {
            result.errors.push(format!(
                "task '{}' mapping '{}' transform.scale_by must not be zero",
                task.id, mapping.label
            ));
        }
    }

    validate_mapping_target(task, mapping, profile, result);
}

fn validate_mapping_source(
    task: &TaskDefinition,
    mapping: &MappingDefinition,
    source: &MappingSource,
    result: &mut ValidationResult,
) {
    if !is_valid_column_name(&source.column) {
        result.errors.push(format!(
            "task '{}' mapping '{}' source column '{}' is invalid",
            task.id, mapping.label, source.column
        ));
    }

    match &source.row {
        MappingRow::Index(0) => result.errors.push(format!(
            "task '{}' mapping '{}' source row must be greater than zero",
            task.id, mapping.label
        )),
        MappingRow::Index(_) => {}
        MappingRow::Selector(selector) if is_supported_row_selector(selector) => {}
        MappingRow::Selector(selector) => result.errors.push(format!(
            "task '{}' mapping '{}' source row selector '{}' is not supported",
            task.id, mapping.label, selector
        )),
    }
}

fn validate_mapping_target(
    task: &TaskDefinition,
    mapping: &MappingDefinition,
    profile: &ReportLayoutProfile,
    result: &mut ValidationResult,
) {
    if !profile.workbooks.contains_key(&mapping.target.workbook) {
        result.errors.push(format!(
            "task '{}' mapping '{}' references unknown workbook '{}'",
            task.id, mapping.label, mapping.target.workbook
        ));
    }

    if mapping.target.sheet.trim().is_empty() {
        result.errors.push(format!(
            "task '{}' mapping '{}' target sheet must not be empty",
            task.id, mapping.label
        ));
    }

    if !is_valid_cell_reference(&mapping.target.cell) {
        result.errors.push(format!(
            "task '{}' mapping '{}' target cell '{}' is invalid",
            task.id, mapping.label, mapping.target.cell
        ));
    }

    if mapping
        .target
        .number_format
        .as_deref()
        .is_some_and(str::is_empty)
    {
        result.warnings.push(format!(
            "task '{}' mapping '{}' target number_format is empty",
            task.id, mapping.label
        ));
    }
}

fn is_valid_column_name(column: &str) -> bool {
    let trimmed = column.trim();

    (1..=3).contains(&trimmed.len()) && trimmed.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn is_valid_cell_reference(cell: &str) -> bool {
    let trimmed = cell.trim();
    let letter_count = trimmed.bytes().take_while(u8::is_ascii_uppercase).count();

    if letter_count == 0 || letter_count > 3 || letter_count == trimmed.len() {
        return false;
    }

    trimmed[letter_count..]
        .parse::<u32>()
        .is_ok_and(|row| row > 0)
}

fn is_supported_row_selector(selector: &str) -> bool {
    matches!(
        selector,
        "first_data_after_header" | "last_data_after_header" | "last_data" | "last_numeric"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::automation::tasks::automation_tasks;

    #[test]
    fn default_layout_profile_loads_without_warnings() {
        let profile =
            ReportLayoutProfile::from_json(DEFAULT_PROFILE_JSON).expect("profile should parse");
        let validation = profile.validate();

        assert!(validation.is_valid(), "{validation:#?}");
        assert_eq!(validation.warnings, Vec::<String>::new());
        assert_eq!(profile.profile_id, "pdu500.rev02");
        assert_eq!(profile.task_count(), 65);
    }

    #[test]
    fn default_layout_profile_matches_automation_task_ids() {
        let profile =
            ReportLayoutProfile::from_json(DEFAULT_PROFILE_JSON).expect("profile should parse");
        let profile_task_ids = profile
            .task_groups
            .iter()
            .flat_map(|group| group.tasks.iter())
            .map(|task| task.id.as_str())
            .collect::<HashSet<_>>();
        let automation_tasks = automation_tasks();
        let automation_task_ids = automation_tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<HashSet<_>>();

        assert_eq!(profile_task_ids, automation_task_ids);
    }

    #[test]
    fn default_transformer_tasks_define_production_mappings() {
        let profile =
            ReportLayoutProfile::from_json(DEFAULT_PROFILE_JSON).expect("profile should parse");

        for (task_id, sheet) in [
            ("208v-transformer", "XFMR Check_208VAC"),
            ("415v-transformer", "XFMR Check_415VAC"),
        ] {
            let task = profile
                .task_groups
                .iter()
                .flat_map(|group| group.tasks.iter())
                .find(|task| task.id == task_id)
                .expect("transformer task should exist");
            let cells = task
                .mappings
                .iter()
                .map(|mapping| {
                    (
                        mapping
                            .source
                            .as_ref()
                            .expect("mapping source")
                            .column
                            .as_str(),
                        mapping.target.sheet.as_str(),
                        mapping.target.cell.as_str(),
                    )
                })
                .collect::<Vec<_>>();

            assert_eq!(
                cells,
                vec![
                    ("Z", sheet, "B9"),
                    ("AE", sheet, "B10"),
                    ("BG", sheet, "B11"),
                    ("BL", sheet, "B12"),
                ]
            );
        }
    }

    #[test]
    fn example_profile_loads_with_expected_warnings_only() {
        let profile = parse_example_profile_fixture().expect("example profile should parse");
        let validation = profile.validate();

        assert!(validation.is_valid(), "{validation:#?}");
        assert_eq!(profile.profile_id, "pdu500.rev02.example");
        assert_eq!(profile.task_count(), 3);
        assert!(validation
            .warnings
            .iter()
            .any(|warning| warning.contains("system-burn-in")));
    }

    #[test]
    fn duplicate_task_ids_are_rejected() {
        let mut profile = parse_example_profile_fixture().expect("example profile should parse");
        profile.task_groups[0].tasks[1].id = profile.task_groups[0].tasks[0].id.clone();

        let validation = profile.validate();

        assert!(!validation.is_valid());
        assert!(validation
            .errors
            .iter()
            .any(|error| error.contains("duplicate task id")));
    }

    #[test]
    fn unknown_workbook_references_are_rejected() {
        let mut profile = parse_example_profile_fixture().expect("example profile should parse");
        profile.task_groups[0].tasks[0].mappings[0].target.workbook = "missing".to_string();

        let validation = profile.validate();

        assert!(!validation.is_valid());
        assert!(validation
            .errors
            .iter()
            .any(|error| error.contains("unknown workbook")));
    }

    #[test]
    fn main_and_print_workbooks_are_required() {
        let mut profile = parse_example_profile_fixture().expect("example profile should parse");
        profile.workbooks.remove("print");

        let validation = profile.validate();

        assert!(!validation.is_valid());
        assert!(validation
            .errors
            .iter()
            .any(|error| error.contains("workbooks must define 'print'")));
    }

    #[test]
    fn production_profiles_cannot_keep_pending_steps() {
        let mut profile = parse_example_profile_fixture().expect("example profile should parse");
        profile.status = None;

        let validation = profile.validate();

        assert!(!validation.is_valid());
        assert!(validation
            .errors
            .iter()
            .any(|error| error.contains("must use a numeric step")));
    }

    #[test]
    fn load_from_path_rejects_invalid_json() {
        let temp = TempDir::new().expect("temp dir");
        let profile_path = temp.path().join("broken.layout.json");
        fs::write(&profile_path, "{not valid json").expect("write broken profile");

        let error = load_from_path(profile_path).expect_err("invalid JSON should fail");

        assert!(matches!(error, LayoutProfileError::InvalidJson { .. }));
    }

    #[test]
    fn load_with_resource_dir_uses_bundled_profile_before_builtin() {
        let temp = TempDir::new().expect("temp dir");
        let resource_profile_dir = temp
            .path()
            .join("_up_")
            .join("config")
            .join("report-layouts");
        fs::create_dir_all(&resource_profile_dir).expect("create resource layout dir");
        let resource_json = DEFAULT_PROFILE_JSON
            .replace(
                r#""profile_id": "pdu500.rev02""#,
                r#""profile_id": "pdu500.rev02.resource-test""#,
            )
            .replace(
                r#""display_name": "PDU500 0.2CT Rev02""#,
                r#""display_name": "Resource Loaded Layout""#,
            );
        fs::write(resource_profile_dir.join(PROFILE_FILE_NAME), resource_json)
            .expect("write resource profile");

        let profile =
            load_layout_profile_with_resource_dir(temp.path()).expect("resource profile loads");

        assert_eq!(profile.profile_id, "pdu500.rev02.resource-test");
        assert_eq!(profile.display_name, "Resource Loaded Layout");
    }

    #[test]
    fn load_from_path_rejects_validation_errors() {
        let temp = TempDir::new().expect("temp dir");
        let profile_path = temp.path().join("invalid.layout.json");
        fs::write(
            &profile_path,
            r#"{
  "schema_version": 99,
  "profile_id": "invalid",
  "display_name": "Invalid",
  "templates": {
    "default_template_root": "C:/PDU500/00_Template",
    "main_report_template": "main.xlsx",
    "print_report_template": "print.xlsx"
  },
  "workbooks": {
    "main": { "file_pattern": "*.xlsx" }
  },
  "task_groups": [
    {
      "id": "group",
      "label": "Group",
      "tasks": [
        {
          "id": "task",
          "label": "Task",
          "step": 1,
          "processor": "built_in",
          "csv_pattern": "*.csv"
        }
      ]
    }
  ]
}"#,
        )
        .expect("write invalid profile");

        let error = load_from_path(profile_path).expect_err("invalid profile should fail");

        match error {
            LayoutProfileError::InvalidProfile { details, .. } => {
                assert!(details.contains("unsupported schema_version"));
            }
            other => panic!("expected invalid profile error, got {other:?}"),
        }
    }
}
