mod csv_data;
mod mapped;
mod processors;
mod reports;
pub mod tasks;
mod unit_candidates;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::Serialize;
use thiserror::Error;

use crate::config::load_layout_profile;

use self::csv_data::detected_steps;
use self::processors::{FailureDetail, ProcessorResult};
use self::reports::{
    inspect_reports, setup_reports, setup_reports_with_serial_number,
    write_transformer_serial_number, ReportError, ReportSetup,
};
use self::tasks::{automation_tasks, find_task};
pub use self::unit_candidates::{LatestUnitCandidateResult, UnitCandidate};

#[derive(Debug, Error)]
pub enum AutomationError {
    #[error("{0}")]
    Report(#[from] ReportError),
    #[error("unknown automation task id: {0}")]
    UnknownTask(String),
    #[error("{0}")]
    OpenReport(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationCommandError {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
}

impl AutomationCommandError {
    fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    fn report(
        code: impl Into<String>,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Some(details.into()),
        }
    }

    fn from_report_error(error: ReportError) -> Self {
        match &error {
            ReportError::Io(source) if is_locked_file_error(source) => Self::report(
                "workbook_locked",
                "The main report workbook is locked or open in another program. Close Excel or ATS access to the report and try again.",
                error.to_string(),
            ),
            ReportError::Zip(zip::result::ZipError::Io(source))
                if is_locked_file_error(source) =>
            {
                Self::report(
                    "workbook_locked",
                    "The main report workbook is locked or open in another program. Close Excel or ATS access to the report and try again.",
                    error.to_string(),
                )
            }
            ReportError::Io(_) => Self::report(
                "report_io_failed",
                "The report workbook could not be read or written.",
                error.to_string(),
            ),
            ReportError::MainReportMissing(_) => Self::report(
                "main_report_missing",
                "The main report workbook was not found. Check the selected unit folder and report template setup.",
                error.to_string(),
            ),
            ReportError::SheetMissing(sheet) => Self::report(
                "report_sheet_missing",
                format!("The main report workbook is missing the required sheet '{sheet}'."),
                error.to_string(),
            ),
            ReportError::InvalidCell(cell) => Self::report(
                "report_cell_invalid",
                format!("The required report cell '{cell}' is invalid."),
                error.to_string(),
            ),
            ReportError::UnitFolderMissing(_) => Self::report(
                "unit_folder_missing",
                "The selected unit folder does not exist.",
                error.to_string(),
            ),
            _ => Self::report(
                "report_write_failed",
                "The Transformer SN could not be written to the report workbook.",
                error.to_string(),
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UnitFolderSummary {
    pub unit_folder: String,
    pub serial_number: Option<String>,
    pub report_path: Option<String>,
    pub print_report_path: Option<String>,
    pub detected_count: usize,
    pub tasks: Vec<TaskStatus>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskStatus {
    pub task_id: String,
    pub label: String,
    pub step: String,
    pub state: String,
    pub detected_steps: Vec<u16>,
    pub latest_csv: Option<String>,
    pub latest_csv_created_ms: Option<u64>,
    pub latest_csv_readable: Option<bool>,
    pub timer_start_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskProcessResult {
    pub task_id: String,
    pub state: String,
    pub code: u8,
    pub message: String,
    pub log: Vec<String>,
    pub report_path: Option<String>,
    pub print_report_path: Option<String>,
    pub failure: Option<FailureDetail>,
}

pub fn setup_unit_folder(unit_folder: String) -> Result<UnitFolderSummary, AutomationError> {
    let unit_folder = PathBuf::from(unit_folder);
    let report_setup = setup_reports(&unit_folder)?;

    Ok(build_summary(&unit_folder, report_setup))
}

pub fn find_latest_unit_candidate() -> LatestUnitCandidateResult {
    unit_candidates::latest_unit_candidate()
}

pub fn setup_unit_folder_with_transformer_sn(
    unit_folder: String,
    unit_serial_number: Option<String>,
    transformer_sn: String,
) -> Result<UnitFolderSummary, AutomationCommandError> {
    let transformer_sn = transformer_sn.trim();

    if transformer_sn.is_empty() {
        return Err(AutomationCommandError::validation(
            "blank_transformer_sn",
            "Transformer SN is required before setup can continue.",
        ));
    }

    let unit_folder = PathBuf::from(unit_folder);
    let unit_serial_number = unit_serial_number
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let report_setup = setup_reports_with_serial_number(&unit_folder, unit_serial_number)
        .map_err(AutomationCommandError::from_report_error)?;

    write_transformer_serial_number(&unit_folder, transformer_sn)
        .map_err(AutomationCommandError::from_report_error)?;

    Ok(build_summary(&unit_folder, report_setup))
}

pub fn scan_unit_folder(unit_folder: String) -> Result<UnitFolderSummary, AutomationError> {
    let unit_folder = PathBuf::from(unit_folder);
    let report_setup = inspect_reports(&unit_folder)?;

    Ok(build_summary(&unit_folder, report_setup))
}

pub fn process_task(
    unit_folder: String,
    task_id: String,
) -> Result<TaskProcessResult, AutomationError> {
    let unit_folder = PathBuf::from(unit_folder);
    let task = find_task(&task_id).ok_or_else(|| AutomationError::UnknownTask(task_id.clone()))?;
    let result = process_task_with_profile_mapping(&task_id, &unit_folder)
        .unwrap_or_else(|| processors::process_task(&task, &unit_folder));

    Ok(to_task_process_result(task_id, result))
}

pub fn open_report_path(unit_folder: String, path: String) -> Result<(), AutomationError> {
    let report_path = validate_report_path(&unit_folder, &path)?;

    open_path_with_default_app(&report_path)
}

pub fn open_report_location(
    unit_folder: String,
    path: String,
    sheet: String,
    cell: String,
) -> Result<(), AutomationError> {
    let report_path = validate_report_path(&unit_folder, &path)?;
    validate_sheet_name(&sheet)?;
    validate_cell_reference(&cell)?;

    open_excel_at_location(&report_path, &sheet, &cell)
}

fn build_summary(unit_folder: &Path, report_setup: ReportSetup) -> UnitFolderSummary {
    let detected = detected_steps(unit_folder);
    let mut steps_to_paths = HashMap::<u16, Vec<PathBuf>>::new();

    for (step, path) in detected {
        steps_to_paths.entry(step).or_default().push(path);
    }

    let mut detected_task_ids = HashSet::<String>::new();
    let tasks = automation_tasks()
        .into_iter()
        .map(|task| {
            let detected_for_task = task
                .detection_steps
                .iter()
                .copied()
                .filter(|step| steps_to_paths.contains_key(step))
                .collect::<Vec<_>>();
            let latest_csv_info = latest_csv_for_steps(&task.detection_steps, &steps_to_paths);
            let latest_csv = latest_csv_info
                .as_ref()
                .map(|info| info.path.display().to_string());
            let latest_csv_created_ms = latest_csv_info.as_ref().and_then(|info| info.created_ms);
            let latest_csv_readable = latest_csv_info.as_ref().map(|info| info.readable);
            let timer_start_ms =
                timer_start_millis_for_steps(&task.detection_steps, &steps_to_paths)
                    .or(latest_csv_created_ms);
            let state = if detected_for_task.is_empty() {
                "off"
            } else {
                detected_task_ids.insert(task.id.clone());
                "detected"
            };

            TaskStatus {
                task_id: task.id,
                label: task.label,
                step: task.step_display,
                state: state.to_string(),
                detected_steps: detected_for_task,
                latest_csv,
                latest_csv_created_ms,
                latest_csv_readable,
                timer_start_ms,
            }
        })
        .collect::<Vec<_>>();

    UnitFolderSummary {
        unit_folder: unit_folder.display().to_string(),
        serial_number: report_setup.serial_number,
        report_path: report_setup.report_path,
        print_report_path: report_setup.print_report_path,
        detected_count: detected_task_ids.len(),
        tasks,
        warnings: report_setup.warnings,
    }
}

#[derive(Debug, Clone)]
struct CsvFileInfo {
    path: PathBuf,
    modified: SystemTime,
    created_ms: Option<u64>,
    readable: bool,
}

fn latest_csv_for_steps(
    steps: &[u16],
    steps_to_paths: &HashMap<u16, Vec<PathBuf>>,
) -> Option<CsvFileInfo> {
    steps
        .iter()
        .filter_map(|step| steps_to_paths.get(step))
        .filter_map(|paths| latest_csv_for_paths(paths))
        .max_by_key(|info| info.modified)
}

fn timer_start_millis_for_steps(
    steps: &[u16],
    steps_to_paths: &HashMap<u16, Vec<PathBuf>>,
) -> Option<u64> {
    steps.iter().find_map(|step| {
        steps_to_paths
            .get(step)
            .and_then(|paths| latest_csv_for_paths(paths))
            .and_then(|info| info.created_ms)
    })
}

fn latest_csv_for_paths(paths: &[PathBuf]) -> Option<CsvFileInfo> {
    paths
        .iter()
        .filter_map(|path| {
            let metadata = path.metadata().ok()?;
            let modified = metadata.modified().ok()?;
            let created_ms = csv_start_time_millis(&metadata);
            let readable = csv_file_is_readable(path);

            Some(CsvFileInfo {
                path: path.clone(),
                modified,
                created_ms,
                readable,
            })
        })
        .max_by_key(|info| info.modified)
}

fn csv_file_is_readable(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut buffer = [0_u8; 1];

    file.read(&mut buffer).is_ok()
}

fn is_locked_file_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33 | 1224))
        || matches!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
        )
}

fn csv_start_time_millis(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .created()
        .or_else(|_| metadata.modified())
        .ok()
        .and_then(system_time_millis)
}

fn system_time_millis(time: SystemTime) -> Option<u64> {
    let millis = time.duration_since(UNIX_EPOCH).ok()?.as_millis();

    u64::try_from(millis).ok()
}

fn to_task_process_result(task_id: String, result: ProcessorResult) -> TaskProcessResult {
    TaskProcessResult {
        task_id,
        state: result.state,
        code: result.code,
        message: result.message,
        log: result.log,
        report_path: result.report_path,
        print_report_path: result.print_report_path,
        failure: result.failure,
    }
}

fn process_task_with_profile_mapping(task_id: &str, unit_folder: &Path) -> Option<ProcessorResult> {
    let profile = load_layout_profile().ok()?;
    let task = profile
        .task_groups
        .iter()
        .flat_map(|group| group.tasks.iter())
        .find(|task| task.id == task_id)?;

    if task.mappings.is_empty() {
        return None;
    }

    Some(mapped::process_task(&profile, task, unit_folder))
}

fn validate_report_path(unit_folder: &str, path: &str) -> Result<PathBuf, AutomationError> {
    let unit_folder = canonicalize_path(unit_folder)
        .map_err(|error| AutomationError::OpenReport(error.to_string()))?;
    let report_path =
        canonicalize_path(path).map_err(|error| AutomationError::OpenReport(error.to_string()))?;

    if !unit_folder.is_dir() {
        return Err(AutomationError::OpenReport(format!(
            "unit folder does not exist: {}",
            unit_folder.display()
        )));
    }

    if !report_path.is_file() {
        return Err(AutomationError::OpenReport(format!(
            "report file does not exist: {}",
            report_path.display()
        )));
    }

    if !report_path.starts_with(&unit_folder) {
        return Err(AutomationError::OpenReport(format!(
            "report is outside the selected unit folder: {}",
            report_path.display()
        )));
    }

    let allowed = report_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "xlsx" | "xlsm" | "xls"
            )
        });

    if !allowed {
        return Err(AutomationError::OpenReport(format!(
            "report file must be an Excel workbook: {}",
            report_path.display()
        )));
    }

    Ok(report_path)
}

fn canonicalize_path(path: &str) -> std::io::Result<PathBuf> {
    fs::canonicalize(PathBuf::from(path))
}

fn validate_sheet_name(sheet: &str) -> Result<(), AutomationError> {
    let trimmed = sheet.trim();

    if trimmed.is_empty() || trimmed.len() > 31 {
        return Err(AutomationError::OpenReport(format!(
            "invalid Excel sheet name: {sheet}"
        )));
    }

    if trimmed
        .chars()
        .any(|ch| matches!(ch, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
    {
        return Err(AutomationError::OpenReport(format!(
            "invalid Excel sheet name: {sheet}"
        )));
    }

    Ok(())
}

fn validate_cell_reference(cell: &str) -> Result<(), AutomationError> {
    let cell_re = Regex::new(r"^[A-Z]{1,3}[1-9][0-9]{0,6}$").expect("cell regex is valid");

    if cell_re.is_match(cell.trim()) {
        return Ok(());
    }

    Err(AutomationError::OpenReport(format!(
        "invalid Excel cell reference: {cell}"
    )))
}

#[cfg(target_os = "windows")]
fn open_path_with_default_app(path: &Path) -> Result<(), AutomationError> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map_err(|error| {
            AutomationError::OpenReport(format!(
                "failed to open report {}: {error}",
                path.display()
            ))
        })?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn open_path_with_default_app(path: &Path) -> Result<(), AutomationError> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|error| {
            AutomationError::OpenReport(format!(
                "failed to open report {}: {error}",
                path.display()
            ))
        })?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn open_excel_at_location(path: &Path, sheet: &str, cell: &str) -> Result<(), AutomationError> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$path = $env:PDU_REPORT_PATH
$sheet = $env:PDU_REPORT_SHEET
$cell = $env:PDU_REPORT_CELL
try {
  $excel = [Runtime.InteropServices.Marshal]::GetActiveObject('Excel.Application')
} catch {
  $excel = New-Object -ComObject Excel.Application
}
$excel.Visible = $true
$workbook = $null
foreach ($candidate in @($excel.Workbooks)) {
  if ([string]::Equals($candidate.FullName, $path, [System.StringComparison]::OrdinalIgnoreCase)) {
    $workbook = $candidate
    break
  }
}
if ($null -eq $workbook) {
  $workbook = $excel.Workbooks.Open($path)
}
$worksheet = $workbook.Worksheets.Item($sheet)
$worksheet.Activate()
$range = $worksheet.Range($cell)
$range.Select()
if ($excel.ActiveWindow -ne $null) {
  $excel.ActiveWindow.ScrollRow = [Math]::Max(1, $range.Row - 5)
  $excel.ActiveWindow.ScrollColumn = [Math]::Max(1, $range.Column - 3)
}
$excel.Activate()
"#;

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            SCRIPT,
        ])
        .env("PDU_REPORT_PATH", path)
        .env("PDU_REPORT_SHEET", sheet)
        .env("PDU_REPORT_CELL", cell)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| {
            AutomationError::OpenReport(format!(
                "failed to open Excel location {}!{} in {}: {error}",
                sheet,
                cell,
                path.display()
            ))
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };

    Err(AutomationError::OpenReport(format!(
        "failed to open Excel location {sheet}!{cell} in {}{}",
        path.display(),
        if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        }
    )))
}

