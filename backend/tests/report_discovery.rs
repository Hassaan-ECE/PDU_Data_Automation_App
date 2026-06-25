use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use pdu_data_automation_app_lib::automation::{
    save_final_operator_name, save_transformer_sn, scan_unit_folder,
};
use tempfile::TempDir;

const MAIN_REPORT: &str = "PDUD500442AM088_Test Report_0.2CT_Rev02_SN262343000072.xlsx";
const ALT_MAIN_REPORT: &str = "PDUD500442AM088_Test Report_0.2CT_Rev02_SN999999999999.xlsx";
const PREFIX_MAIN_REPORT: &str = "PDUD500442AM088_Test Report_0.2CT_Rev02_DRAFT.xlsx";
const BROAD_MAIN_REPORT: &str = "PDUD500442AM088_misc.xlsx";
const PRINT_REPORT: &str = "PDUD500442AA088_0.2CT Test Report Print.xlsx";
const ALT_PRINT_REPORT: &str = "PDUD500442AA088_ALT_Print.xlsx";
const UNRELATED_REPORT: &str = "Unrelated_Report.xlsx";

#[test]
fn discovers_main_and_print_report_fixtures_by_current_prefixes() {
    let temp = TempDir::new().expect("temp dir");
    let unit_folder = temp.path().join("262343000072");
    fs::create_dir_all(&unit_folder).expect("unit folder");
    let main_path = copy_workbook_fixture(&unit_folder, MAIN_REPORT);
    let print_path = copy_workbook_fixture(&unit_folder, PRINT_REPORT);
    copy_workbook_fixture(&unit_folder, UNRELATED_REPORT);

    let summary =
        scan_unit_folder(unit_folder.display().to_string()).expect("unit folder should scan");

    assert_path_eq(summary.report_path.as_deref(), &main_path);
    assert_path_eq(summary.print_report_path.as_deref(), &print_path);
}

#[test]
fn unrelated_workbook_does_not_satisfy_report_discovery() {
    let temp = TempDir::new().expect("temp dir");
    let unit_folder = temp.path().join("262343000072");
    fs::create_dir_all(&unit_folder).expect("unit folder");
    copy_workbook_fixture(&unit_folder, UNRELATED_REPORT);

    let summary =
        scan_unit_folder(unit_folder.display().to_string()).expect("unit folder should scan");

    assert!(summary.report_path.is_none());
    assert!(summary.print_report_path.is_none());
}

#[test]
fn empty_unit_folder_missing_workbooks_fail_clearly() {
    let temp = TempDir::new().expect("temp dir");
    let unit_folder = temp.path().join("262343000072");
    fs::create_dir_all(&unit_folder).expect("unit folder");

    let main_error = save_transformer_sn(unit_folder.display().to_string(), "TX-123".to_string())
        .expect_err("missing main report should fail");

    assert_eq!(main_error.code, "main_report_missing");
    assert!(main_error
        .message
        .contains("main report workbook was not found"));

    let print_error =
        save_final_operator_name(unit_folder.display().to_string(), "Sean".to_string())
            .expect_err("missing print report should fail");

    assert_eq!(print_error.code, "print_report_missing");
    assert!(print_error
        .message
        .contains("print report workbook was not found"));
}

#[test]
fn multiple_matching_reports_follow_current_priority_and_latest_rules() {
    let temp = TempDir::new().expect("temp dir");
    let unit_folder = temp.path().join("262343000072");
    fs::create_dir_all(&unit_folder).expect("unit folder");

    copy_workbook_fixture(&unit_folder, MAIN_REPORT);
    copy_after_timestamp_tick(&unit_folder, ALT_MAIN_REPORT);
    copy_after_timestamp_tick(&unit_folder, PREFIX_MAIN_REPORT);
    copy_after_timestamp_tick(&unit_folder, BROAD_MAIN_REPORT);

    copy_workbook_fixture(&unit_folder, PRINT_REPORT);
    let newest_print = copy_after_timestamp_tick(&unit_folder, ALT_PRINT_REPORT);

    let summary =
        scan_unit_folder(unit_folder.display().to_string()).expect("unit folder should scan");
    let expected_main = unit_folder.join(ALT_MAIN_REPORT);

    assert_path_eq(summary.report_path.as_deref(), &expected_main);
    assert_path_eq(summary.print_report_path.as_deref(), &newest_print);
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend should be under repo root")
        .to_path_buf()
}

fn copy_workbook_fixture(unit_folder: &Path, file_name: &str) -> PathBuf {
    let source = repo_root()
        .join("fixtures")
        .join("workbooks")
        .join(file_name);
    let target = unit_folder.join(file_name);
    let bytes = fs::read(&source)
        .unwrap_or_else(|error| panic!("read workbook fixture {}: {error}", source.display()));

    fs::write(&target, bytes).unwrap_or_else(|error| {
        panic!(
            "write workbook fixture {} to {}: {error}",
            source.display(),
            target.display()
        )
    });

    target
}

fn copy_after_timestamp_tick(unit_folder: &Path, file_name: &str) -> PathBuf {
    thread::sleep(Duration::from_millis(25));
    copy_workbook_fixture(unit_folder, file_name)
}

fn assert_path_eq(actual: Option<&str>, expected: &Path) {
    let expected = expected.display().to_string();

    assert_eq!(actual, Some(expected.as_str()));
}