#[cfg(not(target_os = "windows"))]
fn open_excel_at_location(path: &Path, _sheet: &str, _cell: &str) -> Result<(), AutomationError> {
    open_path_with_default_app(path)
}

#[cfg(test)]
mod smoke_tests {
    use std::fs;
    use std::fs::File;
    use std::io::{self, Read};
    use std::path::Path;

    use tempfile::TempDir;
    use walkdir::WalkDir;
    use zip::ZipArchive;

    use super::*;

    #[test]
    fn report_open_validation_accepts_workbook_inside_unit_folder() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join("report.xlsx");
        File::create(&report).expect("report file");

        let validated = validate_report_path(
            &unit_folder.display().to_string(),
            &report.display().to_string(),
        )
        .expect("report should be valid");

        assert_eq!(
            validated,
            fs::canonicalize(report).expect("canonical report")
        );
    }

    #[test]
    fn report_open_validation_rejects_workbook_outside_unit_folder() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        let other_folder = temp.path().join("other");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        fs::create_dir_all(&other_folder).expect("other folder");
        let report = other_folder.join("report.xlsx");
        File::create(&report).expect("report file");

        let error = validate_report_path(
            &unit_folder.display().to_string(),
            &report.display().to_string(),
        )
        .expect_err("outside report should be rejected");

        assert!(error
            .to_string()
            .contains("outside the selected unit folder"));
    }

    #[test]
    fn timer_start_prefers_first_detection_step_for_multistep_tasks() {
        let temp = TempDir::new().expect("temp dir");
        let step71 = temp.path().join("unit_STEP71_system.csv");
        let step72 = temp.path().join("unit_STEP72_system.csv");

        fs::write(&step71, "a,b\n1,2\n").expect("write step71");
        fs::write(&step72, "a,b\n3,4\n").expect("write step72");

        let mut steps_to_paths = HashMap::<u16, Vec<PathBuf>>::new();
        steps_to_paths.insert(71, vec![step71.clone()]);
        steps_to_paths.insert(72, vec![step72]);

        let expected = latest_csv_for_paths(&[step71])
            .and_then(|info| info.created_ms)
            .expect("step71 should have a start time");

        assert_eq!(
            timer_start_millis_for_steps(&[71, 72], &steps_to_paths),
            Some(expected)
        );
    }

    #[test]
    fn transformer_setup_rejects_blank_transformer_sn() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");

        let error = setup_unit_folder_with_transformer_sn(
            unit_folder.display().to_string(),
            Some("262343000072".to_string()),
            "   ".to_string(),
        )
        .expect_err("blank transformer SN should fail");

        assert_eq!(error.code, "blank_transformer_sn");
        assert!(error.message.contains("Transformer SN is required"));
    }

    #[test]
    fn transformer_setup_maps_report_errors_to_stable_codes() {
        let locked = AutomationCommandError::from_report_error(ReportError::Io(
            io::Error::from_raw_os_error(32),
        ));
        assert_eq!(locked.code, "workbook_locked");

        let missing = AutomationCommandError::from_report_error(ReportError::MainReportMissing(
            "C:/PDU500/262343000072".to_string(),
        ));
        assert_eq!(missing.code, "main_report_missing");

        let missing_sheet = AutomationCommandError::from_report_error(ReportError::SheetMissing(
            "Test Summary".to_string(),
        ));
        assert_eq!(missing_sheet.code, "report_sheet_missing");
        assert!(missing_sheet.message.contains("Test Summary"));
    }

    #[test]
    #[ignore = "requires a real PDU sample unit folder; set PDU_SAMPLE_UNIT_FOLDER"]
    fn real_sample_processes_representative_tasks_without_touching_source() {
        let sample = std::env::var("PDU_SAMPLE_UNIT_FOLDER")
            .unwrap_or_else(|_| "C:/PDU500/262343000072".to_string());
        let sample = Path::new(&sample);

        assert!(
            sample.is_dir(),
            "sample folder missing: {}",
            sample.display()
        );

        let temp = TempDir::new().expect("temp dir");
        let unit_copy = temp.path().join("unit");
        copy_dir(sample, &unit_copy);

        let summary =
            setup_unit_folder(unit_copy.display().to_string()).expect("setup should work");

        assert_eq!(summary.serial_number.as_deref(), Some("262343000072"));
        assert!(summary.report_path.is_some());
        assert!(summary.detected_count >= 60);

        for task_id in [
            "208v-transformer",
            "208v-system-100% Load",
            "208v-breaker-1-100% Load",
            "415v-system-100% Load",
            "415v-breaker-1-100% Load",
            "system-burn-in",
            "breaker-burn-in-1",
        ] {
            let result =
                process_task(unit_copy.display().to_string(), task_id.to_string()).unwrap();

            assert_ne!(result.code, 2, "{task_id}: {}", result.message);
        }

        let report_path = summary.report_path.expect("main report path");
        assert_workbook_package_is_valid_after_patch(Path::new(&report_path));
    }

    fn copy_dir(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("create destination");

        for entry in WalkDir::new(source).into_iter().filter_map(Result::ok) {
            let relative = entry
                .path()
                .strip_prefix(source)
                .expect("entry should be under source");
            let target = destination.join(relative);

            if entry.file_type().is_dir() {
                fs::create_dir_all(&target).expect("create directory");
            } else {
                fs::copy(entry.path(), &target).expect("copy file");
            }
        }
    }

    fn assert_workbook_package_is_valid_after_patch(path: &Path) {
        let mut archive = ZipArchive::new(File::open(path).expect("open workbook")).unwrap();
        let mut workbook_xml = String::new();

        assert!(
            archive.by_name("xl/calcChain.xml").is_err(),
            "stale calcChain should be removed"
        );

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("zip entry");
            let name = entry.name().to_string();

            if !(name.ends_with(".xml") || name.ends_with(".rels")) {
                continue;
            }

            let mut xml = String::new();
            entry.read_to_string(&mut xml).expect("XML should be UTF-8");
            roxmltree::Document::parse(&xml)
                .unwrap_or_else(|error| panic!("{name} should be valid XML: {error}"));

            if name == "xl/workbook.xml" {
                workbook_xml = xml;
            }
        }

        assert!(workbook_xml.contains(r#"calcMode="auto""#));
        assert!(workbook_xml.contains(r#"fullCalcOnLoad="1""#));
        assert!(workbook_xml.contains(r#"forceFullCalc="1""#));
    }
}
